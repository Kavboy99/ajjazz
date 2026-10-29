"""Read selected public PAN1080 ZDK 1.3.1 sources; no device access/execution.

The public SDK is a reference, NOT firmware for AJ159 APEX.
Uses the official download linked by https://wiki.panchip.com/ble/pan1080_series/.
"""
import hashlib
import json
from pathlib import Path
import urllib.request
import zipfile

import panchip_sdk_archive as sdk

URL = 'https://wiki.panchip.com/download/430/'
SIZE = 617905424
PREFIX = 'pan1080-zdk-v1.3.1/'
SOURCES = [
    '01_SDK/bootloader/mcuboot/boot/zephyr/usb_dfu.c',
    '01_SDK/bootloader/mcuboot/boot/zephyr/spi_dfu.c',
    '01_SDK/zephyr/samples_panchip/subsys/usb/panchip_dfu/src/usb_dfu.c',
    '03_MCU/mcu_samples/USB_DFU/src/usb_panchip.c',
    '03_MCU/mcu_samples/USB_DFU/src/descript.h',
    '04_DOC/docs-zdk/03_samples/solutions_hid/hs_mult_ms.html',
]


class WikiZip(sdk.RemoteZip):
    def __init__(self):
        # HEAD on the WordPress endpoint returns the landing-page headers.
        # GET Range includes the actual archive size; validate every response.
        self.size = SIZE
        self.pos = 0
        self.transferred = 0
        self.blocks = {}

    def fetch_block(self, index):
        start = index * 65536
        end = min(start + 65536, self.size) - 1
        # Endpoint has a measured off-by-one Content-Length. Request one extra
        # byte, retaining the exact block; only the archive's last comment byte
        # may be absent. ZIP validates the CRC of every extracted source.
        request_end = min(end + 1, self.size - 1)
        request = urllib.request.Request(URL, headers={'Range': f'bytes={start}-{request_end}'})
        with urllib.request.urlopen(request, timeout=25) as response:
            expected_range = f'bytes {start}-{request_end}/{self.size}'
            if response.status != 206 or response.headers.get('Content-Range') != expected_range:
                raise ValueError('Unexpected SDK archive range or changed archive size')
            data = response.read(end - start + 1)
        expected = end - start + 1
        if len(data) != expected and not (end == self.size - 1 and len(data) == expected - 1):
            raise ValueError('Truncated SDK archive block')
        return index, data


def main():
    out = Path(__file__).resolve().parents[1] / 'research/vendor/panchip-reference'
    remote = WikiZip()
    with zipfile.ZipFile(remote) as archive:
        entries = [{'path': item.filename, 'bytes': item.file_size, 'compressed': item.compress_size}
                   for item in archive.infolist()]
        (out / 'sdk-131-index.json').write_text(json.dumps(
            {'url': URL, 'archive_bytes': remote.size, 'entries': entries},
            ensure_ascii=False, indent=2), encoding='utf-8')
        proofs = []
        for relative in SOURCES:
            name = PREFIX + relative
            info = archive.getinfo(name)
            if info.file_size > 1024 * 1024:
                raise ValueError('Unexpected source size')
            data = archive.read(info)
            target = out / 'sdk-131' / Path(*name.split('/'))
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            proof = {'path': name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
            proofs.append(proof)
            print(json.dumps(proof), flush=True)
        (out / 'sdk-131-extracted.json').write_text(json.dumps(
            {'url': URL, 'range_bytes': remote.transferred, 'sources': proofs}, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
