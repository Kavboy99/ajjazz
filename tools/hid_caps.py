"""Read HID descriptors only. No device reports are sent."""
import ctypes as c
import json
from pathlib import Path
from ctypes import wintypes as w
import hid

class Caps(c.Structure):
    _fields_ = [(n, w.USHORT) for n in ["usage", "usage_page", "input_bytes", "output_bytes", "feature_bytes"]] + [("reserved", w.USHORT * 17)] + [(n, w.USHORT) for n in ["link_nodes", "input_buttons", "input_values", "input_indices", "output_buttons", "output_values", "output_indices", "feature_buttons", "feature_values", "feature_indices"]]

k = c.WinDLL("kernel32", use_last_error=True)
h = c.WinDLL("hid")
k.CreateFileW.argtypes = [w.LPCWSTR, w.DWORD, w.DWORD, c.c_void_p, w.DWORD, w.DWORD, w.HANDLE]
k.CreateFileW.restype = w.HANDLE
k.CloseHandle.argtypes = [w.HANDLE]
h.HidD_GetPreparsedData.argtypes = [w.HANDLE, c.POINTER(c.c_void_p)]
h.HidD_FreePreparsedData.argtypes = [c.c_void_p]
h.HidP_GetCaps.argtypes = [c.c_void_p, c.POINTER(Caps)]
results = []
for d in hid.enumerate():
    if d["vendor_id"] not in (0x0C45, 0x3151):
        continue
    handle = k.CreateFileW(d["path"].decode(), 0, 3, None, 3, 0, None)
    if handle == c.c_void_p(-1).value:
        continue
    data = c.c_void_p()
    try:
        if h.HidD_GetPreparsedData(handle, c.byref(data)):
            caps = Caps()
            status = h.HidP_GetCaps(data, c.byref(caps))
            if status == 0x110000:
                results.append({"vid": d["vendor_id"], "pid": d["product_id"], "product": d["product_string"], "interface": d["interface_number"], **{n: getattr(caps, n) for n in ("usage", "usage_page", "input_bytes", "output_bytes", "feature_bytes")}})
    finally:
        if data: h.HidD_FreePreparsedData(data)
        k.CloseHandle(handle)
out = json.dumps(results, indent=2, ensure_ascii=False)
Path("research/local/hid-caps.json").write_text(out, encoding="utf8")
print(out)
