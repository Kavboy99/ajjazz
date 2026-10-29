"""Read Qt v1 resource tables from a PE file without loading vendor code.

Supply the three virtual addresses passed to qRegisterResourceData(1, tree,
names, data). They must be identified by static analysis, not guessed.
Only firmware and configuration files are saved; all resources are indexed.
Requires pefile from requirements-firmware.txt. No HID or network operations.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import zlib


def extract_resources(content, data, names, tree):
    def read(offset, size):
        if offset < 0 or size < 0 or offset + size > len(content):
            raise ValueError('Resource outside file')
        return content[offset:offset + size]

    def u32(offset):
        return struct.unpack('>I', read(offset, 4))[0]

    result, seen = [], set()

    def walk(index, parent='', depth=0):
        if depth > 32 or index in seen or len(seen) > 20000:
            raise ValueError('Invalid resource tree')
        seen.add(index)
        offset = tree + index * 14
        name_offset, flags = struct.unpack('>IH', read(offset, 6))
        length = struct.unpack('>H', read(names + name_offset, 2))[0]
        name = read(names + name_offset + 6, length * 2).decode('utf-16be') if index else ''
        path = parent + '/' + name if name else parent
        if flags & 2:
            count, first = struct.unpack('>II', read(offset + 6, 8))
            if count > 20000:
                raise ValueError('Too many resource children')
            for child in range(first, first + count):
                walk(child, path, depth + 1)
        else:
            payload_offset = data + u32(offset + 10)
            payload = read(payload_offset + 4, u32(payload_offset))
            if flags & ~1:
                raise ValueError('Unsupported Qt compression flags')
            if flags & 1:
                expected = struct.unpack('>I', payload[:4])[0]
                if expected > 64 * 1024 * 1024:
                    raise ValueError('Oversized Qt resource')
                decoder = zlib.decompressobj()
                payload = decoder.decompress(payload[4:], expected + 1)
                if len(payload) != expected or not decoder.eof:
                    raise ValueError('Invalid Qt compressed resource')
            result.append((path, payload))

    walk(0)
    return result


def main():
    import pefile
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pe', type=Path)
    for name in ['tree', 'names', 'data']:
        parser.add_argument('--' + name, type=lambda x: int(x, 0), required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    content = args.pe.read_bytes()
    pe = pefile.PE(data=content)
    base = pe.OPTIONAL_HEADER.ImageBase
    tree, names, data = [pe.get_offset_from_rva(getattr(args, key) - base)
                         for key in ['tree', 'names', 'data']]
    resources = extract_resources(content, data, names, tree)
    args.output.mkdir(parents=True, exist_ok=True)
    index = []
    for number, (path, payload) in enumerate(resources):
        item = {'path': path, 'bytes': len(payload), 'sha256': hashlib.sha256(payload).hexdigest()}
        if Path(path).suffix.lower() in ('.bin', '.hex', '.ufw', '.config', '.xml', '.ini', '.json'):
            # Flatten names and add a unique numeric prefix; never use embedded paths.
            filename = f'{number:04d}-' + Path(path).name
            (args.output / filename).write_bytes(payload)
            item['saved_as'] = filename
        index.append(item)
    manifest = {'source_sha256': hashlib.sha256(content).hexdigest(),
                'qt_version': 1, 'virtual_addresses': {k: getattr(args, k) for k in ['tree', 'names', 'data']},
                'resources': index, 'vendor_code_executed': False}
    (args.output / 'index.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding='utf-8')
    print(f'Indexed {len(index)} resources; extracted {sum("saved_as" in item for item in index)} files')


if __name__ == '__main__':
    main()
