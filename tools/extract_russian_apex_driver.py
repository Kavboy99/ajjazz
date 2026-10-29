"""Read one exact member of the published Russian AJ159 APEX driver ZIP.

Static extraction only. Never runs installers, opens HID or follows instructions
inside the package. HTTP source is recorded, not asserted cryptographically
authentic; ZIP CRC and pinned member geometry detect transfer corruption.
"""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import struct
import urllib.request
import zipfile
import zlib

import panchip_sdk_archive as sdk

URL = 'http://www.ajazz.ru/upload/drayvera/AJAZZ_AJ159%20APEX_Tripe%20Mode_Win%20System_Mouse%20Driver.zip'
SIZE = 238496662
NAME = 'AJAZZ_setup_2.1.88(WIN20250107).exe'
OUT = Path(__file__).resolve().parents[1] / 'research/vendor/panchip-reference/russian-apex-2188'


def main():
    sdk.URL = URL
    remote = sdk.RemoteZip()
    if remote.size != SIZE:
        raise ValueError('Archive changed size')
    with zipfile.ZipFile(remote) as archive:
        matches = [i for i in archive.infolist() if Path(i.filename).name == NAME]
        if len(matches) != 1:
            raise ValueError('Unexpected installer members')
        item = matches[0]
    if item.file_size != 72043568 or item.compress_size != 71953127 or item.compress_type != 8:
        raise ValueError('Installer geometry changed')

    def read_range(start, end):
        request = urllib.request.Request(URL, headers={'Range': f'bytes={start}-{end - 1}'})
        with urllib.request.urlopen(request, timeout=20) as response:
            if (response.status != 206 or response.headers.get('Content-Range') != f'bytes {start}-{end - 1}/{SIZE}'
                    or response.headers.get('ETag') != remote.etag):
                raise ValueError('Archive changed or incorrect HTTP Range')
            result = response.read(end - start + 1)
        if len(result) != end - start:
            raise ValueError('Truncated/oversize HTTP response')
        return result

    header = read_range(item.header_offset, item.header_offset + 30)
    if header[:4] != b'PK\x03\x04':
        raise ValueError('Invalid local ZIP header')
    name_len, extra_len = struct.unpack_from('<HH', header, 26)
    start = item.header_offset + 30 + name_len + extra_len
    end = start + item.compress_size
    if end > SIZE:
        raise ValueError('ZIP member outside archive')
    ranges = [(offset, min(offset + 1024 * 1024, end)) for offset in range(start, end, 1024 * 1024)]
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        packed = b''.join(pool.map(lambda pair: read_range(*pair), ranges))
    decoder = zlib.decompressobj(-15)
    data = decoder.decompress(packed, item.file_size + 1)
    if (len(data) != item.file_size or not decoder.eof or decoder.unused_data
            or zlib.crc32(data) != item.CRC or data[:2] != b'MZ'):
        raise ValueError('Invalid installer size, DEFLATE, CRC or PE header')
    OUT.mkdir(exist_ok=True)
    digest = hashlib.sha256(data).hexdigest()
    target = OUT / NAME
    if target.exists() and hashlib.sha256(target.read_bytes()).hexdigest() != digest:
        raise ValueError('Existing analysis file differs')
    target.write_bytes(data)
    report = {'url': URL, 'archive_bytes': SIZE, 'etag': remote.etag, 'member': item.filename,
              'bytes': len(data), 'sha256': digest, 'crc32': f'{item.CRC:08x}',
              'vendor_code_executed': False, 'hardware_access': False,
              'source_transport': 'public HTTP; authenticity not cryptographically established'}
    (OUT / 'provenance.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(report, ensure_ascii=True, indent=2))


if __name__ == '__main__':
    main()
