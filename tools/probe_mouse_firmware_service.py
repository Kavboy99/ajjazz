"""Read version/identity via the running vendor service; never enter DFU.

Only GET_INFOR 8F, GET_REV 80 and dock GETOLED_VERSION AD are allowed.
The service owns HID, so this probe does not open a competing device handle.
"""
import argparse
import json
import time
from pathlib import Path

from driver_inventory import fields
from probe_mouse_reads import blob, rpc


def read_version(path, command, route):
    assert (command, route) in ((0x8F, 2), (0x80, 2), (0xAD, 0))
    raw = route == 0
    message = blob(1, path.encode()) + blob(2, bytes([command]) + bytes(63 if raw else 7))
    if route:
        message += bytes([32, route])
    sent = rpc('sendRawFeature' if raw else 'sendMsg', message)
    error = next((v.decode() for n, w, v in sent if n == 1), '')
    if error:
        raise RuntimeError(error)
    time.sleep(.02)
    response = rpc('readRawFeature' if raw else 'readMsg', blob(1, path.encode()))
    error = next((v.decode() for n, w, v in response if n == 1), '')
    if error:
        raise RuntimeError(error)
    data = next((v for n, w, v in response if n == 2), b'')
    if len(data) != 64 or data[0] != command:
        raise RuntimeError(f'Unexpected {command:02X} response: {data.hex()}')
    return list(data)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    inventory = rpc('watchDevList', b'')
    matches = []
    for number, wire, entry in inventory:
        if number != 1:
            continue
        for tag, wire, common in fields(entry):
            if tag != 2:
                continue
            values = fields(common)
            nums = {key: value for key, kind, value in values if kind == 0}
            paths = [v.decode() for key, kind, v in values if key == 3 and kind == 2]
            if (nums.get(7), nums.get(8)) == (0x3151, 0x5007) and len(paths) == 1:
                matches.append((paths[0], nums.get(6)))
    if len(matches) != 1:
        raise RuntimeError('Expected one AJ159 APEX dock in service inventory')
    path, mouse_id = matches[0]
    report = {'transport': 'running AJAZZ service', 'service_mouse_id': mouse_id,
              'firmware_programming': False, 'boot_requested': False, 'queries': []}
    for command, route in [(0xAD, 0), (0x8F, 2), (0x80, 2)]:
        result = {'command': command, 'route': route}
        try:
            data = read_version(path, command, route)
            result['raw'] = data
            if command == 0x8F:
                result['device_id'] = int.from_bytes(bytes(data[1:5]), 'little')
            else:
                result['version_raw'] = int.from_bytes(bytes(data[1:3]), 'little')
        except Exception as exc:
            result['error'] = str(exc)
        report['queries'].append(result)
    args.output.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
