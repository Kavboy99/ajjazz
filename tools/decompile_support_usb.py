"""Offline decompilation of the exact support package USB component."""
import hashlib
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
image = ROOT / 'research/vendor/ajazz-support-20261023/dock/decoded/x1-verified.bin'
if hashlib.sha256(image.read_bytes()).hexdigest() != 'a4398f2e8cfe2af3c59c6073836298c8cd350230c5850fed90d73164b83507d8':
    raise ValueError('Support USB image mismatch')
project = ROOT / 'research/local/ajazz-support-20261023/ghidra-usb-xw'
project.mkdir(parents=True, exist_ok=True)
jdk = ROOT / '.tools/jdk-21.0.12.1+1'
env = dict(os.environ, JAVA_HOME=str(jdk), PATH=str(jdk / 'bin') + os.pathsep + os.environ.get('PATH', ''))
subprocess.run([str(ROOT / '.tools/ghidra_12.1.4_PUBLIC/support/analyzeHeadless.bat'),
    str(project), 'support-usb',
    *(['-process', image.name] if '--refresh' in sys.argv else
      ['-import', str(image), '-loader', 'BinaryLoader', '-processor', 'RISCV:LE:32:QingKeXW', '-cspec', 'gcc']),
    '-scriptPath', str(ROOT / 'tools/ghidra'), '-preScript', 'SeedSupportRv.java',
    '-postScript', 'ExportPanReference.java', str(project), '-analysisTimeoutPerFile', '120',
    '-max-cpu', '2', '-log', str(project / 'analysis.log')], env=env, cwd=ROOT, check=True)
