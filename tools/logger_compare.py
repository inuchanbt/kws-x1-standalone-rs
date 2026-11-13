"""Reproduce the published KWS/CY4500 AVS capture comparison (no hardware I/O)."""
import csv
import json
from pathlib import Path
import sys


def compare(directory):
    directory = Path(directory)
    samples, fragments, notices = [], {}, []
    for path in sorted(directory.glob("LOG*.CSV")):
        for row in csv.DictReader(path.open(encoding="utf-8")):
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
          for r in csv.DictReader((directory / "CY4500.csv").open())
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
    return {"samples": len(samples), "last_sample_ms": times[-1],
            "sample_interval_ms": [min(intervals), max(intervals)],
            "kws_packets": len(kws), "cy_packets": len(cy),
            "request_count": len(kr), "matched_overlap_packets": len(matched),
            "unmatched_kws_in_overlap": hi_k-lo_k+1-len(matched),
            "unmatched_cy_in_overlap": hi_c-lo_c+1-len(matched),
            "clock_scale": slope, "clock_offset_ms": offset,
            "max_residual_ms": max(m[2] for m in matched), "notices": notices,
            "limitation": "One capture; matching uses type, MessageID and payload, not analog waveform/CRC or role bits. Clock fit is not calibration."}


if __name__ == "__main__":
    directory = sys.argv[1] if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "tests/data/logger-avs-20261113"
    print(json.dumps(compare(directory), indent=2))
