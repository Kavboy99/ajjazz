"""Build an AJSC telemetry extension and a pinned-image candidate; no USB access."""
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]
BASE, INJECT, END = 0x01000200, 0x0105A000, 0x01060000
SHA = 'f767078270a554813fd3b134613bbb743610422fdd754402df55ecb769341fe9'
SOURCE = ROOT / 'research/vendor/ajazz-support-20261023/dock/decoded/x5-verified.bin'
DIRECTORY = ROOT / 'firmware/aj159-screen'
OUT = DIRECTORY / 'build'
BIN = ROOT / '.tools/llvm-mingw-20261006-ucrt-x86_64/bin'


def thumb_bl(site, target):
    delta = target - (site + 4)
    if delta & 1 or not -(1 << 22) <= delta < (1 << 22):
        raise ValueError('Thumb-1 BL target out of range')
    return struct.pack('<HH', 0xF000 | ((delta >> 12) & 0x7FF),
                       0xF800 | ((delta >> 1) & 0x7FF))


def build():
    data = SOURCE.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA:
        raise ValueError('Exact AJAZZ support display image required')
    OUT.mkdir(exist_ok=True, parents=True)
    font = json.loads((DIRECTORY / 'font.json').read_text())
    declarations = []
    for name, count in (('digits', 10), ('letters', 26)):
        rows = font[name]
        if len(rows) != count or any(len(row) != 7 or any(not 0 <= n < 32 for n in row) for row in rows):
            raise ValueError('Invalid 5x7 font')
        declarations.append(f'static const uint8_t {name}[{count}][7] = {{' +
                            ','.join('{' + ','.join(map(str,row)) + '}' for row in rows) + '};')
    (OUT / 'font.h').write_text('\n'.join(declarations) + '\n')
    modern=json.loads((DIRECTORY/'modern-font.json').read_text())['fonts']
    declarations=['typedef struct { uint16_t offset; uint8_t width,advance; } ModernGlyph;',
                  'typedef struct { unsigned height; const ModernGlyph *glyphs; const uint8_t *data; } ModernFont;']
    for i,font in enumerate(modern):
        for g in font['glyphs']:
            if g['offset']+(g['width']*font['height']+3)//4>len(font['data']):raise ValueError('Glyph outside bitmap')
        declarations.append(f'static const ModernGlyph mg{i}[]={{'+','.join('{%d,%d,%d}'%(g['offset'],g['width'],g['advance']) for g in font['glyphs'])+'};')
        declarations.append(f'static const uint8_t md{i}[]={{'+','.join(map(str,font['data']))+'};')
    declarations.append('static const ModernFont modern_fonts[]={'+','.join('{%d,mg%d,md%d}'%(f['height'],i,i) for i,f in enumerate(modern))+'};')
    (OUT/'modern-font.h').write_text('\n'.join(declarations)+'\n')
    elf = OUT / 'screen.elf'
    command = [str(BIN / 'clang-23.exe'), '--target=arm-none-eabi', '-mcpu=cortex-m0',
               '-mthumb', '-Oz', '-Wall', '-Wextra', '-Werror', '-ffreestanding', '-fno-builtin',
               '-ffunction-sections', '-fdata-sections', '-nostdlib', '-fuse-ld=lld',
               f'-Wl,-T,{DIRECTORY / "link.ld"}', f'-Wl,-Map,{OUT / "screen.map"}',
               '-I', str(OUT), '-o', str(elf), str(DIRECTORY / 'screen.c')]
    subprocess.run(command, check=True, cwd=ROOT)
    listing = subprocess.check_output([str(BIN / 'llvm-nm.exe'), '--defined-only', str(elf)], text=True)
    symbols = {m.group(2): int(m.group(1), 16) for line in listing.splitlines()
               if (m := re.match(r'^([0-9a-fA-F]+)\s+[A-Za-z]\s+(\S+)$', line))}
    extension_path = OUT / 'extension.bin'
    subprocess.run([str(BIN / 'llvm-objcopy.exe'), '-O', 'binary', str(elf), str(extension_path)], check=True)
    extension = extension_path.read_bytes()
    if INJECT + len(extension) > END:
        raise ValueError('Extension does not fit the existing application range')
    image = bytearray(data)
    image.extend(b'\xff' * (INJECT - BASE - len(image)))
    image.extend(extension)
    # Absolute addresses and original instructions are checked before replacement.
    patches = []

    def patch(at, expected, value, label):
        expected = bytes.fromhex(expected)
        if data[at-BASE:at-BASE+len(expected)] != expected or len(expected) != len(value):
            raise ValueError(f'Original instructions changed at {at:08x}')
        image[at-BASE:at-BASE+len(value)] = value
        patches.append({'address': hex(at), 'before': expected.hex(), 'after': value.hex(), 'purpose': label})

    usb_expected = data[0x01013D6C-BASE:0x01013D6C-BASE+8].hex()
    # SHA above pins this full image; explicit bytes here additionally document the displaced copy loop.
    if usb_expected != 'd64a00200b5c1354':
        raise ValueError(f'Unexpected nested USB entry: {usb_expected}')
    patch(0x01013D6C, usb_expected, bytes.fromhex('004b1847') +
          struct.pack('<I', symbols['screen_usb_hook'] | 1), 'Nested 0x41 USB envelope; stock fallback retained')
    patch(0x010104EA, '05f09ffd', thumb_bl(0x010104EA, symbols['screen_main_hook']), 'Cooperative main-loop telemetry renderer')
    patch(0x010140E2, 'fff7e7fe', thumb_bl(0x010140E2, symbols['screen_button_hook']), 'Rear button opcode 0x42; original ACK/native fallback retained')
    patch(0x01016380, 'fff73cff', thumb_bl(0x01016380, symbols['screen_config_write']), 'Preserve custom settings in existing configuration page')
    patch(0x01016438, 'fff7e0fe', thumb_bl(0x01016438, symbols['screen_config_write']), 'Preserve custom settings in factory-default configuration page')
    patch(0x010163B2, 'fff740ff', thumb_bl(0x010163B2, symbols['screen_config_read']), 'Load custom settings from existing configuration page')
    if image[:0x10000] != data[:0x10000]:
        raise ValueError('Boot component was modified')
    # All unlisted bytes of the manufacturer payload must remain identical.
    allowed = {n for p in patches for n in range(int(p['address'], 16)-BASE,
               int(p['address'], 16)-BASE+len(bytes.fromhex(p['after'])))}
    if any(a != b and i not in allowed for i, (a, b) in enumerate(zip(data, image))):
        raise ValueError('Unexpected change outside the patch sites')
    candidate = OUT / 'aj159-screen-telemetry-v1-candidate.bin'
    candidate.write_bytes(image)
    report = {'manufacturer_image_sha256': SHA, 'manufacturer_image': str(SOURCE),
              'extension_sources_sha256': {name: hashlib.sha256((DIRECTORY / name).read_bytes()).hexdigest()
                                          for name in ('screen.c', 'screens_extra.h', 'screens_game.h', 'screens_metrics.h', 'screens_composition.h', 'modern.h', 'modern-font.json', 'link.ld', 'font.json')},
              'compiler': subprocess.check_output([str(BIN / 'clang-23.exe'), '--version'], text=True).splitlines()[0],
              'candidate_sha256': hashlib.sha256(image).hexdigest(), 'candidate_bytes': len(image),
              'candidate': str(candidate), 'injection_address': hex(INJECT),
              'extension_bytes': len(extension), 'symbols': {k: hex(v) for k,v in symbols.items()},
              'patches': patches, 'boot_component_unchanged': True,
              'reserved_ram': {'start': '0x2c000', 'bytes': 512},
              'configuration_storage': {'original_external_flash_page': '0x100000',
                                        'offset': 128, 'bytes': 106},
              'hardware_access': False, 'hardware_installation_verified': False,
              'status': 'candidate; emulator validation and physical recovery validation required'}
    (OUT / 'manifest.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f'Built {len(extension)}-byte extension; candidate {len(image)} bytes; no USB access')
    print(candidate)
    return report


if __name__ == '__main__':
    build()
