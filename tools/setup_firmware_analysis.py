"""Fetch pinned official portable Ghidra/JDK for offline firmware analysis.

No installer is executed, no OS configuration changes, no device access.
Source metadata lives in .tools/*-release-metadata.json. Both SHA256 hashes
are published by the respective upstream projects and are checked before unzip.
"""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path, PurePosixPath
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1] / '.tools'


def prepare(item):
    name, url, size, checksum = item
    target = ROOT / name
    if not target.exists() or hashlib.sha256(target.read_bytes()).hexdigest() != checksum:
        partial = target.with_suffix(target.suffix + '.part')
        digest = hashlib.sha256()
        received = 0
        with urllib.request.urlopen(url, timeout=25) as response, partial.open('wb') as output:
            while block := response.read(1024 * 1024):
                received += len(block)
                if received > size:
                    raise ValueError('Archive exceeds published size')
                output.write(block)
                digest.update(block)
                if received % (50 * 1024 * 1024) == 0:
                    print(f'{name}: {received // 1024 // 1024} MiB', flush=True)
        if received != size or digest.hexdigest() != checksum:
            raise ValueError('Official tool archive size/hash mismatch')
        partial.replace(target)
    with zipfile.ZipFile(target) as archive:
        entries = archive.infolist()
        if len(entries) > 100000 or sum(i.file_size for i in entries) > 3 * 1024**3:
            raise ValueError('Unexpected tool archive expansion')
        for entry in entries:
            path = PurePosixPath(entry.filename)
            if path.is_absolute() or '..' in path.parts or ':' in entry.filename or '\\' in entry.filename:
                raise ValueError('Unsafe archive path')
        archive.extractall(ROOT)
    print(f'{name}: verified and extracted', flush=True)


def main():
    ghidra = json.loads((ROOT / 'ghidra-release-metadata.json').read_text())['assets'][0]
    jdk = json.loads((ROOT / 'jdk-release-metadata.json').read_text())[0]['binary']['package']
    jobs = [(ghidra['name'], ghidra['browser_download_url'], ghidra['size'], ghidra['digest'].split(':')[1]),
            (jdk['name'], jdk['link'], jdk['size'], jdk['checksum'])]
    with ThreadPoolExecutor(max_workers=2) as pool:
        for _ in pool.map(prepare, jobs):
            pass


if __name__ == '__main__':
    main()
