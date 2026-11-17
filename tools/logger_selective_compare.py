"""Read-only comparison for the published eight-transition passive AVS trial.

Requires exactly eight EPR requests and unambiguous same-run references. This
is not a general full-traffic or arbitrary-session completeness validator.
"""
import argparse
import json
from collections import Counter
from pathlib import Path
from logger_decode import decode

def routine(packet):
    sop, header, payload = packet['signature']
    count = header >> 12 & 7
    return sop == 'SOP' and (
        (not header & 0x8000 and header & 31 == 1 and count == 0)
        or (header & 0x8000 and header & 31 == 16 and count == 1
            and payload in ('02800300', '02800400')))

def compare(run, session=3):
    run = Path(run)
    decoded = [r for r in decode(sorted(run.glob('VI*.CSV')) + sorted(run.glob('PD*.CSV'))) if int(r['session']) == session]
    samples = [r for r in decoded if r['kind'] == 'SAMPLE']
    notices = [r for r in decoded if r['kind'] not in ('SAMPLE', 'PD_RX')]
    if not samples or notices:
        raise ValueError('Missing VI samples or loss/sniffer-stop notices present')
    kws = []
    for r in decoded:
        if r['kind'] != 'PD_RX':
            continue
        if not r['status'] == 'COMPLETE':
            raise ValueError('Incomplete KWS packet')
        payload = b''.join((int(word, 16).to_bytes(4, 'little') for word in r.get('data_hex', '').split())).hex()
        kws.append(dict(signature=(r['sop'], int(r['header_hex'], 16), payload), time_ms=int(r['elapsed_ms']), event_id=int(r['event_id'])))
    cy = []
    for line in (run / 'CY4500.records.jsonl').read_text().splitlines():
        r = json.loads(line)
        f = r['fields']
        if f['PKT_TYPE'] != 0:
            continue
        if not (f['OK'] and (not any((f[k] for k in ('CRC_ERROR', 'EOP_ERROR', 'IDLE_ERROR'))))):
            raise ValueError('CY receive validity/error flags failed')
        if r['decoder_flag_differences']:
            raise ValueError('CY metadata/raw decoder flags differ')
        header = f['MSG_TYPE'] | f['DATA_ROLE'] << 5 | f['SPEC_REV'] << 6 | f['POWER_ROLE'] << 8 | f['MSG_ID'] << 9 | f['OBJ_COUNT'] << 12 | f['EXTENDED'] << 15
        raw = bytes.fromhex(r['raw_hex'])
        if not 0 <= f['OBJ_COUNT'] <= 7 or len(raw) < 20 + 4*f['OBJ_COUNT']:
            raise ValueError('Invalid or truncated CY record')
        if not header == int.from_bytes(raw[16:18], 'little'):
            raise ValueError('CY header fields/raw differ')
        payload = raw[20:20 + 4 * f['OBJ_COUNT']].hex()
        cy.append(dict(signature=(r['sop'], header, payload), time_ms=r['start_time_us'] / 1000, name=r['message'], record_index=r['index']))
    kr = [p for p in kws if p['signature'][1] & 32799 == 9 and p['signature'][1] >> 12 & 7 == 2]
    cr = [p for p in cy if p['name'] == 'EPR_REQUEST']
    if not (len(kr) == 8 and (not notices)):
        raise ValueError('Expected eight EPR requests and no loss/stop notices')
    candidates = [i for i in range(len(cr) - len(kr) + 1) if [p['signature'] for p in cr[i:i + len(kr)]] == [p['signature'] for p in kr]]
    if not len(candidates) == 1:
        raise ValueError('Request reference interval ambiguous/missing')
    refs = cr[candidates[0]:candidates[0] + len(kr)]
    x = [p['time_ms'] for p in refs]
    y = [p['time_ms'] for p in kr]
    mx = sum(x) / len(x)
    my = sum(y) / len(y)
    slope = sum(((a - mx) * (b - my) for a, b in zip(x, y))) / sum(((a - mx) ** 2 for a in x))
    offset = my - slope * mx
    matches = []
    used = set()
    for i, p in enumerate(kws):
        options = [(abs(p['time_ms'] - (offset + slope * q['time_ms'])), j) for j, q in enumerate(cy) if j not in used and p['signature'] == q['signature']]
        if not options:
            raise ValueError('Retained KWS signature absent from CY')
        residual, j = min(options)
        if not residual < 10:
            raise ValueError('Timing residual exceeds predefined 10 ms criterion')
        if not (not matches or j > matches[-1]['cy_index']):
            raise ValueError('Packet order differs')
        used.add(j)
        matches.append(dict(kws_index=i, cy_index=j, residual_ms=residual, event_id=p['event_id'], name=cy[j]['name']))
    low, high = (matches[0]['cy_index'], matches[-1]['cy_index'])
    important = {j for j in range(low, high + 1) if not routine(cy[j])}
    if not important == used:
        raise ValueError('Unexplained critical omission or routine retention')
    if not Counter((m['name'] for m in matches)) == {'EPR_REQUEST': 8, 'ACCEPT': 8, 'PS_RDY': 8}:
        raise ValueError('Expected eight Request/Accept/PS_RDY sequences')
    if not all(([m['name'] for m in matches[i:i + 3]] == ['EPR_REQUEST', 'ACCEPT', 'PS_RDY'] for i in range(0, 24, 3))):
        raise ValueError('Important response order differs')
    diagnostics = json.loads((run / 'CY4500.diagnostics.json').read_text())
    return dict(
        status='PASS within observed critical-packet overlap',
        session=session,
        vi_count=len(samples),
        first_vi_ms=int(samples[0]['elapsed_ms']),
        last_vi_ms=int(samples[-1]['elapsed_ms']),
        retained_pd=len(kws),
        retained_pd_fragments=sum((1 + (p['signature'][1] >> 12 & 7) for p in kws)),
        important_sequences=8,
        match_fields=['SOP', 'complete raw16-bit header incl roles/revision/MessageID', 'all data objects', 'order'],
        clock_scale=slope,
        clock_offset_ms=offset,
        max_residual_ms=max((m['residual_ms'] for m in matches)),
        cy_overlap_sop_packets=high - low + 1,
        routine_omitted_in_overlap=high - low + 1 - len(matches),
        routine_omitted_by_name=dict(Counter((cy[j]['name'] for j in range(low, high + 1) if routine(cy[j])))),
        unexplained_important_omissions=0,
        incomplete_packets=0,
        queue_drop_notices=0,
        sniffer_stop_notices=0,
        cy_diagnostics=diagnostics['counts'],
        matches=matches,
        limitations=['One short artificial passive sweep, external ASD is active Sink.', 'Comparison window is first retained Request through last retained PS_RDY; no completeness claim outside it.', 'Clock fit is alignment, not calibration. No general loss-free or active-Sink logging claim.', 'VI selection cannot recover discarded internal readings; native XSL1/XSP1 differs from PC XSC1.'],
    )
if __name__ == '__main__':
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('directory', type=Path)
    ap.add_argument('--session', type=int, default=3)
    ap.add_argument('--out', type=Path)
    args = ap.parse_args()
    result = compare(args.directory, args.session)
    output = json.dumps(result, indent=2) + '\n'
    if args.out:
        with args.out.open('x', encoding='utf-8') as f:
            f.write(output)
    else:
        print(output, end='')
