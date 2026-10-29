"""Inspect official DFU bytecode/resources statically. Never import vendor code.

Requires the separately downloaded official package member pinned below.
No USB, firmware writes or permission-changing operations.
"""
import hashlib
import io
import json
import marshal
from pathlib import Path
import struct
import zlib

from xdis import Bytecode, get_opcode_module
from xdis.unmarshal import load_code
from xdis.version_info import PythonImplementation

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / 'research/vendor/panchip-reference/sdk-131-tool-directory'
SHA = '49f59c1e5fa1597ae049401f75ed6328168910235f938622f9d48fc2327e1c0f'
MODULES = ('dfu_usb_com', 'dfu_program', 'dfu_trans_dev_program', 'dfu_face_dlg')


def inflate(data, limit):
    decoder = zlib.decompressobj()
    result = decoder.decompress(data, limit + 1)
    if not decoder.eof or len(result) > limit:
        raise ValueError('Truncated/oversize resource')
    return result


def main():
    data = (BASE / 'PanchipDFUTool V0.2.001.exe').read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Pinned DFU executable mismatch')
    cookie = data.rfind(b'MEI\x0c\x0b\x0a\x0b\x0e')
    _, length, table, size, pyver, _ = struct.unpack('!8sIIII64s', data[cookie:cookie + 88])
    if pyver != 308:
        raise ValueError('Unsupported bytecode version')
    origin = cookie + 88 - length
    cursor, end = origin + table, origin + table + size
    entries = []
    while cursor < end:
        length, pos, packed, unpacked, compressed, kind = struct.unpack('!IIIIBc', data[cursor:cursor + 18])
        if length < 18 or cursor + length > end or origin + pos + packed > cookie:
            raise ValueError('Invalid TOC entry')
        name = data[cursor + 18:cursor + length].rstrip(b'\0').decode()
        entries.append({'name': name, 'offset': origin + pos, 'packed': packed,
                        'bytes': unpacked, 'compressed': compressed})
        cursor += length

    def resource(name):
        row = next(r for r in entries if r['name'] == name)
        packed = data[row['offset']:row['offset'] + row['packed']]
        value = inflate(packed, 8 * 1024 * 1024) if row['compressed'] else packed
        if len(value) != row['bytes']:
            raise ValueError('Resource size mismatch')
        return value

    out = BASE / 'dfu-static'
    out.mkdir(exist_ok=True)
    pyz = resource('PYZ.pyz')
    if pyz[:4] != b'PYZ\0':
        raise ValueError('Invalid PYZ')
    toc = dict(marshal.loads(pyz[struct.unpack('!I', pyz[8:12])[0]:]))
    magic = int.from_bytes(pyz[4:6], 'little')
    opcode = get_opcode_module((3, 8), PythonImplementation.CPython)
    reports = []
    for name in MODULES:
        _, pos, packed = toc[name]
        raw = inflate(pyz[pos:pos + packed], 1024 * 1024)
        code = load_code(io.BytesIO(raw), magic)
        functions, blocks = [], []

        def walk(obj):
            functions.append({'name': obj.co_name, 'references': list(obj.co_names)})
            blocks.append('\n# ' + obj.co_name + '\n' + Bytecode(obj, opcode).dis())
            for item in obj.co_consts:
                if hasattr(item, 'co_consts'):
                    walk(item)

        walk(code)
        (out / (name + '.dis.txt')).write_text('\n'.join(blocks), encoding='utf-8')
        reports.append({'module': name, 'marshal_sha256': hashlib.sha256(raw).hexdigest(),
                        'functions': functions})
    template = resource('res\\file\\PanchipDfu.exe')
    markers = {name: template.count(marker) for name, marker in
               [('mcuboot_magic', bytes.fromhex('3db8f396')), ('PAN108OLED', b'PAN108OLED')]}
    import pefile
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
    pe = pefile.PE(data=template)
    decoder = Cs(CS_ARCH_X86, CS_MODE_32)
    marker_locations = []
    cursor = 0
    while (cursor := template.find(bytes.fromhex('3db8f396'), cursor)) >= 0:
        section = next((s for s in pe.sections
                        if s.PointerToRawData <= cursor < s.PointerToRawData + s.SizeOfRawData), None)
        row = {'offset': cursor, 'section': section.Name.rstrip(b'\0').decode() if section else None}
        if cursor > 0:
            instruction = next(decoder.disasm(template[cursor - 1:cursor + 4], cursor - 1), None)
            if instruction and instruction.size == 5:
                row['preceding_instruction'] = instruction.mnemonic + ' ' + instruction.op_str
                row['comparison_constant'] = instruction.mnemonic == 'cmp' and instruction.op_str == 'eax, 0x96f3b83d'
        marker_locations.append(row)
        cursor += 4
    result = {'source_sha256': SHA, 'vendor_code_executed': False, 'hardware_access': False,
              'resource_names': [r['name'] for r in entries if r['name'].startswith('res\\file\\')],
              'template_exe_sha256': hashlib.sha256(template).hexdigest(),
              'template_image_markers': markers, 'template_marker_locations': marker_locations,
              'modules': reports,
              'limitations': 'Command builders and named resources inspected; no proof of target MCU, recovery, arbitrary read capability or image compatibility. Missing markers do not exclude other formats.'}
    (out / 'audit.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(f'Inspected {len(reports)} protocol/program modules; template markers: {markers}')


if __name__ == '__main__':
    main()
