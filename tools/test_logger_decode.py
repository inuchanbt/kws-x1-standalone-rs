import csv
import tempfile
import unittest
from pathlib import Path
from logger_decode import decode

class DecodeTests(unittest.TestCase):
    def test_split_packet_missing_payload_unsigned_word_and_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = []
            # PDO payload split across physical sectors, plus an interrupted frame.
            groups = [[(3, 100, 256, 0x71e82), (3, 200, 512, 0x71e82)],
                      [(3, 100, 257, 0xffffffff), (3, 250, 8, 4), (3, 260, 777, 1)]]
            for index, rows in enumerate(groups):
                path = Path(directory) / f"LOG{index}.CSV"
                with path.open("w", newline="") as stream:
                    writer = csv.writer(stream)
                    writer.writerow(["session", "elapsed_ms", "event_tag", "raw_u32"])
                    writer.writerows(rows)
                paths.append(path)
            rows = decode(list(reversed(paths)))
            self.assertEqual(rows[0]["status"], "COMPLETE")
            self.assertEqual(rows[0]["data_hex"], "FFFFFFFF")
            self.assertEqual(rows[0]["message_id"], 7)
            self.assertEqual(rows[0]["sop"], "SOP")
            self.assertEqual(rows[1]["status"], "INCOMPLETE")
            self.assertEqual(rows[2]["count"], 4)
            self.assertEqual(rows[3]["kind"], "SNIFFER_STOPPED")
            with self.assertRaises(ValueError):
                decode(paths + paths)

if __name__ == "__main__":
    unittest.main()
