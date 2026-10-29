"""Execute isolated GET handlers from pinned AJ179 reference code in Unicorn.

No USB/HID/network access. Flash is read/execute only; writes can only target
synthetic RAM. Only audited GET handlers are accepted. Background housekeeping is stubbed,
so this is NOT whole-firmware, radio, sensor or physical-hardware validation.
"""
import hashlib
import json
from pathlib import Path
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_HOOK_CODE, UC_PROT_READ, UC_PROT_WRITE, UC_PROT_EXEC
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_XPSR

ROOT = Path(__file__).resolve().parents[1]
IMAGE = ROOT / 'research/vendor/panchip-reference/aj179-reference/mouse-MV303-DV233-reference.bin'
CHECKSUM = '2c4e52961ded6f3043b6b4437f6093a880364c8f933f49108a55d924b98483ba'
BUFFER = 0x200053EC
STATE = 0x200053E4
OPTIONS = 0x2000502C
STOP = 0xE0000


def run_get(image, command, arguments=b'', *, profile_offset=0, memory_trace=None):
    if command not in (0x80, 0x8F, 0xD3, 0x96, 0xD0, 0xD1):
        raise ValueError('Only audited reference GETs are accepted')
    if len(arguments) > 6 or not 0 <= profile_offset <= 255:
        raise ValueError('Invalid isolated GET arguments')
    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
    cpu.mem_map(0, 0x100000, UC_PROT_READ | UC_PROT_EXEC)
    cpu.mem_write(0, image)
    cpu.mem_map(0x20000000, 0x10000, UC_PROT_READ | UC_PROT_WRITE | UC_PROT_EXEC)
    cpu.mem_write(STATE + 1, b'\x01')
    cpu.mem_write(0x20005764, b'\x06')  # Observed wired connection state in the reference dispatcher.
    request = bytearray(64)
    request[0] = command
    request[1:1 + len(arguments)] = arguments
    request[7] = (255 - sum(request[:7])) & 255
    cpu.mem_write(BUFFER, bytes(request))
    # Distinct synthetic values test field placement; not user settings.
    for offset, value in {0: 0, 2: 1, 0x13: 2, 0x14: 5, 0x15: 0,
                          0x122: 90, 0x123: 95, 0x124: 1, 0x125: 10,
                          0x126: 10, 0x127: 2, 0x128: 1}.items():
        cpu.mem_write(OPTIONS + offset, bytes([value]))
    cpu.mem_write(OPTIONS + 0x16, bytes([profile_offset]))

    def hook(engine, address, size, _):
        if address == STOP:
            engine.emu_stop()
        elif address == 0x32C84:
            # Unrelated per-iteration housekeeping before request dispatch.
            engine.reg_write(UC_ARM_REG_PC, engine.reg_read(UC_ARM_REG_LR))
        elif address == 0x3697C and memory_trace is not None:
            # Record the source before the original finisher copies it. Flash
            # above the supplied image is synthetic zero-filled mapped memory.
            memory_trace.append({
                'source': struct.unpack('<I', engine.mem_read(0x2000562C, 4))[0],
                'length': struct.unpack('<I', engine.mem_read(0x200053DC, 4))[0],
                'copy_pending': bool(engine.mem_read(STATE + 3, 1)[0]),
            })

    cpu.hook_add(UC_HOOK_CODE, hook)
    cpu.reg_write(UC_ARM_REG_SP, 0x2000F000)
    cpu.reg_write(UC_ARM_REG_LR, STOP | 1)
    cpu.reg_write(UC_ARM_REG_XPSR, 0x01000000)
    cpu.emu_start(0x36A51, STOP, timeout=1_000_000, count=20000)
    if cpu.reg_read(UC_ARM_REG_PC) != STOP:
        raise RuntimeError('Isolated handler did not return within limits')
    return bytes(cpu.mem_read(BUFFER, 64))


def main():
    image = IMAGE.read_bytes()
    if hashlib.sha256(image).hexdigest() != CHECKSUM:
        raise ValueError('Reference image mismatch')
    output = {'reference_image_sha256': CHECKSUM, 'hardware_access': False,
              'scope': 'Isolated GET dispatcher with synthetic RAM; housekeeping stubbed',
              'results': []}
    for command in (0x80, 0x8F, 0xD3):
        response = run_get(image, command)
        if response[0] != command:
            raise ValueError('Unexpected emulated response')
        output['results'].append({'command': command, 'bytes': list(response), 'hex': response.hex(' ')})
        print(f'{command:02X}: {response[:16].hex(" ")}')
    version, identity, options = [bytes(r['bytes']) for r in output['results']]
    assert version[1:3] == bytes([3, 3])
    assert struct.unpack_from('<I', identity, 1)[0] == 1852
    assert options[9:11] == bytes([1, 2])
    assert options[50:54] == bytes([90, 95, 2, 1])
    output['configuration_read_boundaries'] = []
    # Boundary values are executed ONLY in the emulator. Sending invalid profile
    # indices to physical devices is deliberately not supported by this script.
    for command, profile, part, current, expected in (
        (0x96, 0, 0, 0, 0x70000),
        (0x96, 255, 4, 0, 0x80000),
        (0xD0, 0, 0, 0, 0x6E000),
        (0xD0, 255, 0, 255, 0x81EC0),
        (0xD1, 0, 0, 0, 0x6F000),
        (0xD1, 255, 1, 0, 0x72FC0),
    ):
        trace = []
        run_get(image, command, bytes([profile, part]),
                profile_offset=current, memory_trace=trace)
        if len(trace) != 1 or trace[0] != {'source': expected, 'length': 64, 'copy_pending': True}:
            raise RuntimeError(f'Unexpected isolated memory-read trace: {trace}')
        output['configuration_read_boundaries'].append({
            'command': command, 'request_profile': profile, 'request_part': part,
            'synthetic_current_profile_offset': current, **trace[0],
        })
    output['configuration_read_limits'] = {
        'scope': 'These reference handlers only; not a claim about every firmware or ROM interface',
        'minimum_source': 0x6E000, 'maximum_source': 0x81EC0,
        'length': 64, 'application_code_end_exclusive': 0x59834,
        'overlaps_reference_application_code': False,
        'hardware_boundary_probes_performed': False,
    }
    (ROOT / 'research/local/mouse-firmware-20261021/emulated-reference-gets.json').write_text(
        json.dumps(output, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
