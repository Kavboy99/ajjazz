"""Synthetic XW load/store hook for offline Unicorn execution.

Encoding source: https://gist.github.com/ArcaneNibble/461edd3bc88a5284098d48941172bfa4
The hook implements only these eight memory operations; it does not emulate MMIO.
"""
from unicorn.riscv_const import UC_RISCV_REG_X0, UC_RISCV_REG_PC


def instruction(word):
    group = word & 0xe003
    reg = 8 + ((word >> 2) & 7)
    base = 8 + ((word >> 7) & 7)
    if group in (0x2000, 0xa000):
        offset = ((word >> 12) & 1) | (((word >> 10) & 3) << 3) | (((word >> 5) & 3) << 1)
        return group == 0xa000, 1, reg, base, offset
    if group in (0x2002, 0xa002):
        offset = (((word >> 10) & 7) << 3) | (((word >> 5) & 3) << 1)
        return group == 0xa002, 2, reg, base, offset
    if word & 0xfc03 == 0x8000:
        operation = (word >> 5) & 3
        offset = (word >> 7) & 15
        if operation & 1:
            offset = (offset & 14) | ((offset & 1) << 4)
        return bool(operation & 2), 1 + (operation & 1), reg, 2, offset
    return None


def hook(cpu, address, size, user):
    if size != 2:
        return
    decoded = instruction(int.from_bytes(cpu.mem_read(address, 2), 'little'))
    if decoded is None:
        return
    store, width, reg, base, offset = decoded
    target = (cpu.reg_read(UC_RISCV_REG_X0 + base) + offset) & 0xffffffff
    if store:
        value = cpu.reg_read(UC_RISCV_REG_X0 + reg) & ((1 << (8 * width)) - 1)
        cpu.mem_write(target, value.to_bytes(width, 'little'))
    else:
        cpu.reg_write(UC_RISCV_REG_X0 + reg, int.from_bytes(cpu.mem_read(target, width), 'little'))
    if isinstance(user, dict):
        user['xw_instructions'] = user.get('xw_instructions', 0) + 1
    cpu.reg_write(UC_RISCV_REG_PC, address + 2)
