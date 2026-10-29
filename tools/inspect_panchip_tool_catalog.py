"""Statically read the pinned official tools directory; never execute vendor code.

Extracts only Python marshal constants through xdis, not imports or bytecode
execution. No USB, credentials, private endpoints or device settings are used.
"""
import hashlib
import io
import json
from pathlib import Path
import struct
import zlib

from xdis.unmarshal import load_code

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'research/vendor/panchip-reference/sdk-131-tool-directory'
SHA = 'ea051e86c92faf94e053c3d403302cc8c51778e00c8968429b344e9ec83d2cd6'


def main():
    data = (OUT / 'Panchip Tools Directory V0.0.016.exe').read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Pinned source mismatch')
    cookie = data.rfind(b'MEI\x0c\x0b\x0a\x0b\x0e')
    _, package_size, toc_offset, toc_size, version, _ = struct.unpack(
        '!8sIIII64s', data[cookie:cookie + 88])
    if version != 308:
        raise ValueError('Unsupported Python version')
    base = cookie + 88 - package_size
    cursor, end = base + toc_offset, base + toc_offset + toc_size
    entries = {}
    while cursor < end:
        size, offset, packed, unpacked, compressed, kind = struct.unpack(
            '!IIIIBc', data[cursor:cursor + 18])
        if size < 18 or cursor + size > end:
            raise ValueError('Invalid archive TOC')
        name = data[cursor + 18:cursor + size].rstrip(b'\0').decode()
        if base + offset < base or base + offset + packed > cookie:
            raise ValueError('Archive entry outside payload')
        entries[name] = (base + offset, packed, unpacked, compressed, kind)
        cursor += size
    offset, packed, _, compressed, _ = entries['PYZ.pyz']
    if compressed:
        raise ValueError('Unexpected PYZ compression')
    pyz = data[offset:offset + packed]
    if pyz[:4] != b'PYZ\0':
        raise ValueError('Invalid PYZ header')
    magic = int.from_bytes(pyz[4:6], 'little')
    toc_start = struct.unpack('!I', pyz[8:12])[0]
    # This table contains only names/integers, decoded without pickle/eval.
    import marshal
    toc = dict(marshal.loads(pyz[toc_start:]))
    reports = []
    for name in ('tool_infos', 'tool_list_widget', 'version'):
        _, pos, size = toc[name]
        decoder = zlib.decompressobj()
        raw = decoder.decompress(pyz[pos:pos + size], 1024 * 1024)
        if not decoder.eof:
            raise ValueError('Oversized/truncated code module')
        code = load_code(io.BytesIO(raw), magic)
        strings = []

        def walk(obj):
            for value in obj.co_consts:
                if isinstance(value, str):
                    strings.append(value)
                elif hasattr(value, 'co_consts'):
                    walk(value)

        walk(code)
        reports.append({'module': name, 'marshal_sha256': hashlib.sha256(raw).hexdigest(),
                        'urls': [s for s in strings if s.startswith(('http://', 'https://'))]})
    result = {'source_sha256': SHA, 'vendor_code_executed': False,
              'hardware_access': False, 'modules': reports}
    (OUT / 'verified-catalog-urls.json').write_text(
        json.dumps(result, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(f'Extracted URLs from {len(reports)} modules without execution')


if __name__ == '__main__':
    main()
