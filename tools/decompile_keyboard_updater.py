"""Statically decompile the pinned Sonix keyboard updater. No hardware access."""
from pathlib import Path
import hashlib
import os
import subprocess
import argparse

ROOT = Path(__file__).resolve().parents[1]
SHA = 'edd4914bdd93bc3d557173e554efd40e7b531f5d6275b5bc8c8c6f7f4aa570d4'

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export-existing', action='store_true')
    parser.add_argument('--details', action='store_true')
    args = parser.parse_args()
    source = ROOT / 'research/vendor/ak820max-support-20261024/support-updater.exe'
    if hashlib.sha256(source.read_bytes()).hexdigest() != SHA:
        raise ValueError('Updater evidence hash mismatch')
    project = ROOT / 'research/local/keyboard-updater-re/ghidra'
    project.mkdir(parents=True, exist_ok=True)
    jdk = ROOT / '.tools/jdk-21.0.12.1+1'
    env = dict(os.environ, JAVA_HOME=str(jdk), PATH=str(jdk / 'bin') + os.pathsep + os.environ.get('PATH', ''))
    subprocess.run([str(ROOT / '.tools/ghidra_12.1.4_PUBLIC/support/analyzeHeadless.bat'),
        str(project), 'keyboard-updater',
        *(['-process',source.name,'-noanalysis'] if args.export_existing or args.details else ['-import', str(source)]),
        '-scriptPath',str(ROOT / 'tools/ghidra'),'-postScript',
        'ExportKeyboardUpdaterDetails.java' if args.details else 'ExportKeyboardUpdater.java',str(project),
        '-analysisTimeoutPerFile','160','-max-cpu','2','-log',str(project/'analysis.log')],
        cwd=ROOT, env=env, check=True)

if __name__ == '__main__':
    main()
