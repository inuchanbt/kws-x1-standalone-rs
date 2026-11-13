"""The published capture must compare identically with legacy and typed names."""
from pathlib import Path
import shutil
import tempfile
import unittest
from logger_compare import compare
from logger_analog_compare import TARGETS, compare_asd
import csv
import hashlib


class FilenameTests(unittest.TestCase):
    def test_observer_v07_fixture_and_input_checksums(self):
        source = Path(__file__).resolve().parents[1] / "tests/data/logger-observer-v07-avs-20261113"
        for line in (source / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
            digest, name = line.split("  ", 1)
            self.assertEqual(hashlib.sha256((source / name).read_bytes()).hexdigest(), digest, name)
        result = compare(source, session=1)
        self.assertEqual(result["samples"], 56)
        self.assertEqual(result["kws_packets"], 476)
        self.assertEqual(result["matched_overlap_packets"], 476)
        self.assertEqual(result["unmatched_kws_in_overlap"], 0)
        self.assertEqual(result["unmatched_cy_in_overlap"], 0)
        self.assertEqual(result["asd"]["compared_steps"], 9)
        self.assertEqual(result["asd"]["max_abs_voltage_median_difference_mv"], 15)
        self.assertEqual(result["asd"]["max_abs_current_median_difference_ma"], 2)

    def test_typed_names_preserve_the_published_packet_comparison(self):
        source = Path(__file__).resolve().parents[1] / "tests/data/logger-avs-20261113"
        with tempfile.TemporaryDirectory() as name:
            destination = Path(name)
            for old, new in [("LOG00001.CSV", "VI000101.CSV"),
                             ("LOG00002.CSV", "PD000101.CSV"),
                             ("LOG00003.CSV", "PD000102.CSV"),
                             ("CY4500.csv", "CY4500.csv")]:
                shutil.copyfile(source / old, destination / new)
            result = compare(destination)
            self.assertEqual(result["samples"], 65)
            self.assertEqual(result["matched_overlap_packets"], 516)
            self.assertEqual(result["request_count"], 8)
            self.assertEqual(result["unmatched_kws_in_overlap"], 0)
            self.assertEqual(result["unmatched_cy_in_overlap"], 0)
            self.assertEqual(result["asd"]["status"], "unavailable")

    def test_published_capture_includes_asd_comparison(self):
        source = Path(__file__).resolve().parents[1] / "tests/data/logger-avs-20261113"
        result = compare(source)
        self.assertEqual(result["asd"]["status"], "compared")
        self.assertEqual(result["asd"]["compared_steps"], 9)
        self.assertEqual([p["target_voltage_v"] for p in result["asd"]["steps"]], TARGETS)
        self.assertEqual(result["matched_overlap_packets"], 516)

    def test_session_selection_and_separate_reference_directory(self):
        source = Path(__file__).resolve().parents[1] / "tests/data/logger-avs-20261113"
        with tempfile.TemporaryDirectory() as name:
            destination = Path(name)
            for old, new in [("LOG00001.CSV", "VI001101.CSV"),
                             ("LOG00002.CSV", "PD001101.CSV"),
                             ("LOG00003.CSV", "PD001102.CSV")]:
                with (source / old).open(newline="", encoding="utf-8-sig") as f:
                    reader = csv.DictReader(f)
                    fields = reader.fieldnames
                    rows = list(reader)
                for row in rows:
                    row["session"] = "11"
                with (destination / new).open("w", newline="", encoding="utf-8") as f:
                    writer = csv.DictWriter(f, fieldnames=fields)
                    writer.writeheader()
                    writer.writerows(rows)
            (destination / "VI000101.CSV").write_text(
                "session,elapsed_ms,voltage_mV,current_mA\n1,0,5000,0\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "Select one"):
                compare(destination, reference_dir=source)
            result = compare(destination, session=11, reference_dir=source)
            self.assertEqual(result["session"], 11)
            self.assertEqual(result["samples"], 65)
            self.assertEqual(result["matched_overlap_packets"], 516)
            self.assertEqual(result["asd"]["compared_steps"], 9)


class AnalogTests(unittest.TestCase):
    @staticmethod
    def inputs():
        samples, requests, asd = [], [], []
        for i, target in enumerate(TARGETS):
            if i:
                requests.append(("EPR_REQUEST", 0, "", i * 5000))
            for elapsed in (0, 1500, 2500):
                samples.append({"elapsed_ms": i * 5000 + elapsed,
                                "voltage_mV": target * 1000 - 180, "current_mA": 502})
            asd.append({"mode": "logger-outbound" if i < 5 else "logger-return",
                        "target_voltage_v": str(target), "target_load_current_a": "0.5",
                        "actual_voltage_v": str(target - 0.2), "actual_current_a": "0.5"})
        return samples, requests, asd

    def test_ordered_legs_and_known_meter_difference_without_timestamps(self):
        result = compare_asd(*self.inputs())
        self.assertEqual(result["status"], "compared")
        self.assertEqual(result["max_abs_voltage_median_difference_mv"], 20)
        self.assertEqual(result["max_abs_current_median_difference_ma"], 2)
        self.assertEqual([p["leg"] for p in result["steps"]], ["outbound"] * 5 + ["return"] * 4)
        self.assertEqual(result["steps"][1]["kws_excluded_settle"], 1)

    def test_low_current_transients_are_counted_and_excluded(self):
        samples, requests, asd = self.inputs()
        samples[4]["current_mA"] = 0
        asd.insert(2, {**asd[1], "actual_current_a": "0"})
        result = compare_asd(samples, requests, asd)
        point = result["steps"][1]
        self.assertEqual(point["kws_excluded_outside_steady_window"], 1)
        self.assertEqual(point["asd_excluded_outside_steady_window"], 1)
        self.assertEqual(point["kws_minus_asd_current_ma"], 2)

    def test_missing_step_rejected_and_no_stable_samples_reported(self):
        samples, requests, asd = self.inputs()
        self.assertEqual(compare_asd(samples, requests, asd[:-1])["status"], "unavailable")
        asd[2]["actual_current_a"] = "nan"
        result = compare_asd(samples, requests, asd)
        self.assertEqual(result["status"], "partial")
        self.assertEqual(result["compared_steps"], 8)
        self.assertIsNone(result["steps"][2]["kws_minus_asd_voltage_mv"])


if __name__ == "__main__":
    unittest.main()
