"""Census of validated, uncompressed MCUboot components in pinned updaters.

Reads PE bytes as data. Does not execute them, access devices or claim that
compressed/encrypted/other-format payloads cannot exist.
"""
import hashlib
import json
from pathlib import Path
import struct

from extract_ry_reference_firmware import PINNED, inspect_component


def main():
    root = Path(__file__).resolve().parents[1]
    report = {'device_access': False, 'vendor_code_executed': False,
              'exact_target_firmware_obtained': False,
              'scope': 'Uncompressed MCUboot headers with valid embedded SHA256 only',
              'updaters': []}
    for (family, component), (expected, image_start, image_length, _) in PINNED.items():
        source = root / 'research/vendor/panchip-reference' / family / f'{component}-ry_upgrade.exe'
        data = source.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if digest != expected:
            raise ValueError(f'Pinned updater hash mismatch: {source.name}')
        hits, valid, rejected = [], [], []
        cursor = 0
        while True:
            cursor = data.find(struct.pack('<I', 0x96F3B83D), cursor)
            if cursor < 0:
                break
            hits.append(cursor)
            try:
                part = inspect_component(data, cursor)
                part['within_previously_extracted_image'] = (
                    image_start <= cursor < part['end_offset'] <= image_start + image_length)
                valid.append(part)
            except ValueError as error:
                rejected.append({'offset': cursor, 'reason': str(error)})
            cursor += 1
        report['updaters'].append({
            'family': family, 'component': component, 'source_sha256': digest,
            'source_bytes': len(data), 'magic_occurrences': hits,
            'validated_components': valid, 'rejected_candidates': rejected,
        })
    target = root / 'research/local/mouse-firmware-20261021/updater-image-inventory.json'
    target.write_text(json.dumps(report, indent=2), encoding='utf-8')
    for entry in report['updaters']:
        parts = entry['validated_components']
        print(entry['family'], entry['component'],
              f"{len(parts)} hash-validated MCUboot components; "
              f"{sum(not p['within_previously_extracted_image'] for p in parts)} outside extracted image")
    print(target)


if __name__ == '__main__':
    main()
