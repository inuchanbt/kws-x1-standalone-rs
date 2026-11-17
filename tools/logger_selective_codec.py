"""Experimental host-only selective log codec; no device flash writes.

Variable records carry CRC32 and 4-byte alignment in 8 KiB sectors. Unknown
events are JSON escaped intact rather than silently discarded. Not firmware.
"""
import argparse
import hashlib
import json
import struct
import zlib
from pathlib import Path

from logger_decode import decode, SOP
from logger_capacity_study import classify, selected

SECTOR = 8192
HEADER = 32
MAGIC = b'XSC1'
SOP_CODES = {v: k for k, v in SOP.items()}


def canonical(row):
    if row['kind'] == 'SAMPLE':
        return {k: row[k] for k in ('kind', 'elapsed_ms', 'voltage_mV', 'current_mA')}
    if (row['kind'] == 'PD_RX' and row.get('status') == 'COMPLETE'
            and row.get('sop') in SOP_CODES):
        return {k: row[k] for k in ('kind', 'elapsed_ms', 'event_id', 'sop', 'header_hex', 'data_hex')}
    return dict(row)


def select(rows, mv=100, ma=100, heartbeat=60000):
    vi = selected([r for r in rows if r['kind'] == 'SAMPLE'], mv, ma, heartbeat)
    identities = {id(r) for r in vi}
    result = []
    for row in rows:
        # Only suppress complete known SOP routine traffic. Preserve other SOPs.
        suppress = row.get('sop') == 'SOP' and classify(row) in {
            'goodcrc', 'epr_keepalive', 'epr_keepalive_ack'}
        if (row['kind'] == 'SAMPLE' and id(row) in identities) or (row['kind'] != 'SAMPLE' and not suppress):
            result.append(canonical(row))
    return result


def pack_record(kind, at, ident, payload):
    # 12-byte prefix: type, reserved flags, payload length, ms, event ID.
    head = struct.pack('<BBHII', kind, 0, len(payload), at, ident)
    raw = head + payload
    raw += struct.pack('<I', zlib.crc32(raw))
    return raw + b'\xff' * (-len(raw) % 4)


def encode(events, session, metadata):
    if not isinstance(metadata, dict):
        raise ValueError('metadata must be a JSON object')
    records = [pack_record(3, 0, 0, json.dumps(metadata, sort_keys=True, separators=(',', ':')).encode())]
    for row in events:
        kind, at, ident = row['kind'], row['elapsed_ms'], row.get('event_id', 0)
        if kind == 'SAMPLE':
            record = pack_record(1, at, 0, struct.pack('<Ii', row['voltage_mV'], row['current_mA']))
        elif kind == 'PD_RX' and 'header_hex' in row and row.get('sop') in SOP_CODES and 'status' not in row:
            payload = struct.pack('<BH', SOP_CODES[row['sop']], int(row['header_hex'], 16))
            payload += b''.join(struct.pack('<I', int(x, 16)) for x in row['data_hex'].split())
            record = pack_record(2, at, ident, payload)
        else:
            record = pack_record(5, at, ident, json.dumps(row, sort_keys=True, separators=(',', ':')).encode())
        records.append(record)
    records.append(pack_record(4, events[-1]['elapsed_ms'] if events else 0, 0, struct.pack('<I', len(events))))
    pages, cursor, committed = [], SECTOR, 0
    for record in records:
        if len(record) > SECTOR - HEADER:
            raise ValueError('record exceeds a sector')
        if cursor + len(record) > SECTOR:
            prefix = struct.pack('<4sIIIIII', MAGIC, 1, session, len(pages), SECTOR, HEADER, 0)
            page = bytearray(prefix + struct.pack('<I', zlib.crc32(prefix)) + b'\xff' * (SECTOR - HEADER))
            pages.append(page)
            cursor = HEADER
            committed += HEADER
        pages[-1][cursor:cursor + len(record)] = record
        cursor += len(record)
        committed += len(record)
    return b''.join(pages), {'committed_bytes': committed, 'allocated_bytes': len(pages) * SECTOR,
                            'event_record_bytes': [len(r) for r in records[1:-1]],
                            'metadata_record_bytes': len(records[0]), 'end_record_bytes': len(records[-1])}


def decode_binary(data):
    if not data or len(data) % SECTOR:
        raise ValueError('truncated sector image')
    events, metadata, session, ended = [], None, None, False
    for index in range(len(data) // SECTOR):
        if ended:
            raise ValueError('sector after end marker')
        page = data[index * SECTOR:(index + 1) * SECTOR]
        magic, version, sid, sequence, size, header, reserved, crc = struct.unpack('<4sIIIIIII', page[:HEADER])
        if (magic, version, sequence, size, header, reserved) != (MAGIC, 1, index, SECTOR, HEADER, 0) or zlib.crc32(page[:28]) != crc:
            raise ValueError('invalid sector header')
        if session is not None and session != sid:
            raise ValueError('mixed sessions')
        session = sid
        cursor = HEADER
        while cursor < SECTOR:
            if page[cursor:] == b'\xff' * (SECTOR - cursor):
                break
            if ended or cursor + 16 > SECTOR:
                raise ValueError('record after end or partial prefix')
            kind, flags, length, at, ident = struct.unpack('<BBHII', page[cursor:cursor + 12])
            end = cursor + 12 + length
            aligned = (end + 4 + 3) // 4 * 4
            if flags or aligned > SECTOR or zlib.crc32(page[cursor:end]) != struct.unpack('<I', page[end:end + 4])[0] or page[end + 4:aligned] != b'\xff' * (aligned - end - 4):
                raise ValueError('invalid record CRC/length/padding')
            payload = page[cursor + 12:end]
            if kind == 3 and index == 0 and cursor == HEADER:
                metadata = json.loads(payload)
                if not isinstance(metadata, dict):
                    raise ValueError('metadata must be a JSON object')
            elif metadata is None:
                raise ValueError('first record must be metadata')
            elif kind == 1 and length == 8:
                voltage, current = struct.unpack('<Ii', payload)
                events.append(dict(kind='SAMPLE', elapsed_ms=at, voltage_mV=voltage, current_mA=current))
            elif kind == 2 and length >= 3 and (length - 3) % 4 == 0:
                sop, hdr = struct.unpack('<BH', payload[:3])
                if sop not in SOP or length != 3 + 4 * ((hdr >> 12) & 7):
                    raise ValueError('invalid PD SOP or header/object count')
                events.append(dict(kind='PD_RX', elapsed_ms=at, event_id=ident, sop=SOP[sop], header_hex=f'{hdr:04X}',
                                   data_hex=' '.join(f'{x[0]:08X}' for x in struct.iter_unpack('<I', payload[3:]))))
            elif kind == 5:
                events.append(json.loads(payload))
            elif kind == 4 and length == 4 and struct.unpack('<I', payload)[0] == len(events):
                ended = True
            else:
                raise ValueError('invalid record type or count')
            cursor = aligned
    if metadata is None or not ended:
        raise ValueError('missing metadata or end marker')
    return session, metadata, events


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs', nargs='+', type=Path)
    parser.add_argument('--out-dir', required=True, type=Path)
    args = parser.parse_args()
    rows = decode(args.inputs)
    sessions = {r['session'] for r in rows}
    if len(sessions) != 1:
        parser.error('one session required')
    events = select(rows)
    metadata = dict(voltage_threshold_mV=100, current_threshold_mA=100, heartbeat_ms=60000,
                    source_sample_interval='approximately 1 s', policy='omit_known_SOP_routine_v1')
    session = sessions.pop()
    image, stats = encode(events, session, metadata)
    sid, restored_meta, restored = decode_binary(image)
    if sid != session or restored != events or restored_meta != metadata:
        raise ValueError('round trip mismatch')
    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / 'selective.bin').write_bytes(image)
    (args.out_dir / 'events.json').write_text(json.dumps(restored, indent=2) + '\n', encoding='utf-8')
    stats.update(round_trip_equal=True, session=sid, selected_events=len(events),
                 binary_sha256=hashlib.sha256(image).hexdigest(),
                 inputs={str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in args.inputs})
    (args.out_dir / 'result.json').write_text(json.dumps(stats, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(stats, indent=2))


if __name__ == '__main__':
    main()
