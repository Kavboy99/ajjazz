"""Run local Ghidra against the pinned support image; no device access."""
from pathlib import Path
import hashlib
import os
import subprocess

root=Path(__file__).resolve().parents[1]
image=root/'research/vendor/ak820max-support-20261024/user-1.36.bin'
if hashlib.sha256(image.read_bytes()).hexdigest()!='58db8a73ea8be7b8f730d67177f3ad78ec1b28b2b73ac15861e67963a1b116ff':
    raise ValueError('Keyboard support image hash mismatch')
project=root/'research/local/ak820max-support-20261024/ghidra';project.mkdir(parents=True,exist_ok=True)
jdk=root/'.tools/jdk-21.0.12.1+1'
env=dict(os.environ,JAVA_HOME=str(jdk),PATH=str(jdk/'bin')+os.pathsep+os.environ.get('PATH',''))
subprocess.run([str(root/'.tools/ghidra_12.1.4_PUBLIC/support/analyzeHeadless.bat'),
 str(project),'keyboard-136','-import',str(image),'-loader','BinaryLoader','-processor','ARM:LE:32:Cortex','-cspec','default',
 '-scriptPath',str(root/'tools/ghidra'),'-preScript','SeedKeyboard136.java',
 '-postScript','ExportKeyboard136.java',str(project),'-analysisTimeoutPerFile','150','-max-cpu','2',
 '-log',str(project/'analysis.log')],cwd=root,env=env,check=True)
