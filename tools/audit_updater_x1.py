"""Bounded static audit for the independently documented Rongyuan x1 wrapper.

Never executes vendor code or accesses HID. A matching source-control fixture
is required so a broken decoder cannot silently produce a negative result.
This is a heuristic search, not proof that no other encoded images exist.
"""
import hashlib
import json
from pathlib import Path
import re
import struct
import zlib

import pefile
from extract_ry_reference_firmware import PINNED

ROOT = Path(__file__).resolve().parents[1]
CONTROL_SHA = 'fe64df53410701b83ee61d14717036ea61247bdf7570ea16b4d903ec486ead3a'


def main():
    fixture = ROOT / 'research/vendor/monsgeek-akko-reference/m1v5-v408-x1.bin'
    data = fixture.read_bytes()
    if hashlib.sha256(data).hexdigest() != CONTROL_SHA:
        raise ValueError('Pinned x1 format fixture mismatch')
    decoder = zlib.decompressobj(-15)
    decoded = decoder.decompress(data[:1] + data[203:], 2 * 1024 * 1024)
    if len(decoded) != 155363 or decoded[0x5000:0x500d] != b'AT32F405 8KMK':
        raise ValueError('Control decode failed; no negative audit may be reported')
    control = {'source_sha256': CONTROL_SHA, 'decoded_bytes': len(decoded),
               'stream_eof': decoder.eof, 'target_compatibility': False}
    reports = []
    for (family, role), (sha, _, _, _) in PINNED.items():
        path = ROOT / 'research/vendor/panchip-reference' / family / f'{role}-ry_upgrade.exe'
        data = path.read_bytes()
        if hashlib.sha256(data).hexdigest() != sha:
            raise ValueError('Pinned updater mismatch')
        pe = pefile.PE(data=data)
        count, matches = 0, []
        for section in pe.sections:
            if section.Characteristics & 0x20000000:
                continue
            raw = data[section.PointerToRawData:section.PointerToRawData + section.SizeOfRawData]
            for match in re.finditer(b'\xec', raw):
                start = match.start()
                if start + 203 + 128 >= len(raw):
                    continue
                count += 1
                packed = raw[start:start + 1] + raw[start + 203:start + 203 + 1024 * 1024]
                decoder = zlib.decompressobj(-15)
                try:
                    head = decoder.decompress(packed, 2048)
                except zlib.error:
                    continue
                if len(head) < 32:
                    continue
                sp, pc = struct.unpack('<II', head[:8])
                vector = 0x20000000 <= sp <= 0x20080000 and pc & 1 and (
                    pc < 0x100000 or 0x08000000 <= pc < 0x08200000)
                marker = any(m in head for m in (b'PAN108OLED', b'AT32F405', b'CH32'))
                if vector or marker:
                    matches.append({'offset': section.PointerToRawData + start,
                                    'head': head[:32].hex(), 'vector': bool(vector), 'marker': marker})
        reports.append({'file': str(path.relative_to(ROOT)), 'sha256': sha,
                        'candidates_tested': count, 'matches': matches})
        print(f'{family}/{role}: {count} candidates, {len(matches)} matches', flush=True)
    result = {'positive_control': control, 'files': reports, 'hardware_access': False,
              'limitations': 'Only EC-start x1 with203-byte continuation, non-executable PE sections, 1MiB compressed/2KiB initial-output bounds and selected vector/marker heuristics. Control stream itself lacks a final DEFLATE EOF; its bytes are a pinned format fixture, not a verified target image.'}
    target = ROOT / 'research/local/mouse-firmware-20261021/updater-x1-format-audit.json'
    target.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
