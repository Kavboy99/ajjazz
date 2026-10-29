"""Only the source-confirmed GET_DEVICE_INFO query; no settings or firmware writes."""
import hid
import json
from pathlib import Path

info = next(d for d in hid.enumerate(0x0C45, 0xFEFC) if d["usage_page"] == 0xFF80 and d["usage"] == 1)
device = hid.device()
device.open_path(info["path"])
responses = []
try:
    for address in (0, 24):
        packet = [0] * 33
        packet[1:9] = [0xAA, 0x10, 24, address, 0, 0, int(address == 24), 0]
        device.write(packet)
        for _ in range(8):
            data = bytes(device.read(65, 500))
            if len(data) > 1 and data[0] == 0: data = data[1:]
            if len(data) >= 32 and data[:2] == bytes((0x55, 0x10)) and data[3] == address:
                responses.append(list(data)); break
        else: raise RuntimeError("No matching GET_DEVICE_INFO response")
finally:
    device.close()
result = {"query": "GET_DEVICE_INFO 0x10", "responses": responses, "payload": [v for row in responses for v in row[8:32]]}
Path("research/local/keyboard-info.json").write_text(json.dumps(result, indent=2), encoding="utf8")
print(json.dumps(result, indent=2))
