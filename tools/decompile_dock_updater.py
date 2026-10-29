"""Statically analyse pinned updater PE without running vendor executable."""
import hashlib
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--export-existing', action='store_true')
    parser.add_argument('--retry-large-functions', action='store_true',
                        help='Retry only the two large async protocol handlers; preserve previous exports')
    parser.add_argument('--usb-bridge-candidates', action='store_true',
                        help='Analyse audited prologues around unresolved dangle4k.rs references')
    args = parser.parse_args()
    source = ROOT / 'research/vendor/panchip-reference/aj179-reference/dock-ry_upgrade.exe'
    if hashlib.sha256(source.read_bytes()).hexdigest() != '9fa30ca90a927ad5827bd9a4ee0b15dc6ffe40b5fb56fbb6b29bcc81ae31345c':
        raise ValueError('Pinned updater mismatch')
    jdk = ROOT / '.tools/jdk-21.0.12.1+1'
    project = ROOT / 'research/local/mouse-firmware-20261021/ghidra-updater'
    project.mkdir(exist_ok=True)
    env = dict(os.environ, JAVA_HOME=str(jdk), PATH=str(jdk / 'bin') + os.pathsep + os.environ.get('PATH', ''))
    command = [str(ROOT / '.tools/ghidra_12.1.4_PUBLIC/support/analyzeHeadless.bat'),
               str(project), 'dock-updater',
               *(['-process', source.name, '-noanalysis'] if args.export_existing or args.retry_large_functions or args.usb_bridge_candidates else ['-import', str(source)]),
               '-scriptPath', str(ROOT / 'tools/ghidra'),
               '-postScript', 'ExportUpdaterUsbBridge.java' if args.usb_bridge_candidates else 'RetryUpdaterLargeFunctions.java' if args.retry_large_functions else 'ExportUpdaterEvidence.java', str(project),
               '-analysisTimeoutPerFile', '180', '-max-cpu', '2',
               '-log', str(project / 'analysis.log')]
    subprocess.run(command, env=env, check=True, cwd=ROOT)


if __name__ == '__main__':
    main()
