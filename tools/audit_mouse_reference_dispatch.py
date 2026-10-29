"""Reproduce command-table evidence from pinned reference firmware; no devices.

This is an audit of AJ179 reference files, not an AJ159 firmware implementation.
Command labels are analyst annotations grounded in the saved disassembly.
"""
import hashlib
import json
from pathlib import Path
import struct

from capstone import Cs, CS_ARCH_ARM, CS_MODE_THUMB, CS_MODE_MCLASS

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'research/local/mouse-firmware-20261021'
BASE = ROOT / 'research/vendor/panchip-reference/aj179-reference'
PINS = {
    'mouse': '2c4e52961ded6f3043b6b4437f6093a880364c8f933f49108a55d924b98483ba',
    'dock': '57eeedd50368cce8d412b5ea08ea9027a1a548c1c475c9f1007cef650f70e0db',
}
READ_HANDLERS = {
    0x7F: ('boot_transition', False),  # High bit does NOT guarantee a read-only command.
    0x80: ('version_03_03', True),
    0x82: ('no_data_handler', None),
    0x83: ('no_data_handler', None),
    0x84: ('poll_rate', True),
    0x85: ('profile', True),
    0x8D: ('no_data_handler', None),
    0x8F: ('device_id_1852_and_connection_mode', True),
    0x96: ('macro_configuration_window', True),
    0xD0: ('button_configuration_window', True),
    0xD1: ('fn_button_configuration_window', True),
    0xD3: ('mouse_options', True),
    0xD4: ('dpi_options', True),
    0xD5: ('profile_setting_byte', True),
    0xEF: ('fixed_8_byte_manufacturing_metadata_at_0xE000', True),
}


def image(component):
    data = (BASE / f'{component}-MV303-DV233-reference.bin').read_bytes()
    if hashlib.sha256(data).hexdigest() != PINS[component]:
        raise ValueError('Reference image hash mismatch')
    return data


def table(data, offset, first, count, start, end):
    entries = []
    for command in range(first, first + count):
        target = struct.unpack_from('<I', data, offset + (command - first) * 4)[0]
        if target & 1 or not start <= target < end:
            raise ValueError('Jump-table target outside audited dispatcher')
        entries.append({'command': command, 'target': target})
    return entries


def main():
    mouse, receiver = image('mouse'), image('dock')
    reads = table(mouse, 0x530C4, 0x7F, 113, 0x36A94, 0x36C88)
    writes = table(mouse, 0x53288, 0, 86, 0x36A94, 0x36C88)
    for entry in reads:
        command = entry['command']
        label, readonly = READ_HANDLERS.get(command, ('default_unhandled', None))
        entry.update(annotation=label, read_only_in_reference_handler=readonly)
        if command not in READ_HANDLERS and entry['target'] != 0x36A94:
            raise ValueError('Unclassified non-default handler')
    receiver_table = table(receiver, 0x39AD0, 0xBB, 13, 0x320B4, 0x32210)
    result = {
        'scope': 'Pinned related AJ179 firmware only; no physical command probes',
        'image_sha256': PINS, 'hardware_access': False,
        'reference_mouse_id': 1852, 'attached_mouse_id': 1755,
        'target_compatibility_established': False,
        'reference_boot_transition': {
            'command': 0x7F, 'handler': 0x35E10, 'read_only': False,
            'flash_erase_calls': [{'address': 0xF000, 'length': 4096},
                                 {'address': 0x6D000, 'length': 4096}],
            'flash_write_call': {'address': 0xF000, 'hex': 'aa55aa55'},
            'proof': 'emulated-boot-entry-side-effects.json',
            'physical_target_implementation_verified': False,
        },
        'mouse_dispatcher': 0x36A50, 'mouse_read_table': reads,
        'mouse_write_table': writes,
        'configuration_read_expressions': {
            '0x96': '0x70000 + uint8(request[1])*256 + uint8(request[2])*64; copy only if request[2]<5',
            '0xD0': '0x6E000 + uint8(request[1])*256 + uint8(settings[0x16])*64',
            '0xD1': '0x6F000 + uint8(request[1])*64; copy only if request[2]<=1',
            '0xEF': 'SPI flash fixed offset 0xE000, 8 bytes; address is not request-controlled',
        },
        'receiver_dispatcher': 0x31FD8, 'receiver_envelope_marker': 0x44,
        'receiver_command_table': receiver_table,
        'receiver_usb_pid': '4027', 'attached_dock_usb_pid': '5007',
        'oled_firmware_extracted': False,
        'custom_firmware_created': False,
    }
    (OUT / 'reference-command-audit.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
    decoder = Cs(CS_ARCH_ARM, CS_MODE_THUMB | CS_MODE_MCLASS)
    for name, data, begin, end in (
        ('mouse', mouse, 0x36A50, 0x36C88),
        ('receiver', receiver, 0x31FD8, 0x32308),
    ):
        listing = [f'; SHA-256 {PINS["dock" if name == "receiver" else name]}\n']
        for ins in decoder.disasm(data[begin:end], begin):
            listing.append(f'{ins.address:08x}: {ins.bytes.hex():10} {ins.mnemonic:8} {ins.op_str}\n')
        (OUT / f'reference-{name}-dispatch-audited.asm').write_text(''.join(listing), encoding='utf-8')
    print(f'Audited {len(reads)} read-table, {len(writes)} write-table and {len(receiver_table)} receiver entries; no hardware access')


if __name__ == '__main__':
    main()
