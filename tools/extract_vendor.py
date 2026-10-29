"""Extract literal data and research slices. Never execute vendor JavaScript."""
import hashlib
import json
import re
from pathlib import Path
import json5
import jsbeautifier

root = Path(__file__).resolve().parents[1]
vendor = root / "research/vendor"
s = (vendor / "layout-classic-0DHAQysC.js").read_text(encoding="utf8")

def literal(text, marker):
    start = text.index(marker) + len(marker) - 1
    level, quote, escaped = 0, None, False
    for end in range(start, len(text)):
        ch = text[end]
        if quote:
            if escaped: escaped = False
            elif ch == "\\": escaped = True
            elif ch == quote: quote = None
        elif ch in "\"'`": quote = ch
        elif ch == "{": level += 1
        elif ch == "}":
            level -= 1
            if not level: break
    raw = text[start:end+1].replace("!0", "true").replace("!1", "false")
    converted, pos = [], 0
    while pos < len(raw):
        if raw[pos] not in "\"'`":
            converted.append(raw[pos]); pos += 1; continue
        delim, begin = raw[pos], pos
        pos += 1
        while pos < len(raw):
            if raw[pos] == "\\": pos += 2; continue
            if raw[pos] == delim: break
            pos += 1
        segment = raw[begin:pos+1]
        converted.append(json.dumps(re.sub(r"\\(.)", lambda m: {"n": "\n", "r": "\r", "t": "\t"}.get(m[1], m[1]), segment[1:-1]), ensure_ascii=False) if delim == "`" else segment)
        pos += 1
    return json5.loads("".join(converted))

config = literal(s, "qG={")
(vendor / "ak820max-config.json").write_text(json.dumps(config, ensure_ascii=False, indent=2), encoding="utf8")
rows = []
for row in config["keyList"]:
    rows.append([{"id": k["value"], "label": k["name"], "hid": k.get("keyCode", 0), "physical": k.get("key", ""), "width": 6.25 if k.get("key") == "Space" else 2.25 if k.get("key") in ("Enter", "ShiftLeft") else 2.0 if k.get("key") in ("Backspace", "ShiftRight") else 1.5 if k.get("key") in ("Tab", "Backslash") else 1.25 if "Control" in k.get("key", "") or "Alt" in k.get("key", "") or k.get("key") in ("CapsLock", "MetaLeft", "Fn") else 1.0} for k in row])
(root / "assets").mkdir(exist_ok=True)
(root / "assets/ak820max-layout.json").write_text(json.dumps(rows, ensure_ascii=False, indent=2), encoding="utf8")
mouse = (vendor / "mouse-renderer.js").read_text(encoding="utf8")
for name, start_marker, end_marker in [("mouse-apex", "var _ra=", "var "), ("dock", "var qpa=", "$pa=")]:
    start = mouse.index(start_marker)
    end = mouse.find(end_marker, start + len(start_marker)) if name == "dock" else start + 22000
    (vendor / f"{name}-slice.js").write_text(jsbeautifier.beautify(mouse[start:end]), encoding="utf8")
manifest = {p.name: {"sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "bytes": p.stat().st_size} for p in vendor.iterdir() if p.is_file() and ".pretty." not in p.name and "slice" not in p.name}
(root / "research/source-manifest.json").write_text(json.dumps({"collected": "2026-10-16", "keyboard_url": "https://ajazz.driveall.cn/", "mouse_install": "AJAZZ Driver (R) 2.1.94", "files": manifest}, indent=2), encoding="utf8")
print(f"Extracted {sum(map(len, rows))} keyboard keys and source hashes")
