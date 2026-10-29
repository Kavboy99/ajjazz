"""One ISP version query on the existing FF67 Feature interface, no reboot.

Exact target identity is required. Only the documented 01 AA 55 version opcode
is sent, never the boot magic, security, erase, program or reset commands.
An unsupported/zero reply ends the experiment; no command or report-ID sweep.
"""
import ctypes
import datetime
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.tools/py-research'))
from probe_keyboard_config_alias import query
import hid


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
    control, feature = hid.device(), hid.device()
    result = {'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'mode_transition_requested': False, 'firmware_program_commands': False,
              'security_commands': False, 'settings_set_commands': False,
              'isp_version_acknowledged': False, 'firmware_dump_obtained': False}
    try:
        if not owned:
            raise RuntimeError('Keyboard mutex busy; no reports sent')
        endpoints = hid.enumerate(0x0C45, 0x80A0)
        def endpoint(page):
            found = [d for d in endpoints if (d['usage_page'], d['usage'], d['product_string']) == (page, 97, 'AK820MAX')]
            assert len(found) == 1, 'Expected one exact target endpoint'
            return found[0]
        control.open_path(endpoint(0xFF68)['path'])
        info = query(control, 0x10, 0, 48)
        assert info[4:10] == bytes.fromhex('450ca0803001') and info[12:16] == bytes.fromhex('66010410')
        before = {c: query(control, c, 0, n) for c, n in [(0x11, 32), (0x13, 16)]}
        feature.open_path(endpoint(0xFF67)['path'])
        result['feature_before_hex'] = bytes(feature.get_feature_report(0, 65)).hex()
        report = bytearray(65)
        report[1:5] = bytes.fromhex('01aa5500')
        assert bytes(report[1:]) == bytes.fromhex('01aa5500') + bytes(60)
        result['version_request_hex'] = report.hex()
        result['feature_send_bytes'] = feature.send_feature_report(report)
        reply = bytes(feature.get_feature_report(0, 65))
        result['version_reply_hex'] = reply.hex()
        payload = reply[1:] if len(reply) == 65 and reply[0] == 0 else reply
        result['isp_version_acknowledged'] = payload[:8] == bytes.fromhex('01aa5500fafafafa')
        after = query(control, 0x10, 0, 48)
        result['identity_unchanged'] = info[4:17] == after[4:17]
        result['settings_unchanged'] = {hex(c): query(control, c, 0, len(v)) == v for c, v in before.items()}
    except Exception as exc:
        result['error'] = str(exc)
    finally:
        feature.close()
        control.close()
        if owned:
            kernel.ReleaseMutex(lock)
        kernel.CloseHandle(lock)
        path = ROOT / 'research/local/keyboard-isp-presence-usb-proof.json'
        path.write_text(json.dumps(result, indent=2), encoding='utf8')
        print(json.dumps(result, indent=2))
    if result.get('error'):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
