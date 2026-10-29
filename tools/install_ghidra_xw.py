"""Install an isolated QingKe XW language beside bundled Ghidra languages.

Does not replace Ghidra's standard RISC-V language. Encodings are independently
documented by ArcaneNibble and LLVM's Xwchc extension implementation.
"""
from pathlib import Path
import os
import subprocess

ROOT = Path(__file__).resolve().parents[1]
LANG = ROOT / '.tools/ghidra_12.1.4_PUBLIC/Ghidra/Processors/RISCV/data/languages'
SPEC = '''define endian=little;
@define XLEN 4
@define XLEN2 8
@define FLEN 4
@define CONTEXTLEN 4
@define ADDRSIZE "32"
@define FPSIZE "32"
@include "riscv.reg.sinc"
@include "riscv.table.sinc"
@include "riscv.instr.sinc"
@include "riscv.custom.sinc"
@include "ajazz-xw.sinc"
'''
XW = '''# WCH QingKe XW byte/halfword compression. No floating point D opcodes.
xwbyte: imm is cop1212 & cop1011 & cop0506 [ imm = cop1212 | (cop1011 << 3) | (cop0506 << 1); ] { export *[const]:4 imm; }
xwhalf: imm is cop1012 & cop0506 [ imm = (cop1012 << 3) | (cop0506 << 1); ] { export *[const]:4 imm; }
xwstackbyte: cop0710 is cop0710 { export *[const]:4 cop0710; }
xwstackhalf: imm is cop0710 [ imm = (cop0710 & 14) | ((cop0710 & 1) << 4); ] { export *[const]:4 imm; }
:qk.c.lbu cr0204s,xwbyte(cr0709s) is cr0204s & cr0709s & cop0001=0 & cop1315=1 & xwbyte {
  local ea:4 = cr0709s + xwbyte; cr0204s = zext(*[ram]:1 ea);
}
:qk.c.sb cr0204s,xwbyte(cr0709s) is cr0204s & cr0709s & cop0001=0 & cop1315=5 & xwbyte {
  local ea:4 = cr0709s + xwbyte; *[ram]:1 ea = cr0204s:1;
}
:qk.c.lhu cr0204s,xwhalf(cr0709s) is cr0204s & cr0709s & cop0001=2 & cop1315=1 & xwhalf {
  local ea:4 = cr0709s + xwhalf; cr0204s = zext(*[ram]:2 ea);
}
:qk.c.sh cr0204s,xwhalf(cr0709s) is cr0204s & cr0709s & cop0001=2 & cop1315=5 & xwhalf {
  local ea:4 = cr0709s + xwhalf; *[ram]:2 ea = cr0204s:2;
}
:qk.c.lbusp cr0204s,xwstackbyte(sp) is cr0204s & sp & cop1315=4 & cop1012=0 & cop0506=0 & cop0001=0 & xwstackbyte {
  local ea:4 = sp + xwstackbyte; cr0204s = zext(*[ram]:1 ea);
}
:qk.c.lhusp cr0204s,xwstackhalf(sp) is cr0204s & sp & cop1315=4 & cop1012=0 & cop0506=1 & cop0001=0 & xwstackhalf {
  local ea:4 = sp + xwstackhalf; cr0204s = zext(*[ram]:2 ea);
}
:qk.c.sbsp cr0204s,xwstackbyte(sp) is cr0204s & sp & cop1315=4 & cop1012=0 & cop0506=2 & cop0001=0 & xwstackbyte {
  local ea:4 = sp + xwstackbyte; *[ram]:1 ea = cr0204s:1;
}
:qk.c.shsp cr0204s,xwstackhalf(sp) is cr0204s & sp & cop1315=4 & cop1012=0 & cop0506=3 & cop0001=0 & xwstackhalf {
  local ea:4 = sp + xwstackhalf; *[ram]:2 ea = cr0204s:2;
}
'''
LDEFS = '''<?xml version="1.0" encoding="UTF-8"?>
<language_definitions>
  <language processor="RISCV" endian="little" size="32" variant="QingKeXW"
      version="1.0" slafile="ajazz-xw.sla" processorspec="RV32.pspec" id="RISCV:LE:32:QingKeXW">
    <description>Offline AJAZZ support USB: QingKe XW</description>
    <compiler name="gcc" spec="riscv32.cspec" id="gcc"/>
  </language>
</language_definitions>
'''
for name, text in [('ajazz-xw.slaspec', SPEC), ('ajazz-xw.sinc', XW), ('ajazz-xw.ldefs', LDEFS)]:
    (LANG / name).write_text(text, encoding='utf-8')
jdk = ROOT / '.tools/jdk-21.0.12.1+1'
env = dict(os.environ, JAVA_HOME=str(jdk), PATH=str(jdk / 'bin') + os.pathsep + os.environ.get('PATH', ''))
subprocess.run([str(ROOT / '.tools/ghidra_12.1.4_PUBLIC/support/sleigh.bat'),
                str(LANG / 'ajazz-xw.slaspec')], env=env, check=True)
