"""USB DKS configuration roundtrip using the public Rust CLI; no key injection."""
import sys
from keyboard_usb_roundtrip import run, save, matches


def main():
    before = run('keyboard-usb-snapshot')
    baseline = save('keyboard-dks-usb-09-before.json', before)
    settings = {'points': [16, 30, 30, 16], 'actions': [69, 68, 0, 0],
                'states': [[1, 0, 0, 0], [0, 1, 0, 0], [0] * 4, [0] * 4]}
    config = save('keyboard-dks-usb-09-edit.json', settings)
    receipt = None
    try:
        receipt = run('dks-apply', baseline, '12', config)
        save('keyboard-dks-usb-09-applied.json', receipt)
        during = run('keyboard-usb-snapshot')
        save('keyboard-dks-usb-09-during.json', during)
        reference = during['blocks']['keymap'][48:52]
        assert reference[0] == 8 and reference[1] < 64
        offset = reference[1] * 16
        assert during['blocks']['dks'][offset:offset + 4] == settings['points']
        for name, values in before['blocks'].items():
            if name not in ('dks', 'keymap'):
                assert during['blocks'][name] == values, name
        print('DKS bank and keymap reference verified.', flush=True)
    finally:
        if receipt:
            save('keyboard-dks-usb-09-restored.json', run('dks-restore', receipt['backup_path']))
    assert matches(before, run('keyboard-usb-snapshot')), 'Restoration mismatch'
    save('keyboard-dks-usb-09-proof.json', {'usb': True, 'bank_and_reference_verified': True,
         'all_six_blocks_restored': True, 'physical_key_action_observed': False})
    print('All six blocks restored.', flush=True)


if __name__ == '__main__':
    sys.stdout.reconfigure(encoding='utf-8')
    main()
