"""Analytical protocol checks only: no peripheral access."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('keyboard_audit', ROOT / 'tools/audit_keyboard_revision.py')
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


class ResearchFormats(unittest.TestCase):
    def test_backquote_key_is_not_template_delimiter(self):
        value = audit.parse_literal('{name:"` ~", key:`Backquote`, enabled:!0}')
        self.assertEqual(value, {'name': '` ~', 'key': 'Backquote', 'enabled': True})

    def test_crc_matches_ccitt_false_check_vector(self):
        self.assertEqual(audit.crc16(b'123456789'), 0x29B1)

    def test_ota_roundtrip_and_crc_boundaries_on_both_channels(self):
        original = bytes(i % 251 for i in range(8193))
        for size in (32, 64):
            packets = audit.ota_packets(original, size)
            restored, offset = bytearray(), 0
            for index, packet in enumerate(packets, 1):
                self.assertEqual(len(packet), size)
                self.assertEqual(packet[:2], b'\xaa\x83')
                self.assertEqual(int.from_bytes(packet[2:4], 'big'), index)
                self.assertEqual(int.from_bytes(packet[4:6], 'big'), len(packets))
                count = packet[6]
                end = offset + count
                self.assertEqual(offset // 4096, (end - 1) // 4096)
                expected_crc = audit.crc16(original[(end - 1) // 4096 * 4096:end]) if end % 4096 == 0 or end == len(original) else 0xFFFF
                self.assertEqual(int.from_bytes(packet[7:9], 'big'), expected_crc)
                restored.extend(packet[9:9 + count])
                offset = end
            self.assertEqual(bytes(restored), original)

    def test_gif_payload_has_no_key_ids(self):
        colors = bytes([255, 0, 0, 0, 0, 255])
        packets = audit.gif_packets(colors, 2, 56, 32)
        self.assertEqual(packets[0][:18], bytes.fromhex('aa4200000001000002003200ff00000000ff'))

    def test_vendor_exact_multiple_count_disagrees_with_packet_count(self):
        # 4 header bytes + 36*3 RGB bytes = exactly two 56-byte chunks.
        packets = audit.gif_packets(bytes(108), 1, 56, 64)
        self.assertEqual(len(packets), 2)
        self.assertEqual(int.from_bytes(packets[0][4:6], 'big'), 3)

    def test_hex_bad_checksum_is_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'bad.hex'
            path.write_text(':040000000000000001\n:00000001FF\n', encoding='ascii')
            with self.assertRaises(AssertionError):
                audit.intel_hex(path)


if __name__ == '__main__':
    unittest.main()
