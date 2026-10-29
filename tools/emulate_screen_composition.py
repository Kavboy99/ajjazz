"""Native UI3 composition pixels, compact records and flash reload."""
import json,struct
from PIL import Image
from emulate_screen_firmware import Machine,OUT,request,seal
def main():
    manifest=json.loads((OUT/'manifest.json').read_text());image=(OUT/'aj159-screen-telemetry-v1-candidate.bin').read_bytes();symbols={k:int(v,16)for k,v in manifest['symbols'].items()};results=[]
    for path in sorted((OUT/'composition').glob('*.json')):
        v=json.loads(path.read_text());m=Machine(image)
        for address,value in [(0x2a14e,12),(0x2a14d,34),(0x2a14c,0)]:m.cpu.mem_write(address,bytes([value]))
        assert m.usb(request(1,bytes(v['config_bytes'])))[6]==0
        assert m.usb(request(6,bytes(v['pages_bytes'])))[6]==0
        tick=v['phase']*100+51200;m.cpu.mem_write(0x2a138,struct.pack('<I',tick))
        if v['sample']:assert m.usb(request(15,bytes(v['sample'])))[6]==0
        m.call(symbols['screen_tick'])
        actual=bytes(v for c in m.pixels for v in (((c>>11)&31)*255//31,((c>>5)&63)*255//63,(c&31)*255//31));expected=Image.open(path.with_suffix('.png')).convert('RGB').tobytes()
        if actual!=expected:
            Image.frombytes('RGB',(128,128),actual).save(path.with_name(path.stem+'-actual.png'))
            raise AssertionError((path.name,sum(a!=b for a,b in zip(actual,expected))))
        assert m.fill_calls==0
        if (v['pages_bytes'][8]&127) not in (13,19):
            before=len(m.transfers);m.cpu.mem_write(0x2a138,struct.pack('<I',tick+200));m.call(symbols['screen_tick']);assert len(m.transfers)>before and m.fill_calls==0
        original=bytes((i*17)&255 for i in range(256));m.cpu.mem_write(0x28000,original);m.call(symbols['screen_config_write'],(0x28000,0x100000,256))
        assert m.flash_written[:128]==original[:128] and m.flash_written[234:]==original[234:]
        cold=Machine(image);cold.flash_read=m.flash_written;cold.call(symbols['screen_config_read'],(0x28000,0x100000,256));assert cold.usb(request(4))[8:56]==bytes(v['pages_bytes'])
        for packed in (0,0x11,0xf,0x101):
            bad=bytearray(v['pages_bytes']);bad[10:12]=struct.pack('<H',packed);seal(bad);assert m.usb(request(5,bad))[6]==2
        results.append(path.stem)
    assert len(results)>=105
    # Custom duration must use its seconds byte, not the metric-nibble value.
    base=json.loads((OUT/'composition/composition-0-1-1.json').read_text());m=Machine(image);p=bytearray(base['pages_bytes']);p[0]=3;p[1]=2;p[9]=2;p[12:16]=struct.pack('<BBH',3,0,3);seal(p)
    assert m.usb(request(6,p))[6]==0
    for second,index in [(0,0),(2,1),(5,0)]:
        m.cpu.mem_write(0x2a14e,b'\0');m.cpu.mem_write(0x2a14d,b'\0');m.cpu.mem_write(0x2a14c,bytes([second]));m.cpu.mem_write(0x2a138,struct.pack('<I',600+second*900));m.call(symbols['screen_tick']);assert m.usb(request(4))[56]==index
    m.uart(0x42);m.cpu.mem_write(0x2a138,struct.pack('<I',6000));m.call(symbols['screen_tick']);assert m.usb(request(4))[56]==1
    report={'candidate_sha256':manifest['candidate_sha256'],'hardware_access':False,'pixel_exact_cases':results,'checks':['adaptive 1-4 cells','six autonomous backdrops','missing and signed values','invalid/duplicate metric rejection','untouched media bytes','flash reload']};(OUT/'composition-validation.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
if __name__=='__main__':main()
