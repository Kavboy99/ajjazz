"""Reproduce static lighting RE for one explicitly incompatible candidate.

No HID, network, executable loading, emulation, patching or flashing. Addresses
are accepted only for the pinned SHA256. A different image must be analyzed
again; matching strings, names or manufacturer alone do not establish identity.
"""
from pathlib import Path
import hashlib
import json
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.tools/py-research'))
from capstone import Cs, CS_ARCH_ARM, CS_MODE_THUMB, CS_MODE_LITTLE_ENDIAN, CS_OP_MEM, CS_OP_IMM

EXPECTED = 'ef67c8a05093961b799eebc39804b89707459c72477c6f26bf25e946b5810850'


def analyze(data):
    digest = hashlib.sha256(data).hexdigest()
    if digest != EXPECTED:
        raise ValueError('Unknown image: refusing to reuse candidate addresses')
    dis = Cs(CS_ARCH_ARM, CS_MODE_THUMB | CS_MODE_LITTLE_ENDIAN)
    dis.detail = True
    def instruction(address):
        return next(dis.disasm(data[address:address + 4], address, count=1))
    def table(address, count):
        op = instruction(address)
        assert op.mnemonic == 'tbb' and op.op_str == '[pc, r0]'
        base = address + 4
        result = []
        for index, value in enumerate(data[base:base + count]):
            target = base + 2 * value
            op = instruction(target)
            handler = op.operands[0].imm if op.mnemonic == 'bl' and op.operands[0].type == CS_OP_IMM else None
            result.append({'index': index, 'branch_address': hex(target),
                           'handler': hex(handler) if handler is not None else None,
                           'branch_instruction': f'{op.mnemonic} {op.op_str}'})
        return result
    init = table(0xCDCA, 25)
    tick = table(0x651A, 31)
    assert all(row['handler'] is None for row in init[20:24])
    assert init[24]['handler'] == '0x66e8'
    assert init[11]['handler'] == '0xbf0c' and tick[10]['handler'] == '0xbe84'
    info = data[0x9000:0x9030]
    vid, pid = struct.unpack_from('<HH', info, 4)
    manufacturer, product = struct.unpack_from('<HH', info, 12)
    assert (vid, pid, manufacturer, product) == (0x0C45, 0x8033, 0x0166, 0)
    version = f'{info[9]}.{(info[8] >> 4) * 10 + (info[8] & 15):02}'
    assert version == '1.04'
    ranges = {
        'load_lighting_config': (0x8EC, 0x9BE),
        'write_dispatch_and_reload': (0xD40E, 0xD526),
        'init_dispatch_prefix': (0xCDA8, 0xCDCE),
        'init_dispatch_branches': (0xCDE8, 0xCE6C),
        'tick_dispatch_prefix': (0x64FE, 0x651E),
        'tick_dispatch_branches': (0x653E, 0x6618),
        'mode11_init': (0xBF0C, 0xBFCA),
        'runtime10_tick': (0xBE84, 0xBF0C),
        'flash_page_update_prefix': (0x380C, 0x387C),
    }
    lines = []
    for name, (start, end) in ranges.items():
        lines.append(f'\n; {name} [{start:#x},{end:#x})')
        for op in dis.disasm(data[start:end], start):
            note = ''
            if op.mnemonic == 'ldr' and len(op.operands) > 1 and op.operands[1].type == CS_OP_MEM and op.reg_name(op.operands[1].mem.base) == 'pc':
                address = ((op.address + 4) & ~3) + op.operands[1].mem.disp
                note = f' ; literal[{address:#x}]={struct.unpack_from("<I", data, address)[0]:#x}'
            lines.append(f'{op.address:08x}: {op.bytes.hex():10} {op.mnemonic:7} {op.op_str}{note}')
    report = {
        'schema': 1, 'sha256': digest, 'file_bytes': len(data), 'offline_only': True,
        'candidate_identity': {'vid': hex(vid), 'pid': hex(pid), 'version': version,
                               'manufacturer': hex(manufacturer), 'product': hex(product)},
        'target_identity': {'vid': '0xc45', 'pid': '0x80a0', 'version': '1.30',
                           'manufacturer': '0x166', 'product': '0x1004'},
        'compatible_with_target': False, 'patch_generated': False, 'flashable': False,
        'mcu_identity_confirmed_from_this_analysis': False,
        'config_address_in_candidate': '0x9600', 'loaded_config_ram': '0x200002ea',
        'selected_init_index_ram': '0x200002a4', 'selected_tick_index_ram': '0x200002a1',
        'rgb_planes_in_candidate': ['0x20002bec', '0x20002c6a', '0x20002ce8'],
        'intensity_buffer_in_candidate': '0x20002d66',
        'init_table': init, 'tick_table': tick,
        'case_20_to_23': 'No BL handler in init table; this is not an implemented extension API',
        'write_path': '0x23 -> 0x9600 through 0x380c page update -> 0x8ec reload -> 0xcda8 init dispatch',
        'mapping_warning': 'Wire effect number, init index and tick index are different namespaces',
        'required_for_target': ['Exact 80A0/1.30 image or verified readout',
                                'Target controller/board identity and recovery path',
                                'Target RGB buffers, timer and dispatcher addresses',
                                'Flash map separating code, settings, calibration and radio data'],
    }
    return report, '\n'.join(lines) + '\n'


def main():
    path = ROOT / 'research/vendor/revision-hunt/candidate-SN34F280-app.bin'
    report, listing = analyze(path.read_bytes())
    # Rejecting a different image is a key property of this tool.
    try:
        analyze(b'\0' + path.read_bytes()[1:])
    except ValueError:
        pass
    else:
        raise AssertionError('Changed input was accepted')
    output = ROOT / 'research/local'
    (output / 'candidate-lighting-dispatch.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    (output / 'candidate-lighting-dispatch.asm').write_text(listing, encoding='utf-8')
    print('Offline analysis reproduced: 25 init branches, 31 tick branches; input hash and identity checked.')
    print('Candidate is 8033/1.04/product0000, incompatible with target 80A0/1.30/product1004. No patch or device access.')


if __name__ == '__main__': main()
