"""Bounded read-only HTTP ZIP index for the official PAN1080 SDK.

No SDK binaries or scripts are executed and no devices are accessed.
--extract accepts exact archive paths from the saved index; no extractall.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import urllib.request
import zipfile
from concurrent.futures import ThreadPoolExecutor

URL = 'https://wiki.panchip.com/wp-content/uploads/dlm_uploads/2023/04/pan1080-dk-v0.8.0.zip'
OUT = Path(__file__).resolve().parents[1] / 'research/vendor/panchip-reference'


class RemoteZip(io.RawIOBase):
    def __init__(self):
        with urllib.request.urlopen(urllib.request.Request(URL, method='HEAD'), timeout=20) as response:
            self.size = int(response.headers['Content-Length'])
            self.etag = response.headers.get('ETag')
        self.pos = 0
        self.transferred = 0
        self.blocks = {}

    def seekable(self):
        return True

    def tell(self):
        return self.pos

    def seek(self, offset, whence=0):
        self.pos = offset if whence == 0 else self.pos + offset if whence == 1 else self.size + offset
        if not 0 <= self.pos <= self.size:
            raise ValueError('Invalid archive seek')
        return self.pos

    def read(self, size=-1):
        if size < 0:
            size = self.size - self.pos
        size = min(size, self.size - self.pos)
        if not size:
            return b''
        if size > 16 * 1024 * 1024 or self.transferred + size > 64 * 1024 * 1024:
            raise ValueError('Remote ZIP read limit exceeded')
        block_size = 65536
        needed = list(range(self.pos // block_size, (self.pos + size - 1) // block_size + 1))
        missing = [index for index in needed if index not in self.blocks]
        if self.transferred + len(missing) * block_size > 64 * 1024 * 1024:
            raise ValueError('Remote ZIP transfer budget exceeded')
        with ThreadPoolExecutor(max_workers=6) as pool:
            for index, data in pool.map(self.fetch_block, missing):
                self.blocks[index] = data
                self.transferred += len(data)
        data = b''.join(self.blocks[index] for index in needed)
        offset = self.pos % block_size
        data = data[offset:offset + size]
        self.pos += size
        return data

    def fetch_block(self, index):
        start = index * 65536
        end = min(start + 65536, self.size) - 1
        req = urllib.request.Request(URL, headers={'Range': f'bytes={start}-{end}'})
        with urllib.request.urlopen(req, timeout=25) as response:
            if response.status != 206 or response.headers.get('Content-Range') != f'bytes {start}-{end}/{self.size}':
                raise ValueError('Server did not honor bounded Range request')
            if response.headers.get('ETag') != self.etag:
                raise ValueError('Archive changed during inspection')
            data = response.read(end - start + 2)
        if len(data) != end - start + 1:
            raise ValueError('Truncated/oversize Range response')
        return index, data


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--extract', action='append', default=[])
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    remote = RemoteZip()
    with zipfile.ZipFile(remote) as archive:
        entries = [{'path': item.filename, 'bytes': item.file_size, 'compressed': item.compress_size}
                   for item in archive.infolist()]
        (OUT / 'sdk-080-index.json').write_text(json.dumps({'url': URL, 'archive_bytes': remote.size, 'etag': remote.etag, 'entries': entries}, ensure_ascii=False, indent=2), encoding='utf-8')
        if not args.extract:
            relevant = [e for e in entries if any(t in e['path'].lower() for t in ('dfu', 'boot_serial', 'bootutil', 'usb_hid', 'flash_map', 'partition', 'usb_upgrade'))]
            print(json.dumps({'entries': len(entries), 'relevant': relevant[:150], 'range_bytes': remote.transferred}, ensure_ascii=False, indent=2))
        for name in args.extract:
            path = PurePosixPath(name)
            if path.is_absolute() or '..' in path.parts or ':' in name or '\\' in name:
                raise ValueError('Unsafe archive path')
            info = archive.getinfo(name)
            if info.is_dir() or info.file_size > 8 * 1024 * 1024 or path.suffix.lower() in ('.exe', '.dll', '.bat', '.ps1', '.com'):
                raise ValueError('Not a bounded research source')
            data = archive.read(info)
            target = OUT / 'sdk-080' / Path(*path.parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            print(json.dumps({'path': name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}))


if __name__ == '__main__':
    main()
