# Capture / PD hardware evidence — 2026-11-16

[Report and scope](../../../docs/CAPTURE-PD-VALIDATION-20261116.md) ·
[Offline replay / instrumentation](../../../tools/capture-autotest/README.md)

| Condition | Original run | Snapshot | Images |
| --- | --- | --- | --- |
| BASE | kws_avs_uqry5xx1 | baseline/KWS_trace_snapshot.bin | 0 |
| SCREEN | kws_avs_d2euwgp4 | screen/KWS_CAPFLASH.bin | 8, 240x135 |
| TALL | kws_avs__azwvlpx | tall/KWS_CAPFLASH.bin | 8, 240x192 |

Each condition includes the verbatim ASD/CY4500 CSV exports, randomized dwell
plan, decoded trace CSV and JSON comparison. `PROVENANCE.json` records original
and distributed snapshot hashes. The BASE file is a derived trace-only snapshot:
its selected committed 8 KiB sector is unchanged at its original offset; all
other sectors are filled with 0xff to omit unrelated earlier records. SCREEN
and TALL snapshots are complete, byte-identical 48 KiB device exports.

`images/` contains all sixteen original BMP exports with SCREEN_/TALL_ prefixes.
PNG previews were converted losslessly; every decoded pixel was checked against
the corresponding BMP. `build/` contains the actual instrumented package audit,
package sizes and memory report. No device-specific maintenance firmware is
included in this measurement dataset.

`SHA256SUMS` covers every distributed evidence file. `.gitattributes` preserves
their exact bytes across platforms. Raw CSV metadata retains the paths emitted
by the original controllers. The collected source files remain unchanged.

Recheck everything without hardware:

```powershell
python tools/capture-autotest/replay.py
```
