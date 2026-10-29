"""Read current mouse/dock USB configuration through Windows hub IOCTLs.

Only node information and standard GET_DESCRIPTOR(configuration) are allowed.
No HID settings, driver changes, interface claiming, resets or boot commands.
See Microsoft USB_DESCRIPTOR_REQUEST docs and libusb windows_winusb.h layouts.
"""
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import struct
import uuid

HUB = uuid.UUID('f18a0e88-c30c-11d0-8815-00a0c906bed8').bytes_le
GET_NODE, GET_DESCRIPTOR, GET_CONNECTION = 0x220408, 0x220410, 0x220448
ALLOWED = {GET_NODE, GET_DESCRIPTOR, GET_CONNECTION}
TARGETS = {(0x3151, 0x4026), (0x3151, 0x5007), (0x3151, 0x5008)}


class Guid(c.Structure):
    _fields_ = [('raw', c.c_ubyte * 16)]


class Interface(c.Structure):
    _fields_ = [('size', w.DWORD), ('guid', Guid), ('flags', w.DWORD), ('reserved', c.c_void_p)]


def apis():
    kernel, setup = c.WinDLL('kernel32', use_last_error=True), c.WinDLL('setupapi', use_last_error=True)
    kernel.CreateFileW.argtypes = [w.LPCWSTR, w.DWORD, w.DWORD, c.c_void_p, w.DWORD, w.DWORD, w.HANDLE]
    kernel.CreateFileW.restype = w.HANDLE
    kernel.CloseHandle.argtypes = [w.HANDLE]
    kernel.DeviceIoControl.argtypes = [w.HANDLE, w.DWORD, c.c_void_p, w.DWORD, c.c_void_p, w.DWORD, c.POINTER(w.DWORD), c.c_void_p]
    setup.SetupDiGetClassDevsW.argtypes = [c.POINTER(Guid), w.LPCWSTR, w.HWND, w.DWORD]
    setup.SetupDiGetClassDevsW.restype = w.HANDLE
    setup.SetupDiEnumDeviceInterfaces.argtypes = [w.HANDLE, c.c_void_p, c.POINTER(Guid), w.DWORD, c.POINTER(Interface)]
    setup.SetupDiGetDeviceInterfaceDetailW.argtypes = [w.HANDLE, c.POINTER(Interface), c.c_void_p, w.DWORD, c.POINTER(w.DWORD), c.c_void_p]
    setup.SetupDiDestroyDeviceInfoList.argtypes = [w.HANDLE]
    return kernel, setup


def hubs(setup):
    guid = Guid((c.c_ubyte * 16).from_buffer_copy(HUB))
    handle = setup.SetupDiGetClassDevsW(c.byref(guid), None, None, 0x12)
    if handle == c.c_void_p(-1).value:
        raise c.WinError(c.get_last_error())
    try:
        for index in range(64):
            entry = Interface()
            entry.size = c.sizeof(entry)
            if not setup.SetupDiEnumDeviceInterfaces(handle, None, c.byref(guid), index, c.byref(entry)):
                if c.get_last_error() == 259:
                    return
                raise c.WinError(c.get_last_error())
            required = w.DWORD()
            setup.SetupDiGetDeviceInterfaceDetailW(handle, c.byref(entry), None, 0, c.byref(required), None)
            if not 8 <= required.value <= 16384:
                raise ValueError('Unexpected hub path size')
            buffer = c.create_string_buffer(required.value)
            struct.pack_into('<I', buffer, 0, 8 if c.sizeof(c.c_void_p) == 8 else 6)
            if not setup.SetupDiGetDeviceInterfaceDetailW(handle, c.byref(entry), buffer, len(buffer), c.byref(required), None):
                raise c.WinError(c.get_last_error())
            yield c.wstring_at(c.addressof(buffer) + 4)
        raise ValueError('Hub enumeration limit exceeded')
    finally:
        setup.SetupDiDestroyDeviceInfoList(handle)


def ioctl(kernel, handle, code, content, output_size):
    if code not in ALLOWED or not len(content) <= output_size <= 4096:
        raise ValueError('Not an allowed bounded descriptor query')
    buffer = c.create_string_buffer(output_size)
    c.memmove(buffer, content, len(content))
    count = w.DWORD()
    if not kernel.DeviceIoControl(handle, code, buffer, len(content), buffer, len(buffer), c.byref(count), None):
        raise c.WinError(c.get_last_error())
    if count.value > output_size:
        raise ValueError('Oversize IOCTL response')
    return bytes(buffer[:count.value])


def configuration(kernel, handle, port, index, length):
    if not 9 <= length <= 4084 or not 0 <= index <= 7:
        raise ValueError('Invalid configuration descriptor request')
    packet = struct.pack('<IBBHHH', port, 0x80, 6, 0x200 | index, 0, length)
    data = ioctl(kernel, handle, GET_DESCRIPTOR, packet, 12 + length)
    if len(data) < 21:
        raise ValueError('Truncated configuration descriptor')
    return data[12:]


def decode_configuration(data):
    if len(data) < 9 or data[1] != 2 or struct.unpack_from('<H', data, 2)[0] != len(data):
        raise ValueError('Invalid complete configuration descriptor')
    descriptors, offset = [], 0
    while offset < len(data):
        length = data[offset]
        if length < 2 or offset + length > len(data):
            raise ValueError('Invalid descriptor boundary')
        d = data[offset:offset + length]
        item = {'type': d[1], 'hex': d.hex(' ')}
        if d[1] == 4:
            if length != 9:
                raise ValueError('Malformed interface descriptor')
            item.update(interface=d[2], alternate=d[3], endpoints=d[4],
                        interface_class=d[5], subclass=d[6], protocol=d[7])
        elif d[1] == 5 and length >= 7:
            item.update(endpoint=d[2], attributes=d[3], max_packet=struct.unpack_from('<H', d, 4)[0], interval=d[6])
        descriptors.append(item)
        offset += length
    return {'interface_count': data[4], 'descriptors': descriptors}


def main():
    kernel, setup = apis()
    report = {'settings_written': False, 'boot_requested': False, 'standard_configuration_gets_only': True,
              'devices': [], 'hub_errors': [], 'hubs_inspected': 0}
    for path in hubs(setup):
        handle = kernel.CreateFileW(path, 0, 3, None, 3, 0, None)
        if handle == c.c_void_p(-1).value:
            report['hub_errors'].append({'operation': 'open', 'error': c.get_last_error()})
            continue
        try:
            node = ioctl(kernel, handle, GET_NODE, bytes(76), 76)
            if len(node) < 7 or not 1 <= node[6] <= 127:
                raise ValueError('Unexpected USB hub port count')
            report['hubs_inspected'] += 1
            for port in range(1, node[6] + 1):
                info = ioctl(kernel, handle, GET_CONNECTION, struct.pack('<I', port) + bytes(31), 4096)
                if len(info) < 35:
                    continue
                vid, pid = struct.unpack_from('<HH', info, 12)
                if (vid, pid) not in TARGETS:
                    continue
                device = {'vid': vid, 'pid': pid, 'speed': info[23], 'device_descriptor_hex': info[4:22].hex(' '), 'configurations': []}
                report['devices'].append(device)
                if not 1 <= info[21] <= 8:
                    raise ValueError('Unexpected USB configuration count')
                for index in range(info[21]):
                    header = configuration(kernel, handle, port, index, 9)
                    total = struct.unpack_from('<H', header, 2)[0]
                    full = configuration(kernel, handle, port, index, total)
                    device['configurations'].append(decode_configuration(full))
        except (OSError, ValueError) as error:
            report['hub_errors'].append({'operation': 'read', 'error': str(error)})
        finally:
            kernel.CloseHandle(handle)
    out = Path(__file__).resolve().parents[1] / 'research/local/mouse-firmware-20261021/usb-configuration-descriptors.json'
    out.write_text(json.dumps(report, indent=2), encoding='utf-8')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
