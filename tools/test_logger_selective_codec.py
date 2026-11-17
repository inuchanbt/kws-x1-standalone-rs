"""Host validation: selection loss, raw PD fidelity, CRC and sector boundaries."""
import hashlib
import json
import struct
import unittest
import zlib
from pathlib import Path

import logger_selective_codec as codec
from logger_decode import decode


class CodecTests(unittest.TestCase):
    def test_archived_captures_are_reproducible(self):
        root = Path(__file__).resolve().parents[1]
        study = root / 'tests/data/logger-selective-study'
        for name, vi_count, byte_count in [('v07', 12, 1060), ('v04', 14, 1108)]:
            with self.subTest(capture=name):
                fixture = study / name
                report = json.loads((fixture / 'result.json').read_text(encoding='utf-8'))
                # Reports generated on Windows must also work on other hosts.
                paths = [root / p.replace('\\', '/') for p in report['inputs']]
                for path, expected in zip(paths, report['inputs'].values()):
                    self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), expected)
                events = codec.select(decode(paths))
                self.assertEqual(sum(r['kind'] == 'SAMPLE' for r in events), vi_count)
                self.assertEqual(sum(r['kind'] == 'PD_RX' for r in events), 24)
                archived = (fixture / 'selective.bin').read_bytes()
                sid, metadata, restored = codec.decode_binary(archived)
                image, stats = codec.encode(events, sid, metadata)
                self.assertEqual(image, archived)
                self.assertEqual(hashlib.sha256(image).hexdigest(), report['binary_sha256'])
                self.assertEqual(restored, events)
                self.assertEqual(restored, json.loads((fixture / 'events.json').read_text(encoding='utf-8')))
                self.assertEqual(stats['committed_bytes'], byte_count)
                for key, value in stats.items():
                    self.assertEqual(value, report[key], key)
                self.assertEqual(report['session'], sid)
                self.assertEqual(report['selected_events'], len(events))
                self.assertIs(report['round_trip_equal'], True)

    def test_public_capture_round_trip(self):
        root = Path(__file__).resolve().parents[1] / 'tests/data/logger-observer-v07-avs-20261113'
        rows = decode([root / 'VI000101.CSV', root / 'PD000101.CSV'])
        events = codec.select(rows)
        self.assertEqual(sum(r['kind'] == 'SAMPLE' for r in events), 12)
        self.assertEqual(sum(r['kind'] == 'PD_RX' for r in events), 24)
        data, _ = codec.encode(events, 1, {'test': True})
        self.assertEqual(codec.decode_binary(data), (1, {'test': True}, events))

    def test_multisector_negative_current_and_full_pd_payload(self):
        events = [dict(kind='SAMPLE', elapsed_ms=i * 1000, voltage_mV=48000, current_mA=-500) for i in range(1100)]
        # Maximum 7 data objects: do not truncate to the short EPR request case.
        events.append(dict(kind='PD_RX', elapsed_ms=1100000, event_id=999, sop='SOP_PRIME',
                           header_hex='700F', data_hex=' '.join(f'{i:08X}' for i in range(7))))
        data, stats = codec.encode(events, 32, {})
        self.assertGreater(stats['allocated_bytes'], codec.SECTOR)
        self.assertEqual(codec.decode_binary(data)[2], events)
        # Reordering sectors or mixing sessions must fail, even with valid CRCs.
        swapped = data[8192:16384] + data[:8192] + data[16384:]
        with self.assertRaises(ValueError):
            codec.decode_binary(swapped)

    def test_unknown_diagnostics_are_preserved(self):
        events = [dict(kind='QUEUE_DROPPED', elapsed_ms=30, session=1, count=7),
                  dict(kind='PD_RX', elapsed_ms=40, session=1, event_id=9, status='INCOMPLETE_NO_HEADER', data_hex='1:DEADBEEF')]
        chosen = codec.select(events)
        data, _ = codec.encode(chosen, 1, {})
        self.assertEqual(codec.decode_binary(data)[2], events)

    def test_unknown_sop_preserves_all_fields(self):
        event = dict(kind='PD_RX', elapsed_ms=40, session=1, event_id=9,
                     status='COMPLETE', sop='31', header_hex='0001', data_hex='',
                     message_type=1, message_id=0, extended=0,
                     diagnostic='unrecognized SOP: retain the original row')
        chosen = codec.select([event])
        self.assertEqual(chosen, [event])
        data, _ = codec.encode(chosen, 1, {})
        self.assertEqual(codec.decode_binary(data)[2], [event])

    def test_end_marker_is_in_final_sector(self):
        data, _ = codec.encode([], 1, {})
        prefix = struct.pack('<4sIIIIII', codec.MAGIC, 1, 1, 1, codec.SECTOR, codec.HEADER, 0)
        extra = prefix + struct.pack('<I', zlib.crc32(prefix)) + b'\xff' * (codec.SECTOR - codec.HEADER)
        with self.assertRaises(ValueError):
            codec.decode_binary(data + extra)

    def test_metadata_must_be_an_object_in_first_record(self):
        with self.assertRaises(ValueError):
            codec.encode([], 1, None)
        valid, _ = codec.encode([], 1, {})
        records = (codec.pack_record(3, 0, 0, b'null') +
                   codec.pack_record(3, 0, 0, b'{}') +
                   codec.pack_record(4, 0, 0, struct.pack('<I', 0)))
        malformed = valid[:codec.HEADER] + records
        malformed += b'\xff' * (codec.SECTOR - len(malformed))
        with self.assertRaises(ValueError):
            codec.decode_binary(malformed)

    def test_heartbeat_and_non_sop_routine(self):
        vi = [dict(kind='SAMPLE', elapsed_ms=i * 1000, voltage_mV=5000, current_mA=500) for i in range(121)]
        chosen = codec.select(vi)
        self.assertEqual([r['elapsed_ms'] for r in chosen], [0, 60000, 120000])
        pd = dict(kind='PD_RX', status='COMPLETE', elapsed_ms=1, event_id=1, sop='SOP_PRIME',
                  header_hex='0001', data_hex='', message_type=1, extended=0)
        self.assertEqual(len(codec.select([pd])), 1)

    def test_corruption_and_truncation_are_rejected(self):
        events = [dict(kind='SAMPLE', elapsed_ms=1, voltage_mV=5000, current_mA=500)]
        data, stats = codec.encode(events, 1, {})
        # Every committed byte, including headers, metadata, CRC and padding.
        for offset in range(stats['committed_bytes']):
            bad = bytearray(data)
            bad[offset] ^= 1
            with self.assertRaises((ValueError, KeyError), msg=str(offset)):
                codec.decode_binary(bytes(bad))
        for length in [0, 1, 31, 32, len(data) - 1]:
            with self.assertRaises(ValueError):
                codec.decode_binary(data[:length])
        # A torn final record may leave a full-size image, but is not complete.
        bad = bytearray(data)
        bad[stats['committed_bytes'] - 20:] = b'\xff' * (len(data) - stats['committed_bytes'] + 20)
        with self.assertRaises(ValueError):
            codec.decode_binary(bytes(bad))


if __name__ == '__main__':
    unittest.main()
