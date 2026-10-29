"""Execute the AJSC candidate and stock fallback in Unicorn. No HID access.

Panel rectangles, RTC writes and external-flash I/O are callbacks. UART parsing,
stock command fallback, extension code, PWM register writes and configuration
validation execute original/candidate instructions. Not a physical boot test.
"""
import binascii
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_HOOK_CODE, UC_HOOK_MEM_WRITE, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_PRIMASK, UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'firmware/aj159-screen/build'
BASE, STOP, STATE = 0x01000200, 0x0106F000, 0x2C000
REGS = (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)


def crc(data):
    return binascii.crc_hqx(data, 0xffff)


def seal(data):
    data[-2:] = struct.pack('<H', crc(data[:-2]))
    return data


def request(action=0, payload=b''):
    p = bytearray(64)
    p[0] = 0x2c; p[7] = 0xd3
    p[8:12] = b'AJSC'; p[12:16] = bytes((1, action, len(payload), 0))
    p[16:16+len(payload)] = payload
    return p


class Machine:
    def __init__(self, image):
        self.cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
        u = self.cpu
        u.mem_map(0x1000000, 0x70000, UC_PROT_READ | UC_PROT_EXEC)
        u.mem_write(BASE, image)
        u.mem_map(0, 0x10000, UC_PROT_READ | UC_PROT_EXEC)
        u.mem_map(0x20000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
        u.mem_map(0xf0000, 0x10000, UC_PROT_READ | UC_PROT_WRITE)
        self.pixels = [0] * (128*128)
        self.rectangles = 0
        self.fill_calls = 0
        self.transfers = []
        self.pwm_writes = []
        self.flash_read = bytes(256)
        self.flash_written = None
        self.external = bytearray([0xa5]) * 0x800000
        self.flash_reads = []
        self.erased = []
        self.spi_cursor = 0
        u.mem_write(0x2a141,b'\x01')
        self.loop_return = False
        self.rtc_writes = []
        # Trap only driver entry points. The renderer's instructions still execute
        # in Unicorn; a Python callback on every instruction hides MCU work cost.
        for at in (0x010104C4,0x01010952,0x01010C60,0x01016610,0x01016236,
                   0x010161FC,0x01015FA8,0x010108E6,0x010108B2,0x010108C4,0x0101602C):
            u.hook_add(UC_HOOK_CODE,self.hook,begin=at,end=at)
        u.hook_add(UC_HOOK_MEM_WRITE, lambda u,access,address,size,value,data:
                   self.pwm_writes.append((address,value)), begin=0xf0c40,end=0xf0c47)

    def hook(self, u, at, size, data):
        if at == 0x010104C4 and self.loop_return:
            u.emu_stop(); return
        if at == 0x01010952:
            x,y,w,h = [u.reg_read(r) for r in REGS]
            color = struct.unpack('<I', u.mem_read(u.reg_read(UC_ARM_REG_SP),4))[0]
            assert 0 <= x < 128 and 0 <= y < 128 and 0 < w <= 128-x and 0 < h <= 128-y
            for row in range(y,y+h):
                self.pixels[row*128+x:row*128+x+w] = [color] * w
            self.rectangles += 1
            self.fill_calls += 1
            self.transfers.append((x,y,w,h,[color]*(w*h)))
        elif at == 0x01010C60:
            x,y,w,h = [u.reg_read(r) for r in REGS]
            pointer = struct.unpack('<I',u.mem_read(u.reg_read(UC_ARM_REG_SP),4))[0]
            assert 0 <= x < 128 and 0 <= y < 128 and 0 < w <= 128-x and 0 < h <= 128-y
            raw=bytes(u.mem_read(pointer,w*h*2))
            colors=list(struct.unpack('>'+'H'*(w*h),raw))
            for row in range(h):
                self.pixels[(y+row)*128+x:(y+row)*128+x+w]=colors[row*w:(row+1)*w]
            self.rectangles += 1
            self.transfers.append((x,y,w,h,colors))
        elif at == 0x01016610:
            self.rtc_writes.append([u.reg_read(UC_ARM_REG_R0), u.reg_read(UC_ARM_REG_R1)])
        elif at == 0x01016236:
            p,address,n = [u.reg_read(r) for r in REGS[:3]]
            assert 0x100000 <= address and address+n <= 0x800000
            self.flash_reads.append((address,n))
            u.mem_write(p,self.flash_read if address==0x100000 and n==256 else bytes(self.external[address:address+n]))
        elif at == 0x010161FC:
            p,address,n = [u.reg_read(r) for r in REGS[:3]]
            assert address == 0x100000 and n == 256
            self.flash_written = bytes(u.mem_read(p,n))
            self.flash_read = self.flash_written
        elif at == 0x01015FA8:
            address=u.reg_read(UC_ARM_REG_R0)
            assert address % 4096 == 0 and 0x100000 <= address < 0x800000
            self.erased.append(address)
            self.external[address:address+4096]=bytes([255])*4096
        elif at == 0x010108E6:
            if u.reg_read(UC_ARM_REG_R0)==0x2c:self.spi_cursor=0
        elif at == 0x010108B2:
            pass  # Panel address/data byte; native media routine sets a full 128x128 window.
        elif at == 0x010108C4:
            pointer,n=[u.reg_read(r) for r in REGS[:2]]
            assert n%2==0
            colors=list(struct.unpack('>'+'H'*(n//2),u.mem_read(pointer,n)))
            assert self.spi_cursor+len(colors)<=16384
            self.pixels[self.spi_cursor:self.spi_cursor+len(colors)]=colors
            self.spi_cursor+=len(colors)
        elif at != 0x0101602C:
            return
        u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR))

    def call(self, entry, args=()):
        u=self.cpu
        for reg,value in zip(REGS,args): u.reg_write(reg,value)
        u.reg_write(UC_ARM_REG_SP,0x2ff00)
        u.reg_write(UC_ARM_REG_LR,STOP | 1)
        u.emu_start(entry | 1,STOP,count=3_000_000,timeout=10_000_000)
        expected = 0x010104C4 if self.loop_return else STOP
        assert u.reg_read(UC_ARM_REG_PC) == expected, hex(u.reg_read(UC_ARM_REG_PC))
        assert u.reg_read(UC_ARM_REG_SP) == 0x2ff00, hex(u.reg_read(UC_ARM_REG_SP))
        return u.reg_read(UC_ARM_REG_R0)

    def usb(self, body, corrupt_outer=False):
        assert len(body) == 64
        return self.uart(0x41,body,corrupt_outer)[3:67]

    def uart(self, opcode, body=b'', corrupt_outer=False):
        frame=bytearray((0x55,len(body)+2,0,opcode))+body
        frame.append(sum(frame[1:]) & 255)
        if corrupt_outer: frame[-1] ^= 1
        u=self.cpu
        u.mem_write(0x2b5d8,bytes(84))
        u.mem_write(0x2002b,b'\0')
        u.mem_write(0x2a574,bytes(frame))
        u.mem_write(0x2b62c,bytes((1,4,0,0,0,0,0,0)))
        u.mem_write(0x20030,struct.pack('<I',len(frame)))
        self.call(0x0101412C)
        return bytes(u.mem_read(0x2b5d8,84))


def incremental_checks(image,symbols,config,telemetry):
    machine=Machine(image)
    now=300
    hour,minute=12,34

    def time_state(m,tick):
        m.cpu.mem_write(0x2a138,struct.pack('<I',tick))
        m.cpu.mem_write(0x2a14c,bytes((23,minute,hour)))

    def cold_pixels(c,t,fresh=True):
        cold=Machine(image)
        time_state(cold,now)
        assert cold.usb(request(1,c))[6]==0
        if fresh: assert cold.usb(request(2,t))[6]==0
        cold.call(symbols['screen_tick'])
        return cold.pixels

    def verify(start,expected):
        assert machine.pixels==expected, 'Incremental frame differs from a fresh composition'
        assert machine.fill_calls==0, 'Renderer called native background fill'
        for x,y,w,h,colors in machine.transfers[start:]:
            assert h==1
            # Check every intermediate transfer, not just the final frame. A
            # clear-then-repaint implementation would fail this assertion.
            assert colors==expected[y*128+x:y*128+x+w], 'Transient erase/non-final pixel sent'

    time_state(machine,now)
    assert machine.usb(request(1,config))[6]==0
    assert machine.usb(request(2,telemetry))[6]==0
    machine.call(symbols['screen_tick'])
    verify(0,cold_pixels(config,telemetry))
    assert len(machine.pwm_writes)==2
    initial_pixels=sum(w*h for _,_,w,h,_ in machine.transfers)
    assert initial_pixels==16384

    # A new sequence, one raw promille and the seconds can change without any
    # visible pixel changing. In that case there must be zero LCD/PWM writes.
    now+=100;time_state(machine,now)
    machine.cpu.mem_write(0x2a14c,b'\x18')
    same=seal(bytearray(struct.pack('<HHIIHH',235,625,10000,16000,43,0)))
    start=len(machine.transfers)
    machine.usb(request(2,same));machine.call(symbols['screen_tick'])
    assert len(machine.transfers)==start and len(machine.pwm_writes)==2

    now+=100;time_state(machine,now)
    changed=seal(bytearray(struct.pack('<HHIIHH',750,300,5000,16000,44,0)))
    machine.usb(request(2,changed));machine.call(symbols['screen_tick'])
    verify(start,cold_pixels(config,changed))
    changed_pixels=sum(w*h for _,_,w,h,_ in machine.transfers[start:])
    assert 0<changed_pixels<4096
    assert all(25<=y<105 for _,y,_,_,_ in machine.transfers[start:]), 'Static title/clock rewritten'
    assert len(machine.pwm_writes)==2

    # Only the clock digit area changes when the minute advances.
    now+=100;minute=35;time_state(machine,now)
    start=len(machine.transfers);machine.call(symbols['screen_tick'])
    verify(start,cold_pixels(config,changed))
    assert start<len(machine.transfers)
    assert all(112<=y<119 and 50<=x and x+w<=80 for x,y,w,_,_ in machine.transfers[start:])

    # Shorter numbers must remove old digits without a separate blank phase.
    for sequence,cpu,ram,used,total in ((45,1000,1000,16000,16000),
                                        (46,0,0,0,16000),
                                        (47,99,5,87,16000)):
        now+=100;time_state(machine,now)
        changed=seal(bytearray(struct.pack('<HHIIHH',cpu,ram,used,total,sequence,0)))
        start=len(machine.transfers);machine.usb(request(2,changed));machine.call(symbols['screen_tick'])
        verify(start,cold_pixels(config,changed))

    now+=3000;time_state(machine,now)
    start=len(machine.transfers);machine.call(symbols['screen_tick'])
    verify(start,cold_pixels(config,changed,False))
    now+=100;time_state(machine,now)
    start=len(machine.transfers);machine.usb(request(2,changed));machine.call(symbols['screen_tick'])
    verify(start,cold_pixels(config,changed))

    # Overlapping, moved and clipped widgets must keep the same layer ordering
    # as a complete frame, even across a layout/color/brightness change.
    for edge in (False,True):
        moved=bytearray(config)
        if edge:
            for offset in (2,3,4,5,6,7,21,22):moved[offset]=127
            moved[1]=3;moved[10]=2;moved[9]=2
        else:
            moved[2:8]=bytes((4,8,4,8,4,8));moved[21:23]=bytes((4,8))
            moved[12:21]=bytes((255,200,100,10,20,80,200,60,180));moved[11]=61
        seal(moved);now+=100;time_state(machine,now)
        start=len(machine.transfers);machine.usb(request(1,moved));machine.call(symbols['screen_tick'])
        verify(start,cold_pixels(moved,changed))

    disabled=bytearray(config);disabled[0]&=~1;seal(disabled)
    machine.usb(request(1,disabled));assert machine.call(symbols['screen_tick'])==0
    assert machine.fill_calls==1 and machine.pixels==[0]*16384
    assert bytes(machine.cpu.mem_read(0x2a124,4))==b'\xff'*4
    machine.fill_calls=0  # The explicit ownership handoff is allowed one clear.
    machine.pixels=[0x1234]*16384  # Stock UI overwrites the display while disabled.
    now+=100;time_state(machine,now)
    start=len(machine.transfers);machine.usb(request(1,config));machine.call(symbols['screen_tick'])
    verify(start,cold_pixels(config,changed))
    return {'initial_pixels':initial_pixels,'changed_cpu_ram_pixels':changed_pixels,
            'native_background_fill_calls':machine.fill_calls,
            'all_intermediate_pixels_final':True}


def main():
    manifest=json.loads((OUT/'manifest.json').read_text())
    image=Path(manifest['candidate']).read_bytes()
    assert hashlib.sha256(image).hexdigest() == manifest['candidate_sha256']
    stock=Path(manifest['manufacturer_image']).read_bytes()
    assert hashlib.sha256(stock).hexdigest() == manifest['manufacturer_image_sha256']
    symbols={k:int(v,16) for k,v in manifest['symbols'].items()}
    original=Machine(stock); patched=Machine(image)
    ad=bytearray(64);ad[0]=0xad;ad[7]=0x52
    assert patched.usb(ad) == original.usb(ad)
    assert patched.usb(ad)[:8] == bytes.fromhex('ad04010000000000')
    clock=bytearray(64);clock[0]=0x28;clock[7]=0xd7
    clock[8:15]=bytes((7,234,10,23,12,34,23))
    assert patched.usb(clock)==original.usb(clock)
    assert patched.cpu.mem_read(0x2a14c,5)==bytes((23,34,12,23,10))
    assert original.usb(request()) == bytes(64)  # Existing 0x2c is a no-op.
    reply=patched.usb(request())
    assert reply[:7] == b'\x2cAJSC\x01\x00' and sum(reply[:8]) & 255 == 255
    assert reply[63] == 255
    config=bytearray(reply[8:56]);assert crc(config[:-2]) == int.from_bytes(config[-2:],'little')
    config[0]=63;seal(config)
    vectors=json.loads((OUT/'telemetry-host.json').read_text())
    assert list(config)==vectors['config_bytes']
    assert list(request(1,config))==vectors['config_report'][1:]
    assert patched.usb(request(1,config))[8:56] == config
    before=bytes(patched.cpu.mem_read(STATE,80))
    malformed=[]
    for offset,value in ((0,128),(1,0),(8,0),(9,13),(10,3),(11,101),(23,21),(44,4),(24,0)):
        bad=bytearray(config);bad[offset]=value;seal(bad)
        r=patched.usb(request(1,bad));assert r[6] == 2 and r[8:56] == config
        malformed.append({'offset':offset,'value':value})
    for bit in range(48*8):
        bad=bytearray(config);bad[bit//8] ^= 1 << (bit%8)
        assert patched.usb(request(1,bad))[6] == 2
    assert patched.usb(request(1,config),corrupt_outer=True) == bytes(64)
    bad_header=request();bad_header[7]^=1
    assert patched.usb(bad_header)[6] == 1
    bad_version=request();bad_version[12]=2
    assert patched.usb(bad_version)[6] == 1
    assert patched.usb(request(99))[6] == 3
    telemetry=seal(bytearray(struct.pack('<HHIIHH',234,625,10000,16000,42,0)))
    assert list(telemetry)==vectors['sample_bytes']
    assert list(request(2,telemetry))==vectors['sample_report'][1:]
    r=patched.usb(request(2,telemetry))
    assert r[6] == 0 and r[56:62] == telemetry[:4]+telemetry[12:14]
    for bad in (seal(bytearray(struct.pack('<HHIIHH',1001,625,10000,16000,43,0))),
                seal(bytearray(struct.pack('<HHIIHH',234,625,17000,16000,43,0)))):
        assert patched.usb(request(2,bad))[6] == 2
    patched.cpu.mem_write(0x2a138,struct.pack('<I',300))
    patched.cpu.mem_write(0x2a14c,bytes((23,34,12)))
    assert patched.call(symbols['screen_tick']) == 1 and patched.rectangles > 100
    pwm=struct.unpack('<II',patched.cpu.mem_read(0xf0c40,8))
    assert pwm == (26214,39321)
    from PIL import Image
    preview=Image.new('RGB',(128,128))
    preview.putdata([(((p>>11)&31)*255//31,((p>>5)&63)*255//63,(p&31)*255//31) for p in patched.pixels])
    preview.save(OUT/'telemetry-emulated.png')
    assert preview.tobytes()==Image.open(OUT/'telemetry-host.png').convert('RGB').tobytes()
    incremental=incremental_checks(image,symbols,config,telemetry)
    count=patched.rectangles
    assert patched.call(symbols['screen_tick']) == 1 and patched.rectangles == count
    patched.cpu.mem_write(0x2a138,struct.pack('<I',4000))
    assert patched.call(symbols['screen_tick']) == 1 and patched.rectangles > count
    old_page=bytes((i*7)&255 for i in range(256))
    patched.cpu.mem_write(0x2e000,old_page)
    patched.call(symbols['screen_config_write'],(0x2e000,0x100000,256))
    # A RAM-only edit must not become persistent when the stock UI saves its own page.
    assert patched.flash_written[133]&1==0
    saved=patched.usb(request(3,config));assert saved[6] == 0
    assert patched.cpu.mem_read(0x2a142,1) == b'\x01'
    patched.cpu.mem_write(0x2e000,old_page)
    patched.call(symbols['screen_config_write'],(0x2e000,0x100000,256))
    page=patched.flash_written
    assert page[:128] == old_page[:128] and page[234:] == old_page[234:]
    assert page[128:133] == b'AJSC\x01' and page[133:181] == config
    reboot=Machine(image);reboot.flash_read=page
    reboot.call(symbols['screen_config_read'],(0x2e000,0x100000,256))
    assert reboot.usb(request())[8:56] == config
    changed=bytearray(config);changed[18]=255;seal(changed)
    assert reboot.usb(request(1,changed))[8:56] == changed
    reboot.call(symbols['screen_config_read'],(0x2e000,0x100000,256))
    assert reboot.usb(request())[8:56] == changed  # Native reread cannot discard a RAM edit.
    # Nested critical sections restore the incoming interrupt mask.
    masked=Machine(image);masked.cpu.reg_write(UC_ARM_REG_PRIMASK,1)
    masked.usb(request())
    assert masked.cpu.reg_read(UC_ARM_REG_PRIMASK)==1
    # Main-loop hook must preserve its caller's saved registers and stack.
    saved_regs=(UC_ARM_REG_R4,UC_ARM_REG_R5,UC_ARM_REG_R6,UC_ARM_REG_R7)
    for reg in saved_regs: patched.cpu.reg_write(reg,0x12340000+reg)
    patched.loop_return=True
    # Enter through the patched manufacturer BL, not just the C symbol.
    patched.call(0x010104EA)
    for reg in saved_regs: assert patched.cpu.reg_read(reg) == 0x12340000+reg
    patched.loop_return=False
    config[0]=0;seal(config);assert patched.usb(request(1,config))[6] == 0
    patched.call(symbols['screen_main_hook'])
    from emulate_screen_pages import checks as page_checks
    pages_checked=page_checks(image,symbols,bytes(vectors['config_bytes']),bytes(vectors['sample_bytes']))
    graphics=[]
    for display in ['both','bars','percent']:
        v=json.loads((OUT/f'page-game-{display}.json').read_text())
        game=Machine(image);c=bytes(v['config_bytes']);p=bytearray(v['pages_bytes']);p[0]=(p[0]|1)&~2;seal(p)
        assert game.usb(request(13))[:9]==bytes([0x2c,65,74,71,77,1,0,(255-sum([0x2c,65,74,71,77,1]))&255,1])
        assert game.usb(request(1,c))[6]==0 and game.usb(request(5,p))[6]==0
        sample=bytes(v['sample_bytes']);assert game.usb(request(14,sample))[6]==0
        game.cpu.mem_write(0x2a138,struct.pack('<I',300));assert game.call(symbols['screen_tick'])==1
        screen=Image.new('RGB',(128,128));screen.putdata([(((p>>11)&31)*255//31,((p>>5)&63)*255//63,(p&31)*255//31) for p in game.pixels])
        screen.save(OUT/f'game-emulated-{display}.png')
        assert screen.tobytes()==Image.open(OUT/f'page-game-{display}.png').convert('RGB').tobytes(),f'Game preview mismatch: {display}'
        for bit in range(32*8):
            bad=bytearray(sample);bad[bit//8]^=1<<(bit%8);assert game.usb(request(14,bad))[6]==2
        # A new extended envelope cannot bypass the original CPU sample CRC.
        bad=bytearray(sample);bad[0]^=1;seal(bad);assert game.usb(request(14,bad))[6]==2
        for at,value in [(16,2),(17,8),(18,255),(24,0)]:
            bad=bytearray(sample);bad[at]=value
            if at==18:bad[19]=255
            if at==24:bad[24:28]=bytes(4)
            seal(bad);assert game.usb(request(14,bad))[6]==2
        game.cpu.mem_write(0x2a138,struct.pack('<I',3500));assert game.call(symbols['screen_tick'])==1
        # A legacy sample clears extension flags; it cannot retain stale FPS.
        assert game.usb(request(2,telemetry))[6]==0
        reply=game.usb(request(13));assert reply[32:48]==bytes(16)
        graphics.append(f'GPU/FPS {display}: pixel-exact Rust/ARM,256 corruptions,CPU CRC,range guards,stale and legacy reset')
    design_checks=[]
    for path in sorted(OUT.glob('page-edge-*.json')):
        v=json.loads(path.read_text());m=Machine(image)
        assert m.usb(request(1,bytes(v['config_bytes'])))[6]==0
        assert m.usb(request(5,bytes(v['pages_bytes'])))[6]==0
        marker=m.usb(request(13));assert marker[9:12]==b'UI\x04' and sum(marker[9:13])%256==255
        if v['sample_bytes'] is not None:assert m.usb(request(14,bytes(v['sample_bytes'])))[6]==0
        m.cpu.mem_write(0x2a138,struct.pack('<I',300));assert m.call(symbols['screen_tick'])==1
        actual=bytes(v for p in m.pixels for v in (((p>>11)&31)*255//31,((p>>5)&63)*255//63,(p&31)*255//31))
        assert actual==Image.open(path.with_suffix('.png')).convert('RGB').tobytes(),path.name
        assert m.fill_calls==0
        before=len(m.transfers);assert m.call(symbols['screen_tick'])==1 and len(m.transfers)==before
        if v['sample_bytes'] is not None:
            # Exercise shorter antialiased numbers and Stop with the new layouts.
            changed=bytearray(v['sample_bytes']);changed[0:4]=bytes(4)
            changed[12:14]=struct.pack('<H',(int.from_bytes(changed[12:14],'little')+1)&65535)
            changed[:16]=seal(changed[:16]);changed[18:24]=bytes(6)
            if changed[17]&4:changed[28:30]=b'\x0a\0'
            seal(changed)
            def final_pixels(sample):
                cold=Machine(image);cold.usb(request(1,bytes(v['config_bytes'])))
                cold.usb(request(5,bytes(v['pages_bytes'])))
                if sample is not None:cold.usb(request(14,sample))
                else:cold.usb(request(9))  # Stop selects the listed clock, when present.
                cold.cpu.mem_write(0x2a138,struct.pack('<I',300));cold.call(symbols['screen_tick'])
                return cold.pixels
            for tick,sample in ((400,changed),(500,None)):
                start=len(m.transfers)
                m.cpu.mem_write(0x2a138,struct.pack('<I',tick))
                if sample is None:
                    assert m.usb(request(9))[6]==0 and m.usb(request())[62]==0
                else:assert m.usb(request(14,sample))[6]==0
                m.call(symbols['screen_tick']);expected=final_pixels(sample)
                assert m.pixels==expected and m.fill_calls==0,path.name
                for x,y,w,h,colors in m.transfers[start:]:
                    assert h==1 and colors==expected[y*128+x:y*128+x+w], 'Modern transition sent transient pixels'
        design_checks.append(path.stem)
    result={'candidate_sha256':manifest['candidate_sha256'],'hardware_access':False,
        'checks':['original AD/version and clock replies unchanged','stock 0x2c no-op',
                  'new capabilities/configuration/telemetry replies','10 invalid config ranges',
                  '384 single-bit configuration corruptions','outer and inner checksums',
                  'unsupported version/action','telemetry ranges',
                  'host and firmware packet/CRC equality','pixel-exact host/firmware preview',
                  'incremental LCD writes contain only final pixels; no clear phase',
                  'unchanged visible samples and seconds produce zero LCD/PWM writes',
                  'shorter numbers, minute changes, stale/recovered samples and overlapping/clipped layouts',
                  'original PWM writes','bounded rectangle rendering','unchanged frame skipped',
                  'RAM-only edits remain separate from persistent settings',
                  'native reread preserves unsaved edits','interrupt mask preservation','stale data timeout',
                  'existing configuration bytes preserved','configuration restored after simulated reboot',
                  'patched main-loop callsite and stack/register preservation','stock UI fallback'],
        'unverified':['physical flashing/recovery','actual UART/PWM/SPI timing',
                      'panel appearance/orientation','real external-flash writes','power-cycle retention on hardware'],
        'page_manager_checks':pages_checked,'graphics_checks':graphics,'design_checks':design_checks,
        'incremental_rendering':incremental,'frame_preview':str(OUT/'telemetry-emulated.png')}
    (OUT/'emulation.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(result,indent=2))


if __name__=='__main__':main()
