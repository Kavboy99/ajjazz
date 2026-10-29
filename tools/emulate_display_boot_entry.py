"""Trace the reference display boot startup until app launch or erase call.

Metadata verifier, IRQ/UART setup and flash reads are stubbed. Stops BEFORE
the erase routine. No device access, real flash mutation or boot request.
"""
import hashlib
import json
from pathlib import Path

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_HOOK_CODE, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2

ROOT = Path(__file__).resolve().parents[1]
SHA = 'de636a8fa6eb0dd127b4151c1adb563d681a75c189b5546186a00fc8e402b45b'


def run(data, update_flag):
    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    cpu.mem_map(0x1000000, 0x60000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(0x1000200, data)
    cpu.mem_map(0x20000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
    cpu.reg_write(UC_ARM_REG_SP, 0x2FF00)
    cpu.reg_write(UC_ARM_REG_LR, 0x105F001)
    events = []

    def hook(uc, address, size, user):
        if address in (0x1000A76, 0x1000418, 0x1001E72, 0x1000C3C):
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))
        elif address == 0x1002464:
            source, dest, count = [uc.reg_read(r) for r in (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2)]
            if source != 0x1000120 or count != 32:
                raise ValueError('Unexpected startup flash read')
            uc.mem_write(dest, (b'\x00' if update_flag else b'\xff') + b'\xff' * 31)
            events.append({'operation': 'synthetic_boot_flag_read', 'address': source, 'bytes': count})
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))
        elif address == 0x10002F8:
            events.append({'operation': 'launch_existing_application'})
            uc.emu_stop()
        elif address == 0x100232C:
            events.append({'operation': 'erase_range_routine_called_not_executed',
                           'virtual_address': uc.reg_read(UC_ARM_REG_R0),
                           'bytes': uc.reg_read(UC_ARM_REG_R1)})
            uc.emu_stop()

    cpu.hook_add(UC_HOOK_CODE, hook)
    cpu.emu_start(0x1000921, 0x105F000, timeout=1_000_000, count=5000)
    expected = 'erase_range_routine_called_not_executed' if update_flag else 'launch_existing_application'
    if not events or events[-1]['operation'] != expected:
        raise ValueError('Startup path did not reach expected boundary')
    if update_flag and (events[-1]['virtual_address'], events[-1]['bytes']) != (0x1010000, 0x50000):
        raise ValueError('Unexpected application erase request')
    return {'synthetic_update_flag': update_flag, 'events': events}


def main():
    image = ROOT / 'research/vendor/panchip-reference/keydous-screen-reference/firmwareOledFile.bin'
    data = image.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Reference display image mismatch')
    report = {'sha256': SHA, 'file_load_address': '0x01000200', 'hardware_access': False,
              'flash_erase_executed': False, 'physical_target_behavior_verified': False,
              'limitations': 'Metadata verifier and hardware init stubbed; stop before erase routine',
              'cases': [run(data, False), run(data, True)]}
    out = ROOT / 'research/local/mouse-firmware-20261021/emulated-display-boot-entry.json'
    out.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
