"""Run official portable Ghidra on an extracted reference file only."""
import os
import argparse
import hashlib
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--component', choices=('mouse', 'dock'), default='mouse')
    parser.add_argument('--apex1755', action='store_true', help='Analyse the newly obtained 1755_v302 image')
    parser.add_argument('--controller', action='store_true',
                        help='Process existing project, seed and export first signed controller component')
    args = parser.parse_args()
    if args.controller and (args.apex1755 or args.component != 'dock'):
        parser.error('Controller RAM relocation is audited only for --component dock reference')
    ghidra = ROOT / '.tools/ghidra_12.1.4_PUBLIC'
    jdk = ROOT / '.tools/jdk-21.0.12.1+1'
    image = ROOT / f'research/vendor/panchip-reference/aj179-reference/{args.component}-MV303-DV233-reference.bin'
    project = ROOT / 'research/local/mouse-firmware-20261021/ghidra'
    name = 'aj179-reference'
    if args.apex1755:
        image = ROOT / 'research/vendor/panchip-reference/aj159-apex-1755-reference/firmwareFile.bin'
        if hashlib.sha256(image.read_bytes()).hexdigest() != '82f35e611ab37861f99bb450c70b8b769c3385053dc4dc22c0393e6828a44289':
            raise ValueError('Pinned1755 image mismatch')
        project = ROOT / 'research/local/mouse-firmware-20261021/ghidra-apex1755'
        name = 'apex1755-v302'
    elif hashlib.sha256(image.read_bytes()).hexdigest() != {
        'mouse': '2c4e52961ded6f3043b6b4437f6093a880364c8f933f49108a55d924b98483ba',
        'dock': '57eeedd50368cce8d412b5ea08ea9027a1a548c1c475c9f1007cef650f70e0db',
    }[args.component]:
        raise ValueError('Pinned reference image mismatch')
    project.mkdir(exist_ok=True)
    env = dict(os.environ, JAVA_HOME=str(jdk), PATH=str(jdk / 'bin') + os.pathsep + os.environ.get('PATH', ''))
    command = [str(ghidra / 'support/analyzeHeadless.bat'), str(project), name,
               *(['-process', image.name] if args.controller else
                 ['-import', str(image), '-loader', 'BinaryLoader', '-processor', 'ARM:LE:32:Cortex', '-cspec', 'default']),
               '-scriptPath', str(ROOT / 'tools/ghidra'),
               '-preScript', 'SeedPanControllerApi.java' if args.controller else 'SeedPanReference.java',
               '-postScript', 'ExportPanController.java' if args.controller else 'ExportPanReference.java', str(project),
               '-analysisTimeoutPerFile', '180', '-max-cpu', '2',
               '-log', str(project / f'{args.component}-analysis.log')]
    # No shell-composed arguments; Windows uses the vendor batch launcher.
    subprocess.run(command, env=env, check=True, cwd=ROOT)


if __name__ == '__main__':
    main()
