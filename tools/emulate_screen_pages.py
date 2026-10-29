"""Regression checks for the actual Cortex-M0 page manager; no hardware I/O."""
import json
import struct
from PIL import Image
from emulate_screen_firmware import Machine, OUT, STATE, request, seal


def checks(image, symbols, config, sample):
    def clock(m, second=23, minute=34, hour=12, tick=300):
        m.cpu.mem_write(0x2a138,struct.pack('<I',tick))
        m.cpu.mem_write(0x2a14c,bytes((second,minute,hour,23,10)))
        m.cpu.mem_write(0x2a154,struct.pack('<I',2026))

    def step(m, **kw):
        clock(m,**kw)
        return m.call(symbols['screen_tick'])

    def pages(entries, selected=0, flags=7):
        p=bytearray(json.loads((OUT/'page-clock.json').read_text())['pages_bytes'])
        p[0:3]=bytes((flags,len(entries),selected));p[8:40]=bytes(32)
        for i,(kind,asset,seconds) in enumerate(entries):
            p[8+i*4:12+i*4]=struct.pack('<BBH',kind,asset,seconds)
        return seal(p)

    def status(m):
        r=m.usb(request(4));assert r[:7]==b'\x2cAJPG\x01\0'
        assert sum(r[:8])%256==255 and r[63]==255
        return r

    def asset(m, slot, frames=1):
        m.cpu.mem_write(0x2005c+slot+0x25,b'\x01')
        m.cpu.mem_write(0x2005c+slot+0x1f,bytes((frames,)))

    # Actual validated 0x42 UART dispatch and stock ACK, not a synthetic UI index.
    m=Machine(image);clock(m);native=Machine(image)
    expected_ack=native.uart(0x42)
    p=pages([(0,0,10),(1|128,0,10),(2,0,10),(3,0,10)],flags=5)
    m.usb(request(5,p));m.usb(request(1,config));m.usb(request(2,sample));step(m)
    assert m.uart(0x42)==expected_ack
    assert status(m)[56]==0  # IRQ only queues; rendering/navigation runs in main.
    step(m,tick=400);assert status(m)[56:58]==bytes((2,2))
    m.usb(request(2,sample));step(m,tick=500);assert status(m)[56]==2
    for _ in range(3):assert m.uart(0x42)==expected_ack
    step(m,tick=600);assert status(m)[56]==2  # 2 -> 3 -> 0 -> 2, skipping disabled1.
    m.usb(request(9));step(m,tick=700);assert status(m)[57]==3
    m.uart(0x42);step(m,tick=800);assert status(m)[57]==3  # No stale-PC page after Stop.
    # A bad outer checksum must not register a press.
    m.usb(request(2,sample));step(m,tick=900)
    active=status(m)[56]
    m.uart(0x42,corrupt_outer=True);step(m,tick=1000);assert status(m)[56]==active
    # Native-mode button follows the original handler and response exactly.
    m=Machine(image);native=Machine(image)
    assert m.uart(0x42)==native.uart(0x42)
    assert m.cpu.mem_read(0x2a118,4)==native.cpu.mem_read(0x2a118,4)

    # Independent Rust/C codecs and final pixels for every functional page.
    for name in ('summary','cpu','memory','clock','analog','calendar','focus','stopwatch','panel','minimal','modern-cpu','modern-memory','modern-clock'):
        m=Machine(image);clock(m)
        vector=json.loads((OUT/f'page-{name}.json').read_text())
        p=bytes(vector['pages_bytes'])
        assert bytes(request(5,p))==bytes(vector['pages_report'][1:])
        assert m.usb(request(1,bytes(vector.get('config_bytes',config))))[6]==0
        assert m.usb(request(5,p))[8:56]==p
        assert m.usb(request(2,sample))[6]==0
        step(m)
        actual=bytes(v for pixel in m.pixels for v in (((pixel>>11)&31)*255//31,((pixel>>5)&63)*255//63,(pixel&31)*255//31))
        assert actual==Image.open(OUT/f'page-{name}.png').convert('RGB').tobytes(), name
        assert m.fill_calls==0

    m=Machine(image);clock(m);p=pages([(0,0,2),(1|128,0,1),(2,0,2),(3,0,2)])
    assert m.usb(request(5,p))[6]==0
    for bit in range(384):
        bad=bytearray(p);bad[bit//8]^=1<<(bit%8)
        assert m.usb(request(5,bad))[6]==2
    for offset,value in ((0,8),(1,0),(1,9),(2,4),(3,0),(4,128),(6,2),(8,13),(9,1),(10,0),(40,4),(45,1)):
        bad=bytearray(p);bad[offset]=value;seal(bad)
        assert m.usb(request(5,bad))[6]==2, (offset,value)
    assert m.usb(request(7,b'\x01'))[6]==2  # Disabled entry.
    m.usb(request(1,config));m.usb(request(2,sample));step(m)
    assert status(m)[56:58]==bytes((0,0))
    step(m,second=25,tick=400)
    assert status(m)[56:58]==bytes((2,2))  # Skip disabled CPU.
    step(m,second=27,tick=500)
    assert status(m)[56:58]==bytes((3,3))
    assert m.usb(request(9))[6]==0
    step(m,second=28,tick=600)
    before=m.pixels.copy()
    assert status(m)[57]==3 and m.usb(request())[62]==0
    step(m,second=29,tick=700)
    assert m.pixels!=before and m.fill_calls==0  # Clock continues with no host samples.
    m.usb(request(2,sample));step(m,second=30,tick=800)
    assert status(m)[56:58]==bytes((0,0))  # First sample after Stop resumes selected page.
    m.usb(request(7,b'\x02'));step(m,second=31,tick=900)
    assert status(m)[56]==2
    # Manual selection survives a host packet, then normal dwell resumes.
    m.usb(request(2,sample));step(m,second=31,tick=1000)
    assert status(m)[56]==2
    step(m,second=32,tick=1100);assert status(m)[56]==3
    m.usb(request(9));step(m,second=34,tick=1200)
    step(m,second=36,tick=1300);assert status(m)[57]==3
    # Midnight is a normal second, not an unsigned scheduling jump.
    p=pages([(3,0,2),(3,0,2)])
    m.usb(request(5,p));step(m,hour=23,minute=59,second=59,tick=1400)
    step(m,hour=0,minute=0,second=1,tick=1500);assert status(m)[56]==1

    # Explicit native handoff clears exactly once and invalidates stock caches.
    m.cpu.mem_write(0x2a118,struct.pack('<I',0))
    m.cpu.mem_write(0x2a124,struct.pack('<I',0))
    assert m.usb(request(10))[6]==0
    assert step(m,tick=1600)==0 and m.fill_calls==1 and m.pixels==[0]*16384
    assert bytes(m.cpu.mem_read(0x2a124,4))==b'\xff'*4
    assert step(m,tick=1700)==0 and m.fill_calls==1
    start=len(m.transfers)
    m.call(0x0101316c)
    assert any(w==128 and h==128 for x,y,w,h,_ in m.transfers[start:]), 'Stock clock did not redraw background'

    # Timers execute in the MCU, independently of host telemetry and UI ownership.
    m=Machine(image);clock(m)
    c=bytearray(config);c[45]=2;seal(c);m.usb(request(1,c))
    m.usb(request(5,pages([(7,0,10),(8,0,10),(3,0,10)],flags=1)))
    def timer(cmd=None):
        r=m.usb(request(12) if cmd is None else request(11,bytes((cmd,))))
        assert r[:7]==b'\x2cAJTM\x01\0' and sum(r[:8])%256==255
        return r[8],r[9],r[10],struct.unpack('<I',r[12:16])[0],struct.unpack('<I',r[16:20])[0]
    assert timer()==(0,0,0,0,0)
    assert timer(0)==(1,1,0,0,120)
    step(m);step(m,second=25,tick=400);assert timer()==(1,1,0,2,120)
    timer(2);step(m,second=35,tick=500);assert timer()==(1,0,0,2,120)
    timer(3);step(m,second=36,tick=600);step(m,second=37,tick=700)
    assert timer()==(1,1,0,3,120)
    assert timer(4)==(1,0,0,0,120)
    timer(0);step(m,hour=23,minute=59,second=59,tick=800)
    step(m,hour=0,minute=0,second=59,tick=900)
    step(m,hour=0,minute=1,second=59,tick=1000)
    assert timer()==(1,0,1,120,120)
    assert m.usb(request(11,b'\x03'))[6]==2
    timer(1);step(m,second=23,tick=1100)
    m.usb(request(9));step(m,second=24,tick=1200)
    m.usb(request(10));step(m,second=25,tick=1300)
    assert timer()==(2,1,0,2,0)
    assert timer(5)==(0,0,0,0,0)
    for bad in (request(11,b'\x06'),request(11),request(12,b'\0')):
        assert m.usb(bad)[6]==2

    # Save separates active and stored schedules; preserve every unowned byte.
    m=Machine(image);p=pages([(3,0,9),(0,0,4)],1)
    m.usb(request(6,p));old=bytes((i*7)&255 for i in range(256))
    m.cpu.mem_write(0x2e000,old)
    m.call(symbols['screen_config_write'],(0x2e000,0x100000,256));saved=m.flash_written
    assert saved[:128]==old[:128] and saved[234:]==old[234:]
    assert saved[181:186]==b'AJPG\x01' and saved[186:234]==p
    m.call(symbols['screen_config_read'],(0x2e000,0x100000,256))
    ram=pages([(3,0,7)])
    m.usb(request(5,ram));m.call(symbols['screen_config_read'],(0x2e000,0x100000,256))
    assert status(m)[8:56]==ram
    reboot=Machine(image);reboot.flash_read=saved
    reboot.call(symbols['screen_config_read'],(0x2e000,0x100000,256))
    assert status(reboot)[8:56]==p

    # Execute the original media reader/address arithmetic and its 4 KiB chunks.
    m=Machine(image);clock(m);asset(m,0);asset(m,4);asset(m,5,222)
    m.cpu.mem_write(0x2005c+5+0x19,b'\x01')
    p=pages([(4,0,10),(4,4,10),(4,5,10),(3,0,10)],flags=1)
    m.usb(request(5,p));step(m)
    assert m.flash_reads[-8:]==[(0x7d1000+i*4096,4096) for i in range(8)]
    assert m.pixels==[0xa5a5]*16384
    m.usb(request(7,b'\x01'));step(m,tick=400)
    assert m.flash_reads[-8:]==[(0x7f1000+i*4096,4096) for i in range(8)]
    clock(m,tick=500);m.usb(request(7,b'\x02'));step(m,tick=500)
    assert m.flash_reads[-8:]==[(0x101000+i*4096,4096) for i in range(8)]
    step(m,tick=717)
    assert m.flash_reads[-8:]==[(0x101000+217*32768+i*4096,4096) for i in range(8)]
    assert max(a+n for a,n in m.flash_reads if a<0x7d1000)<=0x7d1000
    assert status(m)[59]==218
    m.usb(request(7,b'\x03'));step(m,tick=800)
    assert m.pixels!=[0xa5a5]*16384 and m.fill_calls==0

    # Erase only the selected photo, after metadata invalidation has been saved.
    p=pages([(4,0,10),(3,0,10)])
    m.usb(request(6,p));assert m.usb(request(8,b'\0'))[6]==0
    assert m.usb(request(8,b'\x04'))[6]==3
    step(m,tick=900);assert status(m)[58]&1==0 and status(m)[8+1]==1
    assert status(m)[16]==3 and not m.erased
    step(m,tick=1000);assert not m.erased  # Pending metadata save blocks destructive writes.
    upload=bytearray(64);upload[0]=0xa5;upload[7]=0x5a
    assert m.usb(upload)[:2]==b'\xa5\0'
    m.cpu.mem_write(0x2e000,old)
    m.call(symbols['screen_config_write'],(0x2e000,0x100000,256))
    assert m.flash_written[186+8]==3
    m.cpu.mem_write(0x2a142,b'\0');m.cpu.mem_write(0x2a143,b'\0')
    for i in range(8):step(m,tick=1100+i*100)
    assert m.erased==list(range(0x7d1000,0x7d9000,4096))
    assert status(m)[60:62]==bytes((0,100))
    assert m.external[0x7d1000:0x7d9000]==bytes([255])*32768
    assert m.external[0x7d9000:0x7e1000]==bytes([0xa5])*32768
    assert m.usb(request(8,b'\0'))[6]==2
    # Maximum legacy GIF count cannot erase any photo sector.
    assert m.usb(request(8,b'\x05'))[6]==0
    step(m,tick=2000);m.cpu.mem_write(0x2a142,b'\0');m.cpu.mem_write(0x2a143,b'\0')
    # No display ownership needed while exercising all 1744 erase sectors.
    m.cpu.mem_write(0x2a141,b'\0')
    for _ in range(218*8):m.call(symbols['screen_tick'])
    gif=m.erased[8:]
    assert gif==list(range(0x101000,0x7d1000,4096))
    assert status(m)[60:62]==bytes((0,100))
    return ['autonomous focus/stopwatch: pause/resume/reset/completion/midnight/Stop/native',
            'rear button original UART/ACK, queued presses, disabled/offline skip and native fallback',
            'ten Rust/C page previews pixel exact','playlist codec, 384 corruptions and invalid ranges',
            'ordered dwell, disabled/offline skip, manual selection and midnight',
            'Stop keeps a ticking clock; restarting samples resumes selected page',
            'native handoff clears once and actual stock clock repaints background',
            'playlist persistence, byte preservation and RAM/stored isolation',
            'actual stock media reader: photo boundaries and 218-frame GIF bound',
            'delete prunes active/stored playlist, waits for metadata save and blocks upload',
            'photo erase isolation and all 1744 bounded GIF sectors']


if __name__=='__main__':
    from pathlib import Path
    manifest=json.loads((OUT/'manifest.json').read_text())
    vectors=json.loads((OUT/'telemetry-host.json').read_text())
    result=checks(Path(manifest['candidate']).read_bytes(),{k:int(v,16) for k,v in manifest['symbols'].items()},
                  bytes(vectors['config_bytes']),bytes(vectors['sample_bytes']))
    print(json.dumps(result,indent=2))
