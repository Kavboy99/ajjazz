"""Bounded USB static-frame benchmark, restores lighting and checks all config areas."""
import datetime
import json
import os
import subprocess
import sys
import time
from pathlib import Path
import keyboard_usb_roundtrip as transport
from keyboard_usb_roundtrip import ROOT, OUT, run, save, matches


def main():
    label = sys.argv[1] if len(sys.argv) > 1 else 'current'
    channel = sys.argv[2] if len(sys.argv) > 2 else 'usb'
    assert channel in ('usb', 'radio')
    if len(sys.argv)>3:
        transport.LAB=ROOT/sys.argv[3]
    def snapshot():
        if channel=='usb': return run('keyboard-usb-snapshot')
        path=OUT/f'keyboard-lighting-speed-{label}-latest.json'
        result=subprocess.run([str(transport.LAB),'snapshot',str(path)],capture_output=True,cwd=ROOT)
        if result.returncode: raise RuntimeError(result.stderr.decode('utf-8',errors='replace'))
        return json.loads(path.read_text(encoding='utf-8'))
    assert label.replace('-', '').replace('_', '').isalnum()
    baseline = snapshot()
    baseline_path=save(f'keyboard-lighting-speed-{label}-before.json', baseline)
    scene = OUT / 'ui-scene-before-animation-06.json'
    env = dict(os.environ, AJAZZ_TRACE_RGB='1')
    results = []
    for index in range(1):
        receipt = None
        try:
            start = time.perf_counter()
            command = [str(transport.LAB), 'lighting-apply', str(scene), '0', channel]
            if '--cached' in sys.argv:command.append(str(baseline_path))
            result = subprocess.run(command, capture_output=True, cwd=ROOT, env=env)
            elapsed = time.perf_counter()-start
            (OUT/f'keyboard-lighting-speed-{label}-{index}.log').write_bytes(result.stderr)
            if result.returncode:
                raise RuntimeError(result.stderr.decode('utf-8', errors='replace'))
            receipt = json.loads(result.stdout)
            save(f'keyboard-lighting-speed-{label}-{index}-receipt.json', receipt)
            after = snapshot()
            for name, value in baseline['blocks'].items():
                if name not in ('lighting', 'custom_rgb'):
                    assert after['blocks'][name] == value, f'Unexpected change: {name}'
            results.append({'total_apply_seconds': elapsed, 'unrelated_areas_unchanged': True})
        finally:
            if receipt:
                run('lighting-restore', receipt['backup_path'])
        restored = snapshot()
        assert matches(baseline, restored), 'Baseline mismatch after restore'
        results[-1]['restored_all_six_areas'] = True
        print(json.dumps(results[-1]), flush=True)
    save(f'keyboard-lighting-speed-{label}-proof.json', {'time_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'runs': results})


if __name__ == '__main__':
    main()
