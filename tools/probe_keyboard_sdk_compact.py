"""Exact SDK 2.0.2 USB setMusicData framing with 82 physical records.

Earlier USB probes sent a 128-slot saved table. This probe sends the SDK's
compact physical-key list (328 bytes), not that 512-byte storage table.
Bounded temporary RGB only, and verified full configuration restoration.
"""
import argparse
import json
import time
from pathlib import Path
from benchmark_keyboard_rgb_regions import ROOT, OUT, LAB, run, transaction, hid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--seconds', type=int, default=60)
    parser.add_argument('--enable-custom', action='store_true', help='Bounded test of named 0x30/0x31 custom-function pair; no firmware writes')
    args = parser.parse_args()
    assert 5 <= args.seconds <= 120
    before = run('keyboard-usb-snapshot')
    info = before['info']
    assert (info['vid'], info['pid'], info['version'], info['work_mode'], info['product']) == (0x0c45, 0x80a0, '1.30', 15, 4100)
    assert before['blocks']['lighting'][0] == 20, 'Expected saved-colors mode'
    keys = [key['id'] for row in json.loads((ROOT / 'assets/ak820max-layout.json').read_text(encoding='utf-8')) for key in row]
    assert len(keys) == len(set(keys)) == 82 and max(keys) < 128
    label = 'custom-gate' if args.enable_custom else 'compact'
    backup = OUT / f'keyboard-sdk-{label}-before.json'
    backup.write_text(json.dumps(dict(schema=3, before=before, restore_effect=before['blocks']['lighting'])), encoding='utf-8')
    report = {'source': '@hfd-sdk/keyboard-sdk@2.0.2 Ke / setMusicData, USB64',
              'data_bytes': len(keys) * 4, 'frames': [], 'firmware_writes': False}
    device = hid.device()
    device.open_path(before['endpoint']['path'].encode())
    error = None
    try:
        if args.enable_custom:
            # Named custom-function ON/OFF constants in HFD's protocol enum.
            # The current SDK has no caller; support is explicitly unproven.
            # Neither packet contains an address, firmware or erase operation.
            effect = bytearray(before['blocks']['lighting'])
            effect[0], effect[4], effect[14], effect[15] = 128, 255, 170, 85
            transaction(device, 0x23, 0, effect)
            time.sleep(0.2)
            _, times = transaction(device, 0x30, 0, bytes(24))
            report['custom_on_ack_ms'] = times
        started = time.monotonic()
        print(f'Exact SDK compact RGB for {args.seconds}s: red / blue every 2s', flush=True)
        while time.monotonic() - started < args.seconds:
            elapsed = time.monotonic() - started
            color = [255, 0, 0] if int(elapsed / 2) % 2 == 0 else [0, 0, 255]
            payload = bytes(value for key in keys for value in [key, *color])
            _, timings = transaction(device, 0x32, 0, payload)
            report['frames'].append({'elapsed': elapsed, 'color': color, 'packet_ms': timings, 'frame_ms': sum(timings)})
            if len(report['frames']) % 10 == 1:
                print(f'{elapsed:.1f}s color={color} frame={sum(timings):.1f}ms', flush=True)
            time.sleep(max(0.0, 0.1 - sum(timings) / 1000))
    except Exception as exc:
        error = repr(exc)
        report['error'] = error
    finally:
        if args.enable_custom:
            try:
                _, times = transaction(device, 0x31, 0, bytes(24))
                report['custom_off_ack_ms'] = times
            except Exception as exc:
                report['custom_off_error'] = repr(exc)
        device.close()
        try:
            restored = run('lighting-restore', backup)
            after = run('keyboard-usb-snapshot')
            report['restored'] = before['blocks'] == after['blocks']
            report['restoration_receipt'] = restored['backup_path']
        except Exception as exc:
            report['restore_error'] = repr(exc)
        (OUT / f'keyboard-sdk-{label}-proof.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps({k: v for k, v in report.items() if k != 'frames'}), flush=True)
    assert not error, error
    assert report.get('restored'), 'Restoration not confirmed'


if __name__ == '__main__':
    main()
