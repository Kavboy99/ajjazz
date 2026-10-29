"""Audit reference boot-entry side effects in isolated emulation, never USB.

The original AJ179 application routine is executed. Flash-device lookup,
flash-driver callbacks, settings initialization and reset are stubbed. The
emulated flash driver records operations on an in-memory dictionary only.
This does not establish the physical AJ159's boot-entry implementation.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_HOOK_CODE, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_XPSR, UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3

ROOT = Path(__file__).resolve().parents[1]
SHA = '2c4e52961ded6f3043b6b4437f6093a880364c8f933f49108a55d924b98483ba'
STOP = 0xE0000
DEVICE = 0x2000B000
VTABLE = 0x2000B100
READ, WRITE, ERASE, PROTECT = 0xE0100, 0xE0110, 0xE0120, 0xE0130


def audit():
    data = (ROOT / 'research/vendor/panchip-reference/aj179-reference/mouse-MV303-DV233-reference.bin').read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Reference image mismatch')
    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
    cpu.mem_map(0, 0x100000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(0, data)
    cpu.mem_map(0x20000000, 0x10000, UC_PROT_READ | UC_PROT_WRITE | UC_PROT_EXEC)
    cpu.mem_write(DEVICE + 8, struct.pack('<I', VTABLE))
    cpu.mem_write(VTABLE, struct.pack('<IIII', READ | 1, WRITE | 1, ERASE | 1, PROTECT | 1))
    events, storage = [], {}

    def ret(engine, value=0):
        engine.reg_write(UC_ARM_REG_R0, value)
        engine.reg_write(UC_ARM_REG_PC, engine.reg_read(UC_ARM_REG_LR))

    def hook(engine, address, size, _):
        if address == STOP:
            engine.emu_stop()
        elif address == 0x4E850:
            ret(engine, DEVICE)
        elif address in (READ, WRITE, ERASE, PROTECT):
            a, p, n = [engine.reg_read(r) for r in (UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)]
            if address == ERASE:
                if p > 4096:
                    raise ValueError('Unexpected reference erase length')
                events.append({'operation': 'erase', 'address': a, 'length': p})
                for i in range(p):
                    storage[a + i] = 255
            elif address == WRITE:
                if n > 64:
                    raise ValueError('Unexpected reference write length')
                content = bytes(engine.mem_read(p, n))
                events.append({'operation': 'write', 'address': a, 'length': n, 'hex': content.hex()})
                storage.update({a + i: b for i, b in enumerate(content)})
            elif address == READ:
                if n > 64:
                    raise ValueError('Unexpected reference read length')
                content = bytes(storage.get(a + i, 255) for i in range(n))
                engine.mem_write(p, content)
                events.append({'operation': 'read', 'address': a, 'length': n})
            # Write-protection callbacks are acknowledged by the synthetic driver.
            ret(engine)
        elif address in (0x34004, 0x3A69C):
            events.append({'operation': 'stubbed_settings_initialization' if address == 0x34004 else 'stubbed_reset'})
            ret(engine)

    cpu.hook_add(UC_HOOK_CODE, hook)
    cpu.reg_write(UC_ARM_REG_SP, 0x2000F000)
    cpu.reg_write(UC_ARM_REG_LR, STOP | 1)
    cpu.reg_write(UC_ARM_REG_XPSR, 0x01000000)
    cpu.emu_start(0x35E11, STOP, timeout=1_000_000, count=10000)
    if cpu.reg_read(UC_ARM_REG_PC) != STOP:
        raise ValueError('Reference boot-entry routine did not return')
    changes = [e for e in events if e['operation'] in ('erase', 'write')]
    expected = [
        {'operation': 'erase', 'address': 0xF000, 'length': 4096},
        {'operation': 'write', 'address': 0xF000, 'length': 4, 'hex': 'aa55aa55'},
        {'operation': 'erase', 'address': 0x6D000, 'length': 4096},
    ]
    if changes != expected:
        raise ValueError(f'Unexpected reference flash operations: {changes}')
    return {'reference_image_sha256': SHA, 'entry_function': 0x35E10,
            'hardware_access': False, 'reference_entry_is_read_only': False,
            'physical_target_implementation_verified': False,
            'stubbed_operations': ['flash driver', 'device lookup', 'settings initialization', 'reset'],
            'observed_events': events}


if __name__ == '__main__':
    result = audit()
    output = ROOT / 'research/local/mouse-firmware-20261021/emulated-boot-entry-side-effects.json'
    output.write_text(json.dumps(result, indent=2), encoding='utf-8')
    print(json.dumps(result, indent=2))
