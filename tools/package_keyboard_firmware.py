"""Patch only the OEM user-image resource and matching advertised word sum.
Never launches the updater; stock executable is retained for recovery planning.
"""
from pathlib import Path
import hashlib,json,struct,sys,zipfile
from build_keyboard_firmware import ROOT,OUT,SHA
sys.path.insert(0,str(ROOT/'.tools/py-research'))
import pefile

def digest(p):return hashlib.sha256(p).hexdigest()
def main():
    original=ROOT/'research/vendor/ak820max-support-20261024/support-updater.exe'
    stock=original.read_bytes()
    if digest(stock)!='edd4914bdd93bc3d557173e554efd40e7b531f5d6275b5bc8c8c6f7f4aa570d4':raise ValueError('Wrong updater')
    candidate=(OUT/'ak820max-ajks-v1-candidate.bin').read_bytes()
    manifest=json.loads((OUT/'manifest.json').read_text());emulation=json.loads((OUT/'emulation.json').read_text())
    if digest(candidate)!=manifest['candidate_sha256'] or emulation['candidate_sha256']!=digest(candidate) or not emulation['passed']:raise ValueError('Stale/unverified candidate')
    pe=pefile.PE(data=stock);patched=bytearray(stock);changes=[];resources={}
    for kind in pe.DIRECTORY_ENTRY_RESOURCE.entries:
        if kind.id==10:
            for entry in kind.directory.entries:
                if len(entry.directory.entries)!=1:raise ValueError('Ambiguous resource language')
                r=entry.directory.entries[0].data.struct
                resources[entry.id]=(pe.get_offset_from_rva(r.OffsetToData),r.Size)
    checksum=f'{sum(struct.unpack("<258048H",candidate))&65535:04X}'
    for rid,new in [(4000,candidate),(4014,checksum.encode('utf-16le'))]:
        at,size=resources[rid]
        if size!=len(new):raise ValueError('Resource length changed')
        if rid==4000 and digest(stock[at:at+size])!=SHA:raise ValueError('Wrong resource image')
        patched[at:at+size]=new;changes.append({'resource':rid,'offset':at,'bytes':size,'sha256':digest(new)})
    permitted={i for r in changes for i in range(r['offset'],r['offset']+r['bytes'])}
    if any(a!=b and i not in permitted for i,(a,b) in enumerate(zip(stock,patched))):raise ValueError('Unexpected executable modification')
    # Original loader at VA427276 loads RCDATA4000 directly into its firmware
    # buffer via FindResourceW/LoadResource/LockResource/SizeofResource/memcpy.
    if pe.get_data(0x27276,13)!=bytes.fromhex('68a00f00006a00ff158cb25500'):raise ValueError('OEM image-loader reference changed')
    check=pefile.PE(data=bytes(patched))
    if check.get_data(0x27276,13)!=pe.get_data(0x27276,13):raise ValueError('Updater code changed')
    suffix=digest(candidate)[:12]
    directory=ROOT/f'artifacts/AK820MAX-AJKS-v1-{suffix}';directory.mkdir(parents=True,exist_ok=True)
    name='AK820MAX-AJKS-v1-CANDIDATE.exe';(directory/name).write_bytes(patched)
    (directory/'AK820MAX-stock-1.36-ORIGINAL.exe').write_bytes(stock)
    report=dict(candidate_sha256=digest(candidate),updater_sha256=digest(patched),original_updater_sha256=digest(stock),
                changes=changes,executable_code_unchanged=True,template_zip_unchanged=True,
                updater_executed=False,installed=False,hardware_recovery_verified=False,
                settings_backup='research/local/ak820max-support-20261024/keyboard-before.json',
                live_factory_calibration_dump_available=False)
    (directory/'package.json').write_text(json.dumps(report,indent=2)+'\n')
    for f in ['manifest.json','emulation.json']:(directory/f).write_bytes((OUT/f).read_bytes())
    (directory/'README.txt').write_text('AJKS v1 - AK820MAX ULTRA / SN34F280 / USB 0C45:80A0\n\n'
      'CANDIDATE: not yet installed or tested on the physical keyboard.\n'
      'Changes: procedural layer renderer, 8 saved programs, encoder actions.\n'
      'Original updater code is unchanged. Only RCDATA4000 and its word checksum\n'
      'RCDATA4014 are replaced. The generic ZIP template is deliberately untouched.\n'
      'Do not run the two updaters together. Firmware installation requires approval.\n'
      'Use wired mode. Keep power connected until completion.\n'
      'The vendor requires key calibration after updating. Calibration needs physical\n'
      'key presses; emulator checks cannot establish hardware recovery or timing.\n'
      'Original 1.36 is provided for recovery preparation, but its recovery path has\n'
      'not been tested. A failed update may require an external programmer.\n'
      'This is a patch to the support firmware, not a replacement USB bootloader.\n',encoding='utf-8')
    with zipfile.ZipFile(ROOT/f'artifacts/AK820MAX-AJKS-v1-{suffix}-CANDIDATE.zip','w',zipfile.ZIP_DEFLATED) as z:
        for f in directory.iterdir():z.write(f,f.name)
    print(json.dumps(report,indent=2))
if __name__=='__main__':main()
