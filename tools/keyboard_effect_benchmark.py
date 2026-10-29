"""Compare a single built-in mode change with full independent restoration checks."""
import json
import subprocess
import sys
import time
import keyboard_usb_roundtrip as io

label=sys.argv[1]
assert label.replace('-', '').isalnum()
if len(sys.argv)>2: io.LAB=io.ROOT/sys.argv[2]
path=io.OUT/f'keyboard-effect-speed-{label}-before.json'
result=subprocess.run([str(io.LAB),'snapshot',str(path)],capture_output=True,cwd=io.ROOT)
if result.returncode:raise RuntimeError(result.stderr.decode('utf-8',errors='replace'))
before=json.loads(path.read_text(encoding='utf-8'))
raw=before['blocks']['lighting']
edit={'mode':11 if raw[0]!=11 else 5,'color':raw[1:4],'rainbow':bool(raw[8]),'brightness':raw[9],'speed':raw[10],'direction':0}
editpath=io.save(f'keyboard-effect-speed-{label}-edit.json',edit)
receipt=None
try:
 start=time.perf_counter();receipt=io.run('lighting-effect',path,editpath);elapsed=time.perf_counter()-start
 io.save(f'keyboard-effect-speed-{label}-receipt.json',receipt)
 assert receipt['state']['lighting'][0]==edit['mode']
finally:
 if receipt:io.run('lighting-restore',receipt['backup_path'])
afterpath=io.OUT/f'keyboard-effect-speed-{label}-after.json'
result=subprocess.run([str(io.LAB),'snapshot',str(afterpath)],capture_output=True,cwd=io.ROOT)
if result.returncode:raise RuntimeError(result.stderr.decode('utf-8',errors='replace'))
after=json.loads(afterpath.read_text(encoding='utf-8'))
assert io.matches(before,after)
proof={'apply_seconds':elapsed,'channel':'2.4G','all_six_areas_restored':True,'written_mode':edit['mode']}
io.save(f'keyboard-effect-speed-{label}-proof.json',proof)
print(json.dumps(proof))
