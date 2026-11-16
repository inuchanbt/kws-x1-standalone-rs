"""Read-only check that capture-owned records were cleared before the next run."""
import argparse
from datetime import datetime
import hashlib
import json
from pathlib import Path
import struct
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from analyze import MAGIC, decode_runs


def assess(data):
    report = {'ready_for_screen': False, 'sectors': [], 'errors': []}
    if len(data) != 6 * 8192:
        report['errors'].append('Expected the complete 48 KiB CAPFLASH.BIN.')
        return report
    blank_count = trace_count = capture_count = 0
    for offset in range(0, len(data), 8192):
        block = data[offset:offset + 8192]
        header = struct.unpack_from('<4I', block)
        owner, check = struct.unpack_from('<2I', block, 8192 - 20)
        if block == b'\xff' * 8192:
            kind = 'blank'
            blank_count += 1
        elif header == (MAGIC, 1, MAGIC ^ 0xffffffff, 0xfffffffe):
            kind = 'test_trace'
            trace_count += 1
        elif owner in (0x32435358, 0x31435358) and check == 0xd9a2651e:
            kind = 'capture_owned'
            capture_count += 1
        else:
            kind = 'other_data_preserved'
        report['sectors'].append({'address': hex(0x62000 + offset), 'kind': kind})
    try:
        decode_runs(data)
    except ValueError as error:
        report['errors'].append(str(error))
    if trace_count:
        report['errors'].append(f'{trace_count} test trace(s) remain after clear.')
    if capture_count:
        report['errors'].append(f'{capture_count} capture-owned sector(s) remain after clear.')
    if blank_count < 6:
        report['errors'].append(f'Only {blank_count} blank sector(s); all six are required before the eight-capture plan. Legacy logger data needs the backup-matched maintenance image.')
    report.update(blank_sectors=blank_count, blank_bytes=blank_count * 8192,
                  test_traces=trace_count, capture_owned_sectors=capture_count)
    report['ready_for_screen'] = not report['errors']
    report['scope'] = 'Requires the complete six-sector pool to be blank before observation. The trace later uses one sector, leaving 40 KiB for captures. This does not guarantee that eight arbitrarily large TALL images fit. This checker never erases data.'
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('snapshot', type=Path)
    args = parser.parse_args()
    data = args.snapshot.read_bytes()
    report = assess(data)
    destination = Path(__file__).resolve().parents[2] / 'build/capture-autotest-clear-checks' / datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    destination.mkdir(parents=True)
    (destination / 'CAPFLASH.BIN').write_bytes(data)
    report.update(sha256=hashlib.sha256(data).hexdigest(), snapshot=str(destination / 'CAPFLASH.BIN'))
    (destination / 'clear-check.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, indent=2))
    return int(not report['ready_for_screen'])


if __name__ == '__main__':
    raise SystemExit(main())
