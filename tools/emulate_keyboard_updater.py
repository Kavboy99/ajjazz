"""Execute original x86 packet constructors with USB replaced by memory hooks."""
from pathlib import Path
import json
import struct
import sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'.tools/py-research'))
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import *
from keyboard_updater_lab import PE, require, sha, save_json
import sonix_isp as isp

SOURCE=ROOT/'research/vendor/ak820max-support-20261024/support-updater.exe'
PIN='edd4914bdd93bc3d557173e554efd40e7b531f5d6275b5bc8c8c6f7f4aa570d4'


def execute(data, entry, eax=0, ecx=0, args=(), reply_mode='ok'):
    pe=PE(data);u=Uc(UC_ARCH_X86,UC_MODE_32)
    u.mem_map(0x400000,0x400000)
    for s in pe.sections:
        u.mem_write(pe.image_base+s['rva'],data[s['offset']:s['offset']+s['raw_size']])
    u.mem_map(0x1000000,0x200000);u.mem_map(0x2000000,0x10000);u.mem_map(0x3000000,0x1000)
    context=0x2000000;stack=0x1100000;sentinel=0x3000000
    u.mem_write(stack,struct.pack('<'+'I'*(len(args)+1),sentinel,*args))
    u.reg_write(UC_X86_REG_ESP,stack);u.reg_write(UC_X86_REG_ESI,context)
    u.reg_write(UC_X86_REG_EDI,context+0x200)
    u.reg_write(UC_X86_REG_EAX,eax);u.reg_write(UC_X86_REG_ECX,ecx)
    u.mem_write(context+0x300,bytes(16))
    captured=[]

    def ret(value,cleanup=0):
        sp=u.reg_read(UC_X86_REG_ESP)
        target=int.from_bytes(u.mem_read(sp,4),'little')
        u.reg_write(UC_X86_REG_EAX,value);u.reg_write(UC_X86_REG_ESP,sp+4+cleanup)
        u.reg_write(UC_X86_REG_EIP,target)

    def hook(_,address,size,user):
        if address==sentinel:
            u.emu_stop();return
        if address in (0x40b520,0x52892f):
            ret(0);return
        if address in (0x428680,0x428810):
            sp=u.reg_read(UC_X86_REG_ESP)
            vid,pid,pointer,count=struct.unpack('<4I',u.mem_read(sp+4,16))
            require(count in (8,64),'Unexpected OEM transfer size')
            if address==0x428680:
                captured.append(bytes(u.mem_read(pointer,count)))
                ret(0 if reply_mode=='send_failed' else 1)
            else:
                response=bytearray(64);response[:4]=captured[-1][:4]
                response[4:8]=isp.ACK
                if captured[-1][0]==1:
                    response[8:12]=bytes.fromhex(isp.SN34F280['signature'])
                if reply_mode=='bad_header':response[0]^=0xff
                if reply_mode=='bad_ack':response[4]^=1
                u.mem_write(pointer,bytes(response[:count]))
                ret(0 if reply_mode=='get_failed' else 1)
            return
        # The emulated entry points contain no other OS/USB path. An unexpected
        # call leaves the constructor window and is rejected before execution.
        require(0x40b520<=address<=0x40c1bf,'Unexpected execution outside packet constructors')
    u.hook_add(UC_HOOK_CODE,hook)
    u.emu_start(entry,sentinel,count=20000)
    require(u.reg_read(UC_X86_REG_EIP)==sentinel,'Constructor did not terminate')
    return captured,u.reg_read(UC_X86_REG_EAX)&255


def main():
    data=SOURCE.read_bytes();require(sha(data)==PIN,'Wrong OEM updater')
    cases=[('version',0x40b6a0,0,0,(0x2000200,0x2000204),isp.get_version()),
        ('compare',0x40bac0,0,0,(),isp.compare_option()),
        ('option',0x40b9d0,0x2000300,0,(),isp.set_option()),
        ('erase',0x40bb20,504,0,(),isp.erase()),
        ('program',0x40bdf0,8064,0,(),isp.enable_program()),
        ('checksum',0x40bea0,0,0,(),isp.get_checksum()),
        ('return',0x40c140,0,0x2000000,(),isp.return_user())]
    results=[]
    for name,entry,eax,ecx,args,expected in cases:
        sent,status=execute(data,entry,eax,ecx,args)
        require(sent==[expected],f'OEM codec mismatch: {name}: {sent}')
        results.append(dict(name=name,entry=f'{entry:08X}',payload=sent[0].hex(),status_al=status,
            windows_report=isp.windows_report(sent[0]).hex(),matches_reimplementation=True))
    errors=[]
    for mode,expected in [('send_failed',0),('get_failed',1),('bad_header',2),('bad_ack',4),('ok',5)]:
        sent,status=execute(data,0x40bdf0,eax=8064,reply_mode=mode)
        require(status==expected,f'OEM program status mismatch: {mode}: {status}')
        errors.append(dict(injection=mode,status_al=status))
    image=(ROOT/'research/vendor/ak820max-support-20261024/user-1.36.bin').read_bytes()
    report=dict(schema=1,updater_sha256=PIN,passed=True,constructors=results,error_injections=errors,
        transcript=isp.simulated_transcript(image),execution='Original x86 under Unicorn; USB and delays replaced by hooks',
        hardware_access=False,vendor_process_started=False,scope='Packet constructors; not a bootloader/hardware simulation')
    save_json(ROOT/'research/local/keyboard-updater-re/x86-codec-validation.json',report)
    print(json.dumps({k:v for k,v in report.items() if k not in ('constructors',)},indent=2))


if __name__=='__main__':main()
