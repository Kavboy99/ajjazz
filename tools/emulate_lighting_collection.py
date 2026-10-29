"""Run the new multi-layer programs in the *existing* installed ARM image. No HID."""
from pathlib import Path
import json,struct,hashlib
from emulate_keyboard_firmware import Machine,IMAGE
from build_keyboard_firmware import ROOT,crc

def main():
    expected_image='8b3d2d61ebfde82efa9791a35084b045f07bc1c256e84b504b276bdd0e03179e'
    assert hashlib.sha256(IMAGE.read_bytes()).hexdigest()==expected_image,'Use the installed image, not a newly built candidate'
    looks=json.loads((ROOT/'assets/lighting-collection.json').read_text(encoding='utf-8'))
    m=Machine();m.call('aj_lighting')
    cfg=m.config();cfg[6]=255;cfg[7]=128;cfg[10]=0
    struct.pack_into('<H',cfg,510,crc(cfg[:510]));assert m.upload(0,cfg,True)==0
    native_map=list(m.read(m.symbols['key_to_led'],126))
    transfer=list(m.read(m.symbols['led_input'],256))
    tests=[]
    for look in looks:
        study=json.loads((ROOT/'artifacts/lighting-studies'/f"{look['id']}.json").read_text(encoding='utf-8'))
        def normalized(v):
            if isinstance(v,float):return round(v,5)
            if isinstance(v,list):return [normalized(i) for i in v]
            if isinstance(v,dict):return {k:sorted(i) if k=='keys' else normalized(i) for k,i in v.items()}
            return v
        assert normalized(study['look'])==normalized(look)
        assert m.upload(7,bytes(study['program']))==0
        assert m.packet(5,b'\x07')[0]==0
        for v in study['vectors']:
            m.put(0x20007004,struct.pack('<I',v['frame']-1))
            m.put(0x20000083,[34]);m.put(0x20000088,[1]);m.call('aj_observe')
            before=m.instructions;m.call('aj_lighting');instructions=m.instructions-before
            for id in study['key_ids']:
                for channel in range(3):
                    actual=m.read(0x2000478c+channel*126+native_map[id])[0]
                    expected=transfer[v['rgb'][id][channel]]
                    assert actual==expected,(look['id'],v['frame'],id,channel,actual,expected)
            tests.append(dict(id=look['id'],frame=v['frame'],instructions=instructions,physical_keys_checked=82,passed=True))
        print(look['id'],'4 ARM frames agree',flush=True)
    report=dict(passed=True,installed_image_sha256=expected_image,looks=len(looks),frame_checks=len(tests),tests=tests,hardware_access=False,firmware_modified=False,physical_timing_verified=False)
    (ROOT/'artifacts/lighting-collection-arm.json').write_text(json.dumps(report,indent=2)+'\n')
    print(f'{len(looks)} looks, {len(tests)} ARM frame checks passed; installed firmware unchanged.')
if __name__=='__main__':main()
