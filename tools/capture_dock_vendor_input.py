"""Passively record vendor Input Reports from the AJ159 dock.

Never opens keyboard/mouse usages and never sends Output or SET_FEATURE reports.
Optional GET_FEATURE polling observes the existing response buffer without
submitting a command. A changed buffer alone does not identify an RGB command.
An absence of reports does not prove that the physical RGB button is unrelated
to USB; firmware can keep the selected mode entirely local.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import time

import hid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seconds', type=int, default=120)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--poll-feature', action='store_true',
                        help='Also read the existing FFFF/2 feature buffer; no SET_FEATURE')
    parser.add_argument('--stop-file', type=Path,
                        help='Finish and save when this file appears; no USB writes')
    args = parser.parse_args()
    if not 1 <= args.seconds <= 1800:
        parser.error('Capture must last1–1800 seconds')
    if args.output.exists():
        parser.error('Choose a new output file to preserve previous captures')
    if args.stop_file is not None and args.stop_file.exists():
        parser.error('Stop file already exists; choose a new stop file')
    devices = [d for d in hid.enumerate(0x3151, 0x5007)
               if d['usage_page'] == 0xFFFF and d['usage'] == 1]
    if len(devices) != 1:
        raise RuntimeError(f'Expected exactly one dock vendor Input collection; found{len(devices)}')
    info = devices[0]
    feature_device = None
    if args.poll_feature:
        endpoints = [d for d in hid.enumerate(0x3151, 0x5007)
                     if d['usage_page'] == 0xFFFF and d['usage'] == 2]
        if len(endpoints) != 1:
            raise RuntimeError('Expected exactly one dock FFFF/2 feature collection')
        feature_device = hid.device()
        feature_device.open_path(endpoints[0]['path'])
    device = hid.device()
    device.open_path(info['path'])
    start = time.monotonic()
    report = {'started_at_utc': datetime.now(timezone.utc).isoformat(),
              'vid': '3151', 'pid': '5007', 'usage_page': 'FFFF', 'usage': 1,
              'device_path_sha256': hashlib.sha256(info['path']).hexdigest(),
              'outputs_sent': 0, 'features_sent': 0, 'reports_total': 0,
              'button_press_confirmation': None, 'stop_reason': 'duration_limit',
              'unique_reports': [], 'unique_limit_reached': False}
    report.update({'get_feature_calls': 0, 'feature_transitions': [],
                   'feature_errors': [], 'feature_poll_requested': args.poll_feature})
    previous_feature = None
    feature_failures = 0
    seen = {}
    print('PASSIVE_CAPTURE_READY; no USB writes; press dock RGB button', flush=True)
    try:
        while time.monotonic() - start < args.seconds:
            if args.stop_file is not None and args.stop_file.exists():
                report['stop_reason'] = 'stop_file'
                break
            packet = bytes(device.read(65, 100))
            if feature_device is not None and feature_failures < 3:
                report['get_feature_calls'] += 1
                try:
                    current = bytes(feature_device.get_feature_report(0, 65))
                    feature_failures = 0
                    if current != previous_feature:
                        if len(report['feature_transitions']) < 256:
                            item = {'seconds': round(time.monotonic() - start, 3),
                                    'hex': current.hex(' '),
                                    'initial_sample': previous_feature is None}
                            report['feature_transitions'].append(item)
                            print(f"FEATURE_BUFFER at{item['seconds']}s: {item['hex']}", flush=True)
                        else:
                            report['unique_limit_reached'] = True
                        previous_feature = current
                except OSError as error:
                    feature_failures += 1
                    if len(report['feature_errors']) < 16:
                        report['feature_errors'].append(str(error))
                    if feature_failures == 3:
                        print('GET_FEATURE disabled after 3 consecutive errors', flush=True)
            if not packet:
                continue
            report['reports_total'] += 1
            if packet in seen:
                seen[packet]['count'] += 1
                seen[packet]['last_seconds'] = round(time.monotonic() - start, 3)
            elif len(seen) < 256:
                item = {'first_seconds': round(time.monotonic() - start, 3),
                        'last_seconds': round(time.monotonic() - start, 3),
                        'hex': packet.hex(' '), 'count': 1}
                seen[packet] = item
                report['unique_reports'].append(item)
                print(f"INPUT at{item['first_seconds']}s: {item['hex']}", flush=True)
            else:
                report['unique_limit_reached'] = True
    finally:
        device.close()
        if feature_device is not None:
            feature_device.close()
        report['elapsed_seconds'] = round(time.monotonic() - start, 3)
        args.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f"CAPTURE_COMPLETE reports={report['reports_total']} unique={len(seen)} file={args.output}", flush=True)


if __name__ == '__main__':
    main()
