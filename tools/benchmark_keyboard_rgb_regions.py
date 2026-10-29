"""Bounded USB benchmark of known SET24 subranges; exact config restoration.

No bootloader, erase, firmware, undocumented opcodes or fire-and-forget writes.
"""
import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.tools/py-research'))
import hid

LAB = ROOT / 'target/release/ajazz-lab.exe'
OUT = ROOT / 'research/local'


def run(*args):
    result = subprocess.run([str(LAB), *map(str, args)], cwd=ROOT, capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stderr.decode('utf-8', errors='replace'))
    return json.loads(result.stdout)


def transaction(device, command, address, data=None, length=None, final=True):
    size = len(data) if data is not None else length
    result = bytearray()
    packet_times = []
    for offset in range(0, size, 56):
        count = min(56, size - offset)
        packet = bytearray(65)
        packet[1:9] = bytes([0xaa, command, count, (address + offset) & 255,
                             (address + offset) >> 8, 0, int(final and offset + count == size), 0])
        if data is not None:
            packet[9:9 + count] = data[offset:offset + count]
        started = time.perf_counter()
        device.write(packet)
        deadline = started + 1.5
        while time.perf_counter() < deadline:
            response = bytes(device.read(65, 50))
            if response[:1] == b'\0':
                response = response[1:]
            if (len(response) >= 8 + count and response[:3] == bytes([0x55, command, count])
                    and int.from_bytes(response[3:5], 'little') == address + offset):
                result.extend(response[8:8 + count])
                packet_times.append((time.perf_counter() - started) * 1000)
                break
        else:
            raise RuntimeError(f'No ACK {command:02x} at {address + offset}')
    return bytes(result), packet_times


def main():
    before = run('keyboard-usb-snapshot')
    info = before['info']
    assert (info['vid'], info['pid'], info['version'], info['work_mode']) == (0x0c45, 0x80a0, '1.30', 15)
    original = bytes(before['blocks']['custom_rgb'])
    assert len(original) == 512 and (not any(original) or all(original[i * 4] == i for i in range(128)))
    backup = OUT / 'keyboard-rgb-regions-speed-backup.json'
    backup.write_text(json.dumps(dict(schema=3, before=before, restore_effect=before['blocks']['lighting'])), encoding='utf-8')
    assert before['blocks']['lighting'][0] == 20, 'Select saved-key-colors mode before benchmarking'
    endpoint = before['endpoint']['path'].encode()
    results = []
    device = hid.device()
    device.open_path(endpoint)
    try:
        expected = bytearray(original)
        for size in [4, 56, 112, 512]:
            for index in range(2):
                payload = bytearray(expected[:size])
                for slot in range(size // 4):
                    payload[slot * 4] = slot
                    payload[slot * 4 + 1:slot * 4 + 4] = bytes([180, 0, 40] if index == 0 else [0, 80, 180])
                _, times = transaction(device, 0x24, 0, payload)
                expected[:size] = payload
                observed, _ = transaction(device, 0x14, 0, length=512)
                assert observed == expected, 'Partial update did not preserve the other bytes'
                results.append(dict(bytes=size, write_ms=sum(times), packet_ms=times, verified=True))
                print(json.dumps(results[-1]), flush=True)
    finally:
        device.close()
        restored = run('lighting-restore', backup)
        after = run('keyboard-usb-snapshot')
        report = dict(channel='USB', runs=results, restored=before['blocks'] == after['blocks'],
                      restoration_receipt=restored['backup_path'])
        (OUT / 'keyboard-rgb-regions-speed-proof.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        assert report['restored'], 'Baseline mismatch after restoration'


if __name__ == '__main__':
    main()
