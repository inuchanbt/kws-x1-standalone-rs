"""Compare independent KWS/ASD snapshots by ordered AVS step, without a clock fit."""
import math
from statistics import median

TARGETS = [20, 22, 24, 26, 28, 26, 24, 22, 20]


def compare_asd(samples, requests, rows, settle_ms=1000):
    """Use KWS Request boundaries and ASD phase/target order; never pair timestamps.

    Limits select steady snapshots, not instrument accuracy or USB-PD compliance.
    Explicitly exclude low-current transition samples from steady medians.
    """
    groups = []
    for row in rows:
        mode = row.get("mode")
        if mode not in ("logger-outbound", "logger-return"):
            continue
        target = float(row["target_voltage_v"])
        if not math.isfinite(target):
            raise ValueError("ASD target voltage is not finite")
        key = (mode, target)
        if not groups or groups[-1][0] != key:
            groups.append((key, []))
        groups[-1][1].append(row)
    expected = [("logger-outbound" if i < 5 else "logger-return", v)
                for i, v in enumerate(TARGETS)]
    if [key for key, _ in groups] != expected or len(requests) != 8:
        return {"status": "unavailable", "reason": "Expected the ordered nine-point AVS route and eight KWS requests"}
    starts = [None] + [r[3] for r in requests]
    ends = [r[3] for r in requests] + [None]
    if any(b <= a for a, b in zip(starts[1:], starts[2:])):
        raise ValueError("KWS Request timestamps are not increasing")
    points = []
    for i, ((mode, target), asd_rows) in enumerate(groups):
        currents = [float(r["target_load_current_a"]) for r in asd_rows]
        if not all(math.isfinite(c) and c == currents[0] for c in currents):
            raise ValueError("ASD target current changes within a voltage step")
        current = currents[0]
        kws_rows = [r for r in samples
                    if (starts[i] is None or r["elapsed_ms"] >= starts[i])
                    and (ends[i] is None or r["elapsed_ms"] < ends[i])]
        kws_time_ok = [r for r in kws_rows
                       if starts[i] is None or r["elapsed_ms"] >= starts[i] + settle_ms]

        def steady(voltage, amperes):
            return (math.isfinite(voltage) and math.isfinite(amperes)
                    and abs(voltage - target) <= 0.5 + 1e-12
                    and abs(amperes - current) <= 0.05 + 1e-12)

        kws = [(r["voltage_mV"] / 1000, r["current_mA"] / 1000) for r in kws_time_ok
               if steady(r["voltage_mV"] / 1000, r["current_mA"] / 1000)]
        asd = [(float(r["actual_voltage_v"]), float(r["actual_current_a"])) for r in asd_rows]
        asd = [(v, c) for v, c in asd if steady(v, c)]
        point = {
            "step": i + 1, "leg": "outbound" if i < 5 else "return",
            "target_voltage_v": target, "target_current_a": current,
            "kws_request_start_ms": starts[i], "kws_next_request_ms": ends[i],
            "kws_samples_total": len(kws_rows), "kws_samples_used": len(kws),
            "kws_excluded_settle": len(kws_rows) - len(kws_time_ok),
            "kws_excluded_outside_steady_window": len(kws_time_ok) - len(kws),
            "asd_samples_total": len(asd_rows), "asd_samples_used": len(asd),
            "asd_excluded_outside_steady_window": len(asd_rows) - len(asd),
            "kws_voltage_median_v": median(v for v, _ in kws) if kws else None,
            "asd_voltage_median_v": median(v for v, _ in asd) if asd else None,
            "kws_current_median_a": median(c for _, c in kws) if kws else None,
            "asd_current_median_a": median(c for _, c in asd) if asd else None,
        }
        for metric, scale, unit in (("voltage", 1000, "mv"), ("current", 1000, "ma")):
            # Scale both units to milli-units. The voltage keys use v and current a.
            raw_unit = "v" if metric == "voltage" else "a"
            a = point[f"kws_{metric}_median_{raw_unit}"]
            b = point[f"asd_{metric}_median_{raw_unit}"]
            point[f"kws_minus_asd_{metric}_{unit}"] = round((a - b) * scale, 6) if a is not None and b is not None else None
        points.append(point)
    valid = [p for p in points if p["kws_minus_asd_voltage_mv"] is not None]
    return {
        "status": "compared" if len(valid) == 9 else "partial",
        "method": "Independent per-step medians: KWS PD Request windows; ASD ordered phase/target groups. No cross-device timestamp pairing.",
        "selection": {"kws_after_request_settle_ms": settle_ms,
                      "target_voltage_window_v": 0.5, "target_current_window_a": 0.05,
                      "asd_excluded_modes": ["logger-ready", "logger-stop-wait"]},
        "compared_steps": len(valid),
        "max_abs_voltage_median_difference_mv": max((abs(p["kws_minus_asd_voltage_mv"]) for p in valid), default=None),
        "max_abs_current_median_difference_ma": max((abs(p["kws_minus_asd_current_ma"]) for p in valid), default=None),
        "steps": points,
        "limitation": "Different measurement positions and unsynchronized snapshots; differences include wiring drop and meter offsets. Not calibration, absolute accuracy, transition timing or a compliance verdict. Requires the same physical run.",
    }
