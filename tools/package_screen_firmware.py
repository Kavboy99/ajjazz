"""Prepare display-only candidate/stock vendor packages. Never launch an EXE."""
import hashlib
import json
from pathlib import Path
import shutil
import zlib

from extract_support_firmware import decode_resource

ROOT=Path(__file__).resolve().parents[1]
BUILD=ROOT/'firmware/aj159-screen/build'
UPDATER_SHA='4ee530e50826b59ff4599e49e6fe37116f9c686c1931f590074f38d467c1cf33'


def sha(data):return hashlib.sha256(data).hexdigest()


def encode(data,original):
    temporary=original[:1]+original[203:]
    compressor=zlib.compressobj(9,zlib.DEFLATED,-15)
    stream=compressor.compress(data)+compressor.flush()
    wrapped=stream[:-1]+temporary[-203:-1]+stream[-1:]
    wrapped=wrapped[:1]+original[1:203]+wrapped[1:]
    if decode_resource(wrapped)!=data:raise ValueError('Wrapper round trip failed')
    return wrapped


def main():
    manifest=json.loads((BUILD/'manifest.json').read_text())
    checks=json.loads((BUILD/'emulation.json').read_text())
    candidate=Path(manifest['candidate']).read_bytes()
    if sha(candidate)!=manifest['candidate_sha256'] or checks['candidate_sha256']!=sha(candidate):
        raise ValueError('Build/emulation hashes differ; rerun validation')
    if len(checks.get('page_manager_checks',[]))<11:
        raise ValueError('Page manager regressions must pass before packaging')
    if len(checks.get('design_checks',[]))<18:
        raise ValueError('Modern layout, missing values and transition regressions must pass before packaging')
    metrics=json.loads((BUILD/'metrics-validation.json').read_text())
    if metrics['candidate_sha256']!=sha(candidate) or len(metrics.get('pixel_exact_cases',[]))!=12:
        raise ValueError('Extended sensor/page validation must match the candidate')
    compositions=json.loads((BUILD/'composition-validation.json').read_text())
    if compositions['candidate_sha256']!=sha(candidate) or len(compositions.get('pixel_exact_cases',[]))<105:
        raise ValueError('Native composition validation must match the candidate')
    roots=list((ROOT/'research/vendor/ajazz-support-20261023/dock').glob('*/ry_upgrade.exe'))
    if len(roots)!=1 or sha(roots[0].read_bytes())!=UPDATER_SHA:
        raise ValueError('Exact manufacturer updater required')
    source=roots[0].parent
    original=(source/'resources/x5').read_bytes()
    if sha(decode_resource(original))!=manifest['manufacturer_image_sha256']:
        raise ValueError('Stock display image mismatch')
    output=ROOT/'artifacts'/('AJSC-screen-v1-'+sha(candidate)[:12]+'-oled')
    if output.exists():raise FileExistsError(f'Package already exists: {output}')
    output.mkdir(parents=True)
    config=json.loads((source/'resources/tool_config.json').read_text(encoding='utf-8-sig'))
    config['upgrade_methods']=['oled']
    # OEM base package also uses unlocked ID detection. Its locked-ID mode does
    # not list this dock, although direct F1 positively identifies it as 1759.
    # Restrict USB support below and require a separate direct-ID preflight.
    config['dev_info'].update(unlimited=True,unlimited_boot=False,id=-1,vid=0,pid=0,
                              boot_vid=0x3151,boot_pid=0x402e,merge_upgrade=False,need_check_version=False)
    support=json.loads((source/'resources/support_config.json').read_text(encoding='utf-8-sig'))
    support['support']=[row for row in support['support']
                        if row.get('vid')=='3151' and row.get('pid')=='5007']
    if len(support['support'])!=1:raise ValueError('Unique 3151:5007 support row required')
    for name,payload in [('Install-screen-candidate',encode(candidate,original)),('Restore-stock-screen',original)]:
        directory=output/name
        resources=directory/'resources';resources.mkdir(parents=True)
        shutil.copy2(source/'ry_upgrade.exe',directory/'ry_upgrade.exe')
        # Empty non-display resources also prevent unintended USB/RF images being available.
        for index in range(1,8):(resources/f'x{index}').write_bytes(payload if index==5 else b'')
        for filename,data in [('tool_config.json',config),('support_config.json',support)]:
            (resources/filename).write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
        assert decode_resource((resources/'x5').read_bytes())==(candidate if name.startswith('Install') else decode_resource(original))
    for filename in ('manifest.json','emulation.json','telemetry-emulated.png','telemetry-host.png','telemetry-host.json'):
        shutil.copy2(BUILD/filename,output/filename)
    shutil.copy2(BUILD/'metrics-validation.json',output/'metrics-validation.json')
    shutil.copytree(BUILD/'metrics',output/'metrics')
    shutil.copy2(BUILD/'composition-validation.json',output/'composition-validation.json')
    shutil.copytree(BUILD/'composition',output/'composition')
    for path in BUILD.glob('page-*.png'):
        shutil.copy2(path,output/path.name)
        shutil.copy2(path.with_suffix('.json'),output/path.with_suffix('.json').name)
    shutil.copy2(ROOT/'docs/DOCK-SCREEN-TELEMETRY.md',output/'README.md')
    (output/'licenses').mkdir()
    shutil.copy2(ROOT/'docs/licenses/Inter-OFL.txt',output/'licenses/Inter-OFL.txt')
    files={p.relative_to(output).as_posix():sha(p.read_bytes()) for p in sorted(output.rglob('*')) if p.is_file()}
    (output/'package.json').write_text(json.dumps({'candidate_sha256':sha(candidate),'files':files,
        'updater_executed':False,'firmware_installed':False,'hardware_recovery_verified':False},indent=2)+'\n')
    (BUILD/'package-path.txt').write_text(str(output)+'\n',encoding='utf-8')
    print(output)


if __name__=='__main__':main()
