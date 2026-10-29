"""Offline Sonix updater laboratory. Python standard library; never opens HID.

Inspect/extract PE RCDATA or strict Intel HEX, compare images and assess a
revision profile. No vendor code, DLL, scripts or firmware are executed.
"""
from __future__ import annotations

import argparse
import configparser
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import struct
import zipfile

MAX_INPUT = 32 * 1024 * 1024
MAX_IMAGE = 1024 * 1024
MAX_ARCHIVE = 8 * 1024 * 1024
ROOT = Path(__file__).resolve().parents[1]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def span(data, start, count):
    require(0 <= start <= len(data) and 0 <= count <= len(data) - start,
            f'Truncated/out-of-bounds data at {start:#x}, length {count:#x}')
    return data[start:start + count]


def uint(data, offset, size):
    return int.from_bytes(span(data, offset, size), 'little')


class PE:
    """Bounds-checked PE resource reader. Does not load an executable."""
    def __init__(self, data):
        require(len(data) <= MAX_INPUT and data[:2] == b'MZ', 'Expected bounded PE file')
        self.data = data
        nt = uint(data, 0x3c, 4)
        require(span(data, nt, 4) == b'PE\0\0', 'Invalid PE signature')
        self.machine = uint(data, nt + 4, 2)
        count = uint(data, nt + 6, 2)
        require(1 <= count <= 96, 'Invalid section count')
        optional_size = uint(data, nt + 20, 2)
        opt = nt + 24
        span(data, opt, optional_size)
        magic = uint(data, opt, 2)
        require(magic in (0x10b, 0x20b), 'Unsupported PE optional header')
        directory_offset = 96 if magic == 0x10b else 112
        require(optional_size >= directory_offset + 24, 'Missing resource directory')
        self.image_base = uint(data, opt + (28 if magic == 0x10b else 24), 4 if magic == 0x10b else 8)
        self.header_size = uint(data, opt + 60, 4)
        self.sections = []
        at = opt + optional_size
        for i in range(count):
            raw = span(data, at + i * 40, 40)
            virtual_size, rva, raw_size, offset = struct.unpack_from('<4I', raw, 8)
            span(data, offset, raw_size)
            self.sections.append(dict(name=raw[:8].rstrip(b'\0').decode('ascii', 'replace'),
                rva=rva, virtual_size=virtual_size, raw_size=raw_size, offset=offset))
        self.resource_rva = uint(data, opt + directory_offset + 16, 4)
        self.resource_size = uint(data, opt + directory_offset + 20, 4)

    def offset(self, rva, size):
        matches = []
        if rva < self.header_size and size <= self.header_size - rva:
            span(self.data, rva, size)
            matches.append(rva)
        for s in self.sections:
            relative = rva - s['rva']
            if 0 <= relative <= s['raw_size'] and size <= s['raw_size'] - relative:
                matches.append(s['offset'] + relative)
        require(len(matches) == 1, f'Unmapped/ambiguous RVA {rva:#x}')
        return matches[0]

    def resources(self):
        require(0 < self.resource_size <= MAX_INPUT, 'No bounded PE resources')
        base = self.offset(self.resource_rva, self.resource_size)
        tree = span(self.data, base, self.resource_size)
        result, visited = [], set()

        def walk(at, path):
            require(at not in visited and len(path) <= 3, 'Cyclic/deep resource tree')
            visited.add(at)
            header = span(tree, at, 16)
            count = uint(header, 12, 2) + uint(header, 14, 2)
            require(count <= 4096, 'Resource directory too large')
            for i in range(count):
                name, pointer = struct.unpack('<II', span(tree, at + 16 + i * 8, 8))
                if name & 0x80000000:
                    pos = name & 0x7fffffff
                    name = span(tree, pos + 2, uint(tree, pos, 2) * 2).decode('utf-16le')
                key = path + [name]
                if pointer & 0x80000000:
                    walk(pointer & 0x7fffffff, key)
                else:
                    require(len(key) == 3, 'Invalid resource hierarchy')
                    rva, size, codepage, reserved = struct.unpack('<4I', span(tree, pointer, 16))
                    offset = self.offset(rva, size)
                    result.append(dict(type=key[0], id=key[1], language=key[2], offset=offset,
                        rva=rva, bytes=size, data=span(self.data, offset, size)))
        walk(0, [])
        return result


def ihex(data):
    require(len(data) <= MAX_ARCHIVE, 'HEX input too large')
    text = data.decode('ascii')
    memory, base, eof, entry, records = {}, 0, False, None, 0
    for number, line in enumerate(text.splitlines(), 1):
        require(line.startswith(':') and not eof, f'HEX framing/after EOF at line {number}')
        require(re.fullmatch(r':[0-9a-fA-F]+', line) is not None, f'Non-hex data at line {number}')
        record = bytes.fromhex(line[1:])
        require(len(record) >= 5 and len(record) == record[0] + 5 and sum(record) % 256 == 0,
                f'HEX length/checksum at line {number}')
        size, address, kind = record[0], int.from_bytes(record[1:3], 'big'), record[3]
        payload = record[4:-1]
        records += 1
        if kind == 0:
            require(address + size <= 0x10000, 'HEX data crosses 64 KiB record boundary')
            require(base + address + size <= MAX_IMAGE, 'HEX outside laboratory address limit')
            for i, value in enumerate(payload):
                absolute = base + address + i
                require(absolute not in memory or memory[absolute] == value, 'Conflicting HEX overlap')
                memory[absolute] = value
        elif kind == 1:
            require(size == 0 and address == 0, 'Malformed HEX EOF')
            eof = True
        elif kind in (2, 4):
            require(size == 2 and address == 0, 'Malformed HEX base record')
            base = int.from_bytes(payload, 'big') << (4 if kind == 2 else 16)
        elif kind in (3, 5):
            require(size == 4 and address == 0 and entry is None, 'Malformed/duplicate HEX entry')
            entry = ((int.from_bytes(payload[:2], 'big') << 4) + int.from_bytes(payload[2:], 'big')
                     if kind == 3 else int.from_bytes(payload, 'big'))
        else:
            raise ValueError(f'Unsupported HEX record {kind}')
    require(eof and memory, 'HEX missing EOF or data')
    ranges = []
    for address in sorted(memory):
        if ranges and ranges[-1][1] == address:
            ranges[-1][1] = address + 1
        else:
            ranges.append([address, address + 1])
    return memory, dict(records=records, populated_bytes=len(memory), ranges=ranges,
                        entry=entry, checksums_valid=True)


def word_sum(data):
    require(len(data) % 2 == 0, 'Word checksum requires even length')
    return sum(x[0] for x in struct.iter_unpack('<H', data)) & 0xffff


def image_info(data):
    require(64 <= len(data) <= MAX_IMAGE, 'Invalid image length')
    sp, reset = struct.unpack_from('<II', data)
    info = dict(bytes=len(data), sha256=sha(data), initial_sp=f'{sp:08X}', reset=f'{reset:08X}',
        plausible_vectors=0x20000000 <= sp < 0x20080000 and reset & 1 == 1 and reset < len(data),
        word_sum=f'{word_sum(data):04X}' if len(data) % 2 == 0 else None, tail_16=data[-16:].hex())
    # This block is shared by the captured Sonix magnetic keyboard firmware.
    # Its presence is descriptive evidence, never proof of board compatibility.
    if len(data) >= 0x9020 and data[0x9000:0x9004] == bytes.fromhex('00000012'):
        vid, pid, version = struct.unpack_from('<HHH', data, 0x9004)
        info['sonix_identity_heuristic'] = dict(offset=0x9000, vid=f'{vid:04X}', pid=f'{pid:04X}',
            version_raw=f'{version:04X}', manufacturer=f'{uint(data,0x900c,2):04X}',
            field_900e=f'{uint(data,0x900e,2):04X}', compatibility_proven=False)
    return info


def archive_members(data):
    result = {}
    folded = set()
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        members = z.infolist()
        require(len(members) <= 256 and sum(m.file_size for m in members) <= MAX_ARCHIVE,
                'ZIP expansion limit')
        for m in members:
            name = m.filename.replace('\\', '/')
            path = PurePosixPath(name)
            parts = name.rstrip('/').split('/')
            require(not path.is_absolute() and '..' not in path.parts and ':' not in name
                    and parts and all(part and part not in ('.','..') and part[-1] not in ('.',' ')
                    and not re.search(r'[\x00-\x1f<>"|?*]',part)
                    and re.fullmatch(r'(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])',part.split('.')[0]) is None
                    for part in parts) and name.casefold() not in folded, 'Unsafe/duplicate ZIP member')
            folded.add(name.casefold())
            if not m.is_dir():
                result[name] = z.read(m)
    return result


def decode_text(data):
    if data.startswith(b'\xff\xfe'):
        return data[2:].decode('utf-16le')
    if b'\0' in data[:100]:
        return data.decode('utf-16le')
    for encoding in ('utf-8-sig', 'gb18030'):
        try:
            return data.decode(encoding)
        except UnicodeDecodeError:
            pass
    raise ValueError('Unknown text encoding')


def inspect_updater(data):
    pe = PE(data)
    resources = pe.resources()
    rc = {}
    for r in resources:
        if r['type'] == 10:
            require(r['id'] not in rc, 'Ambiguous RCDATA language')
            rc[r['id']] = r
    require(4000 in rc, 'No RCDATA4000 firmware; unsupported updater container')
    image = rc[4000]['data']
    texts = {str(n): decode_text(rc[n]['data']).rstrip('\0') for n in
             (4007,4008,4009,4010,4013,4014,4016) if n in rc}
    result = dict(schema=1, updater_sha256=sha(data), machine=f'{pe.machine:04X}',
        sections=pe.sections, resources=[{k:v for k,v in r.items() if k!='data'} | {'sha256':sha(r['data'])}
        for r in resources], metadata=texts, active_image=image_info(image),
        hardware_access=False, vendor_code_executed=False)
    if '4014' in texts:
        result['advertised_checksum_matches'] = texts['4014'].upper() == result['active_image']['word_sum']
    members = archive_members(rc[4011]['data']) if 4011 in rc else {}
    result['archive_members'] = {n:dict(bytes=len(b),sha256=sha(b)) for n,b in members.items()}
    configs = [b for n,b in members.items() if n.lower().endswith('/uisettings.ini')]
    require(len(configs)<=1, 'Ambiguous UI settings')
    if configs:
        parser = configparser.ConfigParser(interpolation=None)
        parser.read_string(decode_text(configs[0]))
        result['template_settings'] = {s:dict(parser[s]) for s in parser.sections()}
    templates = []
    for name, blob in members.items():
        if not name.lower().endswith('.hex'):
            continue
        memory, meta = ihex(blob)
        mismatches = [a for a,v in memory.items() if a>=len(image) or image[a]!=v]
        raw = bytearray([255])*max(len(image), max(memory)+1)
        for a,v in memory.items():
            raw[a] = v
        templates.append(dict(name=name, hex=meta, sparse_matches_active=not mismatches,
            differing_populated_bytes=len(mismatches), first_differing_addresses=mismatches[:16],
            padded_image=image_info(raw), hole_fill='FF for analysis only; not selected for flashing'))
    result['template_images'] = templates
    return result, rc, members


def inspect_file(path):
    data = path.read_bytes()
    require(len(data) <= MAX_INPUT, 'Input exceeds size limit')
    if data.startswith(b'MZ'):
        return inspect_updater(data)[0]
    if data.startswith(b':'):
        memory, meta = ihex(data)
        return dict(schema=1,kind='ihex',sha256=sha(data),hex=meta,hardware_access=False)
    require(not data.lstrip().startswith((b'<',b'{')), 'HTML/JSON is not firmware')
    return dict(schema=1,kind='raw',image=image_info(data),hardware_access=False)


def load_image(path):
    data = path.read_bytes()
    require(len(data) <= MAX_INPUT, 'Input exceeds size limit')
    return inspect_updater(data)[1][4000]['data'] if data.startswith(b'MZ') else data


def compare_images(first, second):
    ranges, changed = [], 0
    for at in range(max(len(first),len(second))):
        if (first[at] if at<len(first) else None) == (second[at] if at<len(second) else None):
            continue
        changed += 1
        if ranges and ranges[-1][1] == at:
            ranges[-1][1] = at + 1
        else:
            ranges.append([at,at+1])
    pages = []
    for at in range(0,max(len(first),len(second)),512):
        a,b = first[at:at+512],second[at:at+512]
        if a!=b:
            pages.append(dict(offset=at,first_sha256=sha(a),second_sha256=sha(b)))
    return dict(first=image_info(first),second=image_info(second),changed_bytes=changed,
        changed_ranges=ranges,changed_pages_512=pages,hardware_compatibility_proven=False)


PORT_ANCHORS = [(0x1051c,'lighting'),(0x10530,'usb_dispatch'),(0x57f4,'encoder_rotation'),
                (0x10118,'encoder_press_1'),(0x1013c,'encoder_press_2'),(0x13d94,'key_event')]


def portability(source, target):
    """Discover exact context anchors, never relocate or write a patch."""
    require(sha(source)=='58db8a73ea8be7b8f730d67177f3ad78ec1b28b2b73ac15861e67963a1b116ff',
            'Port baseline must be the audited OEM 1.36 image')
    anchors=[]
    for site,name in PORT_ANCHORS:
        # Keep a relatively long context. Exact matching is deliberately stricter
        # than matching a generic Thumb prologue or 4-byte BL instruction.
        start=site-16
        fingerprint=source[start:site+20]
        positions=[];at=0
        while (at:=target.find(fingerprint,at))>=0:
            positions.append(at+16);at+=1
        anchors.append(dict(name=name,source_site=site,context_sha256=sha(fingerprint),
            exact_candidates=positions,status='unique' if len(positions)==1 else 'missing' if not positions else 'ambiguous'))
    reservations=[]
    for start,end in [(0x20000,0x24000),(0x24000,0x26000),(0x26000,0x26400)]:
        reservations.append(dict(start=start,end_exclusive=end,
            entirely_zero=end<=len(target) and not any(target[start:end])))
    return dict(schema=1,source_sha256=sha(source),target=image_info(target),anchors=anchors,
        reservations=reservations,automatically_portable=False,
        reason='Exact anchors alone do not validate RAM, scatter loading, ABI, LED map, flash routines or board wiring',
        patches_written=False,hardware_access=False)


def assess(report, snapshot, profiles):
    image=report['active_image']
    candidates=[p for p in profiles['boards'] if p['image_sha256']==image['sha256']]
    info=snapshot.get('info',{})
    endpoint=snapshot.get('endpoint',{})
    results=[]
    for p in candidates:
        expected=p['application']
        checks={
            'checksum':report.get('advertised_checksum_matches') is True,
            'image_length':image['bytes']==p['image_bytes'],
            'application_vid':info.get('vid')==int(expected['vid'],16),
            'application_pid':info.get('pid')==int(expected['pid'],16),
            'manufacturer':info.get('manufacturer')==int(expected['manufacturer'],16),
            'product':expected.get('product') is not None and info.get('product')==int(expected['product'],16),
            'current_version':info.get('version') in p['observed_current_versions'],
            'wired_endpoint':endpoint.get('vid')==int(expected['vid'],16) and
                endpoint.get('pid')==int(expected['pid'],16) and endpoint.get('usage_page')==0xff68 and
                endpoint.get('usage')==97 and endpoint.get('interface')==2,
        }
        results.append(dict(profile=p['id'],checks=checks,identity_and_package_match=all(checks.values()),
            hardware_evidence=p['hardware_evidence'],native_transport_hardware_validated=False))
    return dict(schema=1,image_sha256=image['sha256'],candidate_profiles=results,
        native_flashing_enabled=False,hardware_access=False,
        reason='Assessment uses saved evidence. This laboratory has no HID transport; unknown/incomplete identities never become writable.')


def save_json(path, value):
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')


def write_evidence(path, data):
    path.parent.mkdir(parents=True,exist_ok=True)
    require(not path.exists() or path.read_bytes()==data, f'Refusing to replace different evidence: {path}')
    path.write_bytes(data)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    for command in ('inspect','extract'):
        p=sub.add_parser(command)
        p.add_argument('input',type=Path)
        p.add_argument('--out',type=Path,required=True)
    p=sub.add_parser('compare')
    p.add_argument('first',type=Path);p.add_argument('second',type=Path)
    p.add_argument('--out',type=Path,required=True)
    p=sub.add_parser('portability')
    p.add_argument('baseline',type=Path);p.add_argument('target',type=Path)
    p.add_argument('--out',type=Path,required=True)
    p=sub.add_parser('assess')
    p.add_argument('input',type=Path);p.add_argument('snapshot',type=Path)
    p.add_argument('--profiles',type=Path,default=ROOT/'research/keyboard-updater/profiles.json')
    p.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    if args.command=='inspect':
        report=inspect_file(args.input);save_json(args.out,report)
    elif args.command=='extract':
        report,rc,members=inspect_updater(args.input.read_bytes())
        write_evidence(args.out/'active-image.bin',rc[4000]['data'])
        for n,r in rc.items():
            require(isinstance(n,int),'Named RCDATA export is not supported')
            write_evidence(args.out/'resources'/f'{n}.bin',r['data'])
        for n,b in members.items():
            write_evidence(args.out/'template'/n,b)
        save_json(args.out/'manifest.json',report)
    elif args.command=='compare':
        report=compare_images(load_image(args.first),load_image(args.second));save_json(args.out,report)
    elif args.command=='portability':
        report=portability(load_image(args.baseline),load_image(args.target));save_json(args.out,report)
    else:
        report=assess(inspect_file(args.input),json.loads(args.snapshot.read_text(encoding='utf-8-sig')),
                      json.loads(args.profiles.read_text(encoding='utf-8')))
        save_json(args.out,report)
    print(json.dumps(dict(command=args.command,output=str(args.out),hardware_access=False,
                         vendor_code_executed=False),ensure_ascii=False))


if __name__=='__main__':
    main()
