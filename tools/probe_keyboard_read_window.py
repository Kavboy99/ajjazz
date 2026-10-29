"""Bounded USB GET-only probe of a possible config-read window.

No ISP, feature reports, reset, SET, erase or firmware writes. The candidate
8033/8070 image computes GET addresses without an obvious area-bound check;
this does not establish the same behavior in 80A0. Only its first adjacent
16 bytes are inspected here, with identity checked before and afterwards.
"""
import ctypes
import datetime
import json
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.tools/py-research'))
import hid


def query(device, command, offset, length):
    assert (command, offset, length) in {
        (0x10, 0, 48), (0x11, 0, 32), (0x11, 32, 16),
        (0x13, 0, 16), (0x13, 16, 16),
    }
    for _ in range(64):
        if not device.read(65, 2): break
    report = bytearray(65)
    report[1:9] = bytes([0xAA, command, length, offset & 255, offset >> 8, 0, 1, 0])
    assert device.write(report) == 65
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        reply = bytes(device.read(65, 100))
        if reply[:1] == b'\0': reply = reply[1:]
        if len(reply) >= 8 + length and reply[:2] == bytes([0x55, command]) and reply[2] == length and reply[3:5] == offset.to_bytes(2, 'little'):
            return reply[8:8 + length]
    raise RuntimeError(f'No matching GET{command:02X}, offset={offset}, length={length}')


def identity(data):
    return (data[4:8] == bytes.fromhex('450ca080')
            and data[8:10] == bytes.fromhex('3001')
            and data[12:16] == bytes.fromhex('66010410')
            and data[30:33] == bytes(3))


def main():
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_wchar_p]
    kernel.CreateMutexW.restype = ctypes.c_void_p
    kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint]
    kernel.ReleaseMutex.argtypes = [ctypes.c_void_p]
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    lock = kernel.CreateMutexW(None, 0, 'Local\\AjazzStudio-AK820MAX')
    assert lock
    if kernel.WaitForSingleObject(lock, 0) not in (0, 0x80):
        kernel.CloseHandle(lock)
        raise RuntimeError('Keyboard is busy; no packets sent')
    device = hid.device()
    result = {'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'channel': 'usb', 'settings_writes': False, 'firmware_writes': False,
              'queries': []}
    def read(command, offset, length):
        data = query(device, command, offset, length)
        result['queries'].append({'command': hex(command), 'offset': offset, 'length': length, 'payload_hex': data.hex()})
        return data
    try:
        endpoints = [d for d in hid.enumerate(0x0C45, 0x80A0)
                     if d['usage_page'] == 0xFF68 and d['usage'] == 97 and d['product_string'] == 'AK820MAX']
        assert len(endpoints) == 1, 'Expected one wired AK820MAX control interface'
        device.open_path(endpoints[0]['path'])
        info = read(0x10, 0, 48)
        assert identity(info), 'Different revision; stopped before probing'
        general = read(0x11, 0, 32)
        lighting = read(0x13, 0, 16)
        for command, offset in [(0x11, 32), (0x13, 16)]:
            try: read(command, offset, 16)
            except RuntimeError as error:
                result['boundary_error'] = str(error)
                break
        after = read(0x10, 0, 48)
        result['identity_unchanged'] = info[4:17] == after[4:17] and info[29:33] == after[29:33]
        result['general_unchanged'] = read(0x11, 0, 32) == general
        result['lighting_unchanged'] = read(0x13, 0, 16) == lighting
    except Exception as error:
        result['error'] = str(error)
    finally:
        device.close()
        kernel.ReleaseMutex(lock)
        kernel.CloseHandle(lock)
        path = ROOT / 'research/local/keyboard-usb-read-window-probe.json'
        path.write_text(json.dumps(result, indent=2), encoding='utf-8')
        print(json.dumps(result, indent=2))
    if result.get('error'): raise SystemExit(1)


if __name__ == '__main__': main()
