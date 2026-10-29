"""Reproduce exact AJAZZ email package extraction; never execute an updater.

RAR is unpacked by bundled 7-Zip into a private temporary directory. Nested ZIP
resources are read as data. Both inserted 202-byte regions are removed from
the xN wrapper; EOF, trailing bytes, output bounds and pinned hashes are checked.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
ARCHIVE_SHA = '43e64058915b6bad1ba842077bb200b6b73d305804581d12a8e6c93bcb3e53b1'
PINS = {
    ('dock', 'x1'): (31888, 'a4398f2e8cfe2af3c59c6073836298c8cd350230c5850fed90d73164b83507d8'),
    ('dock', 'x2'): (245748, '57eeedd50368cce8d412b5ea08ea9027a1a548c1c475c9f1007cef650f70e0db'),
    ('dock', 'x5'): (367660, 'f767078270a554813fd3b134613bbb743610422fdd754402df55ecb769341fe9'),
    ('mouse', 'x1'): (367028, '82f35e611ab37861f99bb450c70b8b769c3385053dc4dc22c0393e6828a44289'),
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def decode_resource(raw):
    if not 406 <= len(raw) <= 2 * 1024 * 1024:
        raise ValueError('Invalid resource length')
    intermediate = raw[:1] + raw[203:]
    stream = intermediate[:-203] + intermediate[-1:]
    decoder = zlib.decompressobj(-15)
    result = decoder.decompress(stream, 2 * 1024 * 1024 + 1)
    if len(result) > 2 * 1024 * 1024 or decoder.unconsumed_tail:
        raise ValueError('Resource exceeds output bound')
    if not decoder.eof or decoder.unused_data:
        raise ValueError('Incomplete or trailing DEFLATE stream')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    args = parser.parse_args()
    if sha(args.archive.read_bytes()) != ARCHIVE_SHA:
        raise ValueError('Email RAR hash mismatch')
    target = ROOT / 'research/vendor/ajazz-support-20261023'
    manifest = {'archive_sha256': ARCHIVE_SHA, 'source': 'User-provided AJAZZ support email attachment',
                'vendor_updater_executed': False, 'hardware_access': False, 'images': []}
    with tempfile.TemporaryDirectory(prefix='ajazz-support-') as tmp:
        subprocess.run([str(ROOT / '.tools/7zip-26.05/7z.exe'), 'x', str(args.archive.resolve()),
                        '-o' + tmp, '-y'], check=True, capture_output=True)
        zips = list(Path(tmp).rglob('*.zip'))
        if len(zips) != 2:
            raise ValueError('Expected exactly two updater ZIP files')
        for package in zips:
            role = {'Base upgrade tool.zip': 'dock', 'Mouse Upgrade Tool.zip': 'mouse'}.get(package.name)
            if role is None:
                raise ValueError('Unknown updater ZIP')
            with zipfile.ZipFile(package) as archive:
                names = archive.namelist()
                root = [n[:-len('resources/x1')] for n in names if n.endswith('/resources/x1')]
                if len(root) != 1 or len(set(names)) != len(names):
                    raise ValueError('Ambiguous updater structure')
                prefix = root[0]
                updater = archive.read(prefix + 'ry_upgrade.exe')
                if sha(updater) != '4ee530e50826b59ff4599e49e6fe37116f9c686c1931f590074f38d467c1cf33':
                    raise ValueError('Unexpected updater executable')
                configs = {n: json.loads(archive.read(prefix + 'resources/' + n))
                           for n in ('support_config.json', 'tool_config.json')}
                (target / role / 'decoded').mkdir(parents=True, exist_ok=True)
                for index in range(1, 8):
                    name = f'x{index}'
                    raw = archive.read(prefix + 'resources/' + name)
                    if not raw:
                        continue
                    decoded = decode_resource(raw)
                    size, digest = PINS[(role, name)]
                    if (len(decoded), sha(decoded)) != (size, digest):
                        raise ValueError(f'Payload mismatch: {role}/{name}')
                    path = target / role / 'decoded' / (name + '-verified.bin')
                    path.write_bytes(decoded)
                    manifest['images'].append({'role': role, 'resource': name, 'bytes': size,
                        'sha256': digest, 'packed_sha256': sha(raw),
                        'file': path.relative_to(ROOT).as_posix(), 'deflate_eof': True,
                        'updater_sha256': sha(updater), 'configs': configs})
    (target / 'manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    for row in manifest['images']:
        print(row['role'], row['resource'], row['bytes'], row['sha256'])


if __name__ == '__main__':
    main()
