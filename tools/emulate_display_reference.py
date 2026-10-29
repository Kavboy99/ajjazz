"""Isolated original display RX validator; synthetic RAM, no device access.

Dispatch and serial cleanup are stubbed: this verifies framing, not rendering,
GPIO, flash writing or compatibility with the physical AJ159 dock.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_HOOK_CODE, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC

ROOT = Path(__file__).resolve().parents[1]
SHA = 'de636a8fa6eb0dd127b4151c1adb563d681a75c189b5546186a00fc8e402b45b'
FLASH_MAP, BASE, STOP = 0x1000000, 0x1000200, 0x105F000
FLAGS, BUFFER, RX = 0x2B604, 0x2A54C, 0x20024


def run(data, frame):
    if not 5 <= len(frame) <= 80:
        raise ValueError('Only bounded reference link frames')
    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    cpu.mem_map(FLASH_MAP, 0x60000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(BASE, data)
    cpu.mem_map(0x20000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
    cpu.mem_write(FLAGS, bytes((1, 4)))
    cpu.mem_write(BUFFER, bytes(frame))
    cpu.mem_write(RX + 4, struct.pack('<I', len(frame)))
    cpu.reg_write(UC_ARM_REG_SP, 0x2FF00)
    cpu.reg_write(UC_ARM_REG_LR, STOP | 1)
    dispatches = []

    def hook(uc, address, size, user):
        if address == BASE + 0x148CA:
            dispatches.append(bytes(uc.mem_read(BUFFER, len(frame))).hex(' '))
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))
        elif address == BASE + 0x13F04:
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))

    cpu.hook_add(UC_HOOK_CODE, hook)
    cpu.emu_start((BASE + 0x149DA) | 1, STOP, timeout=1_000_000, count=5000)
    if cpu.reg_read(UC_ARM_REG_PC) != STOP:
        raise ValueError('Validator did not return')
    return {'frame_hex': bytes(frame).hex(' '), 'dispatch_count': len(dispatches),
            'error_flag': cpu.mem_read(FLAGS + 7, 1)[0]}


def main():
    path = ROOT / 'research/vendor/panchip-reference/keydous-screen-reference/firmwareOledFile.bin'
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Pinned display reference mismatch')
    # Announce payload of 12 bytes; type3. No actual dispatch or writes execute.
    frame = bytearray((0x55, 14, 0, 3)) + bytearray(12)
    frame.append(sum(frame[1:]) & 255)
    bad_checksum, bad_length = bytearray(frame), bytearray(frame)
    bad_checksum[-1] ^= 1
    bad_length[1] += 1
    cases = []
    for name, packet, expected in [('valid', frame, 1), ('checksum_error', bad_checksum, 0),
                                   ('length_error', bad_length, 0)]:
        result = run(data, packet)
        if result['dispatch_count'] != expected:
            raise ValueError(f'Unexpected validator result: {name} {result}')
        cases.append(dict(name=name, **result))
    report = {'reference_image_sha256': SHA, 'hardware_access': False,
              'file_load_address': hex(BASE),
              'target_dock_compatibility_verified': False,
              'scope': 'RX validator only; dispatcher and serial cleanup are synthetic callbacks',
              'cases': cases}
    target = ROOT / 'research/local/mouse-firmware-20261021/emulated-display-link.json'
    target.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
