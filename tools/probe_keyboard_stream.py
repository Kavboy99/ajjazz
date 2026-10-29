"""Bounded experiment: vendor xZ radio RGB frames, then restore saved lighting.

No bootloader/firmware commands. Captures the current state before any RGB output.
"""
import datetime
import json
from pathlib import Path
import subprocess
import time
import hid
import argparse

parser = argparse.ArgumentParser()
parser.add_argument("--format", choices=("32", "35"), default="35")
arguments = parser.parse_args()

root = Path(__file__).resolve().parents[1]
lab = root / "target/release/ajazz-lab.exe"
snapshot_path = root / "research/local/pre-stream-snapshot.json"
print("Waiting up to 40 seconds for the keyboard to wake", flush=True)
deadline = time.monotonic() + 40
while True:
    captured = subprocess.run([str(lab), "snapshot", str(snapshot_path)], capture_output=True, text=True)
    if captured.returncode == 0:
        break
    if time.monotonic() >= deadline:
        raise RuntimeError("Keyboard did not wake; no temporary frames sent")
    time.sleep(.5)
snapshot = json.loads(snapshot_path.read_text(encoding="utf8"))
if snapshot["info"]["version"] != "1.30" or snapshot["info"]["pid"] != 0x80A0:
    raise RuntimeError("This experiment is limited to the observed keyboard revision")
backup_path = root / "research/local/pre-stream-lighting.json"
backup_path.write_text(json.dumps({"schema": 1, "created_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "info": snapshot["info"], "state": {"lighting": snapshot["blocks"]["lighting"], "custom_rgb": snapshot["blocks"]["custom_rgb"]}}, indent=2), encoding="utf8")
endpoint = next(d for d in hid.enumerate(0x0C45, 0xFEFC) if d["usage_page"] == 0xFF80 and d["usage"] == 1)
device = hid.device()
device.open_path(endpoint["path"])
acknowledgements = []
try:
    print(f"Sending alternating red/blue temporary RGB format 0x{arguments.format} for 60 seconds", flush=True)
    for frame in range(60):
        if arguments.format == "35":
            color = b"\xf8\x00" if frame % 2 == 0 else b"\x00\x1f"
            data = color * 82
            size = 28
        else:
            color = bytes((255, 0, 0) if frame % 2 == 0 else (0, 0, 255))
            layout = json.loads((root / "assets/ak820max-layout.json").read_text(encoding="utf8"))
            data = b"".join(bytes((key["id"],)) + color for row in layout for key in row)
            size = 24
        for index, offset in enumerate(range(0, len(data), size)):
            chunk = data[offset:offset + size]
            if arguments.format == "35":
                header = bytes((0xAA, 0x35, index, 0))
            else:
                header = bytes((0xAA, 0x32, len(chunk), offset & 255, offset >> 8, 0, int(offset + size >= len(data)), 0))
            packet = b"\0" + header + chunk + bytes(size - len(chunk))
            if device.write(packet) != 33:
                raise RuntimeError("Short RGB output")
            # xZ waits at most 10 ms for each non-retried temporary command.
            response = bytes(device.read(65, 10))
            if frame == 0:
                acknowledgements.append(list(response))
        time.sleep(.9)
finally:
    device.close()
    subprocess.run([str(lab), "lighting-restore", str(backup_path)], check=True)
    (root / f"research/local/stream-{arguments.format}-acks.json").write_text(json.dumps(acknowledgements, indent=2), encoding="utf8")
    print("Previous lighting restored and verified", flush=True)
