"""Extract embedded reference images from four statically inspected updaters.

Never executes vendor code or accesses hardware. Offsets AND slice lengths come
from x86 memcpy calls in the pinned executables. These images were distributed
for AJ179 APEX and AJ159 PRO, not as an AJ159 APEX recovery package.
"""
import hashlib
import json
from pathlib import Path
import struct
from collections import defaultdict


PINNED = {
    ('aj179-reference', 'mouse'): ('a5b9f9b2383b4de8aee6d589ac1114984d54e4b3469b8423c8c4a288dd420b12',
              0x11940EF, 0x59984, 0x73B8CC),
    ('aj179-reference', 'dock'): ('9fa30ca90a927ad5827bd9a4ee0b15dc6ffe40b5fb56fbb6b29bcc81ae31345c',
             0x1196F27, 0x3BFF4, 0x6AF14E),
    ('aj159-pro-reference', 'mouse'): ('e02ff94287d47a83389d1b034f09da9a67dcd03bd7aa4f67474f9e01af9aeff2',
             0x119B1C3, 0x591D0, 0x704D2C),
    ('aj159-pro-reference', 'dock'): ('bdbd9d55a7d1d4843994c3143799de079597b72472bd65e1d86991c8de165e98',
             0x1196F27, 0x3A8A0, 0x6AF14E),
}


def inspect_component(data, offset):
    if offset < 0 or offset + 32 > len(data):
        raise ValueError('Truncated MCUboot header')
    magic, load, header_size, protected, size, flags = struct.unpack_from('<IIHHII', data, offset)
    if magic != 0x96F3B83D or header_size != 0x200 or protected:
        raise ValueError('Unexpected MCUboot component header')
    end = offset + header_size + size
    if end + 4 > len(data):
        raise ValueError('Component exceeds image')
    tlv_magic, tlv_size = struct.unpack_from('<HH', data, end)
    if tlv_magic != 0x6907 or tlv_size < 4 or end + tlv_size > len(data):
        raise ValueError('Invalid MCUboot TLV range')
    cursor, entries = end + 4, []
    while cursor < end + tlv_size:
        if cursor + 4 > end + tlv_size:
            raise ValueError('Truncated MCUboot TLV header')
        kind, reserved, length = struct.unpack_from('<BBH', data, cursor)
        cursor += 4
        if cursor + length > end + tlv_size or reserved:
            raise ValueError('Invalid MCUboot TLV entry')
        value = data[cursor:cursor + length]
        if kind == 0x10 and (length != 32 or hashlib.sha256(data[offset:end]).digest() != value):
            raise ValueError('Embedded firmware SHA256 mismatch')
        entries.append({'type': kind, 'bytes': length,
                        'value_hex': value.hex() if length <= 32 else None})
        cursor += length
    if cursor != end + tlv_size or not any(t['type'] == 0x10 for t in entries):
        raise ValueError('Missing or malformed firmware hash')
    return {'image_offset': offset, 'load_address': load, 'header_bytes': header_size,
            'payload_bytes': size, 'flags': flags, 'end_offset': cursor,
            'sha256_verified': True, 'signature_verified': False, 'tlvs': entries}


def verify_signatures(data, components):
    from cryptography.hazmat.primitives.serialization import load_der_public_key
    from cryptography.hazmat.primitives.asymmetric import padding, utils
    from cryptography.hazmat.primitives import hashes

    # This DER key occurs in the inspected reference bootloader. Verifying it
    # establishes internal consistency, NOT compatibility or vendor provenance.
    pattern = bytes.fromhex('3082010a0282010100')
    offset = data[:0x10000].find(pattern)
    if offset < 0:
        raise ValueError('Reference boot public key absent')
    key_size = 4 + int.from_bytes(data[offset + 2:offset + 4], 'big')
    key_bytes = data[offset:offset + key_size]
    digest = hashlib.sha256(key_bytes).digest()
    if digest.hex() != 'fc5701dc6135e1323847bdc40f04d2e5bee5833b23c29f93593d00018cfa9994':
        raise ValueError('Unexpected reference boot public key')
    key = load_der_public_key(key_bytes)
    for component in components:
        start = component['image_offset']
        end = start + component['header_bytes'] + component['payload_bytes']
        cursor, tlvs = end + 4, {}
        while cursor < component['end_offset']:
            kind, _, length = struct.unpack_from('<BBH', data, cursor)
            if kind in tlvs:
                raise ValueError('Duplicate MCUboot TLV')
            tlvs[kind] = data[cursor + 4:cursor + 4 + length]
            cursor += 4 + length
        if tlvs.get(1) != digest or len(tlvs.get(0x20, b'')) != 256:
            raise ValueError('Firmware key hash/signature missing or mismatched')
        key.verify(tlvs[0x20], hashlib.sha256(data[start:end]).digest(),
                   padding.PSS(mgf=padding.MGF1(hashes.SHA256()), salt_length=32),
                   utils.Prehashed(hashes.SHA256()))
        component.update(signature_verified=True,
                         signature_algorithm='RSA-2048/PSS/SHA-256/salt32',
                         key_source='public DER embedded in the same reference bootloader')


def main():
    root = Path(__file__).resolve().parents[1] / 'research/vendor/panchip-reference'
    output = defaultdict(list)
    for (family, name), (source_hash, offset, length, memcpy_va) in PINNED.items():
        base = root / family
        source = base / f'{name}-ry_upgrade.exe'
        content = source.read_bytes()
        if hashlib.sha256(content).hexdigest() != source_hash:
            raise ValueError('Updater hash differs from inspected version')
        data = content[offset:offset + length]
        if len(data) != length:
            raise ValueError('Truncated embedded image')
        components = [inspect_component(data, 0x10000), inspect_component(data, 0x30000)]
        if components[-1]['end_offset'] != length:
            raise ValueError('Embedded slice length does not match final TLV boundary')
        verify_signatures(data, components)
        target = base / f'{name}-MV303-DV233-reference.bin'
        target.write_bytes(data)
        output[family].append({'component': name, 'source': str(source), 'source_sha256': source_hash,
                       'source_offset': offset, 'memcpy_call_arguments_va': memcpy_va,
                       'bytes': length, 'sha256': hashlib.sha256(data).hexdigest(),
                       'file': str(target), 'components': components,
                       'compatible_with_attached_hardware': None,
                       'programming_performed': False})
    for family, entries in output.items():
        (root / family / 'embedded-firmware.json').write_text(json.dumps(entries, indent=2), encoding='utf-8')
        for entry in entries:
            print(json.dumps({'family': family, 'component': entry['component'],
                              'bytes': entry['bytes'], 'sha256': entry['sha256'],
                              'verified_signatures': len(entry['components'])}))


if __name__ == '__main__':
    main()
