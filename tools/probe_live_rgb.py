"""Source-confirmed radio RGB read only (wZ, command 0x37, 82 row-order keys)."""
import json
import time
from pathlib import Path
import hid


def read_colors(device):
    reports = []
    for index in range(6):
        device.write(bytes([0, 0xAA, 0x37, index, 0] + [0] * 28))
        deadline = time.monotonic() + 1.5
        while time.monotonic() < deadline:
            data = bytes(device.read(65, 50))
            if len(data) == 33 and data[0] == 0:
                data = data[1:]
            if len(data) == 32 and data[:3] == bytes((0x55, 0x37, index)):
                reports.append(data)
                break
        else:
            raise RuntimeError(f"No RGB read response for chunk {index}")
    payload = b"".join(row[4:] for row in reports)[:164]
    colors = []
    for offset in range(0, len(payload), 2):
        value = int.from_bytes(payload[offset:offset + 2], "big")
        colors.append([(value >> 11 & 31) << 3, (value >> 5 & 63) << 2, (value & 31) << 3])
    return {"battery": reports[0][3], "colors": colors, "reports": [list(row) for row in reports]}


if __name__ == "__main__":
    info = next(d for d in hid.enumerate(0x0C45, 0xFEFC) if d["usage_page"] == 0xFF80 and d["usage"] == 1)
    device = hid.device()
    device.open_path(info["path"])
    try:
        result = read_colors(device)
    finally:
        device.close()
    Path("research/local/live-rgb-read.json").write_text(json.dumps(result, indent=2), encoding="utf8")
    print(json.dumps({"battery": result["battery"], "keys": len(result["colors"]), "first_colors": result["colors"][:16]}, indent=2))
