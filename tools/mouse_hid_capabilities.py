"""Inspect Windows HID parser metadata for mouse/dock; sends no USB reports.

Structure layouts: local MinGW hidpi.h (HIDP_CAPS, *_CAPS are 64/72 bytes).
Opening with desired access 0 only queries Windows' cached descriptor metadata.
"""
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import hid


class Caps(c.Structure):
    _fields_ = [(name, w.USHORT) for name in (
        'usage', 'usage_page', 'input_bytes', 'output_bytes', 'feature_bytes')]
    _fields_ += [('reserved', w.USHORT * 17)]
    _fields_ += [(name, w.USHORT) for name in (
        'link_nodes', 'input_buttons', 'input_values', 'input_indices',
        'output_buttons', 'output_values', 'output_indices',
        'feature_buttons', 'feature_values', 'feature_indices')]


class ReportCaps(c.Structure):
    _fields_ = [('usage_page', w.USHORT), ('report_id', w.BYTE), ('is_alias', w.BYTE),
                ('bit_field', w.USHORT), ('link_collection', w.USHORT),
                ('link_usage', w.USHORT), ('link_usage_page', w.USHORT),
                ('remainder', w.BYTE * 60)]


def main():
    assert c.sizeof(Caps) == 64 and c.sizeof(ReportCaps) == 72
    kernel = c.WinDLL('kernel32', use_last_error=True)
    parser = c.WinDLL('hid')
    kernel.CreateFileW.argtypes = [w.LPCWSTR, w.DWORD, w.DWORD, c.c_void_p,
                                  w.DWORD, w.DWORD, w.HANDLE]
    kernel.CreateFileW.restype = w.HANDLE
    kernel.CloseHandle.argtypes = [w.HANDLE]
    parser.HidD_GetPreparsedData.argtypes = [w.HANDLE, c.POINTER(c.c_void_p)]
    parser.HidD_FreePreparsedData.argtypes = [c.c_void_p]
    parser.HidP_GetCaps.argtypes = [c.c_void_p, c.POINTER(Caps)]
    for name in ('HidP_GetButtonCaps', 'HidP_GetValueCaps'):
        getattr(parser, name).argtypes = [c.c_int, c.POINTER(ReportCaps),
                                         c.POINTER(w.USHORT), c.c_void_p]
    results = []
    for device in hid.enumerate(0x3151):
        if device['product_id'] not in (0x4026, 0x5007):
            continue
        handle = kernel.CreateFileW(device['path'].decode(), 0, 3, None, 3, 0, None)
        if handle == c.c_void_p(-1).value:
            raise OSError(c.get_last_error(), 'Cannot inspect HID collection')
        data = c.c_void_p()
        try:
            if not parser.HidD_GetPreparsedData(handle, c.byref(data)):
                raise OSError(c.get_last_error(), 'HID parser data unavailable')
            caps = Caps()
            if parser.HidP_GetCaps(data, c.byref(caps)) != 0x110000:
                raise RuntimeError('Invalid HID capabilities')
            reports = []
            for report_type, direction in enumerate(('input', 'output', 'feature')):
                for kind, api in (('buttons', 'HidP_GetButtonCaps'), ('values', 'HidP_GetValueCaps')):
                    length = getattr(caps, f'{direction}_{kind}')
                    if not length:
                        continue
                    if length > 64:
                        raise ValueError('Unexpected capability count')
                    array = (ReportCaps * length)()
                    count = w.USHORT(length)
                    status = getattr(parser, api)(report_type, array, c.byref(count), data)
                    if status != 0x110000 or count.value > length:
                        raise RuntimeError(f'{api}: 0x{status & 0xffffffff:08x}')
                    reports.extend({'direction': direction, 'kind': kind,
                                    'report_id': entry.report_id, 'usage_page': entry.usage_page}
                                   for entry in array[:count.value])
            results.append({'pid': device['product_id'], 'path': device['path'].decode(),
                            'interface': device['interface_number'],
                            **{name: getattr(caps, name) for name in (
                                'usage', 'usage_page', 'input_bytes', 'output_bytes', 'feature_bytes')},
                            'reports': reports})
        finally:
            if data:
                parser.HidD_FreePreparsedData(data)
            kernel.CloseHandle(handle)
    output = {'device_reports_sent': False, 'collections': results}
    target = Path('research/local/mouse-firmware-20261021/hid-report-capabilities.json')
    target.write_text(json.dumps(output, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(output, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
