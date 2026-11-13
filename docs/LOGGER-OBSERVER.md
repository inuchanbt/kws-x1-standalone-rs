# Observation-only logger v0.7 candidate

[Japanese guide](LOGGER-OBSERVER.ja.md). Build with `--features logger-observer`.
This revision fixes wasted space across stop/start cycles and pairs exported
measurement/PD files by recording number. The revised journal and USB export have been verified on one device across ten
short sessions. A later 55-second v0.7 AVS sweep matched all 476 KWS packets
to CY4500 and compared nine ASD steps; its STATUS reports PD continuation into
a second sector, now verified against all raw-storage CRCs and CSV rows. v0.5's one-file install
and passive AVS capture were verified on one device (results below).

## Why short recordings reached FULL

The supplied v0.6 STATUS.TXT reported zero blank sectors, all with recognized
log headers. Session 1 used three sectors (53 VI rows, 511 + 165 PD fragments).
Sessions 2, 3 and 4 each consumed another VI sector with only 7, 3 and 2 rows.
Those six sectors contain 741 committed records plus six headers: 11,952 bytes
of 49,152 bytes. The remaining space was stranded inside stopped sessions.
There were no UNKNOWN owners in that status report. It establishes sector
allocation as a problem in this run, rather than evidence of byte exhaustion.
The status does not establish why sessions 2–4 contain no PD events.

Previously, every session started in new sectors and never reused the unused
tails of stopped recordings. The v0.6 two-sector preflight prevented wasting a
last blank sector but retained that allocation problem. v0.7 changes storage:

- XLG2/XLE2 sectors share their unused tails across sessions, including reboots.
  A CRC-last 16-byte session marker separates recordings without overwriting
  earlier data. VI and PD keep independent tails and share the same session ID.
- Legacy XLG1/XLE1 data stays readable and is never converted in place.
  Legacy tails are not reused. A fully occupied legacy store needs one confirmed
  clear **after backup** to make space for the new format.
- An interrupted record/marker seals its tail; earlier committed rows survive.
  A new blank sector is used when available. No live erase or automatic clear.
- Whole PD packet capacity is checked before its first fragment. Markers count
  against capacity; a packet can continue into another sector without starting
  a packet that cannot fit. A full store stops recording and preserves earlier data.
- A start checks the number of new sectors actually needed (0, 1 or 2).
  `FREE SECTORS: 0/6` is compatible with recording when reusable tails remain.
- At most 32 session IDs are supported between clears. `LOGGER SESSION LIMIT`
  is separate from `LOGGER FULL - COPY/CLEAR`. Actual PD traffic can fill the
  store before 32 sessions. Session IDs reset after a confirmed clear.

This removes the unused-tail penalty; it does not increase the physical 48 KiB
or filter GoodCRC/KeepAlive traffic. Capture duration still depends on traffic.

## Paired files

| File | Contents |
| --- | --- |
| VI000101.CSV | recording 0001: voltage/current snapshots |
| PD000101.CSV | recording 0001: passive PD event fragments |
| VI000201.CSV | recording 0002: voltage/current snapshots |
| PD000201.CSV | recording 0002: passive PD event fragments |
| STATUS.TXT | physical ownership, first session ID and committed slots |
| STORAGE.BIN | exact read-only dump of the six reserved storage sectors |

CSV names encode stream + four-digit recording number + `01` + extension.
The final `01` is retained for naming compatibility. All physical continuation
sectors of one stream/session now merge into **one CSV**, so a long PD capture
no longer creates a third CSV. A session with no PD data has only its VI file.
Legacy records are exported through the same logical grouping; IDs above 9999
use LOG names with the full ID retained in each row. CSV column schemas stay
unchanged. Use `tools/logger_decode.py "VI*.CSV" "PD*.CSV" --out merged.csv`;
it still reads legacy LOG files. Do not mix exports from different post-clear runs.

The read-only FAT12 disk allocates file clusters dynamically and has an 80-entry
root directory for up to 64 stream files plus diagnostics. Diagnostics are
synthesized during export and consume no log flash. STATUS `SLOTS` includes V2
session markers. `FIRST ID` is the sector header's session, not necessarily its
last appended session. The summary gives the latest session across the store.
Raw storage dumps exclude factory/settings regions and are not automatically
published as test fixtures.

## Operation and update

Use the physical slider **OFF** (passthrough). ON is the active/sink position.
At boot three PC13 samples must all report OFF before PD reception starts.
Changing to ON during reception stops it; return OFF and reconnect to resume.
The observer has no active sink future or protocol-trigger task, disables PD
command dispatch, initializes D+/D- trigger outputs undriven, sanitizes active
boot preferences, and uses passthrough navigation. External ASD-PD31 controls
the source/load. USB product: `KWS-X1 Observer`.

Hold Left+Right for about half a second to start/stop. Wait for `LOGGER STOPPED`
before disconnecting. At PC 5 V, reconnect holding Left+Right for CSV export.
For a confirmed clear, reconnect at 5 V holding Left+Right+Back, release, then
hold OK for two seconds at `ERASE LOGS+CAPTURES?`. Back cancels. Restart afterwards.
Clear only after copying any wanted CSVs and STORAGE.BIN. A clear erases all
recognized log/capture sectors; unknown bytes remain protected.

Use `build/logger-observer-v0.7/FLASH.md` and its exact SHA256SUMS to flash
**firmware.X1C once**. Installation retains the log store. For the supplied
all-V1/full store, preserve its backup and then perform the confirmed clear once.
The packer's complete stage tail-aligns the packed upper/main group inside the
existing application envelope; the unpacker format, flash boundaries and erase
margin checks remain unchanged. It installs upper then main internally. Keep
power through installation/reset; interrupted-update recovery is untested.
Default screenshot and full charging-logger variants remain separate choices.

## Offline validation and next device test

The production Writer host adapter covers 32 short VI+PD sessions across
reboots in two sectors, VI-only restarts, zero-blank-sector reuse with unknown
sectors untouched, legacy/full start rejection without writes, torn markers,
complete-packet capacity rejection and rollover after a reused tail. The codec
checks V1/V2 CSV compatibility, 64 logical files at full physical capacity,
FAT chains, random block reads, raw dump boundaries and corruption recovery.
An independent FAT12 reader also extracts all 64 generated CSVs, and the public
decoder reconstructs one complete packet plus one sample per session across all
32 sessions, with no overlapping cluster chains. Default and full logger ARM
checks pass. Host checks do not establish physical flash/USB timing on this device.

After the backed-up legacy store is cleared, repeat six short start/stop
sessions without clearing between them. Reboot once between sessions. Export
and check matching VI/PD recording numbers, exact previous rows and STATUS.
At quiet PC 5 V, a VI-only session is valid; use the external AVS setup when
checking PD capture. Then repeat the nine-point AVS sweep and compare KWS,
ASD and CY4500, including packet rollover. Long recording/full-store behavior,
physical TX isolation and interrupted-update recovery remain hardware work.

## First hardware result — 2026-11-13

The user reported installing this candidate with one file, using the physical
slider OFF (passthrough), and completed the external ASD sweep. Inputs were the
attached LOG00001(1).CSV, LOG00002(1).CSV, LOG00003(1).CSV and local analyzer run
CY4500_logs/kws_avs_k98fuz7e. The terminal reached DONE and saved CY4500.csv.

- KWS measurement file: 53 samples through 52,042 ms; intervals 917–1071 ms.
- PD fragments: 511 + 165 rows, demonstrating continuation into another sector.
  Reconstruction yields 456 complete SOP packets, with no drop/stop notices or
  incomplete fragment groups.
- All 456 packets match one-to-one with a contiguous CY4500 interval (zero-based
  packets 96–551 of 592 total). Type, MessageID and payload match; no frame is
  unmatched inside that interval. Capture windows differ outside the overlap.
- All eight EPR_REQUEST/ACCEPT/PS_RDY sequences match. Request payloads follow
  22,24,26,28,26,24,22,20 V at the configured 0.5 A request current.
- Request pairs fit KWS_ms = -11152.61463 + 0.9969210551 * CY_ms. Matching uses
  a 10 ms residual threshold after this fit; maximum observed residual is
  5.43 ms. The approximately 0.308% scale difference is not clock calibration.
- KWS packet types: 204 Extended_Control, 228 GoodCRC, 8 EPR_Request,
  8 Accept and 8 PS_RDY. CY4500 CRC/EOP/IDLE counters are all zero.
- Return-leg KWS snapshots include 0 mA at 33.090, 39.042 and 45.099 s, then
  recover to approximately 0.502 A. These correspond to the 24/22/20 V return
  transitions, consistent with the low-current snapshots in the ASD trace.
- Final KWS sample: 19.867 V / 0.502 A at 52.042 s.

This establishes one reported single-file install followed by recording/export,
sector rollover and a matched short passive capture. The comparison does not
validate PD role bits, analog waveforms or reception outside the common window.
The user subsequently confirmed at 5 V / no load that ON startup did not receive,
and switching to ON during reception stopped it. Physical TX isolation, long-duration
storage and interrupted-update recovery remain separate hardware checks.

## v0.7 repeated-recording device result — 2026-11-13

The user supplied the ten-session export under log_backup/v07-repeat after
reporting successful repeated recording. Its 18 files comprise ten VI CSVs,
six PD CSVs (sessions 0005–0010), STATUS.TXT and the 49,152-byte STORAGE.BIN.
The raw journal was independently decoded with the specified CRC-32 internal
state (no final XOR, masked to 31 bits) and compared row-for-row with every CSV.

| Check | Result |
| --- | --- |
| Sessions / CSVs | 10 / 16 |
| VI samples | 41 |
| PD fragments / complete packets | 276 / 184 |
| Incomplete groups / queue-drop notices / stop notices | 0 / 0 / 0 |
| Event IDs | contiguous from 1 within each PD session |
| Header and committed-slot CRCs | all valid: 2 headers, 331 slots |
| CSV versus raw storage | all rows match; no extra or missing CSV files |
| VI sector | XLG2: sessions 1–10, 41 samples + 9 markers |
| PD sector | XLE2: sessions 5–10, 276 fragments + 5 markers |
| Entirely blank sectors | 4 of 6 |
| Used bytes, including headers/markers | 5,328 / 49,152 (10.84%) |
| Blank bytes, including reusable tails | 43,824 |
| Torn/nonblank tails | none |

Sample intervals span 907–1,061 ms. The VI readings include approximately
5, 15, 20, 9, 28, 36, 41, 9, 20 and 9 V respectively, all near zero current.
Recorded PD traffic comprises 92 Extended_Control and 92 GoodCRC packets.
Sessions 1–4 have no PD rows; the dump alone cannot distinguish quiet bus
conditions from reception that was not active. Missing PD files are consistent
with zero stored events and are not empty-file/export corruption.

This confirms repeated-session storage, use of previous sector tails, numbered
VI/PD pairing and CSV export on one device. The data does not encode which
cycles included a power removal. Complete fragment groups and contiguous event
IDs establish stored-event integrity, not lossless reception from the bus; no
simultaneous reference capture was supplied for these ten short recordings.
This run uses only two sectors and does not validate V2 physical rollover yet.

A useful next check is to retain these ten sessions, perform the usual external
20→28→20 V sweep as session 11, and compare VI001101.CSV / PD001101.CSV with the
matching ASD/CY4500 run. The PD tail currently has 230 slots, so a sufficiently
long capture should cross into another sector while exporting one PD file.

The local pre-clear backup in log_backup was independently checked: all six
V1 headers and 741 record CRCs match, and the remaining 37,200 bytes are erased
tails. This confirms the prior allocation diagnosis from STATUS.TXT. That backup
contains sessions 1–4 and is separate from the ten-session V2 export. Input
hashes and the detailed repeat-validation report are retained alongside the
local backup; raw dumps were not added as public test fixtures.

## ASD analog comparison support

`tools/logger_compare.py` now includes ASD.csv alongside the existing CY4500
comparison. It reports independent per-step voltage/current medians for all
nine ordered AVS steps, with transition/steady-window exclusions. Multiple KWS
recordings require `--session`; `--reference-dir` selects the same-run ASD/CY
folder when KWS files are copied elsewhere. The launcher already saves the
necessary ASD measurement rows; no firmware or live-workflow change is needed.
See [method and examples](../tools/logger-sweep/README.md#three-instrument-comparison).

Rechecking backed-up v0.5 session 1 against kws_avs_k98fuz7e gives nine analog
steps with KWS-minus-ASD voltage medians +14 to +39 mV and current +2 mA;
the existing 456-packet PD match remains unchanged. These independent medians
include wiring/position, meter offset and sampling-time effects; they are not
an accuracy/calibration verdict and do not describe the pending v0.7 sweep.

## v0.7 AVS sweep with ASD and CY4500 — 2026-11-13

Run kws_avs_qpv94iem uses the same v0.7 candidate. The user confirmed clearing
logs before it, so the actual export is VI000101.CSV / PD000101.CSV and STATUS
LATEST SESSION is 1. It is a fresh post-clear sweep, not the initially proposed
session-11 append to the ten short recordings. Those ten sessions remain in the
separate prior backup; retention of them was not exercised by this sweep.

The attached CSV/STATUS text was read completely. The working CSVs have restored
CRLF endings and match their reported original byte sizes (2,618 and 32,514).
The attached STORAGE(1).BIN was subsequently copied from the KWS drive after
attachment download failed. All 3 headers and 762 record CRCs pass; raw rows
match both CSVs exactly. All unused tails/free sectors are erased. Storage
uses 12,240 / 49,152 bytes, leaving 36,912 blank bytes.

| Check | Result |
| --- | --- |
| VI samples / last timestamp | 56 / 55,099 ms |
| Sample intervals | 917–1,081 ms |
| PD fragments / complete SOP packets | 706 / 476 |
| Incomplete groups / drop notices / stop notices | 0 / 0 / 0 |
| Event IDs | contiguous 1–476 |
| KWS/CY match | all 476, contiguous CY indices 16–491 of 508 |
| Unmatched packets in overlap | 0 on either side |
| Request → Accept → PS_RDY | all 8 sequences and payloads match |
| CY CRC / EOP / IDLE / flag-mismatch errors | all 0 |
| ASD stable comparison | 9 steps; KWS−ASD +13.5 to +15 mV, +2 mA |
| Storage STATUS | 56 VI slots, 511 + 195 PD slots; 3 free sectors |

The 511/195 PD split reported by STATUS falls inside event 345 at 40.545 s:
its header is CSV fragment row 511 and payload is row 512 (one-based data rows).
That event reconstructs completely and matches CY. This establishes a complete
logical continuation across the reported boundary; the raw dump independently verifies
its exact physical placement and storage CRCs.

The clock fit is KWS_ms = -1533.713773 + 0.997197478925 × CY_ms, with maximum
residual 6.785 ms under the existing 10 ms criterion. Capture windows differ
outside the overlap. These are digital content matches, not proof of every bus
frame or analog waveform, and the fit is not clock calibration.

KWS records 0 mA at 30.020 and 36.024 s on the 26/24 V return transitions,
recovering to approximately 0.502 A at the next sample. These snapshots are
consistent with the return-leg low-current warnings in the supplied ASD trace;
no synchronous transient-timing comparison is claimed. Stable differences are
independent per-step medians and include measurement-position/wiring, meter
offset and sampling-time effects, not an absolute accuracy/calibration verdict.

A selected public regression fixture, comparison output and input hashes are in
[tests/data/logger-observer-v07-avs-20261113](../tests/data/logger-observer-v07-avs-20261113).
The raw storage dump remains local; its verified layout/CRC result and hash are
recorded in the fixture. Full local results also remain in log_backup/v07-sweep. Reproduce the digital and analog comparison with:

```powershell
python tools/logger_compare.py log_backup/v07-sweep --session 1 --reference-dir CY4500_logs/kws_avs_qpv94iem
```

## Build and audit the single-file observer

Use the repository's normal Rust/ARM build prerequisites. With the ARM target
installed, the feature selects the receive-only variant and both binaries:

```powershell
rustup target add thumbv7em-none-eabihf
cargo build -p firmware --release --features logger-observer --target thumbv7em-none-eabihf --locked
cargo build -p firmware-pack --release --locked
New-Item -ItemType Directory -Force -Path dist | Out-Null
.\target\release\firmware-pack.exe pack --stage complete --firmware .\target\thumbv7em-none-eabihf\release\firmware --unpacker .\target\thumbv7em-none-eabihf\release\unpacker --out .\dist\logger-observer.X1C
$observerSha = (Get-FileHash -Algorithm SHA256 .\dist\logger-observer.X1C).Hash.ToLowerInvariant()
.\target\release\firmware-pack.exe audit --stage complete --firmware .\target\thumbv7em-none-eabihf\release\firmware --unpacker .\target\thumbv7em-none-eabihf\release\unpacker --image .\dist\logger-observer.X1C --confirm-sha256 $observerSha
```

The tested local v0.7 candidate SHA-256 was
`86c2f7b4a0e07cdc7befc3231f33c4deb457466cfc016b48c3400c2f9f9cc537`.
Its unpacker was 15,340 bytes, compressed main 115,683 bytes, compressed upper
12,233 bytes, with minimum unread-data erase margin 21,500 bytes. A different
build environment can change the output hash; audit and confirm that build's
own hash. The packer refuses an existing output file. No firmware binary or
GitHub Release is published by this source/test-data commit.

On non-Windows hosts, omit executable `.exe` suffixes and use the equivalent
shell directory/hash commands. Installation still requires the original
bootloader and one complete-stage image; it does not require the operator to
flash a separate upper image. See the operating/clear instructions above.
