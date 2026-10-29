"""Real Cortex-M0 telemetry extension: extended packet, history, pixels and transitions."""
import json,struct
from pathlib import Path
from PIL import Image
from emulate_screen_firmware import Machine,OUT,request,seal

def checks():
    manifest=json.loads((OUT/'manifest.json').read_text());image=(OUT/'aj159-screen-telemetry-v1-candidate.bin').read_bytes();symbols={k:int(v,16)for k,v in manifest['symbols'].items()}
    def step(m,tick=400):
        m.cpu.mem_write(0x2a138,struct.pack('<I',tick));m.call(symbols['screen_tick'])
    def pixels(m):return bytes(v for c in m.pixels for v in (((c>>11)&31)*255//31,((c>>5)&63)*255//63,(c&31)*255//31))
    results=[]
    for path in sorted((OUT/'metrics').glob('*.json')):
        v=json.loads(path.read_text());m=Machine(image)
        assert m.usb(request(1,bytes(v['config_bytes'])))[6]==0
        assert m.usb(request(5,bytes(v['pages_bytes'])))[6]==0
        for i,s in enumerate(v['samples']):
            m.cpu.mem_write(0x2a138,struct.pack('<I',100+i*100));assert m.usb(request(15,bytes(s)))[6]==0
        step(m,3400 if v['samples'] else 400)
        assert pixels(m)==Image.open(path.with_suffix('.png')).convert('RGB').tobytes(),path.name
        assert m.fill_calls==0
        before=len(m.transfers);step(m,3500 if v['samples'] else 500);assert len(m.transfers)==before
        if v['samples']:
            s=bytearray(v['samples'][-1]);assert m.usb(request(13))[16:64]==s
            # Retransmit same sequence: no duplicate graph point or display writes.
            before=len(m.transfers);assert m.usb(request(15,s))[6]==0;step(m,3600);assert len(m.transfers)==before
            for bit in range(384):
                bad=s.copy();bad[bit//8]^=1<<(bit%8);assert m.usb(request(15,bad))[6]==2,(path.name,bit)
            for at,value in [(32,2),(33,32),(44,1),(45,1),(34,1501),(36,(-101)&65535)]:
                bad=s.copy()
                if at in (34,36):bad[at:at+2]=struct.pack('<H',value);bad[33]|=1<<((at-34)//2)
                else:bad[at]=value
                seal(bad);assert m.usb(request(15,bad))[6]==2,(at,value)
            # Legacy packets clear temperatures/power, so stale values cannot leak.
            assert m.usb(request(14,s[:32]))[6]==0;assert m.usb(request(13))[48:64]==bytes(16)
            # Stop is opaque and deterministic; no stock clock overlays custom data.
            assert m.usb(request(9))[6]==0;step(m,3800);assert m.usb(request())[62]==0 and m.fill_calls==0
            cold=Machine(image);cold.usb(request(1,bytes(v['config_bytes'])));cold.usb(request(5,bytes(v['pages_bytes'])));cold.usb(request(9));step(cold,3800)
            assert m.pixels==cold.pixels
            assert m.usb(request(10))[6]==0;step(m,3900);assert m.fill_calls==1
        results.append(path.stem)
    assert len(results)==12
    # Four new kinds navigate through the actual rear UART dispatcher with no host.
    base=json.loads((OUT/'metrics/metrics-9-live.json').read_text());m=Machine(image)
    p=bytearray(base['pages_bytes']);p[:3]=bytes((1,4,0));p[8:40]=bytes(32)
    for i,k in enumerate((9,10,11,12)):p[8+i*4:12+i*4]=struct.pack('<BBH',k,0,10)
    seal(p);assert m.usb(request(6,p))[6]==0
    for i,kind in enumerate((9,10,11,12,9)):
        step(m,400+i*100);assert m.usb(request(4))[57]==kind
        m.uart(0x42)
    # Stored schedule retains the new kinds across simulated config flash reload.
    m.cpu.mem_write(0x28000,bytes(256));m.call(symbols['screen_config_write'],(0x28000,0x100000,256))
    cold=Machine(image);cold.flash_read=m.flash_written
    cold.call(symbols['screen_config_read'],(0x28000,0x100000,256));assert cold.usb(request(4))[8:56]==p
    report={'candidate_sha256':manifest['candidate_sha256'],'hardware_access':False,'pixel_exact_cases':results,'checks':['48-byte payload and both nested legacy CRCs','384 single-bit corruptions per live vector','signed temperature bounds and reserved bytes','legacy reset','identical frame skipped','Stop/native ownership','32-point history']}
    (OUT/'metrics-validation.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))

if __name__=='__main__':checks()
