"""Source-confirmed CB and mouse key assignments; no synthesized input."""
import json
import subprocess
import keyboard_usb_roundtrip as io


def snapshot(label):
    path=io.OUT/f'keyboard-shortcuts-{label}-010.json'
    result=subprocess.run([str(io.LAB),'snapshot',str(path)],capture_output=True,cwd=io.ROOT)
    if result.returncode:raise RuntimeError(result.stderr.decode('utf-8',errors='replace'))
    return path,json.loads(path.read_text(encoding='utf-8'))


def main():
    _,original=snapshot('before')
    cases=[{'Combo':{'first':224,'second':225,'key':4}},
           {'Combo':{'first':0,'second':226,'key':43}}]
    cases += [{'Mouse':{'kind':k,'value':v}} for k,v in [(1,1),(1,2),(1,4),(1,8),(1,16),(3,1),(3,255)]]
    for index,assignment in enumerate(cases):
        path,before=snapshot(f'before-{index}')
        assert io.matches(original,before)
        edit=io.save(f'keyboard-shortcuts-edit-{index}-010.json',{'keys':[12],'assignment':assignment})
        receipt=None
        try:
            receipt=io.run('keymap-apply',path,edit)
            io.save(f'keyboard-shortcuts-receipt-{index}-010.json',receipt)
            after=receipt['snapshot']
            expected=[7,*assignment['Combo'].values()] if 'Combo' in assignment else [1,*assignment['Mouse'].values(),0]
            assert after['blocks']['keymap'][48:52]==expected
            assert after['blocks']['keymap'][:48]==before['blocks']['keymap'][:48]
            assert after['blocks']['keymap'][52:]==before['blocks']['keymap'][52:]
            for name,block in before['blocks'].items():
                if name!='keymap':assert after['blocks'][name]==block
        finally:
            if receipt:io.run('keymap-restore',receipt['backup_path'])
        _,returned=snapshot(f'restored-{index}')
        assert io.matches(original,returned)
    proof={'channel':'2.4G','cases':len(cases),'all_six_areas_restored':True,'physical_output_tested':False}
    io.save('keyboard-shortcuts-010-proof.json',proof)
    print(json.dumps(proof))


if __name__=='__main__':main()
