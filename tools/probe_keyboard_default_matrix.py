"""Read-only test of the site's default-matrix commands on exact USB80A0/1.30.

GET1C is used by lZ in the site; GET1F is defined in its protocol enum. A
32-byte nonzero default table is required before two bounded offset checks.
No unknown command sweep, SET, boot, security, or flash-program operations.
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


def read(device, command, offset, length):
    assert (command == 0x10 and offset == 0 and length == 48) or (
        command in (0x1C, 0x1F) and offset in (0, 0x20, 0x200, 0x1000) and length == 32)
    for _ in range(32):
        if not device.read(65, 2):
            break
    packet = bytearray(65)
    packet[1:9] = bytes([0xAA, command, length, offset & 255, offset >> 8, 0, 1, 0])
    assert device.write(packet) == 65
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        reply = bytes(device.read(65, 100))
        if reply[:1] == b'\0':
            reply = reply[1:]
        if len(reply) >= 8 + length and reply[:3] == bytes([0x55, command, length]) and reply[3:5] == offset.to_bytes(2, 'little'):
            return reply[8:8 + length]
    raise RuntimeError(f'No matching reply to GET{command:02X}/{offset:04X}')


def main():
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_wchar_p]
    kernel.CreateMutexW.restype = ctypes.c_void_p
    kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint]
    kernel.ReleaseMutex.argtypes = [ctypes.c_void_p]
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    lock = kernel.CreateMutexW(None, 0, 'Local\\AjazzStudio-AK820MAX')
    assert lock
    owned = kernel.WaitForSingleObject(lock, 0) in (0, 0x80)
    device = hid.device()
    result = {'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'queries': [], 'settings_set_commands': False, 'firmware_program_commands': False,
              'boot_commands': False, 'firmware_dump_obtained': False}

    def query(command, offset, length):
        data = read(device, command, offset, length)
        result['queries'].append({'command': hex(command), 'offset': offset, 'payload_hex': data.hex()})
        return data

    try:
        if not owned:
            raise RuntimeError('Mutex busy; no reports sent')
        endpoints = [d for d in hid.enumerate(0x0C45, 0x80A0)
                     if (d['usage_page'], d['usage'], d['product_string']) == (0xFF68, 97, 'AK820MAX')]
        assert len(endpoints) == 1
        device.open_path(endpoints[0]['path'])
        before = query(0x10, 0, 48)
        assert before[4:10] == bytes.fromhex('450ca0803001') and before[12:16] == bytes.fromhex('66010410')
        for command in (0x1C, 0x1F):
            try:
                first = query(command, 0, 32)
                second = query(command, 0x20, 32)
            except RuntimeError as exc:
                result.setdefault('unsupported', []).append(str(exc))
                continue
            if any(first + second):
                for offset in (0x200, 0x1000):
                    query(command, offset, 32)
        after = query(0x10, 0, 48)
        result['identity_unchanged'] = before[4:17] == after[4:17]
    except Exception as exc:
        result['error'] = str(exc)
    finally:
        device.close()
        if owned:
            kernel.ReleaseMutex(lock)
        kernel.CloseHandle(lock)
        (ROOT / 'research/local/keyboard-default-matrix-usb-proof.json').write_text(json.dumps(result, indent=2), encoding='utf8')
        print(json.dumps(result, indent=2))
    if result.get('error'):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
