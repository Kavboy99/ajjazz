"""Fetch the published MouseDeviceTool archive for static analysis only.

No vendor code is executed and no device is opened. ZIP size/member identity
are pinned to the separately inspected public directory. The PHP endpoint's
exclusive range-end behavior is validated before assembling the archive.
"""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import io
import json
from pathlib import Path
import urllib.request
import zipfile

URL = 'https://wiki.panchip.com/download/3090/'
SIZE = 34340367
MEMBER = 'MouseDeviceTool V0.0.007.exe'
MEMBER_SIZE = 34446979
BLOCK = 65536
OUT = Path(__file__).resolve().parents[1] / 'research/vendor/panchip-reference/sdk-131-tool-directory'


def fetch(start):
    end = min(start + BLOCK, SIZE)
    request = urllib.request.Request(URL, headers={'Range': f'bytes={start}-{end}'})
    with urllib.request.urlopen(request, timeout=25) as response:
        if response.status != 206 or response.headers.get('Content-Range') != f'bytes {start}-{end}/{SIZE}':
            raise ValueError('Unexpected archive range or changed size')
        data = response.read(end - start)
    if len(data) != end - start:
        raise ValueError('Changed PHP range behavior or truncated archive')
    return data


def main():
    with ThreadPoolExecutor(max_workers=6) as pool:
        data = b''.join(pool.map(fetch, range(0, SIZE, BLOCK)))
    if len(data) != SIZE:
        raise ValueError('Invalid assembled ZIP size')
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        if archive.namelist() != [MEMBER] or archive.getinfo(MEMBER).file_size != MEMBER_SIZE:
            raise ValueError('Published package changed')
        executable = archive.read(MEMBER)  # zipfile validates the member CRC.
    if executable[:2] != b'MZ':
        raise ValueError('Not a PE executable')
    target = OUT / MEMBER
    digest = hashlib.sha256(executable).hexdigest()
    if target.exists() and hashlib.sha256(target.read_bytes()).hexdigest() != digest:
        raise ValueError('Existing analysis file differs; preserve both versions manually')
    target.write_bytes(executable)
    report = {'url': URL, 'archive_bytes': SIZE, 'archive_sha256': hashlib.sha256(data).hexdigest(),
              'archive_member': MEMBER, 'bytes': len(executable), 'sha256': digest,
              'vendor_code_executed': False, 'hardware_access': False}
    (OUT / 'mouse-device-exe-provenance.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
