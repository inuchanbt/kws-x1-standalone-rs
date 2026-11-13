"""KWS logger comparison: adopt an already running 20 V / 0.5 A AVS load.

Uses the installed ASD CLI's transport, decoder, protections and CY4500 backend.
No protocol entry, 5 V prime, initial LOAD_OFF, or automatic PDO handoff.
"""
import argparse
import hashlib
import importlib.util
import math
import queue
from pathlib import Path
import sys
import tempfile
import threading
import time

CLI_SHA256 = "860f4b18702376e9283cf60d74efbc69e516dbcbfd088480f5f4c9fab778eb99"
TARGETS = (20, 22, 24, 26, 28, 26, 24, 22, 20)
CURRENT = 0.5
HOLD = 3.0


def panel_object(objects, index):
    """Resolve the touch-panel ordinal, omitting empty native PDO slots."""
    visible = sorted((obj for obj in objects if obj.raw_u32 not in (0, 0xFFFFFFFF)),
                     key=lambda obj: obj.object_number)
    if not 1 <= index <= len(visible):
        raise RuntimeError(f"Panel OBJ{index} unavailable; output left unchanged.")
    return visible[index - 1]


def load_cli(path):
    path = Path(path).resolve()
    if hashlib.sha256(path.read_bytes()).hexdigest() != CLI_SHA256:
        raise RuntimeError("ASD CLI changed since review; revalidate this adapter before use.")
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("asd_logger_upstream", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def verify_initial(m):
    values = (m.voltage_v, m.current_a, m.power_w)
    if any(v is None or not math.isfinite(v) for v in values):
        raise RuntimeError("Initial measurement invalid; existing output left unchanged.")
    if not (19 <= m.voltage_v <= 21 and 0.4 <= m.current_a <= 0.6 and 0 <= m.power_w <= 20):
        raise RuntimeError("Set the selected panel EPR AVS object to 20 V / 0.5 A with LOAD ON first; output left unchanged.")


def wait_for_enter(message, monitor):
    # Read the console's normal stdin stream, including terminal/remote input.
    # Do not discard queued keystrokes on entry.
    print("\n" + message, flush=True)
    received = queue.Queue()

    def read_line():
        try:
            received.put(sys.stdin.readline())
        except Exception as error:
            received.put(error)

    threading.Thread(target=read_line, daemon=True).start()
    while True:
        monitor()
        try:
            line = received.get(timeout=0.5)
        except queue.Empty:
            continue
        if isinstance(line, Exception):
            raise line
        if line == "":
            raise EOFError("Console input closed while waiting for confirmation.")
        print("[LOGGER] Enter received; continuing.", flush=True)
        return


def wait_for_countdown(seconds, monitor):
    deadline = time.monotonic() + seconds
    last = None
    while True:
        remaining = max(0, math.ceil(deadline - time.monotonic()))
        if remaining != last:
            print(f"[START IN {remaining}s] Start KWS recording now. No Enter needed.", flush=True)
            last = remaining
        monitor()
        if time.monotonic() >= deadline:
            return
        time.sleep(min(0.5, max(0, deadline - time.monotonic())))


def sweep_session(api, engine, args, csv_logger, wait=wait_for_enter):
    # An explicit read command avoids native query helpers that re-enter PD3.1.
    response = engine.send_body(bytes.fromhex("01 10 14 01 00"), label="LOGGER_READ_PDO", read_window=0.5)
    if not api.crc_ok(response):
        raise RuntimeError("PDO response CRC invalid; no power command sent.")
    objects = api.decode_pdo_response(response)
    meta = api.analyze_pdo_visibility(objects)
    panel_index = getattr(args, "logger_panel_object", 9)
    selected = panel_object(objects, panel_index)
    plan = api.plan_apdo_sweep(objects, list(TARGETS), CURRENT,
                              object_indices=[selected.object_number], current_policy="skip")
    if len(plan) != 1 or plan[0][0].kind != api.PdoKind.APDO_EPR_AVS.value or meta.get("epr_missing_suspected"):
        raise RuntimeError(f"Panel OBJ{panel_index} must be EPR AVS covering the sweep; output left unchanged.")
    obj = plan[0][0]
    verify_initial(engine.measure_once())
    print(f"[LOGGER] Panel OBJ{panel_index} -> native Object {obj.object_number} "
          f"-> EPR AVS gear 0x{api.pdo_object_to_internal_gear(obj):02X}. Measured 20 V / 0.5 A.", flush=True)
    guard = api.Cy4500Guard(engine, args._cy4500_controller)
    guard.install()
    # Deliberately no guard.verify_path(): its zero-load 5 V probe resets the DUT.
    # First changed target (22 V) must still provide live Request/PS_RDY evidence.
    target = 20
    phase = "logger-ready"

    def sample():
        guard.check("logger hold")
        m = api.measure_pd_load(engine, args, target, CURRENT)
        api.check_measurement_safety(engine, args, m, target, phase, target_current_a=CURRENT)
        api.write_csv_row(csv_logger, engine, args, {
            "timestamp": api.now_iso(), "mode": phase,
            **api.csv_runtime_context(args),
            "target_voltage_v": target, "target_load_current_a": CURRENT,
            "request_current_a": CURRENT,
            "actual_voltage_v": m.voltage_v, "actual_current_a": m.current_a,
            "actual_power_w": m.power_w, "ripple_mv": m.ripple_mv,
            "pdo_kind": obj.kind, "pdo_object_number": obj.object_number,
            "pdo_raw_u32": f"0x{obj.raw_u32:08X}",
            "raw_measure_response": api.hex_bytes(m.raw_response),
        })

    try:
        # From this point the session owns cleanup, including Ctrl+C and EOF/errors.
        start_delay = getattr(args, "logger_start_delay", None)
        if start_delay is None:
            wait("[READY] Start KWS logger, then press Enter to sweep. LOAD remains ON.", sample)
        else:
            wait_for_countdown(start_delay, sample)
        print("[SWEEP START] Beginning 20 -> 28 -> 20 V sweep.", flush=True)
        for index, target in enumerate(TARGETS):
            phase = "logger-outbound" if index <= 4 else "logger-return"
            api.set_sweep_leg_context(args, "outbound" if index <= 4 else "return", 1 if index <= 4 else 2,
                                     20 if index <= 4 else 28, 28 if index <= 4 else 20)
            if index:
                # Initial 20 V is observed only; the first request changes to 22 V.
                print(f"[REQUEST] Setting EPR AVS to {target} V / {CURRENT} A", flush=True)
                engine.set_avs(target, CURRENT, CURRENT, pdo_index=obj.object_number,
                               internal_gear_override=api.pdo_object_to_internal_gear(obj))
                api.prepare_pd_load_voltage(engine, args, target, CURRENT)
                time.sleep(args.continuous_settle)
            started = time.monotonic()
            print(f"[LOGGER] {target} V / {CURRENT} A ({index+1}/{len(TARGETS)})", flush=True)
            sample()
            while time.monotonic() - started < HOLD:
                time.sleep(min(0.5, max(0, HOLD - (time.monotonic() - started))))
                sample()
        phase = "logger-stop-wait"
        wait("[DONE] Stop KWS logger; wait for LOGGER STOPPED. Then press Enter to turn LOAD OFF and save CY4500.", sample)
    finally:
        active_error = sys.exc_info()[1]
        try:
            engine.load_off()
        except Exception as error:
            if active_error is None:
                raise
            print(f"[LOGGER] LOAD_OFF also failed: {error}", file=sys.stderr)
        finally:
            guard.restore()


def workflow(api, args):
    api.initialize_asd_version_metadata(args)
    protocol = api.ProtocolDictionary.load(args.protocol_csv)
    with api.ProtocolEngine(port=args.port, baud=args.baud, timeout=args.timeout,
                            protocol_dict=protocol, verbose=not args.quiet,
                            protocol_entry_raw=args.protocol_entry_raw,
                            protocol_entry_raw_wait=args.protocol_entry_raw_wait,
                            cc_line_selection=args.cc_line_selection) as engine:
        engine.measure_read_window = 0.08
        engine.measure_retry = 0
        engine.measure_retry_delay = 0.05
        api.install_load_voltage_controller(engine, args)
        with api.CsvLogger(args.csv, api.CSV_FIELDS, mode=args.csv_mode) as csv_logger:
            sweep_session(api, engine, args, csv_logger)
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", default="COM3")
    parser.add_argument("--start-delay", type=float,
                        help="Start automatically after this many seconds; no initial Enter")
    parser.add_argument("--verbose", action="store_true", help="Show raw serial traffic")
    parser.add_argument("--panel-object", type=int, default=9,
                        help="Touch-panel PDO ordinal excluding empty slots (default: 9)")
    parser.add_argument("--cable-set", choices=("cc1", "cc2"), default="cc1")
    parser.add_argument("--asd-cli", type=Path, default=Path(r"D:\ASD-PD31\src\current\asd_pd31_cli.py"))
    parser.add_argument("--output-dir", type=Path, default=Path(__file__).resolve().parents[2] / "CY4500_logs")
    parser.add_argument("--plan", action="store_true", help="Print the sequence without connecting to hardware")
    options = parser.parse_args()
    if options.start_delay is not None and (not math.isfinite(options.start_delay) or options.start_delay < 0):
        parser.error("--start-delay must be finite and nonnegative")
    if options.plan:
        print(f"Manually establish panel OBJ{options.panel_object} EPR AVS 20 V / 0.5 A, LOAD ON.\n"
              f"Read-only preflight -> {'READY/Enter' if options.start_delay is None else str(options.start_delay) + 's countdown (no Enter)'} -> 20,22,24,26,28,26,24,22,20 V\n"
              "Each point >=3 s; DONE/Enter after LOGGER STOPPED -> LOAD_OFF -> CY4500 export.\n"
              "Errors/Ctrl+C after adoption still turn LOAD OFF immediately.")
        return 0
    if not sys.stdin.isatty():
        parser.error("Run in an interactive PowerShell console (Enter checkpoints required).")
    api = load_cli(options.asd_cli)
    options.output_dir.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="kws_avs_", dir=options.output_dir))
    print(f"[LOGGER] New output folder: {directory}", flush=True)
    # Reuse the original parser/validation and capture start/stop/export lifecycle.
    # Only the hardware workflow is replaced in this process; source files are untouched.
    def run_workflow(args):
        args.logger_panel_object = options.panel_object
        args.logger_start_delay = options.start_delay
        return workflow(api, args)
    api._run_workflow = run_workflow
    return api.main([
        "load", "--port", options.port, "--cable-set", options.cable_set,
        "--apdo-sweep", "20:28:2:0.5", "--apdo-objects", "11",
        "--continuous-sweep", "--round-trip-sweep", "--current-policy", "skip",
        "--request-current", "0.5", "--power-limit", "20", "--actual-current-limit", "0.7",
        "--no-condition-log",
        *([] if options.verbose else ["--quiet"]),
        "--cy4500-capture", "--cy4500-backend", "cli", "--cy4500-required",
        "--csv", str(directory / "ASD.csv"), "--cy4500-csv", str(directory / "CY4500.csv"),
    ])


if __name__ == "__main__":
    raise SystemExit(main())
