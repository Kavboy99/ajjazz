"""Reproduce offline revision/protocol findings. No HID, network, or vendor JS execution.

Reads captured sources from research/vendor/revision-hunt and emits an evidence
manifest. Packet builders here are analytical fixtures, never a device transport.
"""
from pathlib import Path
import hashlib
import json
import math
import re
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.tools/py-research'))
import json5


def js_object(source, start):
    level, quote, escaped = 0, None, False
    for end in range(start, len(source)):
        ch = source[end]
        if quote:
            if escaped:
                escaped = False
            elif ch == '\\':
                escaped = True
            elif ch == quote:
                quote = None
        elif ch in "\"'`":
            quote = ch
        elif ch == '{':
            level += 1
        elif ch == '}':
            level -= 1
            if level == 0:
                return source[start:end + 1]
    raise ValueError('Unterminated literal')


def parse_literal(raw):
    # Tokenize quoted strings: the Backquote key name itself contains a `.
    converted, pos = [], 0
    while pos < len(raw):
        if raw[pos] not in "\"'`":
            if raw[pos:pos + 2] in ('!0', '!1'):
                converted.append('true' if raw[pos + 1] == '0' else 'false')
                pos += 2
            else:
                converted.append(raw[pos])
                pos += 1
            continue
        delim, begin = raw[pos], pos
        pos += 1
        while pos < len(raw):
            if raw[pos] == '\\':
                pos += 2
                continue
            if raw[pos] == delim:
                break
            pos += 1
        segment = raw[begin:pos + 1]
        if delim == '`':
            assert '${' not in segment, 'Not a static literal'
            value = re.sub(r'\\(.)', lambda m: {'n': '\n', 'r': '\r', 't': '\t'}.get(m[1], m[1]), segment[1:-1])
            converted.append(json.dumps(value, ensure_ascii=False))
        else:
            converted.append(segment)
        pos += 1
    return json5.loads(''.join(converted))


def profiles(path):
    source = path.read_text(encoding='utf-8')
    result = []
    pattern = r'([\w$]+)\s*=\s*(\{\s*vendorId\s*:\s*3141\s*,\s*productId\s*:\s*\d+)'
    for match in re.finditer(pattern, source):
        raw = js_object(source, match.start(2))
        if 'AK820MAX' not in raw and 'AK820 MAX' not in raw:
            continue
        config = parse_literal(raw)
        result.append({
            'symbol': match[1], 'name': config.get('name'),
            'pid_hex': f"{config['productId']:04X}",
            'connection': config.get('connectType'),
            'routes': config.get('routesName'),
            'keys': sum(map(len, config.get('keyList', []))),
            'lighting_config': config.get('lightingConfig'),
            'music_config': config.get('musicConfig'),
            'advanced_keys_config': config.get('advancedKeysConfig'),
            'custom_keys_config': config.get('customKeysConfig'),
        })
    return result


def intel_hex(path):
    memory, base, entry = {}, 0, None
    eof = False
    records = 0
    for number, line in enumerate(path.read_text(encoding='ascii').splitlines(), 1):
        if not line.strip():
            continue
        assert line.startswith(':') and not eof, f'HEX record {number}'
        data = bytes.fromhex(line[1:])
        assert len(data) == data[0] + 5 and sum(data) % 256 == 0, f'HEX checksum {number}'
        address = int.from_bytes(data[1:3], 'big')
        kind, payload = data[3], data[4:-1]
        if kind == 0:
            for offset, value in enumerate(payload):
                absolute = base + address + offset
                assert absolute not in memory or memory[absolute] == value, 'Conflicting HEX records'
                memory[absolute] = value
        elif kind == 1:
            assert not payload
            eof = True
        elif kind == 4:
            assert len(payload) == 2
            base = int.from_bytes(payload, 'big') << 16
        elif kind == 5:
            assert len(payload) == 4
            entry = int.from_bytes(payload, 'big')
        else:
            raise ValueError(f'Unhandled HEX record {kind}')
        records += 1
    assert eof
    ranges = []
    for address in sorted(memory):
        if ranges and address == ranges[-1]['end_exclusive']:
            ranges[-1]['end_exclusive'] += 1
        else:
            ranges.append({'start': address, 'end_exclusive': address + 1})
    vector = bytes(memory.get(i, 255) for i in range(64))
    return {
        'checksums_valid': True, 'records': records, 'data_bytes': len(memory),
        'ranges': ranges, 'entry_point': entry,
        'initial_stack': struct.unpack_from('<I', vector, 0)[0],
        'reset_handler': struct.unpack_from('<I', vector, 4)[0],
        'flashable_for_observed_keyboard': False,
    }


def gif_packets(colors, frames, speed, report_bytes):
    """Exact OZ fixture including the site's unusual total-count expression."""
    assert 1 <= frames <= 65535
    assert len(colors) % (3 * frames) == 0
    assert 1 <= speed <= 105 and report_bytes in (32, 64, 4096)
    data = struct.pack('<HH', frames, math.ceil(106 - speed)) + colors
    capacity = report_bytes - 8
    count = math.ceil(len(data) / capacity)
    assert count <= 65535
    result = []
    for index in range(count):
        # JS uses floor(length/capacity)+1, even for an exact multiple.
        header = bytes([0xAA, 0x42, index >> 8, index & 255,
                        (int(len(data) / capacity) >> 8) & 255,
                        int(len(data) / capacity + 1) & 255, 0, 0])
        result.append(header + data[index * capacity:(index + 1) * capacity].ljust(capacity, b'\0'))
    return result


def crc16(data):
    value = 0xFFFF
    for byte in data:
        value ^= byte << 8
        for _ in range(8):
            value = ((value << 1) ^ (0x1021 if value & 0x8000 else 0)) & 0xFFFF
    return value


def ota_packets(data, report_bytes):
    """XZ/YZ fixtures: no packet crosses a 4096-byte checksum boundary."""
    assert data and report_bytes in (32, 64)
    chunks, offset = [], 0
    while offset < len(data):
        end = min(offset + report_bytes - 9, len(data), (offset // 4096 + 1) * 4096)
        chunks.append((offset, data[offset:end]))
        offset = end
    assert len(chunks) <= 65535
    result = []
    for index, (start, payload) in enumerate(chunks, 1):
        end = start + len(payload)
        boundary = end % 4096 == 0 or end == len(data)
        checksum = crc16(data[(end - 1) // 4096 * 4096:end]) if boundary else 0xFFFF
        packet = bytes([0xAA, 0x83]) + struct.pack('>HHBH', index, len(chunks), len(payload), checksum) + payload
        result.append(packet.ljust(report_bytes, b'\0'))
    return result


def main():
    sys.stdout.reconfigure(encoding='utf-8')
    vendor = ROOT / 'research/vendor'
    hunt = vendor / 'revision-hunt'
    sources = [vendor / 'layout-classic-0DHAQysC.js']
    sources += sorted(hunt.glob('ajazz-ui-artifact-260*/layout*.js'))
    source_profiles = {str(p.relative_to(ROOT)): profiles(p) for p in sources}
    exact = next(p for p in source_profiles[str(sources[0].relative_to(ROOT))] if p['pid_hex'] == '80A0')
    assert not (exact['lighting_config'] or {}).get('isShowGifEffect', False)
    assert crc16(b'123456789') == 0x29B1
    fixture = gif_packets(bytes([255, 0, 0]) * 82 + bytes([0, 0, 255]) * 82, 2, 56, 4096)
    assert len(fixture) == 1 and fixture[0][:12].hex() == 'aa4200000001000002003200'
    ota = ota_packets(bytes(i % 256 for i in range(4097)), 64)
    assert len(ota) == 76 and ota[-2][6] == 26 and ota[-1][6] == 1
    report = {
        'schema': 1, 'date': '2026-10-17', 'offline_only': True,
        'observed_identity': json.loads((ROOT / 'research/local/keyboard-capabilities-rust-07.json').read_text()),
        'source_profiles': source_profiles,
        'firmware_catalog': {'url': 'https://ajazz.driveall.cn/update.json', 'http_status': 200, 'bytes': (hunt / 'update-matched.json').stat().st_size,
                             'match': 'entry.productId === deviceInfo.pid && entry.productName === HID.productName'},
        'gif_fixture': {'frames': 2, 'keys': 82, 'delay_raw': 50, 'report_data_bytes': 4096,
                        'packets': len(fixture), 'first_24_bytes': fixture[0][:24].hex(),
                        'sent_to_device': False,
                        'exact_multiple_count_bug': 'floor(payload/capacity)+1 may exceed ceil(payload/capacity)'},
        'ota_fixture': {'input_bytes': 4097, 'packets_usb': len(ota), 'crc_123456789': '29B1', 'sent_to_device': False},
        'candidate_hex': intel_hex(hunt / 'ak820-recovery-b.exe.zip-SN34F280_User.hex'),
        'candidate_declared_usb_pids': ['8033', '8070'],
        'candidate_rejection': 'Updater metadata does not include observed PID 80A0; no compatibility evidence for manufacturer 0166/product 1004.',
        'wasm_selector': json.loads((hunt / 'wasm-chip-selector-test.json').read_text()),
        'wasm_chip_indices': ['SN34F280', 'SN32F24xB', 'SN32F268', 'SN32F290'],
        'not_established': ['Exact 80A0 1.30 firmware image and MCU', 'Existing firmware handler for GIF command 42',
                            'Persistent animation storage size and layout', 'Flash endurance and RAM/flash behavior of 24',
                            'Atomic frame swap or viable direct RGB on this revision', 'Recovery path preserving radio, calibration and key scanning'],
        'files': {},
    }
    for path in sorted(hunt.rglob('*')):
        if path.is_file() and path.suffix not in ('.wat', '.html', '.txt'):
            report['files'][str(path.relative_to(ROOT))] = {'bytes': path.stat().st_size, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    dest = ROOT / 'research/local/keyboard-revision-audit.json'
    dest.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(f'Wrote {dest}; {sum(map(len, source_profiles.values()))} profiles, {len(report["files"])} hashed artifacts; offline packet checks passed')


if __name__ == '__main__':
    main()
