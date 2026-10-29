"""Create a local, reviewable research package. Does not launch any updater."""
from pathlib import Path
import argparse
import json
import shutil
import zipfile
from keyboard_updater_lab import sha, require, write_evidence, save_json

ROOT=Path(__file__).resolve().parents[1]


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination',type=Path)
    args=parser.parse_args()
    destination=args.destination.resolve()
    require(not destination.exists(),'Use a new package directory to preserve previous evidence')
    destination.mkdir(parents=True)
    files=[
        'docs/KEYBOARD-UPDATER-RE.md',
        'tools/keyboard_updater_lab.py','tools/sonix_isp.py',
        'tools/audit_keyboard_updater.py','tools/emulate_keyboard_updater.py',
        'tools/decompile_keyboard_updater.py','tools/package_keyboard_updater_research.py',
        'tools/ghidra/ExportKeyboardUpdater.java','tools/ghidra/ExportKeyboardUpdaterDetails.java',
        'tests/test_keyboard_updater_lab.py',
        'research/vendor/ak820max-support-20261024/support-updater.exe',
        'research/vendor/ak820max-support-20261024/user-1.36.bin',
        'research/vendor/revision-hunt/ak820-recovery-b.exe',
    ]
    files += [str(p.relative_to(ROOT)) for p in (ROOT/'research/keyboard-updater').glob('*.json')]
    evidence=ROOT/'research/local/keyboard-updater-re/ghidra'
    files += [str(p.relative_to(ROOT)) for p in evidence.iterdir()
              if p.suffix in ('.c','.tsv','.txt')]
    for relative in files:
        source=ROOT/relative
        write_evidence(destination/relative,source.read_bytes())
    # Only the fields needed by offline compatibility tests are retained; do
    # not copy the HID device path, serial, user's macros or calibration data.
    snapshot=json.loads((ROOT/'research/local/ak820max-support-20261024/keyboard-before.json').read_text(encoding='utf-8-sig'))
    clean=dict(info={k:v for k,v in snapshot['info'].items() if k in
        ('vid','pid','manufacturer','product','version')},
        endpoint={k:v for k,v in snapshot['endpoint'].items() if k in
        ('vid','pid','usage_page','usage','interface')})
    save_json(destination/'research/local/ak820max-support-20261024/keyboard-before.json',clean)
    readme='''# Исследование прошивальщика AK820MAX — 26.10.2026

Основной подробный отчёт: [KEYBOARD-UPDATER-RE.md](docs/KEYBOARD-UPDATER-RE.md).
Таблицы, хеши и протокол: `research/keyboard-updater/`.
Декомпиляция и листинги: `research/local/keyboard-updater-re/ghidra/`.

Это офлайн-лаборатория. Она разбирает пакеты, сравнивает версии и проверяет
совместимость по сохранённым данным. Собственный USB transport и замена OEM
в интерфейсе Studio ещё не подключены. Новые физические ревизии не испытаны.

Для основной утилиты нужен только Python. Запускать вложенные EXE для анализа
не требуется: они сохранены как исходные доказательства.

```powershell
python tools/keyboard_updater_lab.py inspect research/vendor/ak820max-support-20261024/support-updater.exe --out inspection.json
python tools/keyboard_updater_lab.py extract research/vendor/ak820max-support-20261024/support-updater.exe --out extracted
python tools/keyboard_updater_lab.py compare research/vendor/ak820max-support-20261024/support-updater.exe research/vendor/revision-hunt/ak820-recovery-b.exe --out comparison.json
python tools/keyboard_updater_lab.py assess research/vendor/ak820max-support-20261024/support-updater.exe research/local/ak820max-support-20261024/keyboard-before.json --out assessment.json
python -m unittest discover -s tests -p test_keyboard_updater_lab.py -v
```

Ghidra/JDK/Unicorn не включены в переносимый архив. Готовые результаты их работы
сохранены; расширенные скрипты предназначены для исходного проекта с `.tools`.
SHA256 всех файлов перечислены в `package-manifest.json`.
'''
    (destination/'README.md').write_text(readme,encoding='utf-8')
    index={str(p.relative_to(destination)).replace('\\','/'):sha(p.read_bytes())
        for p in sorted(destination.rglob('*')) if p.is_file()}
    save_json(destination/'package-manifest.json',dict(schema=1,files=index,
        hardware_access=False,vendor_process_started=False))
    archive=destination.with_suffix('.zip')
    require(not archive.exists(),'Archive already exists')
    with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as z:
        for p in sorted(destination.rglob('*')):
            if p.is_file():z.write(p,str(p.relative_to(destination)).replace('\\','/'))
    print(json.dumps(dict(directory=str(destination),archive=str(archive),files=len(index),
        archive_sha256=sha(archive.read_bytes())),ensure_ascii=False,indent=2))


if __name__=='__main__':main()
