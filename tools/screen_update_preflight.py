"""Read-only ID check for the OLED-only OEM packages. Never starts an updater."""
import hashlib
import json
from pathlib import Path
import sys
import time

from extract_support_firmware import decode_resource


def main():
    import hid

    if len(sys.argv) != 2:
        raise SystemExit('Usage: screen_update_preflight.py PACKAGE_DIRECTORY')
    root = Path(sys.argv[1])
    package = json.loads((root / 'package.json').read_text())
    for name, expected in package['files'].items():
        if hashlib.sha256((root / name).read_bytes()).hexdigest() != expected:
            raise ValueError(f'Package file hash mismatch: {name}')
    for name in ('Install-screen-candidate', 'Restore-stock-screen'):
        resources = root / name / 'resources'
        config = json.loads((resources / 'tool_config.json').read_text())
        support = json.loads((resources / 'support_config.json').read_text())
        if config['upgrade_methods'] != ['oled'] or config['dev_info']['merge_upgrade']:
            raise ValueError('OLED-only non-merge package required')
        if len(support['support']) != 1 or support['support'][0]['vid'] != '3151' or support['support'][0]['pid'] != '5007':
            raise ValueError('Only USB3151:5007 may be listed')
        if any((resources / f'x{i}').stat().st_size for i in (1, 2, 3, 4, 6, 7)):
            raise ValueError('Non-OLED firmware payload present')
        expected = package['candidate_sha256'] if name.startswith('Install') else json.loads((root / 'manifest.json').read_text())['manufacturer_image_sha256']
        if hashlib.sha256(decode_resource((resources / 'x5').read_bytes())).hexdigest() != expected:
            raise ValueError('Decoded OLED image mismatch')
    endpoints = [e for e in hid.enumerate(0x3151, 0x5007)
                 if e['usage_page'] == 0xffff and e['usage'] == 2]
    if len(endpoints) != 1:
        raise ValueError('Exactly one dock USB3151:5007/FFFF/2 required')
    device = hid.device()
    device.open_path(endpoints[0]['path'])
    try:
        replies = {}
        for opcode, expected in ((0xf1, bytes([0, 0xf1, 0xdf, 6])),
                                 (0xad, bytes([0, 0xad, 4, 1]))):
            packet = bytearray(65)
            packet[1], packet[8] = opcode, 255 - opcode
            device.send_feature_report(packet)
            time.sleep(.05)
            reply = bytes(device.get_feature_report(0, 65))
            if len(reply) != 65 or reply[:4] != expected:
                raise ValueError(f'Unexpected identity/version response: {reply.hex()}')
            replies[hex(opcode)] = reply.hex()
    finally:
        device.close()
    print(json.dumps({'id': 1759, 'usb': '3151:5007', 'oled': '0104',
                      'candidate_sha256': package['candidate_sha256'],
                      'raw_replies': replies, 'firmware_written': False}, indent=2))


if __name__ == '__main__':
    main()
