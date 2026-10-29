"""Offline verification of the captured updater's MCU selector.

The sole called export is set_chip_series_index. Every WASM host import throws;
the module receives no HID, filesystem, network, process, or browser capability.
Never substitute start_isp here.
"""
from pathlib import Path
import hashlib
import json
import re
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / '.tools/py-research'))
import wasmtime


def main():
    folder = ROOT / 'research/vendor/revision-hunt'
    binary = (folder / 'sn_isp_lib_bg.wasm').read_bytes()
    assert hashlib.sha256(binary).hexdigest() == '8816f7c9a112265943f0137d1d54b4c9fc13d87850803918d5e53a9569aacba6'
    text = (folder / 'sn_isp_lib_bg.wat').read_text(encoding='utf-8')
    memory = bytearray(2 * 1024 * 1024)
    for match in re.finditer(r'\(data \(;\d+;\) \(i32.const (\d+)\) "((?:[^"\\]|\\.)*)"\)', text):
        value, data, offset = match[2], bytearray(), 0
        while offset < len(value):
            if value[offset] == '\\':
                data.append(int(value[offset + 1:offset + 3], 16))
                offset += 3
            else:
                data.extend(value[offset].encode('utf-8'))
                offset += 1
        address = int(match[1])
        assert address + len(data) <= len(memory)
        memory[address:address + len(data)] = data
    names = []
    for index in range(4):
        size = struct.unpack_from('<I', memory, 1052808 + index * 4)[0]
        pointer = struct.unpack_from('<I', memory, 1052824 + index * 4)[0]
        assert size < 30
        names.append(memory[pointer:pointer + size].decode('ascii'))
    assert names == ['SN34F280', 'SN32F24xB', 'SN32F268', 'SN32F290']
    engine = wasmtime.Engine()
    module = wasmtime.Module(engine, binary)
    results = []

    def blocked(*args):
        raise RuntimeError('Host calls forbidden in offline selector test')

    for index in (0, 1, 2, 3, 4, 5, 255):
        store = wasmtime.Store(engine)
        imports = [wasmtime.Func(store, item.type, blocked) for item in module.imports]
        instance = wasmtime.Instance(store, module, imports)
        accepted = instance.exports(store)['set_chip_series_index'](store, index)
        assert accepted == int(index <= 3)
        results.append({'wasm_index': index, 'accepted': accepted})
    report = {'host_functions': 'All imports throw; no WebHID, file, network or process capabilities provided',
              'chip_names': names, 'results': results}
    (folder / 'wasm-chip-selector-test.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    print('Offline selector verified: 0..3 accepted; 4, 5, 255 rejected. No host calls.')


if __name__ == '__main__':
    main()
