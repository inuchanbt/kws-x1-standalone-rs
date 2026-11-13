"""Offline adapter tests against the reviewed ASD CLI; never open COM or USB."""
import contextlib
import io
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

sys.dont_write_bytecode = True
import kws_avs_sweep as sweep


class SessionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.api = sweep.load_cli(Path(r"D:\ASD-PD31\src\current\asd_pd31_cli.py"))

    def setUp(self):
        api = self.api
        self.args = api.build_parser().parse_args([
            "--power-limit", "20", "--actual-current-limit", "0.7", "--no-condition-log"])
        self.controller = Mock(analyzer_name="CY4500")
        self.controller.health_snapshot.return_value = {}
        self.args._cy4500_controller = self.controller
        self.events = []
        self.voltage = 20
        self.current = 0.5
        # Constructor only; open() is never called. pyserial is unnecessary here.
        with patch.object(sys.modules[api.ProtocolEngine.__module__], "serial", Mock()):
            self.engine = api.ProtocolEngine(port="FAKE", protocol_dict=Mock())
        self.engine.send_frame = Mock(side_effect=self.frame)
        self.engine.measure_once = Mock(side_effect=lambda: SimpleNamespace(
            voltage_v=self.voltage, current_a=self.current, power_w=self.voltage*self.current,
            ripple_mv=0, raw_response=b""))
        original = self.engine.set_avs

        def request(voltage, *args, **kwargs):
            self.voltage = voltage
            self.events.append(("request", voltage))
            return original(voltage, *args, **kwargs)
        self.engine.set_avs = request
        self.obj = api.PdoObject(object_number=11, raw_u32=0xD7C096F0,
                                kind=api.PdoKind.APDO_EPR_AVS.value,
                                min_voltage_v=15, max_voltage_v=48, current_a=5, power_w=240)
        self.clock = 0.0
        self.objects = [api.PdoObject(object_number=n, raw_u32=1,
                                     kind=api.PdoKind.FIXED.value)
                        for n in (1, 2, 3, 4, 5, 8, 9, 10)] + [self.obj]
        self.objects += [api.PdoObject(object_number=n, raw_u32=0,
                                      kind=api.PdoKind.UNKNOWN.value) for n in (6, 7)]

    def frame(self, frame, *args, **kwargs):
        if frame[:5] == bytes.fromhex("01 11 04 03 00"):
            self.events.append("OFF")
        elif frame[:2] == b"\x01\x11" and frame[:4] != bytes.fromhex("01 11 14 0C"):
            self.fail(f"Unexpected write: {frame.hex()}")
        return self.api.append_crc(b"test-pdo-fixture")

    def sleep(self, duration):
        self.clock += duration

    def run_session(self, wait=None):
        if wait is None:
            def wait(message, monitor):
                self.events.append("READY" if "[READY]" in message else "DONE")
                self.assertNotIn("OFF", self.events)
                monitor()
        with contextlib.ExitStack() as stack:
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            stack.enter_context(patch.object(self.api, "decode_pdo_response", return_value=self.objects))
            stack.enter_context(patch.object(self.api, "analyze_pdo_visibility", return_value={"has_avs": True}))
            stack.enter_context(patch.object(self.api, "prepare_pd_load_voltage"))
            stack.enter_context(patch.object(self.api, "write_csv_row"))
            stack.enter_context(patch.object(sweep.time, "monotonic", side_effect=lambda: self.clock))
            stack.enter_context(patch.object(sweep.time, "sleep", side_effect=self.sleep))
            sweep.sweep_session(self.api, self.engine, self.args, None, wait=wait)

    def test_normal_run_has_no_off_until_stop_checkpoint(self):
        self.run_session()
        self.assertEqual(self.events, ["READY"] + [("request", v) for v in sweep.TARGETS[1:]] + ["DONE", "OFF"])
        self.assertEqual(self.controller.wait_for_pd_activity.call_count, 8)
        self.assertGreaterEqual(self.clock, 27)
        self.assertIsNone(self.engine.cy4500_guard)

    def test_countdown_starts_without_initial_enter_and_retains_stop_wait(self):
        self.args.logger_start_delay = 10
        waits = []
        def wait(message, monitor):
            waits.append(message)
            self.assertIn("[DONE]", message)
            self.assertNotIn("OFF", self.events)
            monitor()
        self.run_session(wait)
        self.assertEqual(len(waits), 1)
        self.assertEqual(self.events, [("request", v) for v in sweep.TARGETS[1:]] + ["OFF"])
        self.assertGreaterEqual(self.clock, 37)

    def test_normal_stdin_enter_is_accepted_and_eof_is_an_error(self):
        with contextlib.redirect_stdout(io.StringIO()):
            monitor = Mock()
            with patch.object(sys, "stdin", io.StringIO("\n")):
                sweep.wait_for_enter("test", monitor)
            monitor.assert_called()
            with patch.object(sys, "stdin", io.StringIO("")):
                with self.assertRaises(EOFError):
                    sweep.wait_for_enter("test", monitor)

    def test_countdown_fault_stops_before_any_voltage_request(self):
        self.args.logger_start_delay = 10
        # Measurement fault during the countdown, after successful preflight.
        original_measure = self.engine.measure_once.side_effect
        calls = [0]
        def measure():
            calls[0] += 1
            if calls[0] > 1:
                self.current = 1.0
            return original_measure()
        self.engine.measure_once.side_effect = measure
        with self.assertRaises(self.api.SafetyAbort):
            self.run_session()
        self.assertTrue(self.events)
        self.assertTrue(all(event == "OFF" for event in self.events))

    def test_panel_9_resolves_native_11_and_sends_avs_gear(self):
        self.assertIs(sweep.panel_object(self.objects, 9), self.obj)
        self.run_session()
        frames = [call.args[0] for call in self.engine.send_frame.call_args_list
                  if call.args[0][:4] == bytes.fromhex("01 11 14 0C")]
        self.assertEqual(len(frames), 8)
        self.assertTrue(all(frame[9] == 0x12 for frame in frames))

    def test_panel_fixed_object_rejected_without_writes(self):
        self.args.logger_panel_object = 8
        with self.assertRaises((RuntimeError, ValueError)):
            self.run_session()
        self.assertEqual(self.events, [])

    def test_wrong_initial_load_is_read_only(self):
        self.current = 0
        with self.assertRaisesRegex(RuntimeError, "left unchanged"):
            self.run_session()
        self.assertEqual(self.events, [])

    def test_initial_nonfinite_measurement_is_read_only(self):
        self.voltage = float("nan")
        with self.assertRaisesRegex(RuntimeError, "invalid"):
            self.run_session()
        self.assertEqual(self.events, [])

    def test_interrupt_during_ready_still_turns_load_off(self):
        def interrupt(*args):
            raise KeyboardInterrupt
        with self.assertRaises(KeyboardInterrupt):
            self.run_session(interrupt)
        self.assertEqual(self.events, ["OFF"])

    def test_missing_pd_evidence_stops_before_second_target(self):
        self.controller.wait_for_pd_activity.side_effect = RuntimeError("no PD evidence")
        with self.assertRaises(self.api.Cy4500SafetyError):
            self.run_session()
        self.assertEqual([e for e in self.events if isinstance(e, tuple)], [("request", 22)])
        self.assertIn("OFF", self.events)

    def test_stop_wait_keeps_monitoring_and_fault_stops_load(self):
        def wait(message, monitor):
            if "[DONE]" in message:
                self.current = 1.0
            monitor()
        with self.assertRaises(self.api.SafetyAbort):
            self.run_session(wait)
        self.assertIn("OFF", self.events)

    def test_launcher_parser_capture_lifecycle_and_unique_retry_paths(self):
        api = self.api
        captured = []
        def fake_workflow(module, args):
            self.assertIs(module, api)
            self.assertTrue(args.cy4500_required)
            self.assertEqual(args.actual_current_limit, 0.7)
            captured.append(args.csv)
            return 0
        with tempfile.TemporaryDirectory() as directory, contextlib.ExitStack() as stack:
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            stack.enter_context(patch.object(sys, "argv", ["launcher", "--output-dir", directory]))
            stack.enter_context(patch.object(sys.stdin, "isatty", return_value=True))
            stack.enter_context(patch.object(sweep, "load_cli", return_value=api))
            stack.enter_context(patch.object(sweep, "workflow", side_effect=fake_workflow))
            stack.enter_context(patch.object(api, "_run_workflow"))
            stack.enter_context(patch.object(api, "create_cy4500_controller", return_value=self.controller))
            stack.enter_context(patch.object(sweep.time, "sleep"))
            self.assertEqual(sweep.main(), 0)
            self.assertEqual(sweep.main(), 0)
        self.assertNotEqual(captured[0], captured[1])
        self.assertEqual(self.controller.start.call_count, 2)
        self.assertEqual(self.controller.stop.call_count, 2)
        self.assertEqual(self.controller.export_csv.call_count, 2)


if __name__ == "__main__":
    unittest.main()
