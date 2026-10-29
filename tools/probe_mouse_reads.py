"""Source-confirmed GETs via the running vendor service. No settings writes."""
import http.client
import json
import time
from pathlib import Path
from driver_inventory import fields


def vint(value):
    out = bytearray()
    while value >= 128:
        out.append((value & 127) | 128)
        value >>= 7
    return bytes(out + bytes((value,)))


def blob(number, value):
    return vint(number * 8 + 2) + vint(len(value)) + value


def rpc(method, message):
    conn = http.client.HTTPConnection("127.0.0.1", 3814, timeout=3)
    try:
        conn.request("POST", "/driver.DriverGrpc/" + method,
                     body=b"\0" + len(message).to_bytes(4, "big") + message,
                     headers={"Content-Type": "application/grpc-web+proto", "X-Grpc-Web": "1"})
        response = conn.getresponse()
        header = response.read(5)
        if response.status != 200 or len(header) != 5 or header[0] != 0:
            raise RuntimeError(f"Unexpected RPC response: {response.status}, {header.hex()}")
        size = int.from_bytes(header[1:], "big")
        if size > 65536:
            raise RuntimeError("Oversize RPC response")
        return fields(response.read(size))
    finally:
        conn.close()


def get(path, command, route):
    raw = command == 0xAD
    message = blob(1, path.encode()) + blob(2, bytes((command,)) + bytes(63 if raw else 7))
    if route:
        message += bytes((32, route))
    sent = rpc("sendRawFeature" if raw else "sendMsg", message)
    err = next((v.decode() for n, w, v in sent if n == 1), "")
    if err:
        raise RuntimeError(err)
    time.sleep(.005)
    response = rpc("readRawFeature" if raw else "readMsg", blob(1, path.encode()))
    err = next((v.decode() for n, w, v in response if n == 1), "")
    if err:
        raise RuntimeError(err)
    data = next((v for n, w, v in response if n == 2), b"")
    return {"command": command, "route": route, "bytes": list(data), "hex": data.hex(" ")}


if __name__ == "__main__":
    entries = fields(Path("research/local/driver-inventory.bin").read_bytes())
    paths = []
    for n, w, entry in entries:
        for tag, wire, common in fields(entry):
            if tag == 2:
                values = fields(common)
                paths.extend(v.decode() for key, wire, v in values if key == 3)
    if len(paths) != 1:
        raise RuntimeError("Expected one dock in the vendor inventory")
    result = []
    for command, route in ((0xAD, 0), (0xD3, 2), (0xD4, 2)):
        try:
            result.append(get(paths[0], command, route))
        except Exception as error:
            result.append({"command": command, "route": route, "error": str(error)})
    Path("research/local/mouse-read-probes.json").write_text(json.dumps(result, indent=2), encoding="utf8")
    print(json.dumps(result, indent=2))
