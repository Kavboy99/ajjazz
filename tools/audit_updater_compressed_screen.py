"""Inspect bounded zlib/gzip streams in pinned reference updaters, offline.

No executable is launched. This does not discover raw DEFLATE without framing,
arbitrary codecs, encryption, or prove compatibility of any extracted content.
"""
import hashlib
import json
from pathlib import Path
import re
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]
SOURCES = {
    'aj179-reference/dock-ry_upgrade.exe': '9fa30ca90a927ad5827bd9a4ee0b15dc6ffe40b5fb56fbb6b29bcc81ae31345c',
    'aj179-reference/mouse-ry_upgrade.exe': 'a5b9f9b2383b4de8aee6d589ac1114984d54e4b3469b8423c8c4a288dd420b12',
    'aj159-pro-reference/dock-ry_upgrade.exe': 'bdbd9d55a7d1d4843994c3143799de079597b72472bd65e1d86991c8de165e98',
    'aj159-pro-reference/mouse-ry_upgrade.exe': 'e02ff94287d47a83389d1b034f09da9a67dcd03bd7aa4f67474f9e01af9aeff2',
}
COMPRESSED_LIMIT = 2 * 1024 * 1024
OUTPUT_LIMIT = 4 * 1024 * 1024
ATTEMPT_LIMIT = 4096


def inspect(data):
    # Standard32K zlib headers and gzip. No raw DEFLATE guesses are made.
    matches = list(re.finditer(rb'\x78[\x01\x5e\x9c\xda]|\x1f\x8b\x08', data))
    result = {'header_candidates': len(matches), 'attempts': 0, 'valid_streams': [],
              'limit_reached': len(matches) > ATTEMPT_LIMIT}
    view = memoryview(data)
    for match in matches[:ATTEMPT_LIMIT]:
        start = match.start()
        codec = 31 if data[start] == 0x1F else 15
        decoder = zlib.decompressobj(codec)
        result['attempts'] += 1
        try:
            source = view[start:start + COMPRESSED_LIMIT]
            output = decoder.decompress(source, OUTPUT_LIMIT + 1)
        except zlib.error:
            continue
        if len(output) > OUTPUT_LIMIT or not decoder.eof:
            # Incomplete streams or oversized objects are never extracted.
            continue
        consumed = len(source) - len(decoder.unused_data)
        if consumed < 64 or len(output) < 128:
            continue
        markers = [token.decode() for token in (b'PAN108OLED', b'yc_gpio.c', b'yc_uart.c')
                   if token in output]
        stream = {'offset': hex(start), 'codec': 'gzip' if codec == 31 else 'zlib',
            'compressed_bytes': consumed, 'decompressed_bytes': len(output),
            'decompressed_sha256': hashlib.sha256(output).hexdigest(), 'screen_markers': markers}
        png = data.rfind(b'\x89PNG\r\n\x1a\n', max(0, start - 512), start)
        if png >= 0 and data[png + 12:png + 16] == b'IHDR':
            width, height = struct.unpack_from('>II', data, png + 16)
            stream['enclosing_png'] = {'offset': hex(png), 'width': width, 'height': height,
                                      'bit_depth': data[png + 24], 'color_type': data[png + 25]}
        result['valid_streams'].append(stream)
    return result


def main():
    records = []
    for name, expected in SOURCES.items():
        path = ROOT / 'research/vendor/panchip-reference' / name
        data = path.read_bytes()
        if hashlib.sha256(data).hexdigest() != expected:
            raise ValueError(f'Pinned updater mismatch: {name}')
        record = {'file': name, 'sha256': expected, **inspect(data)}
        records.append(record)
        print(f"{name}: candidates={record['header_candidates']} attempts={record['attempts']} "
              f"streams={len(record['valid_streams'])} markers={sum(bool(s['screen_markers']) for s in record['valid_streams'])}", flush=True)
    report = {'hardware_access': False, 'executable_launched': False,
              'limitations': 'Bounded zlib/gzip scan only; no raw DEFLATE, encryption or other codecs',
              'compressed_limit': COMPRESSED_LIMIT, 'output_limit': OUTPUT_LIMIT,
              'attempt_limit_per_file': ATTEMPT_LIMIT, 'files': records}
    out = ROOT / 'research/local/mouse-firmware-20261021/updater-compressed-screen-audit.json'
    out.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(out)


if __name__ == '__main__':
    main()
