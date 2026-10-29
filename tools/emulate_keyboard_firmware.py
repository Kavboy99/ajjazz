"""Execute the compiled AJKS ARM code, USB parser and original hook sites in Unicorn.
No USB/device access. Native peripherals are explicit, logged stubs.
"""
from pathlib import Path
import json,struct,random,hashlib,sys
import unicorn as uc
from unicorn.arm_const import *
from build_keyboard_firmware import ROOT,OUT,crc,scene

IMAGE=OUT/'ak820max-ajks-v1-candidate.bin'
class Machine:
    def __init__(self,reset_config=True):
        self.cpu=uc.Uc(uc.UC_ARCH_ARM,uc.UC_MODE_THUMB|uc.UC_MODE_MCLASS)
        self.cpu.mem_map(0,0x80000);self.cpu.mem_write(0,IMAGE.read_bytes())
        if reset_config:
            # Protocol/render tests have stable fixtures, independent of the
            # user's seeded bank (which may intentionally contain empty slots).
            self.cpu.mem_write(0x24000,bytes(0x2400))
            fixtures=[('Aurora',7,[[20,65,180],[35,240,180]]),('Plasma',8,[[255,25,100],[60,50,255]]),
                      ('Comet',9,[[0,10,30],[80,220,255]]),('Reactive',6,[[0,0,0],[240,165,45]])]
            for i,(name,effect,colors) in enumerate(fixtures):self.cpu.mem_write(0x24000+i*1024,bytes(scene(name,effect,colors)))
        self.cpu.mem_map(0x20000000,0x30000)
        self.cpu.mem_write(0x20006e00,b'\xa5'*0x200)
        self.cpu.mem_write(0x20008000,b'\x5a'*0x100)
        self.cpu.mem_map(0x40000000,0x200000);self.cpu.mem_map(0xe000e000,0x2000)
        self.symbols={k:int(v,16) for k,v in json.loads((OUT/'manifest.json').read_text())['symbols'].items()}
        self.calls=[];self.writes=[];self.gpio=3;self.instructions=0;self.fail_write=False;self.sent=[];self.real_pwm=False
        self.cpu.hook_add(uc.UC_HOOK_CODE,self.code)
        self.cpu.hook_add(uc.UC_HOOK_MEM_WRITE,self.write)
    def read(self,a,n=1):return bytes(self.cpu.mem_read(a,n))
    def put(self,a,p):self.cpu.mem_write(a,bytes(p))
    def write(self,c,access,a,n,v,user):
        if a<0x80000:raise AssertionError(f'Direct ROM write {a:x}; must use bounded native writer')
        if 0x20006e00<=a<0x20007000 or 0x20008000<=a<0x20008100:raise AssertionError(f'SRAM reservation violation {a:x}')
    def code(self,c,a,n,user):
        self.instructions+=1
        if a==0x7f000:c.emu_stop();return
        if a==0x11414:
            endpoint,address,size=[c.reg_read(r) for r in [UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2]]
            self.sent.append((endpoint,self.read(address,size)));c.reg_write(UC_ARM_REG_R0,0);c.reg_write(UC_ARM_REG_PC,c.reg_read(UC_ARM_REG_LR));return
        if a in [0x1215c,0x5cb8,0x7910,0x1984,0xcf90,0xd264,0xaf8c,0xea90,0x12178,0x13d98,0x13d94,0x11818]:
            self.calls.append(a)
            if a==0xcf90 and self.real_pwm:return
            if a==0x1215c:c.reg_write(UC_ARM_REG_R0,self.gpio)
            if a==0x1984:self.put(0x200001a0,[10])
            if a==0x5cb8:
                src,n,dest=[c.reg_read(r) for r in [UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2]]
                assert n==512 and dest%512==0 and 0x24000<=dest<0x26400
                self.writes.append((dest,n));p=self.read(src,n)
                # Model interrupted page programming: old bank must remain valid.
                self.put(dest,p[:128]+b'\xff'*384 if self.fail_write else p)
            if a==0x13d98:
                # Tail trampoline already pushed native six-register frame.
                sp=c.reg_read(UC_ARM_REG_SP);vals=struct.unpack('<6I',self.read(sp,24))
                for r,v in zip([UC_ARM_REG_R4,UC_ARM_REG_R5,UC_ARM_REG_R6,UC_ARM_REG_R7,UC_ARM_REG_R8,UC_ARM_REG_LR],vals):c.reg_write(r,v)
                c.reg_write(UC_ARM_REG_SP,sp+24)
            c.reg_write(UC_ARM_REG_PC,c.reg_read(UC_ARM_REG_LR))
    def call(self,name,advance_ms=None):
        if advance_ms is None:advance_ms=20 if name=='aj_lighting' else 0
        if advance_ms:
            now=struct.unpack('<I',self.read(0x20000238,4))[0]
            self.put(0x20000238,struct.pack('<I',(now+advance_ms)&0xffffffff))
        c=self.cpu;sp=0x20006d98;c.reg_write(UC_ARM_REG_SP,sp);c.reg_write(UC_ARM_REG_LR,0x7f001)
        preserved=[UC_ARM_REG_R4,UC_ARM_REG_R5,UC_ARM_REG_R6,UC_ARM_REG_R7,UC_ARM_REG_R8,UC_ARM_REG_R9,UC_ARM_REG_R10,UC_ARM_REG_R11]
        for r in preserved:c.reg_write(r,0x12121212+r)
        c.emu_start(self.symbols[name]|1,0x7f000,count=500000)
        assert c.reg_read(UC_ARM_REG_PC)==0x7f000, f'{name} timed out'
        assert c.reg_read(UC_ARM_REG_SP)==sp,f'{name} stack leak'
        assert all(c.reg_read(r)==0x12121212+r for r in preserved),f'{name} clobbered callee-save'
        assert self.read(0x20006e00,512)==b'\xa5'*512
        assert self.read(0x20008000,256)==b'\x5a'*256
    def packet(self,op,p=b'',bad_crc=False):
        q=bytearray(64);q[:8]=bytes([0xaa,0x70,65,75,1,op,37,len(p)]);q[8:8+len(p)]=p
        struct.pack_into('<H',q,62,crc(q[:62])^(1 if bad_crc else 0))
        self.put(0x200052cc,q);self.put(0x2000022f,[1]);self.call('aj_usb')
        r=self.read(0x2000528c,64)
        assert r[:7]==bytes([0x55,0x70,75,83,1,op,37]) and crc(r[:62])==struct.unpack_from('<H',r,62)[0]
        assert self.read(0x2000022f)==b'\0' and self.read(0x2000022e)==b'\1'
        return r[7],r[8:56]
    def upload(self,slot,p,config=False):
        assert self.packet(7 if config else 2,b'' if config else bytes([slot]))[0]==0
        for i in range(16):assert self.packet(8 if config else 3,(b'' if config else bytes([slot]))+bytes([i])+p[i*32:i*32+32])[0]==0
        return self.packet(9 if config else 4,b'' if config else bytes([slot]))[0]
    def config(self):
        return bytearray(b''.join(self.packet(6,bytes([i]))[1][:32] for i in range(16)))

def main():
    tests=[];m=Machine()
    def check(label):tests.append(label)
    assert m.packet(0)[1][:7]==b'AJKS\x01\x08\xff';check('independent capability magic, native default')
    assert m.packet(0)[1][16:24].hex()==json.loads((OUT/'manifest.json').read_text())['build_id']
    m.symbols['native_send']=0x13cac;m.call('native_send')
    assert m.sent[-1][0]==2 and len(m.sent[-1][1])==64 and m.sent[-1][1][:4]==b'\x55\x70KS'
    check('original USB sender transmits AJKS reply as exactly 64 bytes')
    m.call('aj_lighting');assert 0x7910 in m.calls;check('unselected stock lighting fallback')
    m.call('aj_rotation');assert 0x12178 in m.calls;check('default encoder retains native quadrature')
    m.call('aj_press');assert 0x13d94 in m.calls;check('default press retains native handler')
    assert m.packet(5,b'\0')[0]==0
    m.call('aj_lighting');a=m.read(0x2000478c,378)
    for _ in range(25):m.call('aj_lighting')
    assert a!=m.read(0x2000478c,378) and not m.writes;check('animation advances without USB and without flash writes')
    cfg=m.config();cfg[16:28]=bytes([2,0,255,255,3,0,1,0,1,0,0xcd,0])
    struct.pack_into('<H',cfg,510,crc(cfg[:510]));assert m.upload(0,cfg,True)==0
    old=m.packet(0)[1][7]
    # Native clockwise transition 1->3, debounced twice.
    for v in [1,1,1,3,3,3]:m.gpio=v;m.call('aj_rotation')
    m.call('aj_lighting');assert m.packet(0)[1][6]==1;check('clockwise encoder selects next saved autonomous scene')
    for v in [2,2,2,3,3,3]:m.gpio=v;m.call('aj_rotation')
    m.call('aj_lighting');assert m.packet(0)[1][7]==old-16;check('counterclockwise changes brightness')
    m.put(0x20000088,[1]);m.put(0x200000b9,[0xe2,0]);m.call('aj_press')
    assert m.read(0x200000b9,2)==b'\xcd\0';check('press replaces native mute with play/pause')
    for _ in range(4):m.call('aj_lighting')
    assert m.read(0x200000b9,2)==b'\0\0';check('consumer report releases without stuck media key')
    before=len(m.writes)
    for op in range(12):assert m.packet(op,b'',True)[0]!=0
    assert len(m.writes)==before;check('invalid CRC never writes persistent data')
    p=scene('Test',0,[[255,0,0],[0,0,255]])
    assert m.upload(7,p)==0;assert m.packet(5,b'\x07')[0]==0;m.call('aj_lighting')
    assert m.read(0x2000478c)[0]>0 and m.read(0x20004888)[0]==0;check('compiled layer colors reach native LED input planes')
    oldpage=b''.join(m.packet(1,bytes([7,i]))[1][:32] for i in range(16))
    m.fail_write=True;assert m.upload(7,scene('Interrupted',2,[[0,255,0],[255,0,0]]))==3;m.fail_write=False
    assert b''.join(m.packet(1,bytes([7,i]))[1][:32] for i in range(16))==oldpage;check('interrupted programming retains previous scene bank')
    # Reboot only extension RAM; persisted config and scenes reload, no USB upload.
    m.put(0x20007000,bytes(4096));m.call('aj_lighting');assert m.packet(0)[1][6]==0;check('saved mode/config load after extension reset')
    for effect in range(22):
        p=scene('Effects',effect,[[20,60,180],[230,170,40]]);p[5]=8
        for i in range(1,8):p[64+i*48:112+i*48]=p[64:112]
        struct.pack_into('<H',p,510,crc(p[:510]));assert m.upload(6,p)==0;assert m.packet(5,b'\x06')[0]==0
        m.put(0x20000083,[0]);m.put(0x20000088,[1]);m.call('aj_observe')
        start=m.instructions;m.call('aj_lighting');assert m.instructions-start<300000
    check('all 22 effects with eight layers execute inside bounded instruction budget')
    # Exercise real patched prologue and native tail ABI.
    m.call('aj_key_hook');check('key observer trampoline preserves stack and callee-save registers')
    before=len(m.writes);rng=random.Random(1759)
    for _ in range(400):
        op=rng.randrange(12);valid_lengths=[0,2,1,34,1,1,1,0,33,0,1,0]
        n=rng.randrange(49)
        if n==valid_lengths[op]:n=48
        data=bytes(rng.randrange(256) for _ in range(n));assert m.packet(op,data)[0]!=0
    assert len(m.writes)==before;check('400 deterministic malformed commands cannot reach flash writer')
    # Select stock and ensure Fn/effect native transitions do not leave custom frames owned.
    assert m.packet(5,b'\xff')[0]==0;m.calls.clear();m.call('aj_lighting');assert 0x7910 in m.calls
    check('native effect return relinquishes renderer')
    # Integration: execute ORIGINAL MCU PWM conversion and WS2812 serializer.
    # Sparse Hall IDs must be translated into the OEM's compact LED indices.
    pipeline=Machine();white=scene('All82',0,[[255,255,255],[255,255,255]])
    white[7]=255;struct.pack_into('<H',white,510,crc(white[:510]));assert pipeline.upload(7,white)==0
    assert pipeline.packet(5,b'\x07')[0]==0;pipeline.real_pwm=True;pipeline.call('aj_lighting')
    native_map=IMAGE.read_bytes()[0x16338:0x16338+126]
    layout=json.loads((ROOT/'assets/ak820max-layout.json').read_text(encoding='utf-8'))
    physical=[key['id'] for row in layout for key in row]
    assert len({native_map[id] for id in physical})==82
    for id in physical:
        led=native_map[id]
        assert [pipeline.read(0x20004984+126*j+led)[0] for j in range(3)]==[63,63,63],f'Physical key{id} LED{led} is dark'
    pipeline.symbols['native_rgb_send']=0x14b90;pipeline.call('native_rgb_send')
    for id in physical:
        assert pipeline.read(0x200040a4+native_map[id]*12+1,12)==bytes([0x88,0xee,0xee,0xee])*3
    check('all82 physical keys pass ORIGINAL PWM and ORIGINAL WS2812 serializer')
    for name,address,value in [('native zero gain',0x200001b1,0),('native RGB gate',0x2000022b,1),('native per-key gate',0x200001bf,1)]:
        cold=Machine();assert cold.upload(7,white)==0
        assert cold.packet(5,b'\x07')[0]==0;cold.real_pwm=True
        cold.put(address,[value]);cold.call('aj_lighting');cold.symbols['native_rgb_send']=0x14b90;cold.call('native_rgb_send')
        for id in physical:
            led=native_map[id]
            assert [cold.read(0x20004984+126*j+led)[0] for j in range(3)]==[63,63,63],name
            assert cold.read(0x200040a4+led*12+1,12)==bytes([0x88,0xee,0xee,0xee])*3,name
        assert cold.read(address)[0]==value,'Do not mutate native power/calibration flags'
    check('cold boot/wake native gain and RGB gates cannot blank custom PWM; native flags unchanged')
    # A one-key program lights only the corresponding LED, not its Hall index.
    white[84:100]=bytes(16);white[84+33//8]|=1<<(33%8);struct.pack_into('<H',white,510,crc(white[:510]));assert pipeline.upload(7,white)==0
    pipeline.call('aj_lighting')
    lit=[led for led in range(126) if pipeline.read(0x20004984+led)[0]]
    assert lit==[native_map[33]] and native_map[33]!=33
    check('single Q mask is translated once and does not illuminate a different key')
    seeded=Machine(reset_config=False);cap=seeded.packet(0)[1]
    seed_path=json.loads((OUT/'manifest.json').read_text()).get('seed_state')
    if seed_path:
        seed=json.loads(Path(seed_path).read_text(encoding='utf-8-sig'));cfg=seeded.config()
        assert cap[6]==(255 if seed['config']['selected'] is None else seed['config']['selected'])
        assert cap[7]==seed['config']['brightness']
        for slot,p in enumerate(seed['scenes']):
            if p is not None:assert b''.join(seeded.packet(1,bytes([slot,i]))[1][:32] for i in range(16))==bytes(p)
        check('exact saved programs and selected mode/brightness migrate into fixed image')
    # Time is the OEM HAL millisecond clock, independent of the number of hook calls.
    timing=Machine();timing.packet(5,b'\0');timing.call('aj_lighting',0)
    for _ in range(100):timing.call('aj_lighting',0)
    assert struct.unpack('<I',timing.read(0x20007004,4))[0]==0
    for gap in [3,7,9]:timing.call('aj_lighting',gap)
    assert struct.unpack('<I',timing.read(0x20007004,4))[0]==0
    timing.call('aj_lighting',1);assert struct.unpack('<I',timing.read(0x20007004,4))[0]==1
    timing.call('aj_lighting',980);assert struct.unpack('<I',timing.read(0x20007004,4))[0]==50
    cap=timing.packet(0)[1];assert cap[32:36]==b'CLK2' and cap[36:38]==bytes([1,20]) and struct.unpack_from('<I',cap,24)[0]==50
    check('1000ms advances exactly50 frames despite100 redundant hooks and irregular calls; runtime clock reported')
    timing.symbols['original_systick']=0xaff8;timing.put(0x20000234,struct.pack('<I',1))
    tick_before=struct.unpack('<I',timing.read(0x20000238,4))[0]
    for _ in range(1000):timing.call('original_systick')
    assert struct.unpack('<I',timing.read(0x20000238,4))[0]==tick_before+1000
    timing.call('aj_lighting',0);assert struct.unpack('<I',timing.read(0x20007004,4))[0]==100
    check('ORIGINAL SysTick handler and ORIGINAL HAL tick increment produce the same50Hz time base')
    paused=timing.config();paused[10]=1;struct.pack_into('<H',paused,510,crc(paused[:510]));assert timing.upload(0,paused,True)==0
    timing.call('aj_lighting',1000);assert struct.unpack('<I',timing.read(0x20007004,4))[0]==100
    paused[10]=0;struct.pack_into('<H',paused,510,crc(paused[:510]));assert timing.upload(0,paused,True)==0
    timing.call('aj_lighting',20);assert struct.unpack('<I',timing.read(0x20007004,4))[0]==101
    check('pause/resume uses wall-clock deltas without a catch-up jump')
    wrap=Machine();wrap.put(0x20000238,struct.pack('<I',0xfffffff0));wrap.packet(5,b'\0');wrap.call('aj_lighting',40)
    assert struct.unpack('<I',wrap.read(0x20007004,4))[0]==2
    check('OEM millisecond counter wrap keeps correct effect timing')
    # Unequal channels: white-only tests cannot catch channel swaps or a wrong transfer curve.
    def pwm(v):
        x=v/255;linear=x/12.92 if x<=0.04045 else ((x+0.055)/1.055)**2.4
        return int(linear*63+0.5)
    def wire(v):return bytes([0x88,0x8e,0xe8,0xee][(v>>shift)&3] for shift in [6,4,2,0])
    for rgb in [[255,0,0],[0,255,0],[0,0,255],[128,128,128],[31,0,255],[71,63,127],[255,64,17]]:
        color=scene('Color transfer',0,[rgb,rgb]);color[7]=255;struct.pack_into('<H',color,510,crc(color[:510]));assert pipeline.upload(7,color)==0
        pipeline.call('aj_lighting');pipeline.call('native_rgb_send')
        expected=list(map(pwm,rgb));assert [pipeline.read(0x20004984+126*j)[0] for j in range(3)]==expected
        assert pipeline.read(0x200040a5,12)==wire(expected[1])+wire(expected[0])+wire(expected[2])
    check('sRGB primaries, gray128, violet and unequal mixed colors pass ORIGINAL PWM and GRB serializer without exceeding stock63')
    # Autonomous hold indicator: short click is deferred; a long hold never
    # invokes it. Overlay operates after native PWM and restores all prior PWM.
    bat=Machine();bat.packet(5,b'\0');cfg=bat.config()
    cfg[24:28]=bytes([3,0,1,0]);cfg[28:34]=bytes([1,1,0,0,2,3]);cfg[34:40]=bytes([255,0,0,0,0,255]);cfg[40]=255
    struct.pack_into('<H',cfg,510,crc(cfg[:510]));assert bat.upload(0,cfg,True)==0
    start_writes=len(bat.writes);bat.put(0x2000012e,[50]);bat.call('aj_lighting')
    base=bat.read(0x20004984,378)
    bat.put(0x20000088,[1]);bat.call('aj_press');assert bat.packet(0)[1][6]==0
    bat.call('aj_lighting',1999);assert bat.packet(0)[1][41]==0
    # Another physical key overwrites INPUT[8]; it cannot cancel the encoder hold.
    bat.put(0x20000088,[0]);bat.call('aj_lighting',1)
    cap_bat=bat.packet(0)[1];assert cap_bat[38:42]==bytes([22,1,50,1]) and cap_bat[6]==0
    top=[k['id'] for k in layout[0]];assert len(top)==14
    for i,id in enumerate(top):
        led=native_map[id]
        if i<7:assert [bat.read(0x20004984+j*126+led)[0] for j in range(3)]==[63,0,0]
    bat.put(0x20000088,[0]);bat.call('aj_press');bat.call('aj_lighting',2999);assert bat.packet(0)[1][41]==1
    bat.call('aj_lighting',1);assert bat.packet(0)[1][41]==0 and bat.packet(0)[1][6]==0
    assert len(bat.writes)==start_writes
    check('2000ms encoder hold overlays only top row; other keys cannot cancel hold; release expires after3000ms without mode/flash changes')
    bat.put(0x20000088,[1]);bat.call('aj_press');bat.call('aj_lighting',100);assert bat.packet(0)[1][6]==0
    bat.put(0x20000088,[0]);bat.call('aj_press');assert bat.packet(0)[1][6]==1
    bat.call('aj_press');assert bat.packet(0)[1][6]==1
    check('short encoder click executes exactly once on release, long hold suppresses short action')
    # Stock, paused, zero brightness and background restoration. No native calls
    # are replaced by a host stream; the base PWM stays within its existing power.
    for whole in [0,1]:
        for background in [0,1]:
            for percent in [0,1,50,100,255]:
                b=Machine();c=b.config();c[28:34]=bytes([1,1,whole,background,1,1]);c[34:40]=bytes([255,0,0,0,0,255]);c[40]=255
                struct.pack_into('<H',c,510,crc(c[:510]));assert b.upload(0,c,True)==0
                pwm_base=bytes((i*7)%64 for i in range(378));b.put(0x20004984,pwm_base);b.put(0x2000012e,[percent]);b.put(0x20000088,[1]);b.call('aj_press');b.call('aj_lighting',1000)
                if percent<=100:
                    # Read exact generated ordering, and independently check its
                    # coverage against layout and native compact LED map.
                    address=b.symbols['battery_order' if whole else 'top_row'];order=list(b.read(address,82 if whole else 14))
                    assert set(order)==set(physical if whole else top)
                    filled=(percent*len(order)+99)//100
                    for rank,id in enumerate(order):
                        led=native_map[id];value=[b.read(0x20004984+j*126+led)[0] for j in range(3)]
                        expected=[63,0,0] if rank<filled else [0,0,63] if background else [pwm_base[j*126+led] for j in range(3)]
                        assert value==expected,(whole,background,percent,rank,value,expected)
                    untouched=set(range(126))-set(native_map[id] for id in order)
                    assert all(b.read(0x20004984+j*126+led)[0]==pwm_base[j*126+led] for led in untouched for j in range(3))
                else:assert b.read(0x20004984,378)==pwm_base
                b.put(0x20000088,[0]);b.call('aj_press');b.call('aj_lighting',1000);assert b.read(0x20004984,378)==pwm_base
    check('battery0/1/50/100 and invalid255;14/82keys; overlay/background colors; original native PWM restored exactly')
    # Invalid persistent indicator settings must not reach flash.
    for at,value in [(28,2),(29,2),(30,2),(31,2),(32,0),(32,6),(33,0),(33,11)]:
        bad=bytearray(cfg);bad[at]=value;struct.pack_into('<H',bad,510,crc(bad[:510]));writes=len(bat.writes);assert bat.upload(0,bad,True)!=0 and len(bat.writes)==writes
    check('invalid battery flags, hold and display ranges cannot reach flash')
    # Golden vectors come from the ARM execution, not a second Python renderer.
    vectors=[]
    for effect in range(22):
        p=scene('Golden',effect,[[20,60,180],[230,170,40]])
        p[66]=1;p[67]=217;p[71:74]=bytes([143,222,9]);struct.pack_into('<hh',p,78,-181,181)
        struct.pack_into('<H',p,510,crc(p[:510]));assert m.upload(5,p)==0;assert m.packet(5,b'\x05')[0]==0
        cfg=m.config();cfg[6]=203;cfg[7]=142;cfg[10]=0;struct.pack_into('<H',cfg,510,crc(cfg[:510]));assert m.upload(0,cfg,True)==0
        for frame in [1,10,49,101]:
            m.put(0x20007004,struct.pack('<I',frame-1));m.put(0x20000083,[34]);m.put(0x20000088,[1]);m.call('aj_observe');m.call('aj_lighting')
            planes=m.read(0x2000478c,378);logical=[0]*378
            for id in physical:
                for j in range(3):logical[j*126+id]=planes[j*126+native_map[id]]
            vectors.append(dict(page=list(p),frame=frame,pressed_id=34,pressed_at=frame-1,brightness=203,speed=142,rgb=logical,color_transfer=1))
    fixtures=ROOT/'tests/fixtures';fixtures.mkdir(parents=True,exist_ok=True)
    (fixtures/'keyboard-runtime.json').write_text(json.dumps(list(cap))+'\n')
    (fixtures/'keyboard-ajks-preview.json').write_text(json.dumps(vectors,separators=(',',':'))+'\n')
    report={'candidate_sha256':hashlib.sha256(IMAGE.read_bytes()).hexdigest(),'tests':tests,'test_count':len(tests),
        'passed':True,'emulator':'Unicorn ARM M-class; native MMIO/flash writer explicitly stubbed','hardware_access':False,
        'physical_LED_timing_and_recovery_verified':False}
    (OUT/'emulation.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
if __name__=='__main__':main()
