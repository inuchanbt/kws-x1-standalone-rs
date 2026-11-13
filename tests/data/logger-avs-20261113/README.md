# Logger v0.4 AVS capture — 2026-11-13

Single KWS-X1, user-flashed logger v0.4, observed with ASD-PD31 and CY4500-EPR.
Connection: source → cable → CY4500-EPR → KWS-X1 → ASD-PD31.
Source identity, cable model and calibration traceability were not recorded for
this run. This is a functional regression capture, not accuracy certification.

ASD was manually set to touch-panel OBJ9 (native Object 11 EPR AVS), 20 V,
0.5 A, LOAD ON. The launcher observed 20 V, then requested 22,24,26,28,26,24,22,20 V,
with at least three seconds per point. Logging was stopped before LOAD_OFF.

## Contents and provenance

- LOG00001.CSV: exported KWS measurements, 65 rows through 64.042 seconds.
- LOG00002.CSV / LOG00003.CSV: 511 / 309 raw event-fragment rows. Both belong
  to session 1; merge before decoding. Downloaded text uses normalized LF endings.
- CY4500.csv: original analyzer CSV (microsecond Start Time values).
- CY4500.diagnostics.json: original analyzer error/capture counters.
- ASD.csv: selected columns from the original controller CSV; command-line,
  local paths and unrelated empty metadata columns omitted. Measurement values,
  targets, timestamps, PDO identity and raw measurement responses are retained.
- comparison.json: generated comparison result. SHA256SUMS covers the CSV and
  analyzer diagnostics files (not the generated report or this README).

## Reproduce

```sh
python tools/logger_compare.py tests/data/logger-avs-20261113
python tools/logger_decode.py "tests/data/logger-avs-20261113/LOG*.CSV" --out decoded.csv
```

KWS recorded 552 complete packets; CY4500 recorded 540. Capture start/stop
windows differ. The common interval contains 516 packets with one-to-one matching
of type, MessageID and payload. No packet is unmatched inside that interval.
All eight EPR_REQUEST/ACCEPT/PS_RDY sequences match, including request payloads.
There are no KWS dropped/stop notices or incomplete packets. CY4500 CRC/EOP/IDLE
error counters are zero. KWS samples are 918–1085 ms apart.

The ordered eight request pairs fit `KWS_ms = 4104.6781 + 0.99742747 * CY_ms`.
Other packet signatures must then match uniquely within 10 ms; maximum residual
is 6.36 ms. Clock scale differs by about 0.257%, so raw elapsed times must not
be compared with an offset alone. This fit is not clock calibration. Matching
does not validate role bits, analog waveforms, or reception outside the overlap.
The analyzer is a comparison instrument, not proof of every physical bus frame.

This capture demonstrates continuation beyond the old 40-second limit and
successful event-sector rollover. It does not prove all six sectors were free,
maximum duration, power-loss recovery or performance on other hardware.

## 日本語

v0.4で約64秒の記録とPD区画またぎを確認しました。共通区間516パケットと
往復8回の要求・応答が一致し、記録された欠落通知・途中切れはありません。
KWSとCYの時計には約0.257%の差があり、補正して比較しています。
ASDは必要な列のみ掲載し、KWSは改行をLFへ統一しています。
全6区画の空き、長時間動作、電源断耐性や一般的な測定精度の証明ではありません。
