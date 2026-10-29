"""Offline integrity regressions for downloaded reference images; no hardware."""
from pathlib import Path
import unittest

from cryptography.exceptions import InvalidSignature
from extract_ry_reference_firmware import inspect_component, verify_signatures


class ReferenceFirmwareTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = Path(__file__).resolve().parents[1] / 'research/vendor/panchip-reference'
        cls.image = (cls.root / 'aj179-reference/mouse-MV303-DV233-reference.bin').read_bytes()

    def test_all_eight_component_signatures(self):
        for family in ('aj179-reference', 'aj159-pro-reference'):
            for name in ('mouse', 'dock'):
                with self.subTest(family=family, name=name):
                    data = (self.root / family / f'{name}-MV303-DV233-reference.bin').read_bytes()
                    parts = [inspect_component(data, offset) for offset in (0x10000, 0x30000)]
                    verify_signatures(data, parts)
                    self.assertTrue(all(p['signature_verified'] for p in parts))
                    self.assertEqual(parts[-1]['end_offset'], len(data))

    def test_changed_code_rejected(self):
        data = bytearray(self.image)
        data[0x30400] ^= 1
        with self.assertRaisesRegex(ValueError, 'SHA256 mismatch'):
            inspect_component(data, 0x30000)

    def test_changed_signature_rejected_even_when_code_hash_is_valid(self):
        data = bytearray(self.image)
        data[-1] ^= 1
        parts = [inspect_component(data, offset) for offset in (0x10000, 0x30000)]
        with self.assertRaises(InvalidSignature):
            verify_signatures(data, parts)

    def test_truncation_and_invalid_offsets_rejected(self):
        for data, offset in ((self.image[:4], 0), (self.image, -1),
                             (self.image[:-1], 0x30000)):
            with self.subTest(offset=offset, size=len(data)), self.assertRaises(ValueError):
                inspect_component(data, offset)

    def test_oversize_tlv_rejected(self):
        data = bytearray(self.image)
        part = inspect_component(data, 0x30000)
        end = 0x30000 + part['header_bytes'] + part['payload_bytes']
        data[end + 2:end + 4] = b'\xff\xff'
        with self.assertRaisesRegex(ValueError, 'TLV range'):
            inspect_component(data, 0x30000)


if __name__ == '__main__':
    unittest.main()
