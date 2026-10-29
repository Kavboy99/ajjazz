"""Check a reconstructed native wave step against the candidate's ARM bytes.

Only the isolated 0xBE84 routine is emulated. No OS, USB, MMIO, device access,
vendor executable execution or firmware patch. This is not a target-80A0 test.
"""
from pathlib import Path
import hashlib
import json
import random
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.tools/py-research'))
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_HOOK_MEM_WRITE
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR

SHA256 = 'ef67c8a05093961b799eebc39804b89707459c72477c6f26bf25e946b5810850'
PHASES = 0x20002F5E
INTENSITY = 0x20002D66
DIRECTION = 0x200002C5
STACK = 0x20007000
RETURN = 0x1F000


def reference_step(phases, intensity, direction, lut, mapping):
    """Recovered loop: update 21 phases, then scatter each into 6 matrix slots."""
    phases, intensity = bytearray(phases), bytearray(intensity)
    for column in range(21):
        phases[column] = (phases[column] + (1 if direction else -1)) % len(lut)
        value = lut[phases[column]]
        for key in mapping[column]:
            intensity[key] = value
    return bytes(phases), bytes(intensity)


def main():
    image = (ROOT / 'research/vendor/revision-hunt/candidate-SN34F280-app.bin').read_bytes()
    assert hashlib.sha256(image).hexdigest() == SHA256, 'Unknown image; do not reuse these addresses'
    count = image[0x13F2B]
    lut = image[0x13ECB:0x13ECB + count]
    mapping = [image[0x144D2 + column * 8:0x144D2 + column * 8 + 6] for column in range(21)]
    assert count == 96 and max(max(row) for row in mapping) < 126
    rng = random.Random(820)
    cases = 0
    allowed_writes = [(PHASES, PHASES + 21), (INTENSITY, INTENSITY + 126), (STACK - 8, STACK)]
    for direction in [0, 1]:
        cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
        cpu.mem_map(0, 0x20000)
        cpu.mem_write(0, image)
        cpu.mem_map(0x20000000, 0x8000)
        cpu.mem_write(DIRECTION, bytes([direction]))
        phases = bytes([rng.randrange(count) for _ in range(21)])
        intensity = bytes(rng.randrange(256) for _ in range(126))
        cpu.mem_write(PHASES, phases)
        cpu.mem_write(INTENSITY, intensity)
        def check_write(_cpu, _access, address, size, _value, _context):
            assert any(start <= address and address + size <= end for start, end in allowed_writes), f'Unexpected emulated write {address:#x}/{size}'
        cpu.hook_add(UC_HOOK_MEM_WRITE, check_write)
        start_phases = phases
        for _ in range(count):
            phases, intensity = reference_step(phases, intensity, direction, lut, mapping)
            cpu.reg_write(UC_ARM_REG_SP, STACK)
            cpu.reg_write(UC_ARM_REG_LR, RETURN | 1)
            cpu.emu_start(0xBE85, RETURN, count=50_000)
            assert cpu.reg_read(UC_ARM_REG_SP) == STACK
            assert bytes(cpu.mem_read(PHASES, 21)) == phases
            assert bytes(cpu.mem_read(INTENSITY, 126)) == intensity
            cases += 1
        assert phases == start_phases
    proof = {'image_sha256': SHA256, 'candidate_pid': '8033', 'candidate_version': '1.04',
             'target_80a0_tested': False, 'device_access': False, 'firmware_patch_generated': False,
             'arm_routine': '0xbe84', 'reference_steps_compared': cases, 'directions': [0, 1],
             'phase_count': count, 'state_columns': 21, 'matrix_slots_per_column': 6,
             'all_phase_and_intensity_bytes_match': True, 'full_cycles_return_phase_state': True,
             'emulated_writes_restricted_to_phase_intensity_and_stack': True}
    (ROOT / 'research/local/candidate-wave-emulation-proof.json').write_text(json.dumps(proof, indent=2), encoding='utf-8')
    print(json.dumps(proof, indent=2))


if __name__ == '__main__': main()
