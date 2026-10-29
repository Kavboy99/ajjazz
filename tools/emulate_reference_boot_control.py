"""Execute selected reference USB setup handlers, without physical USB.

The USB register page is synthetic mapped memory. This checks fixed descriptor
and report-buffer selection, not the complete USB stack or physical bootloader.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_XPSR, UC_ARM_REG_R0, UC_ARM_REG_R1

ROOT = Path(__file__).resolve().parents[1]
SHA = '2c4e52961ded6f3043b6b4437f6093a880364c8f933f49108a55d924b98483ba'
STOP, REQUEST, TRANSFER = 0xE0000, 0x2000B000, 0x20001C98


def run(data, handler, request):
    if handler not in (0x1624, 0x1378) or len(request) != 8:
        raise ValueError('Only audited setup handlers are accepted')
    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
    cpu.mem_map(0, 0x100000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(0, data)
    cpu.mem_map(0x20000000, 0x10000, UC_PROT_READ | UC_PROT_WRITE | UC_PROT_EXEC)
    cpu.mem_map(0x400A0000, 0x1000, UC_PROT_READ | UC_PROT_WRITE)
    cpu.mem_write(REQUEST, request)
    cpu.reg_write(UC_ARM_REG_R0, REQUEST)
    cpu.reg_write(UC_ARM_REG_R1, REQUEST)
    cpu.reg_write(UC_ARM_REG_SP, 0x2000F000)
    cpu.reg_write(UC_ARM_REG_LR, STOP | 1)
    cpu.reg_write(UC_ARM_REG_XPSR, 0x01000000)
    cpu.emu_start(handler | 1, STOP, timeout=1_000_000, count=10000)
    if cpu.reg_read(UC_ARM_REG_PC) != STOP:
        raise ValueError('Setup handler did not return')
    state, count, source = struct.unpack('<III', cpu.mem_read(TRANSFER, 12))
    return {'handler': handler, 'setup_hex': request.hex(' '), 'transfer_state': state,
            'source': source, 'length': count, 'endpoint_status': cpu.mem_read(0x400A0011, 1)[0]}


def main():
    image = ROOT / 'research/vendor/panchip-reference/aj179-reference/mouse-MV303-DV233-reference.bin'
    data = image.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Reference image mismatch')
    cases = []
    for index in (0, 0xFFFF):
        result = run(data, 0x1624, struct.pack('<BBHHH', 0x80, 6, 0x100, index, 18))
        if result['source'] != 0x8A5C or result['length'] != 18:
            raise ValueError(f'Unexpected device descriptor selection: {result}')
        cases.append(result)
        result = run(data, 0x1378, struct.pack('<BBHHH', 0xA1, 1, 0x300, index, 64))
        if result['source'] != 0x20001B6C or result['length'] != 64:
            raise ValueError(f'Unexpected GET_REPORT selection: {result}')
        cases.append(result)
    result = run(data, 0x1624, struct.pack('<BBHHH', 0xC0, 6, 0x100, 0, 18))
    if result['source'] or result['endpoint_status'] != 0x60:
        raise ValueError(f'Unexpected vendor-type descriptor selection: {result}')
    cases.append(result)
    report = {'reference_image_sha256': SHA, 'hardware_access': False,
              'scope': 'Selected setup handlers only; synthetic RAM and USB registers',
              'physical_target_bootloader_verified': False, 'cases': cases}
    path = ROOT / 'research/local/mouse-firmware-20261021/emulated-boot-control.json'
    path.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
