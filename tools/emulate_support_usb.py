"""Execute exact support USB GET/forward handlers in synthetic RAM.

No HID, no updater execution, no flash programming. USB, SPI, radio, LED and
power behavior are not emulated. The original RV32 code uses QingKe XW.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_RISCV, UC_MODE_RISCV32, UC_HOOK_CODE, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC
from unicorn.riscv_const import UC_RISCV_REG_GP, UC_RISCV_REG_SP, UC_RISCV_REG_RA, UC_RISCV_REG_PC
import qingke_xw

ROOT = Path(__file__).resolve().parents[1]
SHA = 'a4398f2e8cfe2af3c59c6073836298c8cd350230c5850fed90d73164b83507d8'
STOP = 0xf000


def run(image, request):
    if len(request) != 64:
        raise ValueError('Expected a 64-byte report body')
    cpu = Uc(UC_ARCH_RISCV, UC_MODE_RISCV32)
    cpu.mem_map(0, 0x10000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(0, image)
    cpu.mem_map(0x20000000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
    cpu.mem_write(0x20000000, image[0x7bc8:0x7c90])
    # Source-confirmed factory chip-ID register, synthetic value from our GET F0.
    cpu.mem_map(0x1ffff000, 0x1000, UC_PROT_READ)
    cpu.mem_write(0x1ffff704, struct.pack('<I', 0x30520568))
    cpu.mem_write(0x200000f0, b'\0')
    cpu.mem_write(0x20000133, b'\1')
    cpu.mem_write(0x200001c6, request)
    cpu.reg_write(UC_RISCV_REG_GP, 0x200008b8)
    cpu.reg_write(UC_RISCV_REG_SP, 0x2000ff00)
    cpu.reg_write(UC_RISCV_REG_RA, STOP)
    counters = {}
    cpu.hook_add(UC_HOOK_CODE, qingke_xw.hook, counters)
    cpu.emu_start(0x5584, STOP, timeout=1_000_000, count=100000)
    if cpu.reg_read(UC_RISCV_REG_PC) != STOP:
        raise ValueError('Handler exceeded execution bound')
    return (bytes(cpu.mem_read(0x20000186, 64)),
            bytes(cpu.mem_read(0x20000378, 72)), counters)


def main():
    data = (ROOT / 'research/vendor/ajazz-support-20261023/dock/decoded/x1-verified.bin').read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Exact support USB image mismatch')
    rows = []
    for opcode in (0xf1, 0xf0, 0xad, 0x25, 0x28):
        request = bytearray(64)
        request[0] = opcode
        request[7] = (255 - opcode) & 255
        if opcode in (0x25, 0x28):
            request[8:] = bytes(range(56))
        response, forwarded, counters = run(data, bytes(request))
        if opcode == 0xf1:
            assert response[:5] == bytes.fromhex('f1 df 06 00 00')
        elif opcode == 0xf0:
            assert response[:7] == bytes.fromhex('f0 00 03 68 05 52 30')
        else:
            assert forwarded[:2] == bytes.fromhex('41 00')
            assert forwarded[2:66] == request
            assert forwarded[71] == sum(forwarded[1:66]) & 255
        if opcode not in (0xf0, 0xf1):
            assert counters.get('xw_instructions', 0) > 0
        rows.append({'opcode': hex(opcode), 'response_hex': response.hex(' '),
                     'forwarded_frame_fields_hex': forwarded.hex(' '), **counters})
    report = {'image_sha256': SHA, 'hardware_access': False, 'firmware_written': False,
              'scope': 'Original USB GET F1/F0 and report forwarding only; synthetic XW load/store semantics and chip-ID register',
              'does_not_verify': ['physical recovery', 'SPI transport', 'radio', 'OLED execution', 'LED wiring', 'charging'],
              'cases': rows}
    target = ROOT / 'research/local/ajazz-support-20261023/usb-emulation.json'
    target.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print('5 exact-image cases passed: ID1759, USB0300, chip-ID field and intact OLED report forwarding')
    print(target)


if __name__ == '__main__':
    main()
