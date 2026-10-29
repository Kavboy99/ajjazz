"""Malformed-container, provenance, revision and protocol failure regression tests."""
from pathlib import Path
import copy
import io
import json
import struct
import sys
import unittest
import zipfile

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
import keyboard_updater_lab as lab
import sonix_isp as isp


def record(kind,address=0,payload=b''):
    raw=bytes([len(payload)])+address.to_bytes(2,'big')+bytes([kind])+payload
    return ':'+(raw+bytes([-sum(raw)&255])).hex().upper()+'\n'


class InvalidContainers(unittest.TestCase):
    def test_truncated_pe(self):
        for data in (b'MZ',b'MZ'+bytes(80),b'<!DOCTYPE html>'):
            with self.assertRaises(ValueError):lab.PE(data)

    def test_hex_sparse_ranges(self):
        data=(record(4,payload=b'\0\1')+record(0,16,b'ab')+record(1)).encode()
        memory,meta=lab.ihex(data)
        self.assertEqual(memory,{0x10010:97,0x10011:98})
        self.assertEqual(meta['ranges'],[[0x10010,0x10012]])

    def test_hex_bad_checksum(self):
        with self.assertRaises(ValueError):lab.ihex(b':01000000FF01\n:00000001FF\n')

    def test_hex_missing_eof(self):
        with self.assertRaises(ValueError):lab.ihex(record(0,0,b'a').encode())

    def test_hex_data_after_eof(self):
        with self.assertRaises(ValueError):lab.ihex((record(0,0,b'a')+record(1)+record(0,2,b'b')).encode())

    def test_hex_conflicting_overlap(self):
        with self.assertRaises(ValueError):lab.ihex((record(0,0,b'a')+record(0,0,b'b')+record(1)).encode())

    def test_hex_high_address_not_allocated(self):
        with self.assertRaises(ValueError):lab.ihex((record(4,payload=b'\xff\xff')+record(0,0,b'a')+record(1)).encode())

    def test_hex_malformed_base(self):
        with self.assertRaises(ValueError):lab.ihex((record(4,1,b'\0\1')+record(0,0,b'a')+record(1)).encode())

    def test_zip_traversal_and_duplicate(self):
        for names in (('../escape',),('C:/escape',),('a\\..\\escape',),('/escape',),('a','a'),
                      ('a','A'),('Settings/CON.bin',),('Settings/nul',),('a./b',),('a/b ',),('a/./b',)):
            buffer=io.BytesIO()
            with zipfile.ZipFile(buffer,'w') as archive:
                for name in names:archive.writestr(name,b'x')
            with self.assertRaises(ValueError):lab.archive_members(buffer.getvalue())

    def test_zip_expansion_limit(self):
        buffer=io.BytesIO()
        with zipfile.ZipFile(buffer,'w',zipfile.ZIP_DEFLATED) as archive:
            archive.writestr('bomb',bytes(lab.MAX_ARCHIVE+1))
        with self.assertRaises(ValueError):lab.archive_members(buffer.getvalue())


class ProtocolFailures(unittest.TestCase):
    def test_wire_golden_erase_and_program(self):
        # Golden values are also independently checked by original x86 execution.
        self.assertEqual(isp.erase()[:12],bytes.fromhex('04aa550000000000f8010000'))
        self.assertEqual(isp.enable_program()[:12],bytes.fromhex('05aa550000000000801f0000'))
        self.assertEqual(isp.windows_report(isp.return_user())[:9],bytes.fromhex('0007aa550000000000'))

    def test_wrong_geometry(self):
        for call in (lambda:isp.erase(end_page=128),lambda:isp.enable_program(2048),lambda:isp.set_option(security=1)):
            with self.assertRaises(ValueError):call()

    def test_program_ack_and_header(self):
        valid=isp.enable_program()[:4]+isp.ACK+bytes(56)
        self.assertEqual(isp.check_reply(valid,isp.enable_program(),ack=True),valid)
        for data in (valid[:8],b'\0'+valid[1:],valid[:4]+bytes(60)):
            with self.assertRaises(ValueError):isp.check_reply(data,isp.enable_program(),ack=True)

    def test_no_resume_after_failed_identity(self):
        m=isp.DownloadModel(bytes(0x7e000))
        with self.assertRaises(ValueError):m.acknowledge(bytes(64))
        self.assertEqual(m.state,'failed')
        with self.assertRaises(ValueError):m.next_payload()

    def test_full_stream_and_final_checksum_failure(self):
        image=bytes([1,0])*258048
        m=isp.DownloadModel(image)
        version=bytearray(64);version[:4]=isp.get_version()[:4];version[8:12]=bytes.fromhex('20080000')
        m.acknowledge(bytes(version));m.acknowledge();m.acknowledge();m.acknowledge(bytes(64))
        m.acknowledge(isp.enable_program()[:4]+isp.ACK+bytes(56))
        count=0
        while m.state=='data':
            self.assertEqual(m.next_payload(),bytes([1,0])*32)
            m.acknowledge();count+=1
        self.assertEqual(count,8064)
        bad=bytearray(64);struct.pack_into('<H',bad,8,lab.word_sum(image)^1)
        with self.assertRaises(ValueError):m.acknowledge(bytes(bad))
        self.assertEqual(m.state,'failed')


@unittest.skipUnless((ROOT/'research/vendor/ak820max-support-20261024/support-updater.exe').exists(),'Local OEM evidence absent')
class CapturedEvidence(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data=(ROOT/'research/vendor/ak820max-support-20261024/support-updater.exe').read_bytes()
        cls.report,cls.rc,_=lab.inspect_updater(cls.data)
        cls.snapshot=json.loads((ROOT/'research/local/ak820max-support-20261024/keyboard-before.json').read_text(encoding='utf-8-sig'))
        cls.profiles=json.loads((ROOT/'research/keyboard-updater/profiles.json').read_text())

    def test_stdlib_resource_reader_agrees_with_pinned_extractor(self):
        self.assertEqual(self.report['active_image']['sha256'],'58db8a73ea8be7b8f730d67177f3ad78ec1b28b2b73ac15861e67963a1b116ff')
        self.assertTrue(self.report['advertised_checksum_matches'])
        self.assertEqual(self.report['active_image']['word_sum'],'67E7')

    def test_template_is_never_selected_as_active(self):
        t=self.report['template_images'][0]
        self.assertFalse(t['sparse_matches_active'])
        self.assertEqual(t['padded_image']['sonix_identity_heuristic']['pid'],'80BE')
        self.assertEqual(t['differing_populated_bytes'],77665)

    def test_pe_bad_resource_rva_rejected(self):
        data=bytearray(self.data);pe=lab.PE(data)
        nt=lab.uint(data,0x3c,4);struct.pack_into('<I',data,nt+24+96+16,0xfffffff0)
        with self.assertRaises(ValueError):lab.PE(data).resources()

    def test_known_identity_is_assessed_without_enabling_flash(self):
        r=lab.assess(self.report,self.snapshot,self.profiles)
        self.assertTrue(r['candidate_profiles'][0]['identity_and_package_match'])
        self.assertFalse(r['native_flashing_enabled'])

    def test_wrong_model_product_version_and_radio_rejected(self):
        for area,key,value in [('info','pid',0x8033),('info','product',0),('info','version','1.17'),('endpoint','pid',0xfefc)]:
            snapshot=copy.deepcopy(self.snapshot);snapshot[area][key]=value
            r=lab.assess(self.report,snapshot,self.profiles)
            self.assertFalse(r['candidate_profiles'][0]['identity_and_package_match'])

    def test_baseline_has_six_unique_anchors_but_is_not_autovalidated(self):
        image=self.rc[4000]['data'];r=lab.portability(image,image)
        self.assertTrue(all(x['status']=='unique' for x in r['anchors']))
        self.assertFalse(r['automatically_portable'])


if __name__=='__main__':unittest.main()
