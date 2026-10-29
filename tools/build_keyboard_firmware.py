"""Pinned SN34F280 extension build. Offline only; never invokes an updater."""
from pathlib import Path
import hashlib,json,re,struct,subprocess,sys

ROOT=Path(__file__).resolve().parents[1]
DIR=ROOT/'firmware/ak820max'
OUT=DIR/'build'
BIN=ROOT/'.tools/llvm-mingw-20261006-ucrt-x86_64/bin'
SOURCE=ROOT/'research/vendor/ak820max-support-20261024/user-1.36.bin'
SHA='58db8a73ea8be7b8f730d67177f3ad78ec1b28b2b73ac15861e67963a1b116ff'

def initialized_data(stock):
    # Decode the pinned ARM scatter-loader at 0x808. Its source is compressed;
    # offsets in the ROM stream are not offsets in the initialized RAM image.
    source=0x16bd0;end=0x171c8;size=0x10c0;data=bytearray()
    def take():
        nonlocal source
        if source>=end:raise ValueError('Truncated initialized data')
        value=stock[source];source+=1;return value
    while len(data)<size:
        flags=take();literal=flags&7;run=flags>>4
        if literal==0:literal=take()
        if run==0:run=take()
        for _ in range(literal-1):data.append(take())
        if flags&8:
            distance=take()
            if distance==0 or distance>len(data):raise ValueError('Invalid initialized data back-reference')
            for _ in range(run+2):data.append(data[-distance])
        else:data.extend(bytes(run))
        if len(data)>size:raise ValueError('Initialized data overflow')
    return bytes(data)

def branch(site,target,link=True):
    delta=target-site-4
    if delta&1 or not -(1<<24)<=delta<(1<<24):raise ValueError('Thumb-2 branch out of range')
    d=delta&0x1ffffff;s=d>>24;i1=(d>>23)&1;i2=(d>>22)&1
    return struct.pack('<HH',0xf000|(s<<10)|((d>>12)&1023),
                       (0xd000 if link else 0x9000)|((~(i1^s)&1)<<13)|((~(i2^s)&1)<<11)|((d>>1)&2047))

def crc(p):
    n=65535
    for v in p:
        n^=v<<8
        for _ in range(8):n=((n<<1)^(0x1021 if n&0x8000 else 0))&65535
    return n

def scene(name,effect,colors):
    p=bytearray(512);p[:4]=b'AKLP';p[4:8]=bytes([1,1,0,200]);p[12:44]=name.encode()[:32].ljust(32,b'\0')
    p[64:68]=bytes([1,effect,0,255]);p[68:74]=bytes(sum((list(c) for c in colors),[]))
    struct.pack_into('<HHhhH',p,74,64,512,256,0,0)
    for row in json.loads((ROOT/'assets/ak820max-layout.json').read_text(encoding='utf-8')):
        for k in row:p[84+k['id']//8]|=1<<(k['id']%8)
    struct.pack_into('<H',p,510,crc(p[:510]));return p

def positions(stock):
    rows=json.loads((ROOT/'assets/ak820max-layout.json').read_text(encoding='utf-8'))
    if len(rows[0])!=14 or rows[0][0]['label']!='Esc' or rows[0][-1]['label']!='Delete':raise ValueError('Top indicator row changed')
    points=[]
    for y,row in enumerate(rows):
        x=0
        for k in row:
            points.append((k['id'],x+k['width']/2,y));x+=k['width']
    lo=min(p[1] for p in points);hi=max(p[1] for p in points)
    data=[[0,0,0] for _ in range(126)]
    for i,x,y in points:
        if i>=126:raise ValueError('Physical ID outside native LED planes')
        data[i]=[round((x-lo)/(hi-lo)*255),round(y/(len(rows)-1)*255),1]
    mapping=list(stock[0x16338:0x16338+126])
    logical_ids=[p[0] for p in points];compact=[mapping[i] for i in logical_ids]
    native_table=stock[0x15b8a:0x15b8a+stock[0x15bdc]]
    if len(set(compact))!=82 or set(compact)!=set(native_table):raise ValueError('Hall-to-LED map does not cover all82 physical LEDs')
    # sRGB encoded editor colors -> linear light -> OEM's existing6-bit PWM.
    # Choose an input whose ORIGINAL cf90 integer conversion produces the nearest
    # target PWM, including dim mixed channels; keep maximum white at63 as stock.
    lut=[]
    for code in range(256):
        encoded=code/255
        linear=encoded/12.92 if encoded<=0.04045 else ((encoded+0.055)/1.055)**2.4
        pwm=int(linear*63+0.5)
        value=0 if pwm==0 else (pwm*262144+65024)//65025
        if value>255 or (value*65025>>18)!=pwm:raise ValueError('Invalid PWM transfer')
        lut.append(value)
    (OUT/'positions.h').write_text('static const unsigned char pos[126][3] = {'+','.join('{'+','.join(map(str,p))+'}' for p in data)+'};\n'+
       'static const unsigned char key_to_led[126] = {'+','.join(map(str,mapping))+'};\n'+
       'static const unsigned char top_row[14] = {'+','.join(str(k['id']) for k in rows[0])+'};\n'+
       'static const unsigned char battery_order[82] = {'+','.join(str(p[0]) for p in sorted(points,key=lambda p:(p[1],p[2],p[0])))+'};\n'+
       'static const unsigned char led_input[256] = {'+','.join(map(str,lut))+'};\n')

def build():
    stock=SOURCE.read_bytes()
    if hashlib.sha256(stock).hexdigest()!=SHA:raise ValueError('Wrong support image')
    if any(stock[0x18000:0x26400]):raise ValueError('Extension reservation not empty')
    # ARM scatter-load table proves .data+.bss ends exactly at initial MSP.
    a,b=struct.unpack_from('<II',stock,0x7e0)
    records=[struct.unpack_from('<IIII',stock,o) for o in range(a,b,16)]
    if records!=[(0x16bd0,0x20000000,0x10c0,0x808),(0x171c8,0x200010c0,0x5cd8,0x1234e)]:raise ValueError('Scatter load changed')
    if struct.unpack_from('<I',stock)[0]!=0x20006d98:raise ValueError('Native stack changed')
    if any(struct.unpack_from('<I',stock,a)[0]!=0x20000234 for a in [0x4704,0x47e4]):raise ValueError('OEM HAL clock changed')
    if struct.unpack_from('<I',initialized_data(stock),0x234)[0]!=1:raise ValueError('OEM default tick frequency is not1ms')
    OUT.mkdir(parents=True,exist_ok=True);positions(stock)
    build_id=hashlib.sha256(b''.join((DIR/f).read_bytes() for f in ['keyboard.c','link.ld'])+(OUT/'positions.h').read_bytes()).digest()[:8]
    (OUT/'build_id.h').write_text('static const unsigned char aj_build_id[8] = {'+','.join(map(str,build_id))+'};\n')
    elf=OUT/'keyboard.elf'
    subprocess.run([str(BIN/'clang-23.exe'),'--target=arm-none-eabi','-mcpu=cortex-m4','-mthumb','-mfloat-abi=soft','-Oz',
        '-Wall','-Wextra','-Werror','-ffreestanding','-fno-builtin','-ffunction-sections','-fdata-sections','-nostdlib','-fuse-ld=lld',
        '-Wl,-T,'+str(DIR/'link.ld'),'-Wl,-Map,'+str(OUT/'keyboard.map'),'-I',str(OUT),'-o',str(elf),str(DIR/'keyboard.c')],check=True)
    listing=subprocess.check_output([str(BIN/'llvm-nm.exe'),'--defined-only',str(elf)],text=True)
    symbols={m[2]:int(m[1],16) for line in listing.splitlines() if (m:=re.match(r'^([0-9a-fA-F]+)\s+[A-Za-z]\s+(\S+)$',line))}
    subprocess.run([str(BIN/'llvm-objcopy.exe'),'-O','binary',str(elf),str(OUT/'extension.bin')],check=True)
    ext=(OUT/'extension.bin').read_bytes()
    if len(ext)>0x4000:raise ValueError('Extension too large')
    image=bytearray(stock);image[0x20000:0x20000+len(ext)]=ext;patches=[]
    for at,target,old_target,link in [(0x1051c,'aj_lighting',0x7910,True),(0x10530,'aj_usb',0xea90,True),
       (0x57f4,'aj_rotation',0x12178,True),(0x10118,'aj_press',0x13d94,False),(0x1013c,'aj_press',0x13d94,False),
       (0x13d94,'aj_key_hook',None,False)]:
        expected=bytes.fromhex('2de9f041') if old_target is None else branch(at,old_target,link)
        if stock[at:at+4]!=expected:raise ValueError(f'Instruction mismatch at {at:x}: {stock[at:at+4].hex()} != {expected.hex()}')
        value=branch(at,symbols[target],link);image[at:at+4]=value
        patches.append(dict(address=hex(at),before=expected.hex(),after=value.hex(),symbol=target))
    presets=[('Aurora',7,[[20,65,180],[35,240,180]]),('Plasma',8,[[255,25,100],[60,50,255]]),
             ('Comet',9,[[0,10,30],[80,220,255]]),('Reactive',6,[[0,0,0],[240,165,45]])]
    for i,(name,effect,colors) in enumerate(presets):image[0x24000+i*1024:0x24200+i*1024]=scene(name,effect,colors)
    seed_path=None
    if '--seed-state' in sys.argv:
        seed_path=Path(sys.argv[sys.argv.index('--seed-state')+1]);seed=json.loads(seed_path.read_text(encoding='utf-8-sig'))
        if seed['build_id'] not in ['e1e07534974b8693','08b233ded749b0c5','9ad77b0e5c6e44af','f64cfdf56114efc0'] or len(seed['scenes'])!=8:raise ValueError('Wrong installed source for seeded bank migration')
        image[0x24000:0x26400]=bytes(0x2400)
        for slot,p in enumerate(seed['scenes']):
            if p is not None:
                p=bytes(p)
                if len(p)!=512 or p[:5]!=b'AKLP\x01' or crc(p[:510])!=struct.unpack_from('<H',p,510)[0]:raise ValueError('Corrupt saved program')
                image[0x24000+slot*1024:0x24200+slot*1024]=p
        cfg=seed['config'];page=bytearray(512);page[:5]=b'AKCF\x01';page[5]=255 if cfg['selected'] is None else cfg['selected'];page[6]=cfg['brightness'];page[7]=cfg['speed'];page[10]=cfg['paused']
        kinds={'Media':1,'Brightness':2,'Mode':3,'Speed':5}
        for i,action in enumerate(cfg['encoder']):
            if isinstance(action,str):kind,param={'Native':(0,0),'Toggle':(4,0),'Pause':(6,0)}[action]
            else:
                key,param=next(iter(action.items()));kind=kinds[key]
            struct.pack_into('<BBH',page,16+4*i,kind,0,param&65535)
        battery=cfg.get('battery',{'enabled':False,'whole_keyboard':False,'background':False,'hold_seconds':2,'display_seconds':3,'filled':[34,211,134],'empty':[8,12,20],'brightness':255})
        page[28:34]=bytes([1,bool(battery['enabled']) or '--enable-battery' in sys.argv,battery['whole_keyboard'],battery['background'],battery['hold_seconds'],battery['display_seconds']])
        page[34:40]=bytes(battery['filled']+battery['empty']);page[40]=battery['brightness']
        struct.pack_into('<H',page,510,crc(page[:510]));image[0x26000:0x26200]=page
    allowed=set(range(0x20000,0x26400))|{j for p in patches for j in range(int(p['address'],16),int(p['address'],16)+4)}
    if any(a!=b and i not in allowed for i,(a,b) in enumerate(zip(stock,image))):raise ValueError('Unexpected stock change')
    candidate=OUT/'ak820max-ajks-v1-candidate.bin';candidate.write_bytes(image)
    report=dict(stock_sha256=SHA,candidate_sha256=hashlib.sha256(image).hexdigest(),candidate_bytes=len(image),extension_bytes=len(ext),build_id=build_id.hex(),
        symbols={k:hex(v) for k,v in symbols.items()},patches=patches,
        ram={'base':'0x20007000','bytes':4096,'stock_scatter_end':'0x20006d98'},
        flash={'extension':['0x20000','0x24000'],'scene_banks':['0x24000','0x26000'],'config_banks':['0x26000','0x26400']},
        stock_settings_and_calibration_unchanged=True,hardware_access=False,installed=False,
        source_sha256={f:hashlib.sha256((DIR/f).read_bytes()).hexdigest() for f in ['keyboard.c','link.ld']},
        word_checksum=f'{sum(struct.unpack("<258048H",image))&65535:04X}')
    report['seed_state']=str(seed_path) if seed_path else None
    report['all_82_leds_mapped']=True
    report['clock']={'source':'OEM HAL SysTick milliseconds at20000238','logical_frame_ms':20,'main_loop_frequency_independent':True}
    report['color_transfer']={'encoding':'sRGB to linear light, rounded6-bit stock PWM','max_pwm':63,'wire_order':'original OEM serializer, unchanged'}
    report['features']={'effects':22,'battery_source':'OEM live percent at2000012e, native cf90 and GET_INFO radio','battery_config_version':1,'battery_enabled_during_migration':'--enable-battery' in sys.argv,'hold_ms_range':[1000,5000],'release_ms_range':[1000,10000],'overlay_after_original_pwm':True}
    (OUT/'manifest.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({k:report[k] for k in ['candidate_sha256','extension_bytes','word_checksum','installed']}))
    return report
if __name__=='__main__':build()
