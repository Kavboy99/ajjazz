"""Direct HID, only source-confirmed GET OLED version (AD). No RPC or settings writes."""
import json
import time
from pathlib import Path
import hid

info = next(d for d in hid.enumerate(0x3151, 0x5007) if d["usage_page"] == 0xFFFF and d["usage"] == 2)
packet = bytearray(65)
packet[1] = 0xAD
# BIT7 checksum: header bytes 0..7 sum to FF. Prefix byte is HID report ID.
packet[8] = (0xFF - sum(packet[1:8])) & 255
device = hid.device()
device.open_path(info["path"])
try:
    sent = device.send_feature_report(packet)
    if sent != 65:
        raise RuntimeError("Short feature report")
    time.sleep(.005)
    response = bytes(device.get_feature_report(0, 65))
finally:
    device.close()
if len(response) != 65 or response[0:2] != b"\0\xad":
    raise RuntimeError(f"Unexpected OLED response: {response.hex(' ')}")
result = {"transport": "direct HID Feature, no vendor RPC", "query": list(packet), "response": list(response),
          "oled_version_raw": int.from_bytes(response[2:4], "little"),
          "oled_version_hex": response[2:4][::-1].hex(), "flash_version_raw": int.from_bytes(response[4:6], "little")}
Path("research/local/dock-direct-version.json").write_text(json.dumps(result, indent=2), encoding="utf8")
print(json.dumps({k: v for k, v in result.items() if k not in ("query", "response")}, indent=2))
