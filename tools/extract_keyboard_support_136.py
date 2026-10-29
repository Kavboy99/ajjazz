"""Extract the pinned AK820MAX 1.36 support package without executing it.

No USB/HID, firmware writes or downloaded code execution. Verify every Intel HEX
record and compare its sparse contents against the updater's embedded user image.
The included ZIP is a generic updater template and can contain a different image.
"""
from pathlib import Path, PurePosixPath
import hashlib
import io
import json
import struct
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.tools/py-research'))
import pefile

EXPECTED = 'edd4914bdd93bc3d557173e554efd40e7b531f5d6275b5bc8c8c6f7f4aa570d4'
OUT = ROOT / 'research/vendor/ak820max-support-20261024'


def digest(b):
    return hashlib.sha256(b).hexdigest()


def save(name, data):
    path = OUT / name
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists() and path.read_bytes() != data:
        raise ValueError(f'Refusing to replace different evidence: {name}')
    path.write_bytes(data)


def main():
    source = Path(sys.argv[1]) if len(sys.argv) == 2 else next(Path.home().joinpath('Downloads').glob('*1.36*exe'))
    data = source.read_bytes()
    if digest(data) != EXPECTED:
        raise ValueError('Support updater SHA256 mismatch')
    save('support-updater.exe', data)
    pe = pefile.PE(data=data)
    blobs = {}
    for kind in pe.DIRECTORY_ENTRY_RESOURCE.entries:
        if kind.id == 10:
            for entry in kind.directory.entries:
                if len(entry.directory.entries) != 1:
                    raise ValueError('Ambiguous resource language')
                r = entry.directory.entries[0].data.struct
                blobs[entry.id] = pe.get_data(r.OffsetToData, r.Size)
    for number, blob in blobs.items():
        save(f'resources/{number}.bin', blob)
    image = blobs[4000]
    if len(image) != 0x7e000:
        raise ValueError('Unexpected user image size')
    save('user-1.36.bin', image)
    word_checksum = sum(struct.unpack('<%dH' % (len(image)//2), image)) & 65535
    if f'{word_checksum:04X}' != blobs[4014].decode('utf-16le'):
        raise ValueError('Active image does not match advertised updater checksum')
    archive = zipfile.ZipFile(io.BytesIO(blobs[4011]))
    if sum(x.file_size for x in archive.infolist()) > 4 * 1024 * 1024:
        raise ValueError('Unexpected ZIP expansion')
    members = {}
    for entry in archive.infolist():
        path = PurePosixPath(entry.filename.replace('\\', '/'))
        if path.is_absolute() or '..' in path.parts or ':' in entry.filename:
            raise ValueError('Unsafe ZIP member')
        b = archive.read(entry)
        save('embedded/' + str(path), b)
        members[str(path)] = {'bytes': len(b), 'sha256': digest(b)}
    text = archive.read('Settings/FWFiles/SN34F280_User.hex').decode('ascii')
    base = 0
    memory = {}
    ended = False
    for line in text.splitlines():
        if ended or not line.startswith(':'):
            raise ValueError('Invalid Intel HEX framing')
        b = bytes.fromhex(line[1:])
        if len(b) != b[0] + 5 or sum(b) & 255:
            raise ValueError('Invalid Intel HEX record checksum/length')
        offset = int.from_bytes(b[1:3], 'big')
        if b[3] == 0:
            for i, value in enumerate(b[4:-1]):
                address = base + offset + i
                if address in memory and memory[address] != value:
                    raise ValueError('Conflicting Intel HEX ranges')
                memory[address] = value
        elif b[3] == 1:
            ended = True
        elif b[3] == 4 and b[0] == 2:
            base = int.from_bytes(b[4:6], 'big') << 16
        elif b[3] != 5:
            raise ValueError(f'Unsupported Intel HEX record {b[3]}')
    if not ended or not memory:
        raise ValueError('Incomplete Intel HEX')
    # Establish the address relationship from every byte, not the file name.
    offsets = [start for start in (0, 0x2000, 0x200000, 0x202000)
               if all(0 <= a-start < len(image) and image[a-start] == v for a, v in memory.items())]
    if len(offsets) > 1:
        raise ValueError(f'HEX/raw relationship ambiguous: {offsets}')
    if min(memory) < 0 or max(memory) >= len(image):
        raise ValueError('Template HEX outside expected user-image range')
    template = bytearray([255] * len(image))
    for address, value in memory.items():
        template[address] = value
    save('template-user-image.bin', template)
    strings = {str(n): b.decode('utf-16le') for n, b in blobs.items()
               if n in (4007, 4008, 4009, 4010, 4013, 4014, 4016)}
    manifest = {
        'source': str(source), 'updater_sha256': EXPECTED,
        'image_sha256': digest(image), 'image_bytes': len(image),
        'active_image_word_checksum': f'{word_checksum:04X}',
        'active_identity': {'vid': hex(struct.unpack_from('<H',image,0x9004)[0]),
                            'pid': hex(struct.unpack_from('<H',image,0x9006)[0]),
                            'version_raw': hex(struct.unpack_from('<H',image,0x9008)[0]),
                            'manufacturer': hex(struct.unpack_from('<H',image,0x900c)[0]),
                            'product': hex(struct.unpack_from('<H',image,0x900e)[0])},
        'initial_vectors': [hex(v) for v in struct.unpack('<16I', image[:64])],
        'identity_block_0x9000': image[0x9000:0x9040].hex(),
        'embedded_strings': strings, 'zip_members': members,
        'hex_verified': True, 'hex_byte_count': len(memory),
        'hex_address_min': hex(min(memory)), 'hex_address_max': hex(max(memory)),
        'raw_image_hex_base': hex(offsets[0]) if offsets else None,
        'template_matches_active_image': bool(offsets),
        'template_image_sha256': digest(template),
        'template_identity_block_0x9000': template[0x9000:0x9030].hex(),
        'hardware_access': False, 'updater_executed': False,
    }
    # Manifest gains analysis fields as the extractor evolves; payloads above
    # remain immutable and are individually pinned.
    (OUT/'extraction.json').write_text(json.dumps(manifest, indent=2, ensure_ascii=False)+'\n',encoding='utf-8')
    print(json.dumps({k: v for k, v in manifest.items() if k not in ('source', 'zip_members')}, indent=2))


if __name__ == '__main__':
    main()
