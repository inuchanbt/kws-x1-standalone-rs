# Charging logger v0.3 — experimental exclusive variant

[English (primary)](CHARGING-LOGGER.md) · [日本語](CHARGING-LOGGER.ja.md)

Build with `--features charging-logger`. Without that feature, the existing
screenshot firmware remains the default. The logger replaces screenshot capture
and BMP export; it does not change the code/storage boundaries or ENERGY MEMORY.

## Operation

- Boot normally. Logging is **off** until Left + Right are held for about 0.5 s.
  The footer says `LOGGER REC 1s - L+R STOP` while recording. Hold again to stop.
- Calibrated voltage (mV), signed current (mA), session ID and elapsed milliseconds
  are recorded on the next valid meter read, then on a one-second deadline schedule.
  These are snapshots, not one-second averages. Missing reads are skipped;
  actual timestamps expose gaps. Deadlines advance from the session schedule, not
  from each late read, so delays do not accumulate; missed slots are not replayed. This is not a high-speed transient recorder.
- After measurement, disconnect the measurement setup. Hold Left + Right while
  reconnecting the meter to a PC at 5 V to expose the read-only `KWS LOGGER` drive.
  Copy `LOG00001.CSV` through `LOG00006.CSV`. A file corresponds to one occupied
  physical sector; a long session spans files with the same `session` column.
  Measurement and event files have different CSV headers; use the decoder below
  to combine them by `session,elapsed_ms`. Wait for `LOGGER STOPPED` before unplugging
  to let the pending PD queue drain.
- To erase recognized logger and screenshot sectors in the six-sector storage area, boot at 5 V holding Left + Right + Back,
  release the buttons, then hold OK for two seconds at `ERASE LOGS+CAPTURES?`.
  Back cancels. Restart after clearing. Erasing is never part of live recording.

## Passive PD events and AVS comparison (v0.3)

Use an external AVS trigger/sink and CY4500-EPR with KWS-X1 in passthrough mode.
Check the PD SNIFF page is receiving before starting the sweep. Logging does not
start a sniffer, recover a stopped sniffer, or add AVS control. Sink-mode RX/TX
and cable/charger query traffic are not hooked by this version.

- The existing CRC-checked passive receive path copies packets into a 32-entry
  RAM queue without waiting or programming flash. The meter drains at most four
  events per read cycle, releases I2C between packets, and yields for 1 ms.
- RX timestamps use the same monotonic millisecond clock and session start as
  measurements, but mark software receipt, not the wire edge. CY4500 has its own
  clock: align matching requests/payloads and check drift at multiple points.
- Save raw 16-bit header (including MessageID), SOP token category, and up to seven
  raw data objects. CRC was checked by the receiver but is not exported. Extended
  chunks remain raw; this adds no extended-message reassembly or new RX capability.
- Queue overflow exports `QUEUE_DROPPED`; a sniffer that exits while recording
  exports `SNIFFER_STOPPED` (generic notification, reason remains on PD SNIFF).
  These diagnostics also require free storage. They cannot prove lossless capture;
  hardware FIFO loss and failures before recording may not appear in the CSV.
- Stop drains the queue before `LOGGER STOPPED`. Power loss or FULL can leave a
  partial packet; the decoder labels it `INCOMPLETE`, never a complete message.

Copy **all six available CSV files from one export** into a folder, then:

```powershell
python tools/logger_decode.py "logs/LOG*.CSV" --out logs/decoded.csv
```

The merged CSV contains SAMPLE and PD_RX rows, header/payload hex, SOP,
MessageID, numeric message type, extended bit, completeness and diagnostics.
Do not combine different post-clear exports (session numbers may repeat).

For a first comparison, keep the same load and use supported AVS setpoints with
about 3 seconds per step, one ascent and descent over a short range. Start both
recorders before sweeping, stop the logger afterward, and match raw requests,
ACCEPT and PS_RDY where captured. Check diagnostics and incomplete rows before
comparing timing. The 1-second samples show settled levels, not transient waveforms.

### Event storage format

Measurement sectors retain XLG1 and the existing CSV schema; old logs remain
readable. Event sectors use XLE1 (`0x31454c58`), with the same 16-byte header and
CRC-last 16-byte records. Header words are magic, session, segment, checksum.
Record words are elapsed_ms, event_tag, raw_u32, checksum.

`event_tag = (event_id << 8) | fragment`, with a session-local 24-bit ID.
Fragment 0 contains `raw_header | (SOP_token_category << 16)`; fragments 1–7
contain the corresponding data objects. Category 7/6/5 means SOP/SOP'/SOP''.
Tag 8 (ID zero) is a dropped-event count; fragment 9 is a sniffer-stop notice.
Event CSV columns are `session,elapsed_ms,event_tag,raw_u32`, all decimal unsigned.
The decoder groups by session and ID even across physical sector boundaries.

## Storage and limitations

Only `[0x62000,0x6e000)` is allocated: six 8 KiB sectors. Each holds a 16-byte
header and 511 16-byte samples. Six empty sectors allow 3,066 samples, roughly
51 min at one-second spacing with no PD events. Occupied screenshot/unknown sectors reduce
capacity. Measurement and PD streams allocate separate sectors from this shared
pool. Each packet consumes 1 + data-object-count records; control messages also
consume one. Logging stops when either stream needs a new sector and none is
available, even if the other stream has unused tail space. Frequent PD traffic
can exhaust storage much sooner than 51 minutes. Legacy three-sector screenshot ownership includes erased padding.
No factory/settings sector is allocated; ENERGY MEMORY at `0x78000` and
`0x7a000` is untouched by the logger.

Starting again creates a new session in a fresh sector. Unused space in a stopped
session is not reused in this first implementation. A reboot leaves logging off.
Full storage or a write failure stops logging and displays a notice. Existing
logs remain available. Switch back to the screenshot variant to export
old captures; the logger does not export BMPs. Since v0.2, its confirmed clear also erases recognized captures inside the six-sector pool; export wanted images with the screenshot variant before confirming.

The format uses a versioned header and a CRC-derived 31-bit checksum/commit word
(the erased value is excluded). Payload words are programmed before the commit.
Export ignores uncommitted/invalid records and stops at the first invalid record
in each segment. Confirmed clear recognizes valid logger headers and the existing compact/legacy screenshot ownership markers. Unknown data is preserved. A torn
header or interrupted sector erase can leave an unrecognized sector, which is
preserved rather than automatically reclaimed. This may require separate,
backup-verified recovery. Session numbers restart after all logs are cleared.

CSV is generated during USB reads; expanded text is not stored in flash. USB MSC
uses the existing shared `16c0:27df` identity with product `KWS-X1 Logger` and the
same manufacturer string and shared-ID policy described in `USB-IDENTITY.md`.

v0.3 records passive RX packets as described below. Flash programming still
briefly masks interrupts, just as in the existing storage layer. Holding the I2C
bus lease prevents overlap with an in-flight I2C transaction but does **not** prove
PD reception or negotiation is unaffected. A short v0.2 recording and PC CSV copy
have been verified on one device; v0.3 still needs hardware validation. Hardware qualification remains open for PD
traffic coinciding with writes, long charging runs, physical power interruption
during writes/erases, and broader firmware migration cases. No device is flashed by the
build commands below.

## Build and checks

```powershell
cargo test -p firmware-core --lib --locked --offline
cargo build -p firmware --release --target thumbv7em-none-eabihf --features charging-logger --locked --offline
```

Use the normal offline packer and audit for the matching main/upper images.
Never mix upper and main files from different builds. This variant does not
promise upstream-compatible single-file updates.

## Validation and distribution

See [LOGGER-VALIDATION.md](LOGGER-VALIDATION.md) for the tested firmware hashes,
38 passing host tests, packaging audits, short hardware run and CY4500-EPR
comparison. One device completed recording and CSV retrieval after the v0.2
clear fix. Long runs, interrupted flash writes/erases and PD events coinciding
with flash writes remain unqualified.

Logger assets have not been published as a GitHub Release. The existing capture
release files are not logger firmware. Local v0.3 artifacts are under
`build/charging-logger-v0.3`; use their matching `SHA256SUMS` and the two-stage
procedure in [FLASHING.md](FLASHING.md). Do not mix artifacts across variants.

## v0.2 clear fix

The v0.1 clear action left screenshot-owned sectors occupied, so switching from
a full capture pool could leave the logger full even after clearing. The new
confirmation explicitly includes captures. Legacy three-sector ownership is
snapshotted before erasing, with the marker sector erased last. The ownership
test checks compact, legacy, logger, unknown and incomplete markers. If no usable
sector remains, the clear screen says `NO SPACE - UNKNOWN DATA` instead of
suggesting that space was recovered. The user reported clearing succeeded after
installing v0.2; the original occupied sectors were not dumped/classified.


## v0.4 storage recovery

The AVS capture exposed a capacity problem: only physical files 05/06 were
available, and the event sector filled at 40.457 seconds. The old clear routine
preserved unrecognized original-firmware bytes and reported success when even
one sector was free. The saved original 48 KiB dump contains nonblank data in
all six sectors. The observed allocation is consistent with four original-data
sectors remaining blocked; the exact current device contents cannot be read
from the two CSV files alone.

The device-specific v0.4 build embeds five SHA-256 fingerprints from
`build/screen-capture-reclaim-v2/CAPFLASH-original.BIN` using
`KWS_X1_RECLAIM_SHA256` (five concatenated lowercase digests, one per 8 KiB sector).
In the confirmed clear boot it clears recognized logs/captures, plus exact
backup matches in 0x62000 through 0x6BFFF. It checks the hardware hash engine
against a known vector before authorizing any erase. Unmatched data remains
protected. No erase occurs during recording. Factory/settings regions remain
outside this operation. This fingerprinted build is specific to this backup.

After copying wanted data, boot with Left+Right+Back, release the buttons at
`CLEAR LOG+BACKED DATA?`, then hold OK for two seconds. Confirm
`CLEARED - 6/6 FREE`. `PARTIAL - N/6 FREE` means unmatched contents still block
capacity; do not assume all 48 KiB are available. No hashes configured means
only recognized log/capture owners can be cleared.

All six sectors are shared by measurement and raw PD events. GoodCRC and EPR
KeepAlive traffic consume storage even at steady voltage: at the observed traffic
rate, all six sectors provide roughly three minutes, not 51 minutes. The latter
is a measurement-only upper bound. Copy every LOG*.CSV from one export, including
PD continuation files. v0.4 checks capacity for a complete packet before writing
its first fragment; full storage no longer leaves half a packet at the end.
Hardware confirmation of 6/6 free and a complete sweep is still required.


Latest v0.4 hardware result / 最新の実機結果: [64-second AVS capture and reproducible comparison](../tests/data/logger-avs-20261113/README.md). Event-sector rollover and all eight voltage-change sequences were verified on one device.
