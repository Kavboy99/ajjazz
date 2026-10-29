"""Exercise the public macro bank and one F12 binding, restoring both areas.

Does not synthesize any keyboard input or touch firmware. Uses one balanced A
press/release in the first free slot. All existing macros and six areas survive.
"""
import json
import keyboard_usb_roundtrip as io


def main():
    before = io.run('keyboard-macros', 'radio')
    baseline = io.save('keyboard-macros-radio-010-before.json', before)
    used = {m['slot'] for m in before['macros']}
    slot = next(i for i in range(100) if i not in used)
    macro = {'slot': slot, 'actions': [
        {'delay_ms': 0, 'usage': 4, 'pressed': True, 'mouse': False},
        {'delay_ms': 20, 'usage': 4, 'pressed': False, 'mouse': False}]}
    bank_receipt = None
    try:
        edit = {'macros': before['macros'] + [macro], 'binding': None}
        editpath = io.save('keyboard-macros-radio-010-bank-edit.json', edit)
        bank_receipt = io.run('macros-apply', baseline, editpath)
        io.save('keyboard-macros-radio-010-bank-receipt.json', bank_receipt)
        during = io.run('keyboard-macros', 'radio')
        assert macro in during['macros']
        assert io.matches(before['configuration'], during['configuration'])
        current = io.save('keyboard-macros-radio-010-bank.json', during)
        for mode, repeats in [(0, 0), (1, 2), (2, 0)]:
            binding_receipt = None
            try:
                edit['binding'] = {'key': 12, 'slot': slot, 'mode': mode, 'repeats': repeats}
                editpath = io.save(f'keyboard-macros-radio-010-bind-{mode}.json', edit)
                binding_receipt = io.run('macros-apply', current, editpath)
                observed = binding_receipt['snapshot']
                assert observed['raw'] == during['raw']
                assert observed['configuration']['blocks']['keymap'][48:52] == [6, slot, mode, repeats]
                for name, block in during['configuration']['blocks'].items():
                    if name != 'keymap': assert observed['configuration']['blocks'][name] == block
                io.save(f'keyboard-macros-radio-010-bind-{mode}-receipt.json', binding_receipt)
            finally:
                if binding_receipt: io.run('macros-restore', binding_receipt['backup_path'])
            returned = io.run('keyboard-macros', 'radio')
            assert returned['raw'] == during['raw']
            assert io.matches(during['configuration'], returned['configuration'])
    finally:
        if bank_receipt: io.run('macros-restore', bank_receipt['backup_path'])
    after = io.run('keyboard-macros', 'radio')
    io.save('keyboard-macros-radio-010-restored.json', after)
    assert before['raw'] == after['raw']
    assert io.matches(before['configuration'], after['configuration'])
    proof = {'channel': '2.4G', 'bank_and_three_repeat_modes': True,
             'bank_and_all_six_areas_restored': True, 'physical_output_tested': False}
    io.save('keyboard-macros-radio-010-proof.json', proof)
    print(json.dumps(proof))


if __name__ == '__main__': main()
