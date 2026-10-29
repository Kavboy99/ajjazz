"""Pin the supplied OEM engines and reviewed images for Studio's update workflow.
No executable is launched and no hardware is opened.
"""
from pathlib import Path
import hashlib,json,shutil
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'research/vendor/studio-updates'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    OUT.mkdir(parents=True,exist_ok=True);entries=[]
    dock=ROOT/'artifacts/AJSC-screen-v1-b634c6a1d417-oled'
    package=json.loads((dock/'package.json').read_text())
    for n,h in package['files'].items():assert sha(dock/n)==h,n
    keyboard=ROOT/'artifacts/AK820MAX-AJKS-v1-8b3d2d61ebfd'
    k=json.loads((keyboard/'package.json').read_text())
    assert sha(keyboard/'AK820MAX-AJKS-v1-CANDIDATE.exe')==k['updater_sha256']
    mouse=list((ROOT/'research/vendor/ajazz-support-20261023/mouse').glob('*/ry_upgrade.exe'))
    assert len(mouse)==1
    vendor=json.loads((ROOT/'research/vendor/ajazz-support-20261023/manifest.json').read_text())
    expected=next(v['updater_sha256'] for v in vendor['images'] if v['role']=='mouse')
    assert sha(mouse[0])==expected
    for target,title,version,source,exe in [
        ('Dock','APEX Dock','OLED UI4 · b634c6a1d417',dock/'Install-screen-candidate','ry_upgrade.exe'),
        ('Keyboard','AK820MAX ULTRA','AJKS · fd469e3144ab533c',keyboard,'AK820MAX-AJKS-v1-CANDIDATE.exe'),
        ('Mouse','AJ159 APEX','AJAZZ · 3.02',mouse[0].parent,'ry_upgrade.exe')]:
        dest=OUT/target;dest.mkdir(exist_ok=True)
        shutil.copy2(source/exe,dest/exe)
        if target!='Keyboard':
            shutil.copytree(source/'resources',dest/'resources',dirs_exist_ok=True)
        entry=dict(target=target,title=title,version=version,executable=exe,files={p.relative_to(dest).as_posix():sha(p) for p in dest.rglob('*') if p.is_file()})
        entries.append(entry)
    installer=ROOT/'research/vendor/librehardwaremonitor-0.9.6/PawnIO_setup.exe'
    assert sha(installer)=='a3a46226c5e2824f4cdd42be0eecbabfc672c86f7889710f5ab1e6ad385b47a0'
    shutil.copy2(installer,OUT/'PawnIO_setup.exe')
    manifest=dict(entries=entries,sensor_installer_sha256=sha(installer),keyboard_image_offset=1813068,keyboard_image_bytes=516096,keyboard_checksum_offset=2456232)
    (ROOT/'assets/update-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print('Prepared verified local update engines:',', '.join(e['target'] for e in entries))
if __name__=='__main__':main()
