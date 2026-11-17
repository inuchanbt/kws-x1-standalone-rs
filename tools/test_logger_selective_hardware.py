"""Published hardware comparison and PC-only negative/synthetic controls."""
import csv
import hashlib
import json
from pathlib import Path
import shutil
import struct
import tempfile
import unittest

from logger_native_verify import checksum, verify
from logger_selective_compare import compare

FIXTURE = Path(__file__).resolve().parents[1] / 'tests/data/logger-selective-hardware-20261117'


class SelectiveHardwareTests(unittest.TestCase):
    def test_public_hashes_and_reproduced_result(self):
        for line in (FIXTURE / 'SHA256SUMS').read_text().splitlines():
            digest, name = line.split('  ', 1)
            self.assertEqual(hashlib.sha256((FIXTURE / name).read_bytes()).hexdigest(), digest, name)
        self.assertEqual(compare(FIXTURE), json.loads((FIXTURE / 'pd-comparison.json').read_text()))

    def test_header_role_change_and_missing_accept_are_rejected(self):
        for mutation in ('role', 'missing'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as name:
                folder = Path(name)
                for source in FIXTURE.glob('*'):
                    if source.is_file():
                        shutil.copyfile(source, folder / source.name)
                path = folder / 'PD000301.CSV'
                with path.open(newline='') as f:
                    reader = csv.DictReader(f)
                    fields, rows = reader.fieldnames, list(reader)
                index = next(i for i, r in enumerate(rows)
                             if int(r['event_tag']) & 255 == 0
                             and int(r['raw_u32']) & 31 == 3)
                if mutation == 'role':
                    rows[index]['raw_u32'] = str(int(rows[index]['raw_u32']) ^ 0x100)
                else:
                    rows.pop(index)
                with path.open('w', newline='') as f:
                    writer = csv.DictWriter(f, fieldnames=fields)
                    writer.writeheader()
                    writer.writerows(rows)
                with self.assertRaises(ValueError):
                    compare(folder)

    def test_cy_error_flag_and_truncated_raw_are_rejected(self):
        for mutation in ('error', 'truncated'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as name:
                folder = Path(name)
                for source in FIXTURE.glob('*'):
                    if source.is_file():
                        shutil.copyfile(source, folder / source.name)
                path = folder / 'CY4500.records.jsonl'
                rows = [json.loads(line) for line in path.read_text().splitlines()]
                packet = next(r for r in rows if r['fields']['PKT_TYPE'] == 0)
                if mutation == 'error':
                    packet['fields']['CRC_ERROR'] = 1
                else:
                    packet['raw_hex'] = packet['raw_hex'][:-2]
                path.write_text(''.join(json.dumps(r) + '\n' for r in rows))
                with self.assertRaises(ValueError):
                    compare(folder)

    def test_native_crc_signed_csv_and_protected_byte_checks(self):
        def words(a, b, c):
            prefix = struct.pack('<III', a, b, c)
            return prefix + struct.pack('<I', checksum(prefix))

        with tempfile.TemporaryDirectory() as name:
            folder = Path(name)
            baseline = bytearray(b'\xff' * 49152)
            baseline[0] = 0x12  # Existing unknown occupied sector.
            previous = folder / 'before.bin'
            previous.write_bytes(baseline)
            raw = bytearray(baseline)
            raw[8192:8224] = words(0x314c5358, 1, 0) + words(7, 5100, 0xffffffff)
            path = folder / 'STORAGE.BIN'
            path.write_bytes(raw)
            (folder / 'VI000101.CSV').write_text(
                'session,elapsed_ms,voltage_mV,current_mA\n1,7,5100,-1\n')
            self.assertTrue(verify(folder, previous)['raw_csv_rows_equal'])
            raw[8210] ^= 1
            path.write_bytes(raw)
            with self.assertRaisesRegex(ValueError, 'CRC'):
                verify(folder, previous)
            raw[8210] ^= 1
            raw[0] ^= 1
            path.write_bytes(raw)
            with self.assertRaisesRegex(ValueError, 'protected sector'):
                verify(folder, previous)


if __name__ == '__main__':
    unittest.main()
