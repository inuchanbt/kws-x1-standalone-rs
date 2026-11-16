"""Recheck the published BASE/SCREEN/TALL evidence without opening hardware."""
import csv
import hashlib
import json
from pathlib import Path
import statistics
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'tests/data/capture-pd-20261116'


def main():
    for line in (DATA / 'SHA256SUMS').read_text(encoding='utf-8').splitlines():
        expected, name = line.split('  ', 1)
        path = (DATA / name).resolve()
        if not path.is_relative_to(DATA.resolve()):
            raise ValueError('Checksum path escapes the data directory')
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError(f'Published evidence checksum mismatch: {name}')
    out = ROOT / 'build/capture-pd-replay-20261116'
    out.mkdir(parents=True, exist_ok=True)
    results = {}
    for mode in ['baseline', 'screen', 'tall']:
        source = DATA / mode
        name = 'KWS_trace_snapshot.bin' if mode == 'baseline' else 'KWS_CAPFLASH.bin'
        report_path = out / mode / 'comparison.json'
        report_path.parent.mkdir(exist_ok=True)
        subprocess.run([sys.executable, str(Path(__file__).with_name('analyze.py')),
                        str(source / name), '--cy', str(source / 'CY4500.csv'),
                        '--expected-mode', mode, '--out', str(report_path)], check=True)
        report = json.loads(report_path.read_text(encoding='utf-8'))
        run, comparison = report['run'], report['comparison']
        expected_saves = 0 if mode == 'baseline' else 8
        if run['saved'] != expected_saves or run['save_failures']:
            raise ValueError(f'{mode}: unexpected save counters')
        if comparison['critical_packets_matched'] != 24:
            raise ValueError(f'{mode}: incomplete critical PD evidence')
        with (source / 'CY4500.csv').open(encoding='utf-8-sig', newline='') as handle:
            rows = list(csv.DictReader(handle))
        pending, delays = None, []
        for row in rows:
            if row['Message'] != 'VOLT_PKT' and (row['Ok'] != '1' or 'RESET' in row['Message']):
                raise ValueError(f'{mode}: flagged PD error or reset')
            if row['SOP'] != 'SOP':
                continue
            if row['Message'] == 'EPR_REQUEST':
                pending = row
            elif row['Message'] == 'PS_RDY' and pending:
                delays.append((int(row['Start Time']) - int(pending['End Time'])) / 1000)
                pending = None
        if len(delays) != 8:
            raise ValueError(f'{mode}: expected eight completed negotiations')
        dimensions = []
        if mode != 'baseline':
            images = sorted((DATA / 'images').glob(mode.upper() + '_PD*.BMP'))
            if len(images) != 8:
                raise ValueError(f'{mode}: expected eight exported BMPs')
            for image in images:
                width, height = struct.unpack_from('<ii', image.read_bytes(), 18)
                expected_height = 135 if mode == 'screen' else 192
                if (width, abs(height)) != (240, expected_height):
                    raise ValueError(f'Unexpected BMP dimensions: {image.name}')
                dimensions.append([width, abs(height)])
        results[mode] = {
            'saved': run['saved'], 'failed': run['save_failures'],
            'matched_critical_packets': comparison['critical_packets_matched'],
            'successful_captures_with_overlap': comparison['captures_with_supported_overlap'],
            'request_end_to_ps_rdy_start_median_ms': statistics.median(delays),
            'bmp_dimensions': dimensions,
        }
    (out / 'summary.json').write_text(json.dumps(results, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'status': 'all three conditions validated', 'conditions': results,
                      'output': str(out)}, indent=2))


if __name__ == '__main__':
    main()
