"""Verify the pinned reference controller's direct-copy RAM scatter entry.

Executes only the original copy routine, with synthetic RAM and read/execute
flash. No startup, hardware access, firmware programming or decompression.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC, UC_HOOK_MEM_WRITE
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_XPSR

ROOT = Path(__file__).resolve().parents[1]
SHA = '57eeedd50368cce8d412b5ea08ea9027a1a548c1c475c9f1007cef650f70e0db'


def main():
    path = ROOT / 'research/vendor/panchip-reference/aj179-reference/dock-MV303-DV233-reference.bin'
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Pinned receiver image mismatch')
    table = struct.unpack_from('<I', data, 0x10474)[0]
    source, destination, length, function = struct.unpack_from('<4I', data, table)
    if (table, source, destination, length, function) != (0x27DC4, 0x27DF4, 0x2000C800, 0x2B28, 0x10A9A):
        raise ValueError('Audited scatter entry changed')
    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
    cpu.mem_map(0, 0x100000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(0, data)
    cpu.mem_map(0x20000000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
    stop = 0xF0000
    cpu.reg_write(UC_ARM_REG_SP, 0x2000FF00)
    cpu.reg_write(UC_ARM_REG_LR, stop | 1)
    cpu.reg_write(UC_ARM_REG_XPSR, 0x01000000)
    for register, value in zip((UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2), (source, destination, length)):
        cpu.reg_write(register, value)
    writes = []

    def on_write(uc, access, address, size, value, user):
        if not destination <= address < address + size <= destination + length:
            raise ValueError(f'Unexpected copy write outside destination: {address:x}')
        writes.append(address)

    cpu.hook_add(UC_HOOK_MEM_WRITE, on_write)
    cpu.emu_start(function | 1, stop, timeout=1_000_000, count=100000)
    copied = bytes(cpu.mem_read(destination, length))
    if cpu.reg_read(UC_ARM_REG_PC) != stop or copied != data[source:source + length]:
        raise ValueError('Original copy routine did not produce the audited RAM bytes')
    result = {'reference_image_sha256': SHA, 'hardware_access': False,
              'physical_dock_compatibility_verified': False, 'scope': 'Original direct-copy routine only; not full controller startup',
              'scatter_table': hex(table), 'flash_source': hex(source), 'ram_destination': hex(destination),
              'bytes': length, 'copy_function': hex(function), 'word_writes': len(writes),
              'copied_sha256': hashlib.sha256(copied).hexdigest(), 'byte_comparison_passed': True}
    out = ROOT / 'research/local/mouse-firmware-20261021/controller-ram-copy-validation.json'
    out.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
