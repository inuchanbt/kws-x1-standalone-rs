"""Compare KWS PD with CY4500, and steady VI snapshots with ASD (no device I/O)."""
import argparse
import csv
import json
from pathlib import Path
from logger_analog_compare import compare_asd


def read_csv(path):
    with Path(path).open(encoding="utf-8-sig", newline="") as stream:
        return list(csv.DictReader(stream))


def compare(directory, session=None, reference_dir=None):
    directory = Path(directory)
    reference_dir = Path(reference_dir) if reference_dir is not None else directory
    samples, fragments, notices = [], {}, []
    sessions = set()
    for path in sorted({p for pattern in ("LOG*.CSV", "VI*.CSV", "PD*.CSV") for p in directory.glob(pattern)}):
        for row in read_csv(path):
            if session is not None and int(row["session"]) != session:
                continue
            sessions.add(int(row["session"]))
            if "voltage_mV" in row:
                samples.append({key: int(value) for key, value in row.items()})
                continue
            tag = int(row["event_tag"])
            if tag >> 8 == 0 or tag & 255 == 9:
                notices.append(row)
                continue
            key = (int(row["session"]), tag >> 8)
            parts = fragments.setdefault(key, {})
            if tag & 255 in parts:
                raise ValueError("Duplicate event fragment")
            parts[tag & 255] = (int(row["elapsed_ms"]), int(row["raw_u32"]))
    if len(sessions) != 1:
        raise ValueError("Select one KWS recording with --session (no matching session, or multiple sessions present)")
    names = {(1, 1, 16): "EXTENDED_CONTROL_MSG", (0, 0, 1): "GOODCRC",
             (0, 2, 9): "EPR_REQUEST", (0, 0, 3): "ACCEPT", (0, 0, 6): "PS_RDY"}
    kws = []
    for parts in fragments.values():
        timestamp, raw = parts[0]
        header = raw & 65535
        count = header >> 12 & 7
        if set(parts) != set(range(count + 1)):
            raise ValueError("Incomplete PD packet")
        if raw >> 16 != 7:
            raise ValueError("This comparison expects SOP packets")
        name = names[(header >> 15, count, header & 31)]
        payload = b"".join(parts[i][1].to_bytes(4, "little") for i in range(1, count + 1)).hex()
        kws.append((name, header >> 9 & 7, payload, timestamp))
    kws.sort(key=lambda row: row[3])
    cy = [(r["Message"], int(r["Msg Id"]), r["Data"].replace(" ", "").lower(),
           int(r["Start Time"]) / 1000)
          for r in read_csv(reference_dir / "CY4500.csv")
          if r["Message"] != "VOLT_PKT"]
    # Clock fit uses the ordered voltage-change requests, then validates all
    # packet signatures in the overlap with a fixed 10 ms residual limit.
    kr = [r for r in kws if r[0] == "EPR_REQUEST"]
    cr = [r for r in cy if r[0] == "EPR_REQUEST"]
    if len(kr) < 2 or [r[:3] for r in kr] != [r[:3] for r in cr]:
        raise ValueError("Request sequences differ or cannot establish clock fit")
    x, y = [r[3] for r in cr], [r[3] for r in kr]
    mx, my = sum(x) / len(x), sum(y) / len(y)
    slope = sum((a - mx) * (b - my) for a, b in zip(x, y)) / sum((a - mx)**2 for a in x)
    offset = my - slope * mx
    used, matched = set(), []
    for i, row in enumerate(kws):
        candidates = [(abs(row[3] - (offset + slope * ref[3])), j)
                      for j, ref in enumerate(cy) if j not in used and row[:3] == ref[:3]]
        if candidates:
            residual, j = min(candidates)
            if residual < 10:
                used.add(j)
                matched.append((i, j, residual))
    if not matched:
        raise ValueError("No common packets")
    lo_k, hi_k = min(m[0] for m in matched), max(m[0] for m in matched)
    lo_c, hi_c = min(m[1] for m in matched), max(m[1] for m in matched)
    samples.sort(key=lambda r: r["elapsed_ms"])
    times = [r["elapsed_ms"] for r in samples]
    intervals = [b-a for a, b in zip(times, times[1:])]
    analog = (compare_asd(samples, kr, read_csv(reference_dir / "ASD.csv"))
              if (reference_dir / "ASD.csv").exists()
              else {"status": "unavailable", "reason": "No ASD.csv in reference directory"})
    return {"samples": len(samples), "last_sample_ms": times[-1],
            "sample_interval_ms": [min(intervals), max(intervals)],
            "kws_packets": len(kws), "cy_packets": len(cy),
            "request_count": len(kr), "matched_overlap_packets": len(matched),
            "unmatched_kws_in_overlap": hi_k-lo_k+1-len(matched),
            "unmatched_cy_in_overlap": hi_c-lo_c+1-len(matched),
            "clock_scale": slope, "clock_offset_ms": offset,
            "max_residual_ms": max(m[2] for m in matched), "notices": notices,
            "session": next(iter(sessions)), "asd": analog,
            "limitation": "One capture; matching uses type, MessageID and payload, not analog waveform/CRC or role bits. Clock fit is not calibration."}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", nargs="?", type=Path,
                        default=Path(__file__).resolve().parents[1] / "tests/data/logger-avs-20261113")
    parser.add_argument("--session", type=int, help="Select one recording ID from a multi-session export")
    parser.add_argument("--reference-dir", type=Path, help="Same-run analyzer folder containing CY4500.csv and ASD.csv")
    options = parser.parse_args()
    print(json.dumps(compare(options.directory, options.session, options.reference_dir), indent=2))
