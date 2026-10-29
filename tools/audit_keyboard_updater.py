"""Rebuild the keyboard updater evidence index without running vendor code."""
from pathlib import Path
import json
import struct
from keyboard_updater_lab import PE, sha, require, inspect_updater, compare_images, portability, save_json

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'research/keyboard-updater'
SUPPORT=ROOT/'research/vendor/ak820max-support-20261024/support-updater.exe'
RECOVERY=ROOT/'research/vendor/revision-hunt/ak820-recovery-b.exe'


def table(data):
    pe=PE(data)
    require(sha(data)=='edd4914bdd93bc3d557173e554efd40e7b531f5d6275b5bc8c8c6f7f4aa570d4','Table offsets require pinned updater')
    def read(va,size):
        at=pe.offset(va-pe.image_base,size)
        return data[at:at+size]
    result=[]
    for index in range(22):
        va=0x5b30a8+index*51
        record=read(va,51)
        ptr=struct.unpack_from('<I',record,31)[0]
        descriptor=read(ptr,63)
        name_pointer=struct.unpack_from('<I',descriptor)[0]
        name=read(name_pointer,30).split(b'\0')[0].decode('ascii')
        result.append(dict(index=index,table_va=f'{va:08X}',descriptor_va=f'{ptr:08X}',
            name=name,boot_vid=f'{struct.unpack_from("<H",record,4)[0]:04X}',
            boot_pid=f'{struct.unpack_from("<H",record,6)[0]:04X}',
            version_signature=record[8:12].hex(),
            erase_count=struct.unpack_from('<H',descriptor,0x27)[0],
            program_blocks=struct.unpack_from('<H',descriptor,0x2b)[0],
            blank_checksum=f'{struct.unpack_from("<H",descriptor,0x25)[0]:04X}',
            record_hex=record.hex(),descriptor_hex=descriptor.hex(),hardware_validated=False))
    require(result[-1]['name']=='SN34F280' and result[-1]['erase_count']==504
            and result[-1]['program_blocks']==8064,'Unexpected chip table')
    return result


def main():
    support=SUPPORT.read_bytes();recovery=RECOVERY.read_bytes()
    first=inspect_updater(support)[0];second=inspect_updater(recovery)[0]
    save_json(OUT/'support-136.json',first);save_json(OUT/'recovery-104.json',second)
    save_json(OUT/'chip-table.json',table(support))
    a,b=PE(support),PE(recovery)
    sections=[]
    for name in ('.text','.rdata','.data'):
        left=next(s for s in a.sections if s['name']==name)
        right=next(s for s in b.sections if s['name']==name)
        x=support[left['offset']:left['offset']+left['raw_size']]
        y=recovery[right['offset']:right['offset']+right['raw_size']]
        sections.append(dict(name=name,bytes_first=len(x),bytes_second=len(y),
            first_sha256=sha(x),second_sha256=sha(y),identical=x==y))
    raw_a=inspect_updater(support)[1][4000]['data'];raw_b=inspect_updater(recovery)[1][4000]['data']
    compare=compare_images(raw_a,raw_b)
    save_json(OUT/'portability-8033.json',portability(raw_a,raw_b))
    sources={}
    profile_audit=ROOT/'research/local/keyboard-revision-audit.json'
    if profile_audit.exists():
        profile=json.loads(profile_audit.read_text(encoding='utf-8'))
        for name,entries in profile['source_profiles'].items():
            path=ROOT/Path(name)
            sources[name]=dict(sha256=sha(path.read_bytes()),profiles=entries)
        save_json(OUT/'captured-driver-profiles.json',sources)
    ghidra=ROOT/'research/local/keyboard-updater-re/ghidra'
    validation=ROOT/'research/local/keyboard-updater-re/x86-codec-validation.json'
    require(validation.exists(),'Run emulate_keyboard_updater.py first')
    tested=json.loads(validation.read_text())
    require(tested['passed'] and tested['updater_sha256']==sha(support),'Stale x86 validation')
    save_json(OUT/'x86-codec-validation.json',tested)
    index=ghidra/'functions.tsv'
    counts=dict(selected=0,decompiled=0)
    if index.exists():
        rows=index.read_text(encoding='utf-8').splitlines()[1:]
        counts=dict(selected=len(rows),decompiled=sum(r.endswith('\ttrue') for r in rows))
    report=dict(schema=1,date='2026-10-26',sections=sections,decompilation=counts,
        support_updater_sha256=sha(support),recovery_updater_sha256=sha(recovery),
        firmware_comparison=dict(changed_bytes=compare['changed_bytes'],
            changed_ranges=len(compare['changed_ranges']),changed_pages_512=len(compare['changed_pages_512'])),
        profiles_are_evidence_not_automatic_compatibility=True,
        x86_codec_validation_passed=True,hardware_access=False,vendor_process_started=False)
    save_json(OUT/'findings.json',report)
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()
