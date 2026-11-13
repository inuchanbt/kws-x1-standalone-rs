# Charging logger v0.2 — experimental exclusive variant

[English (primary)](CHARGING-LOGGER.md) · [日本語](CHARGING-LOGGER.ja.md)

Build with `--features charging-logger`. Without that feature, the existing
screenshot firmware remains the default. The logger replaces screenshot capture
and BMP export; it does not change the code/storage boundaries or ENERGY MEMORY.

## Operation

- Boot normally. Logging is **off** until Left + Right are held for about 0.5 s.
  The footer says `LOGGER REC 5s - L+R STOP` while recording. Hold again to stop.
- Calibrated voltage (mV), signed current (mA), session ID and elapsed milliseconds
  are recorded on the next valid meter read, then approximately every five seconds.
  These are snapshots, not five-second averages. Missing reads are skipped;
  actual timestamps expose gaps. This is not a high-speed transient recorder.
- After measurement, disconnect the measurement setup. Hold Left + Right while
  reconnecting the meter to a PC at 5 V to expose the read-only `KWS LOGGER` drive.
  Copy `LOG00001.CSV` through `LOG00006.CSV`. A file corresponds to one occupied
  physical sector; a long session spans files with the same `session` column.
  Concatenate rows and sort by `session,elapsed_ms` when combining files.
- To erase recognized logger and screenshot sectors in the six-sector storage area, boot at 5 V holding Left + Right + Back,
  release the buttons, then hold OK for two seconds at `ERASE LOGS+CAPTURES?`.
  Back cancels. Restart after clearing. Erasing is never part of live recording.

## Storage and limitations

Only `[0x62000,0x6e000)` is allocated: six 8 KiB sectors. Each holds a 16-byte
header and 511 16-byte samples. Six empty sectors allow 3,066 samples, roughly
4 h 15 min at five-second spacing. Occupied screenshot/unknown sectors reduce
capacity. Legacy three-sector screenshot ownership includes erased padding.
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

PD messages are **not recorded** in this first version. Flash programming still
briefly masks interrupts, just as in the existing storage layer. Holding the I2C
bus lease prevents overlap with an in-flight I2C transaction but does **not** prove
PD reception or negotiation is unaffected. A short recording and PC CSV copy
have been verified on one device. Hardware qualification remains open for PD
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
release files are not logger firmware. Local v0.2 artifacts are under
`build/charging-logger-v0.2`; use their matching `SHA256SUMS` and the two-stage
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
