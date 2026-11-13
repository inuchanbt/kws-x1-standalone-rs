# Observer v0.7 AVS sweep — 2026-11-13

Single KWS-X1, user-installed single-file logger-observer v0.7, captured with
ASD-PD31 and CY4500-EPR. Source/cable identity and calibration traceability were
not recorded for publication. This is a functional regression capture.

The user confirmed clearing the logs before this sweep. It is **session 1**,
separate from the earlier ten-session repeat test; this capture does not show
retention of those ten recordings. The physical slider was OFF/passthrough.
The external ASD load was manually established at panel OBJ9 (native Object 11
EPR AVS), 20 V / 0.5 A. The route was 20,22,24,26,28,26,24,22,20 V. KWS was
stopped before normal LOAD_OFF and CY4500 export.

## Inputs and provenance

- VI000101.CSV: 56 exported KWS measurements through 55,099 ms.
- PD000101.CSV: 706 exported PD fragments merged from 511 + 195 physical slots.
  The boundary splits event 345 (40.545 s) between header and payload.
- STATUS.TXT: original exported sector/session/slot summary.
- ASD.csv: the 54 original measurement rows with selected fields; local paths,
  command line and unrelated metadata removed. Actual timestamps, phases,
  targets, measurements, PDO identity and raw measurement replies are retained.
- CY4500.csv and CY4500.diagnostics.json: original analyzer capture/diagnostics.
- comparison.json: generated digital and per-step analog comparison.
- storage-validation.json: recorded raw-dump CRC/layout verification and hash.
  The six-sector raw dump is retained locally and is not published here.
- SHA256SUMS: hashes of the input CSV/TXT/diagnostics and storage-validation
  metadata. File bytes are preserved with the local .gitattributes rules.

The two KWS CSVs and STATUS originated as attachments. Complete text reads were
used after attachment byte download failed; CRLF restoration matched each
reported original byte size. A subsequent direct copy of STORAGE.BIN from the
device independently verified all 3 headers and 762 committed-record CRCs and
every CSV row. Unused tails and three free sectors were erased. Use was
12,240 / 49,152 bytes. The dump hash is retained in storage-validation.json;
reproducing raw CRC/layout checks requires that private original dump.

## Reproduce

```powershell
python tools/logger_compare.py tests/data/logger-observer-v07-avs-20261113 --session 1
python tools/logger_decode.py "tests/data/logger-observer-v07-avs-20261113/VI*.CSV" "tests/data/logger-observer-v07-avs-20261113/PD*.CSV" --out decoded.csv
```

All 476 KWS packets match the contiguous zero-based CY interval 16–491 of 508
SOP packets. No packet is unmatched within that overlap. All eight
EPR_Request → Accept → PS_RDY sequences and their payloads agree. Event IDs are
contiguous; incomplete groups, drop notices and stop notices are all zero.
CY CRC/EOP/IDLE/flag-mismatch counts are zero. The Request-based clock fit is
KWS_ms = -1533.713773 + 0.997197478925 × CY_ms, maximum residual 6.785 ms below
the existing 10 ms criterion. This is alignment, not clock calibration.
Windows differ outside the overlap; matching does not establish every bus
frame, physical waveforms or correctness of PD role bits.

All nine ASD steps compare. KWS-minus-ASD voltage medians range +13.5 to
+15 mV; current medians differ by +2 mA. These are independent per-step medians,
not time-paired samples. Changed KWS steps omit the first 1,000 ms after the
Request; both instruments exclude snapshots outside ±0.5 V / ±0.05 A of target.
These are selection windows, not accuracy/compliance limits. Wiring/position,
meter offsets and timing effects are not separated. No absolute accuracy,
calibration or transient-response verdict follows.

KWS captured 0 mA at 30.020 and 36.024 s on the 26/24 V return changes, followed
by recovery to approximately 0.502 A. This agrees qualitatively with the ASD
terminal's low-current transition snapshots; no synchronous transient timing
claim is made. Long-duration/full-storage, interrupted updates and general
compatibility across other devices remain separate tests.

## 日本語要約

消去後の記録1です。約55秒の掃引でKWSの全476パケットがCY4500と一致し、
ASDとの9点比較は電圧差+13.5〜+15 mV、電流差+2 mAでした。
PD区画の境界に跨がるイベント345も正常です。生ダンプでは3ヘッダー・762記録の
CRCとCSV全行一致を確認しました。生ダンプ自体は公開せず、検証結果とハッシュを残します。
これは機能検証であり、校正・規格適合や全機種への保証ではありません。
