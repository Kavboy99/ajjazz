"""Execute the reference screen boot metadata handler with synthetic RAM.

No USB access. Starts after framing validation, so this tests command08 only,
not a full boot, physical compatibility, or a flash read/recovery procedure.
"""
import hashlib
import json
from pathlib import Path

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC

ROOT = Path(__file__).resolve().parents[1]
SHA = 'de636a8fa6eb0dd127b4151c1adb563d681a75c189b5546186a00fc8e402b45b'
BASE, STOP = 0x01000200, 0x0105F000
BUFFER, TX, STATUS = 0x20108, 0x20234, 0x20288
EXPECTED = bytes.fromhex('44 13 88 00 01 01 00 00 00 01 50 41 4e 31 30 38 4f 4c 45 44')


def run(data, fill):
    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    cpu.mem_map(0x01000000, 0x60000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(BASE, data)
    cpu.mem_map(0x20000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
    request = bytearray([fill] * 80)
    request[3] = 8
    cpu.mem_write(BUFFER, bytes(request))
    cpu.mem_write(STATUS + 7, b'\0')
    cpu.reg_write(UC_ARM_REG_SP, 0x2FF00)
    cpu.reg_write(UC_ARM_REG_LR, STOP | 1)
    cpu.emu_start(0x01000F13, STOP, timeout=1_000_000, count=5000)
    if cpu.reg_read(UC_ARM_REG_PC) != STOP:
        raise ValueError('Boot metadata dispatcher did not return')
    response = bytes(cpu.mem_read(TX, len(EXPECTED)))
    if response != EXPECTED:
        raise ValueError(f'Unexpected reference boot metadata: {response.hex()}')
    return {'request_payload_fill': fill, 'response_hex': response.hex(' '),
            'identifier_ascii': response[10:].decode('ascii')}


def main():
    image = ROOT / 'research/vendor/panchip-reference/keydous-screen-reference/firmwareOledFile.bin'
    data = image.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Reference display image mismatch')
    report = {'sha256': SHA, 'file_load_address': hex(BASE), 'hardware_access': False,
              'physical_target_behavior_verified': False,
              'limitations': 'Command08 dispatcher only; framing and real startup not executed',
              'cases': [run(data, 0), run(data, 255)]}
    out = ROOT / 'research/local/mouse-firmware-20261021/emulated-display-boot-metadata.json'
    out.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
