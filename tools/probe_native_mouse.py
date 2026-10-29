"""Source-confirmed RF routing GETs from qmk.top YP; never writes settings.

F1 = dock ID, F6/05 selects mouse transport, F7 = mailbox status,
FC = consume reply. 8F/D3/D4 are the established identity/settings GETs.
"""
import json
import time
from pathlib import Path
import hid

device = hid.device()
endpoint = next(d for d in hid.enumerate(0x3151, 0x5007)
                if d['usage_page'] == 0xffff and d['usage'] == 2)
device.open_path(endpoint['path'])
events = []

def send(data):
    packet = bytes([0]) + bytes(data) + bytes(64 - len(data))
    count = device.send_feature_report(packet)
    if count != 65:
        raise RuntimeError(f'Short feature output: {count}')
    time.sleep(.01)

def raw_read():
    raw = bytes(device.get_feature_report(0, 65))
    if len(raw) != 65 or raw[0] != 0:
        raise RuntimeError(f'Unexpected report {raw.hex()}')
    return raw[1:]

def status():
    send([0xf7])
    return raw_read()

def mouse_get(command, profile=0):
    send([0xf6, 5])
    for _ in range(5):
        time.sleep(.1)
        if status()[5] == 1:
            break
    else:
        raise RuntimeError('Mouse mailbox cannot send')
    packet = bytearray(64)
    packet[0] = command
    packet[1] = profile
    packet[7] = (255 - sum(packet[:7])) & 255
    send(packet)
    for _ in range(5):
        time.sleep(.1)
        if status()[0] == 1:
            break
    else:
        raise RuntimeError('Mouse mailbox reply did not become ready')
    send([0xfc])
    reply = raw_read()
    if reply[0] != command:
        raise RuntimeError(f'Wrong reply for {command:02x}: {reply.hex()}')
    return list(reply)

try:
    send([0xf1])
    identity = raw_read()
    events.append({'dock_id_report': list(identity)})
    if identity[0] != 0xf1 or int.from_bytes(identity[1:3], 'little') != 1759:
        raise RuntimeError('Dock identity is not ID1759')
    print('Native mailbox ready; waiting up to 60 seconds for a physical mouse wake', flush=True)
    deadline = time.monotonic() + 60
    while True:
        current = status()
        events.append({'status': list(current[:9])})
        if current[4] == 0:
            break
        if time.monotonic() >= deadline:
            raise RuntimeError('Mouse asleep; no settings GET sent')
        time.sleep(.25)
    ident = mouse_get(0x8f)
    events.append({'mouse_id_report': ident})
    if int.from_bytes(bytes(ident[1:5]), 'little') not in (1755, 0x800006db):
        raise RuntimeError('Mouse identity mismatch')
    options = mouse_get(0xd3)
    events.append({'raw_options': options})
    events.append({'raw_dpi': mouse_get(0xd4, options[1])})
    print('Native D3/D4 GET completed', flush=True)
except Exception as error:
    events.append({'error': str(error)})
    print(str(error), flush=True)
finally:
    device.close()
    Path('research/local/native-mouse-probe-05.json').write_text(
        json.dumps(events, indent=2), encoding='utf-8')
