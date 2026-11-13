"""Merge logger CSVs and reconstruct passive RX packets (Python 3, stdlib only)."""
import argparse
import csv
import glob
from pathlib import Path

FIELDS = ["session", "elapsed_ms", "kind", "event_id", "sop", "header_hex",
          "message_id", "message_type", "extended", "data_hex", "status",
          "voltage_mV", "current_mA", "count"]
SOP = {7: "SOP", 6: "SOP_PRIME", 5: "SOP_DOUBLE_PRIME", 4: "DEBUG", 3: "DEBUG_DOUBLE_PRIME"}


def decode(paths):
    rows, packets = [], {}
    for path in paths:
        with open(path, newline="", encoding="utf-8-sig") as stream:
            reader = csv.DictReader(stream)
            event = "event_tag" in (reader.fieldnames or [])
            for raw in reader:
                session, elapsed = int(raw["session"]), int(raw["elapsed_ms"])
                row = {"session": session, "elapsed_ms": elapsed}
                if not event:
                    row.update(kind="SAMPLE", voltage_mV=int(raw["voltage_mV"]), current_mA=int(raw["current_mA"]))
                    rows.append(row)
                    continue
                tag, value = int(raw["event_tag"]), int(raw["raw_u32"])
                ident, fragment = tag >> 8, tag & 255
                if fragment in (8, 9):
                    row.update(kind="QUEUE_DROPPED" if fragment == 8 else "SNIFFER_STOPPED", count=value)
                    rows.append(row)
                    continue
                if fragment > 7 or ident == 0:
                    raise ValueError(f"Invalid event tag {tag} in {path}")
                key = (session, ident)
                timestamp, parts = packets.setdefault(key, (elapsed, {}))
                if timestamp != elapsed or fragment in parts:
                    raise ValueError(f"Duplicate/conflicting event {key}; use files from one export only")
                parts[fragment] = value
    for (session, ident), (elapsed, parts) in packets.items():
        row = dict(session=session, elapsed_ms=elapsed, event_id=ident, kind="PD_RX")
        if 0 not in parts:
            row.update(status="INCOMPLETE_NO_HEADER", data_hex=" ".join(f"{k}:{v:08X}" for k,v in sorted(parts.items())))
        else:
            header, sop = parts[0] & 0xffff, parts[0] >> 16
            count = (header >> 12) & 7
            row.update(sop=SOP.get(sop, str(sop)), header_hex=f"{header:04X}",
                       message_id=(header >> 9) & 7, message_type=header & 31,
                       extended=(header >> 15) & 1,
                       data_hex=" ".join(f"{parts[i]:08X}" if i in parts else "MISSING" for i in range(1,count+1)),
                       status="COMPLETE" if set(parts) == set(range(count+1)) else "INCOMPLETE")
        rows.append(row)
    return sorted(rows, key=lambda r: (r["session"], r["elapsed_ms"], r.get("event_id", 0)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", nargs="+", help="VI*.CSV and PD*.CSV (or legacy LOG*.CSV) from one export")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    paths = sorted({Path(p).resolve() for pattern in args.inputs for p in glob.glob(pattern)})
    if not paths:
        parser.error("No input CSVs found")
    if Path(args.out).resolve() in paths:
        parser.error("Output must not overwrite an input")
    rows = decode(paths)
    with open(args.out, "w", newline="", encoding="utf-8") as out:
        writer = csv.DictWriter(out, fieldnames=FIELDS)
        writer.writeheader()
        writer.writerows(rows)
    print(f"Wrote {len(rows)} rows to {args.out}")


if __name__ == "__main__":
    main()
