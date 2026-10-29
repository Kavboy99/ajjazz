"""Verify downloaded1755_v302 container and execute only its two GET handlers.

Synthetic RAM, read/execute flash, background housekeeping stubbed. Does not
validate board compatibility, boot recovery, radio, calibration or charging.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_HOOK_CODE, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_XPSR
from extract_ry_reference_firmware import inspect_component, verify_signatures

ROOT = Path(__file__).resolve().parents[1]
SHA = '82f35e611ab37861f99bb450c70b8b769c3385053dc4dc22c0393e6828a44289'
STATE, BUFFER, MODE, STOP = 0x2000542C, 0x20005434, 0x200057AC, 0xE0000


def get(data, command):
    if command not in (0x80, 0x8F):
        raise ValueError('Only audited ID/version handlers')
    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
    cpu.mem_map(0, 0x100000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(0, data)
    cpu.mem_map(0x20000000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
    cpu.mem_write(STATE + 1, b'\x01')
    cpu.mem_write(MODE, b'\x06')
    request = bytearray(64)
    request[0], request[7] = command, 255 - command
    cpu.mem_write(BUFFER, bytes(request))

    def hook(uc, address, size, user):
        if address == 0x32C50:
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))

    cpu.hook_add(UC_HOOK_CODE, hook)
    cpu.reg_write(UC_ARM_REG_SP, 0x2000F000)
    cpu.reg_write(UC_ARM_REG_LR, STOP | 1)
    cpu.reg_write(UC_ARM_REG_XPSR, 0x01000000)
    cpu.emu_start(0x36A31, STOP, timeout=1_000_000, count=20000)
    if cpu.reg_read(UC_ARM_REG_PC) != STOP:
        raise ValueError('GET did not return within bound')
    response = bytes(cpu.mem_read(BUFFER, 64))
    if response[0] != command:
        raise ValueError('Reply opcode mismatch')
    return response


def main():
    image = ROOT / 'research/vendor/panchip-reference/aj159-apex-1755-reference/firmwareFile.bin'
    data = image.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Pinned1755 image mismatch')
    parts = [inspect_component(data, offset) for offset in (0x10000, 0x30000)]
    verify_signatures(data, parts)
    if parts[-1]['end_offset'] != len(data):
        raise ValueError('Unexpected trailing data')
    version, identity = get(data, 0x80), get(data, 0x8F)
    if int.from_bytes(version[1:3], 'little') != 0x0302 or struct.unpack_from('<I', identity, 1)[0] != 1755:
        raise ValueError('Version or ID does not match expected1755/0302')
    report = {'sha256': SHA, 'bytes': len(data), 'components': parts,
              'hardware_access': False, 'physical_hardware_compatibility_verified': False,
              'scope': 'Isolated GET80/8F, synthetic RAM; housekeeping stubbed',
              'version_raw': '0x0302', 'device_id': 1755,
              'version_reply_hex': version.hex(' '), 'id_reply_hex': identity.hex(' ')}
    path = ROOT / 'research/local/mouse-firmware-20261021/apex1755-image-validation.json'
    path.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(f'ID1755 / version0302 confirmed by original GET code; {len(parts)} signatures verified')
    print(path)


if __name__ == '__main__':
    main()
