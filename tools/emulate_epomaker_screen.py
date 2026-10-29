"""Audit reference OLED104 version and panel setup using original instructions.

Synthetic RAM/MMIO only. Logging, delays, GPIO/SPI setup and panel writes are
callbacks. Neither UART framing nor a physical display/board is validated.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC, UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3

ROOT = Path(__file__).resolve().parents[1]
SHA = 'a9a2c4287b6f29008c961446f9442c757c34c6fe5a84d6204cdd9739451a7274'
BASE, STOP = 0x01000200, 0x0102F000


def machine(data):
    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    cpu.mem_map(0x01000000, 0x30000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(BASE, data)
    cpu.mem_map(0, 0x10000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_map(0x20000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
    cpu.mem_map(0xF0000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
    cpu.reg_write(UC_ARM_REG_SP, 0x2FF00)
    cpu.reg_write(UC_ARM_REG_LR, STOP | 1)
    return cpu


def panel(data):
    cpu = machine(data)
    commands, fills = [], []

    def hook(uc, address, size, user):
        if address == 0x010106A4:
            commands.append({'command': uc.reg_read(UC_ARM_REG_R0), 'data': []})
        elif address == 0x01010670:
            if not commands:
                raise ValueError('Panel data without command')
            commands[-1]['data'].append(uc.reg_read(UC_ARM_REG_R0))
        elif address == 0x0101070C:
            fills.append([uc.reg_read(r) for r in (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)])
        elif address not in (0x010125C0, 0x01013E90, 0x425C):
            return
        uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))

    cpu.hook_add(UC_HOOK_CODE, hook)
    cpu.emu_start(0x0101073B, STOP, timeout=1_000_000, count=10000)
    if cpu.reg_read(UC_ARM_REG_PC) != STOP:
        raise ValueError('Panel initializer did not return')
    selected = {f'{row["command"]:02x}': row['data'] for row in commands if row['command'] in (0x2A, 0x2B, 0x3A)}
    if selected != {'3a': [5], '2a': [0, 0, 0, 239], '2b': [0, 0, 0, 239]} or fills != [[0, 0, 240, 240]]:
        raise ValueError(f'Unexpected reference panel parameters: {selected} {fills}')
    return {'command_data': selected, 'initial_fill_xywh': fills[0], 'logical_panel_size': [240, 240]}


def version(data):
    cpu = machine(data)
    pointer = lambda address: struct.unpack_from('<I', data, address - BASE)[0]
    source = pointer(0x010123E4)
    tx = pointer(0x01012408) - 0x40
    request = bytearray(16)
    request[3] = 0x27
    cpu.mem_write(source, bytes(request))

    def hook(uc, address, size, user):
        if address == 0x01014080:
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))

    cpu.hook_add(UC_HOOK_CODE, hook)
    cpu.emu_start(0x01011FFF, STOP, timeout=1_000_000, count=10000)
    response = bytes(cpu.mem_read(tx, 6))
    if cpu.reg_read(UC_ARM_REG_PC) != STOP or response != bytes.fromhex('44 05 a7 00 04 01'):
        raise ValueError(f'Unexpected OLED104 GET reply: {response.hex()}')
    return {'uart_command': '0x27', 'reply_prefix_hex': response.hex(' '), 'version_raw': '0x0104',
            'rx_buffer': hex(source), 'tx_buffer': hex(tx)}


def main():
    image = ROOT / 'research/vendor/panchip-reference/epomaker-screen-reference/firmwareOledFile.bin'
    data = image.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Pinned EPOMAKER reference mismatch')
    result = {'reference_sha256': SHA, 'file_load_address': hex(BASE), 'hardware_access': False,
              'target_dock_compatibility_verified': False,
              'limitations': 'Isolated initializer/GET only; GPIO/SPI, delays and logging are synthetic; no real UART or panel',
              'panel': panel(data), 'version': version(data)}
    out = ROOT / 'research/local/mouse-firmware-20261021/epomaker-screen-emulation.json'
    out.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
