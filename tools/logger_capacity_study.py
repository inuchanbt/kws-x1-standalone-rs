"""Reproduce a conservative, hypothetical selective-logger sizing study.

This is an offline model, not an encoder or an implementation of a flash format.
"""
import argparse
import collections
import json
import math
from pathlib import Path

from logger_decode import decode


def classify(row):
    if row['kind'] != 'PD_RX' or row.get('status') != 'COMPLETE':
        return 'retain_other_or_incomplete'
    typ, ext, data = row['message_type'], row['extended'], row['data_hex']
    if typ == 1 and ext == 0 and not data:
        return 'goodcrc'
    if typ == 16 and ext == 1 and data in ('00038002', '00048002'):
        return {'00038002': 'epr_keepalive', '00048002': 'epr_keepalive_ack'}[data]
    if ext == 0 and typ == 9 and len(data.split()) == 2:
        return 'epr_request'
    if ext == 0 and not data and typ in (3, 6):
        return {3: 'accept', 6: 'ps_rdy'}[typ]
    return 'retain_other_or_incomplete'


def selected(samples, mv, ma, heartbeat_ms=None):
    kept = []
    for sample in samples:
        if not kept or abs(sample['voltage_mV'] - kept[-1]['voltage_mV']) >= mv or abs(sample['current_mA'] - kept[-1]['current_mA']) >= ma or (heartbeat_ms is not None and sample['elapsed_ms'] - kept[-1]['elapsed_ms'] >= heartbeat_ms):
            kept.append(sample)
    return kept


def modeled_bytes(event_count):
    # Single unified stream, 16-byte sector header and 511 x 16-byte slots.
    # One start and one end marker consume slots; all other events are one slot.
    # Payload expansion, torn writes, wear, independent streams, CRC design and
    # large PD messages are not modeled. Header/markers are hypothetical.
    slots = event_count + 2
    sectors = math.ceil(slots / 511)
    return {'event_slots': event_count, 'marker_slots': 2, 'sector_headers_bytes': sectors * 16,
            'committed_bytes': slots * 16 + sectors * 16,
            'allocated_sector_bytes': sectors * 8192}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('directory', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    rows = decode([args.directory / 'VI000101.CSV', args.directory / 'PD000101.CSV'])
    samples = [r for r in rows if r['kind'] == 'SAMPLE']
    packets = [r for r in rows if r['kind'] != 'SAMPLE']
    classes = collections.Counter(map(classify, packets))
    suppressed = {'goodcrc', 'epr_keepalive', 'epr_keepalive_ack'}
    retained_pd = sum(v for k, v in classes.items() if k not in suppressed)
    thresholds = {}
    for mv, ma, heartbeat in [(100, 100, None), (100, 50, None), (500, 100, None), (100, 100, 10000), (100, 100, 60000)]:
        chosen = selected(samples, mv, ma, heartbeat)
        key = f'{mv}mV_{ma}mA_heartbeat_{heartbeat or "none"}'
        thresholds[key] = {'samples': len(chosen), 'timestamps_ms': [x['elapsed_ms'] for x in chosen],
                           'size': modeled_bytes(len(chosen) + retained_pd)}
    scenarios = {}
    # V/I + retained PD + periodic heartbeat + optional separate ripple event.
    # Hypothetical one-hour rates, not extrapolations from the 55-second sweep.
    for name, vi, pd, heartbeat, ripple in [('quiet', 10, 10, 60, 0), ('moderate', 60, 120, 60, 0), ('busy', 360, 600, 60, 0), ('moderate_ripple_60s', 60, 120, 60, 60), ('moderate_ripple_1s', 60, 120, 60, 3600)]:
        scenarios[name] = {'vi_changes_per_hour': vi, 'pd_events_per_hour': pd,
                           'heartbeats_per_hour': heartbeat, 'ripple_events_per_hour': ripple,
                           'size': modeled_bytes(vi + pd + heartbeat + ripple)}
    output = {'input_sample_count': len(samples), 'first_sample_ms': samples[0]['elapsed_ms'],
              'last_sample_ms': samples[-1]['elapsed_ms'], 'pd_classification': dict(classes),
              'retained_pd_events': retained_pd, 'thresholds': thresholds, 'one_hour_scenarios': scenarios}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(output, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(output, indent=2))


if __name__ == '__main__':
    main()
