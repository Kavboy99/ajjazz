"""Bounded USB check of source-confirmed MT/TGL/RS/SOCD keymap records.

Uses F11/F12, never presses keys, restores the original keymap after every case,
and checks every other configuration block during and after each write.
"""
from pathlib import Path
import datetime
import json
from keyboard_usb_roundtrip import run, save, matches


def main():
    baseline = run('keyboard-usb-snapshot')
    assert baseline['endpoint']['pid'] == 0x80A0 and baseline['info']['work_mode'] == 15
    assert baseline['info']['version'] == '1.30'
    source = save('keyboard-advanced-usb-08-before.json', baseline)
    cases = [
        ('TGL', [12], {'Toggle': {'usage': 68}}, [10, 68, 0, 0]),
        ('MT', [12], {'ModTap': {'hold': 224, 'tap': 68, 'hold_ms': 400}}, [9, 224, 68, 40]),
        ('RS', [11, 12], {'RappySnappy': {'first': 68, 'second': 69}}, [12, 0, 68, 69]),
    ]
    for mode in (3, 1, 2, 4):
        cases.append((f'SOCD-{mode}', [11, 12], {'Socd': {'mode': mode, 'first': 68, 'second': 69}}, [11, mode, 68, 69]))
    proof = []
    for name, keys, assignment, wanted in cases:
        before = run('keyboard-usb-snapshot')
        assert matches(baseline, before), 'Configuration changed; stopping before a write'
        edit = save(f'keyboard-advanced-usb-08-{name}-edit.json', {'keys': keys, 'assignment': assignment})
        receipt = None
        try:
            receipt = run('keymap-apply', source, edit)
            save(f'keyboard-advanced-usb-08-{name}-applied.json', receipt)
            after = run('keyboard-usb-snapshot')
            for key in keys:
                assert after['blocks']['keymap'][key * 4:key * 4 + 4] == wanted
            for key in range(128):
                if key not in keys:
                    assert after['blocks']['keymap'][key * 4:key * 4 + 4] == baseline['blocks']['keymap'][key * 4:key * 4 + 4]
            for block in baseline['blocks']:
                if block != 'keymap':
                    assert after['blocks'][block] == baseline['blocks'][block]
        finally:
            if receipt and receipt.get('backup_path'):
                save(f'keyboard-advanced-usb-08-{name}-restored.json', run('keymap-restore', receipt['backup_path']))
        restored = run('keyboard-usb-snapshot')
        assert restored['blocks'] == baseline['blocks'], 'Restore differs'
        proof.append({'assignment': name, 'keys': keys, 'readback': wanted, 'other_blocks_unchanged': True, 'all_six_blocks_restored': True})
        print(name + ': readback verified, all six blocks restored', flush=True)
    save('keyboard-advanced-usb-08-proof.json', {'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'cycles': proof, 'physical_key_behavior_observed': False})


if __name__ == '__main__':
    main()
