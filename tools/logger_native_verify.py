"""Read-only native journal/CSV and prior-byte preservation checks for this trial."""
import argparse, csv, hashlib, json, struct, zlib
from collections import defaultdict
from pathlib import Path
SECTOR = 8192
MAGICS = {
    0x31474c58: ('VI V1', False, False),
    0x31454c58: ('PD V1', True, False),
    0x32474c58: ('VI V2', False, True),
    0x32454c58: ('PD V2', True, True),
    0x314c5358: ('VI SEL1', False, True),
    0x31505358: ('PD SEL1', True, True),
}
def checksum(data):
    return (~zlib.crc32(data)) & 0x7fffffff
def bitwise(data):
    crc = 0xffffffff
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ (0xedb88320 if crc & 1 else 0)
    return crc & 0x7fffffff
def parse(raw):
    if len(raw) != 6 * SECTOR:
        raise ValueError('expected six sectors')
    sectors = []
    for i in range(6):
        data = raw[i*SECTOR:(i+1)*SECTOR]
        magic, sid, segment, crc = struct.unpack_from('<4I', data)
        item = dict(index=i+1, first_session=sid, segment=segment, records=[], committed_slots=0)
        if data == b'\xff' * SECTOR:
            item.update(kind='FREE', native=False, tail_clean=True, prefix_end=0)
        elif magic not in MAGICS:
            item.update(kind='OTHER/PROTECTED', native=False, tail_clean=False, prefix_end=SECTOR)
        else:
            if crc != checksum(data[:12]) or crc != bitwise(data[:12]):
                raise ValueError(f'invalid header CRC: sector {i+1}')
            kind, event, journal = MAGICS[magic]
            item.update(kind=kind, native=True, event=event, journal=journal)
            cursor = 16
            while cursor < SECTOR:
                slot = data[cursor:cursor+16]
                if slot == b'\xff' * 16:
                    break
                at, a, b, crc = struct.unpack('<4I', slot)
                if crc != checksum(slot[:12]) or crc != bitwise(slot[:12]):
                    raise ValueError(f'invalid record CRC: sector {i+1}, offset {cursor}')
                item['records'].append([at,a,b])
                cursor += 16
            item.update(committed_slots=len(item['records']), prefix_end=cursor,
                        tail_clean=data[cursor:] == b'\xff' * (SECTOR-cursor))
            if not item['tail_clean']:
                raise ValueError(f'non-erased gap/tail: sector {i+1}')
        sectors.append(item)
    expected = defaultdict(list)
    for sector in sorted((s for s in sectors if s['native']), key=lambda s:(s['first_session'],s['segment'],s['index'])):
        sid = sector['first_session']
        for at, a, b in sector['records']:
            if sector['journal'] and at == 0xffffffff and b == 0:
                sid = a
                continue
            value = b if sector['event'] or b < 0x80000000 else b - 0x100000000
            expected[(sid,sector['event'])].append((sid,at,a,value))
    return sectors, dict(expected)
def verify(folder, baseline):
    raw = (folder/'STORAGE.BIN').read_bytes()
    sectors, expected = parse(raw)
    actual = defaultdict(list)
    for path in sorted(folder.glob('*.CSV')):
        with path.open(encoding='utf-8-sig', newline='') as stream:
            reader = csv.DictReader(stream)
            event = 'event_tag' in reader.fieldnames
            fields = ('session','elapsed_ms','event_tag','raw_u32') if event else ('session','elapsed_ms','voltage_mV','current_mA')
            for row in reader:
                value = tuple(int(row[k]) for k in fields)
                actual[(value[0],event)].append(value)
    if dict(actual) != expected:
        raise ValueError('raw journal/CSV rows differ')
    original = baseline.read_bytes()
    old_sectors, old_expected = parse(original)
    preserved = []
    for i, (old, new) in enumerate(zip(old_sectors,sectors)):
        before = original[i*SECTOR:(i+1)*SECTOR]
        after = raw[i*SECTOR:(i+1)*SECTOR]
        if before == after:
            rule = 'unchanged sector'
        elif old['kind'] == 'FREE' and new['native']:
            rule = 'append into previously erased sector'
        elif old['native'] and old['journal'] and old['tail_clean'] and new['native']:
            if before[:old['prefix_end']] != after[:old['prefix_end']]:
                raise ValueError('old committed prefix changed')
            if new['committed_slots'] < old['committed_slots']:
                raise ValueError('old slots removed')
            rule = 'append after unchanged committed prefix'
        else:
            raise ValueError(f'protected sector changed: {i+1}')
        preserved.append(dict(index=i+1, rule=rule))
    if any(expected.get(key,[])[:len(rows)] != rows for key, rows in old_expected.items()):
        raise ValueError('old exported rows changed')
    return dict(
        raw_sha256=hashlib.sha256(raw).hexdigest(), baseline_sha256=hashlib.sha256(original).hexdigest(),
        all_native_header_and_record_crcs_pass=True, raw_csv_rows_equal=True,
        baseline_preservation_pass=True, preservation=preserved,
        sectors=[{k:v for k,v in s.items() if k!='records'} for s in sectors],
        streams=[dict(session=sid, event=event, rows=len(rows)) for (sid,event),rows in expected.items()],
        native_used_bytes=sum(16+s['committed_slots']*16 for s in sectors if s['native']),
    )
if __name__ == '__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--export-dir',type=Path,required=True)
    ap.add_argument('--baseline',type=Path,required=True)
    ap.add_argument('--out',type=Path,required=True)
    args=ap.parse_args()
    result=verify(args.export_dir,args.baseline)
    with args.out.open('x',encoding='utf-8') as stream:
        json.dump(result,stream,indent=2); stream.write('\n')
    print(json.dumps(result,indent=2))
