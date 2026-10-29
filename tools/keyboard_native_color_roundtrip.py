"""Replicate website's separate mode-selection and color-setting actions over 2.4G."""
import json
import subprocess
import time
import sys
import keyboard_usb_roundtrip as io


def snapshot(name):
    path=io.OUT/name
    result=subprocess.run([str(io.LAB),'snapshot',str(path)],capture_output=True,cwd=io.ROOT)
    if result.returncode:raise RuntimeError(result.stderr.decode('utf-8',errors='replace'))
    return path,json.loads(path.read_text(encoding='utf-8'))


def settings(before):
    b=before['blocks']['lighting']
    return dict(mode=b[0],color=b[1:4],rainbow=bool(b[8]),brightness=b[9],speed=b[10],direction=b[11])


def main():
    path,before=snapshot('keyboard-native-colors-radio-010-before.json')
    mode=settings(before);mode['mode']=1
    modepath=io.save('keyboard-native-colors-radio-010-mode.json',mode)
    mode_receipt=None;proof={}
    try:
        mode_receipt=io.run('lighting-effect',path,modepath)
        io.save('keyboard-native-colors-radio-010-mode-receipt.json',mode_receipt)
        path,selected=snapshot('keyboard-native-colors-radio-010-selected.json')
        color=settings(selected);color.update(color=[255,0,0],rainbow=False)
        colorpath=io.save('keyboard-native-colors-radio-010-red.json',color)
        start=time.perf_counter()
        try:
            receipt=io.run('lighting-effect',path,colorpath)
            io.save('keyboard-native-colors-radio-010-red-receipt.json',receipt)
            proof.update(color_accepted=True,color_readback=receipt['effect_color_verified'],seconds=time.perf_counter()-start)
            if '--hold' in sys.argv:
                print('Native red applied for 45 seconds; restoring automatically.',flush=True)
                time.sleep(45)
        except Exception as e:proof.update(color_readback=False,error=str(e))
    finally:
        if mode_receipt:io.run('lighting-restore',mode_receipt['backup_path'])
    _,after=snapshot('keyboard-native-colors-radio-010-restored.json')
    assert io.matches(before,after)
    proof['all_six_areas_restored']=True
    io.save('keyboard-native-colors-radio-010-proof.json',proof)
    print(json.dumps(proof,ensure_ascii=False))


if __name__=='__main__':main()
