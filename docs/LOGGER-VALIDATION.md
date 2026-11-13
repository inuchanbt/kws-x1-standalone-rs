# Charging logger validation — 2026-11-13

[English guide](CHARGING-LOGGER.md) · [日本語ガイド](CHARGING-LOGGER.ja.md)

Scope: experimental logger v0.2 on one user-operated KWS-X1. Flashing and
display observations were reported by the user; the exported CSV and CY4500-EPR
capture were inspected separately. This is not general hardware qualification.

## Offline checks and build identity

- 38 `firmware-core` library tests passed, including five logger format,
  random-access CSV/FAT12 and storage ownership tests.
- Logger ARM release build passed; three unused screenshot-renderer warnings remain.
- Default screenshot variant passed `cargo check` during initial integration.
- Both v0.2 containers passed the offline packer's structural audit against
  their matching firmware and unpacker ELF files.
- Runtime main: 196,544 bytes; upper: 122,656 bytes, with 224 bytes spare.
- The v0.1 complete-container trial exceeded the limit by 9,704 bytes. v0.2
  was packaged as a matching two-stage pair; no single-stage claim is made.

| v0.2 file | SHA-256 |
| --- | --- |
| `firmware-upper.X1C` | `4ebecd4acaeecf77b0e47befcc707ecc5e0171c57bcf3b9ebaf311864233b1ef` |
| `firmware.X1C` | `eeba02de4f6e212dd74e5d02e7e0793d24b59189fca55ac5e74df3fd432fb58c` |

## Hardware observations

The original HID bootloader reported `mode=0`. The user flashed the initial
logger pair and reached the measurement screen. Recording initially reported
full storage, including after the logs-only clear operation. v0.2 expanded the
explicit clear confirmation to include recognized screenshot ownership within
the six dedicated sectors. The user reported success after updating and clearing.
The initial sector contents were not dumped, so their exact classification is unknown.

The exported `LOG00006.CSV` contains session 2 and 11 samples from 0 to
50,410 ms. Intervals range from 5,002 to 5,088 ms, averaging 5,041 ms. The
current scheduler samples on eligible meter reads rather than exact wall-clock
five-second boundaries. Normal disconnect/reconnect and USB CSV retrieval
worked; this was not a deliberate power cut during a flash write.

| CSV elapsed time | Voltage | Current |
| --- | --- | --- |
| 0–10.013 s | 5.085–5.086 V | 0 A |
| 15.046–30.219 s | 19.215 V | 5.027–5.028 A |
| 35.307 s | 9.091 V | 0 A |
| 40.352–50.410 s | 8.585–8.586 V | 3.019 A |

## CY4500-EPR comparison

Local source: `CY4500_logs/logger_01.ccgx3` (334,901 bytes), SHA-256
`e74aa80a73fd88186a16c2500cd0aac09d381a42284be4a7054047305d5aae0f`.
The raw capture is not included in this commit. Its archive contains a packet
list (13 entries, including VBUS_UP) and 58,302 scope samples ending at 58.443462 s.

| CY capture time | Message | Result |
| --- | --- | --- |
| 11.540960 s | REQUEST, `0x1E82 0x4047D1F4` | GoodCRC received |
| 11.542966 s | ACCEPT | GoodCRC received |
| 11.821258 s | PS_RDY, VBUS 19.769 V | GoodCRC received |
| 33.408181 s | REQUEST, `0x1082 0x2044B12C` | GoodCRC received |
| 33.410261 s | ACCEPT | GoodCRC received |
| 33.689094 s | PS_RDY, VBUS 8.977 V | GoodCRC received |

No repeated request, reset or failed message appears in the recorded packet
list. Source capabilities are not present; no advertised PDO voltage is inferred
from the object positions alone. The sequence is consistent with the CSV's
voltage changes. The clocks/start events were not synchronized, so this does not
establish exact logger-to-analyzer latency.

CY scope medians over capture-relative windows were 5.087 V/0.008 A (2–10 s),
19.105 V/4.966 A (16–30 s), and 8.598 V/2.983 A (42–50 s). These are plateau
comparisons, not time-aligned point pairs. Meter current is about 1.2% higher in
the loaded plateaus. Calibration and measurement placement were not controlled;
no correction factor was applied.

## Remaining limits

This establishes a short recording/export path and successful external PD
transactions during that run. It does not prove that PD events coincided with
flash programming or that the KWS-X1 internal receiver lost no packets. No
full-capacity run, repeated endurance test, deliberate interrupted write/erase,
or broad firmware migration qualification has been completed. PD packet logging
is not implemented.

## 日本語要約

38テスト、logger版のARMビルド、v0.2の2ファイルの構造検査を通過。
実機1台で書き込み・消去後に約50秒の11サンプルを保存し、抜き差し後のCSV取り出しを確認しました。
CY4500-EPRには2回のREQUEST→ACCEPT→PS_RDYと各GoodCRCが記録され、CSVの電圧変化と整合します。
ただし両時刻は非同期です。PDイベントとフラッシュ書き込みの同時性、内部受信の無欠落、
長時間・満杯・書き込み／消去中の電源断は未検証です。測定器間の差を校正誤差と断定していません。
