"""Offline format regressions; no hardware or updater execution."""
import sys
import unittest
from pathlib import Path
import zlib

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from extract_support_firmware import decode_resource, PINS, ROOT, sha


class SupportResources(unittest.TestCase):
    def test_complete_resource_wrapper(self):
        body = bytes(range(256)) * 16
        compressor = zlib.compressobj(wbits=-15)
        stream = compressor.compress(body) + compressor.flush()
        # Exact inverse of the two 202-byte noise insertions, with EOF retained.
        raw = stream[:1] + bytes(202) + stream[1:-1] + bytes(202) + stream[-1:]
        self.assertEqual(decode_resource(raw), body)
        for damaged in (raw[:-1], raw + b'\0', raw[:405]):
            with self.assertRaises((ValueError, zlib.error)):
                decode_resource(damaged)

    def test_pinned_email_payloads_when_present(self):
        for (role, name), (size, digest) in PINS.items():
            path = ROOT / 'research/vendor/ajazz-support-20261023' / role / 'decoded' / (name + '-verified.bin')
            if not path.exists():
                self.skipTest('User email payloads are not distributed as source')
            data = path.read_bytes()
            self.assertEqual((len(data), sha(data)), (size, digest))


if __name__ == '__main__':
    unittest.main()
