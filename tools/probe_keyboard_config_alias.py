"""Bounded GET-only test derived from candidate D35E-D384 copy loop.

An incompatible older image selects flash bases 9200/9400/9A00 for GET11/12/15
and adds a 16-bit offset. This hypothesis is not established for target 80A0.
Test exact aliases first, then just three small code windows; never SET/ISP.
No broad command sweep, address sweep, feature report, reset or boot request.
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

ALLOWED = {(0x10, 0, 48), (0x11, 0, 32), (0x12, 0, 32),
           (0x15, 0, 32), (0x13, 0, 16)} | {
    (0x11, offset, 32) for offset in (0x20, 0x200, 0x800, 0x2E00, 0x2E20, 0x3E00)
}


def query(device, command, offset, length):
    assert (command, offset, length) in ALLOWED
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
        if (len(reply) >= 8 + length and reply[:3] == bytes([0x55, command, length])
                and reply[3:5] == offset.to_bytes(2, 'little')):
            return reply[8:8 + length]
    raise RuntimeError(f'No matching GET{command:02X}/{offset:04X}; stopping')


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
    result = {'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'hypothesis_source_sha256': 'ef67c8a05093961b799eebc39804b89707459c72477c6f26bf25e946b5810850',
              'settings_set_commands': False, 'firmware_program_commands': False,
              'boot_commands': False, 'queries': [], 'firmware_dump_obtained': False}
    device = hid.device()

    def read(command, offset, length):
        payload = query(device, command, offset, length)
        result['queries'].append({'command': hex(command), 'offset': offset,
                                  'length': length, 'payload_hex': payload.hex()})
        return payload

    try:
        if not owned:
            raise RuntimeError('Keyboard mutex busy; no packets sent')
        endpoints = [d for d in hid.enumerate(0x0C45, 0x80A0)
                     if (d['usage_page'], d['usage'], d['product_string']) == (0xFF68, 97, 'AK820MAX')]
        assert len(endpoints) == 1, 'Expected one wired control endpoint'
        device.open_path(endpoints[0]['path'])
        info = read(0x10, 0, 48)
        assert (info[4:10] == bytes.fromhex('450ca0803001')
                and info[12:16] == bytes.fromhex('66010410')), 'Different firmware identity'
        before = {c: read(c, 0, n) for c, n in [(0x11, 32), (0x12, 32), (0x15, 32), (0x13, 16)]}
        samples = {offset: read(0x11, offset, 32) for offset in (0x20, 0x200, 0x800)}
        aliases = {'key': samples[0x200] == before[0x12] and any(before[0x12]),
                   'macro': samples[0x800] == before[0x15] and any(before[0x15])}
        result['nonzero_aliases'] = aliases
        # A nonzero matching reference is required before reading outside config.
        if any(aliases.values()):
            for offset in (0x2E00, 0x2E20, 0x3E00):
                read(0x11, offset, 32)
        else:
            result['code_windows_skipped'] = 'No nonzero alias established; hypothesis unconfirmed'
        after = read(0x10, 0, 48)
        result['identity_unchanged'] = info[4:17] == after[4:17]
        result['settings_unchanged'] = {hex(c): read(c, 0, len(v)) == v for c, v in before.items()}
    except Exception as exc:
        result['error'] = str(exc)
    finally:
        device.close()
        if owned:
            kernel.ReleaseMutex(lock)
        kernel.CloseHandle(lock)
        path = ROOT / 'research/local/keyboard-config-alias-usb-proof.json'
        path.write_text(json.dumps(result, indent=2), encoding='utf8')
        print(json.dumps(result, indent=2))
    if result.get('error'):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
