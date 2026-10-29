"""Statically analyse a pinned reference display MCU; never access hardware."""
import hashlib
import argparse
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--refresh-existing', action='store_true')
    parser.add_argument('--family', choices=('keydous', 'epomaker', 'aj159-support'), default='keydous')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    image = root / f'research/vendor/panchip-reference/{args.family}-screen-reference/firmwareOledFile.bin'
    if args.family == 'aj159-support':
        image = root / 'research/vendor/ajazz-support-20261023/dock/decoded/x5-verified.bin'
    expected = {'keydous': 'de636a8fa6eb0dd127b4151c1adb563d681a75c189b5546186a00fc8e402b45b',
                'epomaker': 'a9a2c4287b6f29008c961446f9442c757c34c6fe5a84d6204cdd9739451a7274',
                'aj159-support': 'f767078270a554813fd3b134613bbb743610422fdd754402df55ecb769341fe9'}[args.family]
    if hashlib.sha256(image.read_bytes()).hexdigest() != expected:
        raise ValueError('Display reference hash mismatch')
    project = root / ('research/local/mouse-firmware-20261021/ghidra-display-yc3121' if args.family == 'keydous'
                      else 'research/local/mouse-firmware-20261021/ghidra-display-epomaker')
    if args.family == 'aj159-support':
        project = root / 'research/local/ajazz-support-20261023/ghidra-display'
    project.mkdir(exist_ok=True, parents=True)
    jdk = root / '.tools/jdk-21.0.12.1+1'
    env = dict(os.environ, JAVA_HOME=str(jdk), PATH=str(jdk / 'bin') + os.pathsep + os.environ.get('PATH', ''))
    command = [str(root / '.tools/ghidra_12.1.4_PUBLIC/support/analyzeHeadless.bat'),
               str(project), 'display-reference',
               *(['-process', image.name] if args.refresh_existing else
                 ['-import', str(image), '-loader', 'BinaryLoader', '-processor', 'ARM:LE:32:Cortex', '-cspec', 'default']),
               '-scriptPath', str(root / 'tools/ghidra'), '-preScript', 'SeedDisplayReference.java',
               'epomaker' if args.family == 'aj159-support' else args.family,
               '-postScript', 'ExportPanReference.java', str(project),
               '-analysisTimeoutPerFile', '120', '-max-cpu', '2', '-log', str(project / 'analysis.log')]
    subprocess.run(command, env=env, check=True, cwd=root)


if __name__ == '__main__':
    main()
