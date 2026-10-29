"""Bounded, read-only comparison of vendor GET 0x10 and OTA system GET 0x80.

0x80 is KZ/getDeviceSystemInfo in the captured site. No 0x81 verification,
0x82 boot transition, 0x83 firmware data, or partition commands are sent.
An echoed/all-zero payload is evidence of an unimplemented query, not a version.
"""
import ctypes
import argparse
import datetime
import json
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / '.tools/py-research'))
import hid


def query(device, command, header, size=33):
    assert command in (0x10, 0x80)
    device.read(65, 5)
    packet = bytes([0, 0xAA, command] + header + [0] * (size - 3 - len(header)))
    assert len(packet) == size
    assert device.write(packet) == size
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        reply = bytes(device.read(65, 100))
        if reply[:1] == b'\0':
            reply = reply[1:]
        if len(reply) >= 8 and reply[:2] == bytes([0x55, command]):
            return list(reply)
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--channel', choices=('usb', 'radio'), default='radio')
    arguments = parser.parse_args()
    wired = arguments.channel == 'usb'
    size = 65 if wired else 33
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_wchar_p]
    kernel.CreateMutexW.restype = ctypes.c_void_p
    kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint]
    kernel.ReleaseMutex.argtypes = [ctypes.c_void_p]
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    lock = kernel.CreateMutexW(None, 0, 'Local\\AjazzStudio-AK820MAX')
    assert lock
    owned = kernel.WaitForSingleObject(lock, 0) in (0, 0x80)
    if not owned:
        kernel.CloseHandle(lock)
        raise RuntimeError('Keyboard is busy; no packets sent')
    device = hid.device()
    try:
        endpoints = [d for d in hid.enumerate(0x0C45, 0x80A0 if wired else 0xFEFC)
                     if d['usage_page'] == (0xFF68 if wired else 0xFF80) and d['usage'] == (97 if wired else 1)
                     and d['product_string'] == ('AK820MAX' if wired else 'AJAZZ AK820MAX')]
        assert len(endpoints) == 1
        device.open_path(endpoints[0]['path'])
        before = query(device, 0x10, [24, 0, 0, 0, 1, 0], size)
        assert before and before[8 + 4:8 + 8] == [0x45, 0x0C, 0xA0, 0x80], 'Identity unavailable'
        assert before[16:18] == [0x30, 1], 'Different firmware'
        system = query(device, 0x80, [0] * 6, size)
        after = query(device, 0x10, [24, 0, 0, 0, 1, 0], size)
        result = {
            'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'channel': arguments.channel,
            'commands_sent': ['GET_DEVICE_INFO 0x10', 'OTA_GET_DEVICE_SYSTEM_INFO 0x80', 'GET_DEVICE_INFO 0x10'],
            'before': before, 'system_reply': system, 'after': after,
            'identity_unchanged': bool(after and before[12:24] == after[12:24]),
            'system_query_supported': bool(system and len(system) >= 18 and system[16] in (0x10, 0x30)),
        }
        if result['system_query_supported']:
            result['system_payload_first_10'] = system[8:18]
        path = Path('research/local/keyboard-system-query-usb.json' if wired else 'research/local/keyboard-system-query.json')
        path.write_text(json.dumps(result, indent=2), encoding='utf-8')
        print(json.dumps(result, indent=2))
    finally:
        device.close()
        kernel.ReleaseMutex(lock)
        kernel.CloseHandle(lock)


if __name__ == '__main__':
    main()
