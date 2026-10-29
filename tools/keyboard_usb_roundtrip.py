"""Verify USB routing through public Rust operations, with automatic restoration.

Only established configuration commands: brightness, sleep timeout, F12 mapping.
No input keys are pressed by the script. All six blocks are compared afterwards.
"""
from pathlib import Path
import datetime
import json
import subprocess

ROOT = Path(__file__).resolve().parents[1]
LAB = ROOT / 'target/debug/ajazz-lab.exe'
OUT = ROOT / 'research/local'


def run(*args):
    result = subprocess.run([str(LAB), *map(str, args)], cwd=ROOT, capture_output=True)
    if result.returncode:
        try:
            error = result.stderr.decode('utf-8')
        except UnicodeDecodeError:
            error = result.stderr.decode('cp1251', errors='replace')
        raise RuntimeError(error)
    return json.loads(result.stdout)


def save(name, data):
    path = OUT / name
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding='utf-8')
    return path


def matches(before, after):
    if before['endpoint']['path'] != after['endpoint']['path'] or before['info']['profile'] != after['info']['profile']:
        return False
    for name, values in before['blocks'].items():
        indices = [i for i in range(len(values)) if name != 'lighting' or i not in (4, 14, 15)]
        if any(values[i] != after['blocks'][name][i] for i in indices):
            return False
    return True


def main():
    baseline = run('keyboard-usb-snapshot')
    assert baseline['endpoint']['pid'] == 0x80A0 and baseline['info']['work_mode'] == 15
    assert baseline['info']['version'] == '1.30'
    baseline_path = save('keyboard-usb-roundtrip-08-before.json', baseline)
    raw = baseline['blocks']['lighting']
    lighting = {'mode': raw[0], 'color': raw[1:4], 'rainbow': raw[8] != 0,
                'brightness': 4 if raw[9] == 5 else 5, 'speed': raw[10], 'direction': raw[11]}
    raw_general = baseline['blocks']['game_mode']
    general = {'sleep_minutes': 6 if raw_general[3] == 5 else 5, 'key_delay': raw_general[4],
               'top_dead_zone': raw_general[8], 'bottom_dead_zone': raw_general[9],
               'stability': bool(raw_general[11]), 'auto_calibration': bool(raw_general[14])}
    cases = [('lighting', 'lighting-effect', lighting, 'lighting-restore'),
             ('sleep', 'general-apply', general, 'general-restore'),
             ('F12', 'keymap-apply', {'keys': [12], 'assignment': {'Key': {'usage': 68}}}, 'keymap-restore')]
    proof = []
    for label, apply, settings, restore in cases:
        before = run('keyboard-usb-snapshot')
        assert matches(baseline, before), 'Baseline changed; no further writes'
        receipt = None
        try:
            config = save(f'keyboard-usb-roundtrip-08-{label}-edit.json', settings)
            receipt = run(apply, baseline_path, config)
            save(f'keyboard-usb-roundtrip-08-{label}-applied.json', receipt)
            during = run('keyboard-usb-snapshot')
            assert during['endpoint']['path'] == baseline['endpoint']['path']
            block = 'lighting' if label == 'lighting' else 'game_mode' if label == 'sleep' else 'keymap'
            assert during['blocks'][block] != baseline['blocks'][block], 'Change not reflected'
            for name, values in baseline['blocks'].items():
                if name != block:
                    assert during['blocks'][name] == values, f'Unexpected change: {name}'
        finally:
            if receipt and receipt.get('backup_path'):
                restored_receipt = run(restore, receipt['backup_path'])
                save(f'keyboard-usb-roundtrip-08-{label}-restored.json', restored_receipt)
        after = run('keyboard-usb-snapshot')
        assert matches(baseline, after), 'Configuration differs after restoration'
        proof.append({'field': label, 'endpoint': 'USB 0C45:80A0 FF68/97', 'changed_readback': True, 'other_blocks_preserved': True, 'restored': True})
        print(f'{label}: applied, read back, all blocks restored', flush=True)
    save('keyboard-usb-roundtrip-08-proof.json', {'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'cycles': proof, 'physical_key_output_observed': False})


if __name__ == '__main__':
    main()
