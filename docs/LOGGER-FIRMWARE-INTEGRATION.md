# Selective logger firmware integration and ARM footprint (2026-11-17)

The later [offline package audit](LOGGER-PACKAGE-AUDIT.md) creates matching
two-stage update files and checks dry runs/synthetic install replay. A complete
single-file image exceeds the content budget by 15,773 B. Hardware remains
untested; that follow-up does not change the firmware measured below.

This is an experimental, hardware-untested integration on top of
`abc4dba2bae404edfdae8d2aedc915eef71e3c7c`. The new `selective-logger` feature
retains normal PD sink and QC/SCP/FCP/AFC charging controls. It implies
`charging-logger`; combining it with the reduced `logger-observer` feature
is a compile error. No charging function or memory region was removed to
make this build fit.

## What is integrated

`firmware-core/src/selective_log.rs` implements the 100 mV / 100 mA selection
relative to the last **committed** VI reading, retaining the first valid
reading and the first available sample at or beyond a 60 s heartbeat. The
existing nominal one-second input schedule is unchanged. The selector resets
for every new session. Signed current differences cannot overflow.

The same module recognizes only exact, complete SOP GoodCRC and the known
EPR KeepAlive/ACK object patterns. In `firmware/src/logger.rs`, the selector
is connected to the real VI append path; PD suppression runs after dequeue
and event ID assignment, before flash programming. Retained PD IDs can have
gaps. The existing 32-entry queue and four-event drain budget remain; routine
packets still occupy that queue and count toward the event ID limit. Loss and
sniffer-stop diagnostics bypass suppression. Unknown SOP/type/payload patterns
that the transport supplies are retained.

The checked transport supplies complete CRC-verified packets. This does not
add observation of malformed/incomplete wire frames, PD transmit events or
active-sink traffic. Those limitations already exist in the full logger.
Neither the Python JSON escape nor arbitrary future diagnostic dictionaries
are embedded into the firmware.

## Native storage and USB export

This first integration reuses the bounded fixed-record journal writer and
read-only CSV/FAT12 exporter. **It does not port the experimental host XSC1
variable-record format.** Its firmware footprint therefore applies to the
native selective journal implementation, not to an embedded XSC1 encoder.
This preserves the existing writer and export paths for a controlled policy
comparison and avoids inventing a new flash recovery protocol for this step.

Selective VI headers have magic `XSL1` (0x314c5358); PD headers have `XSP1`
(0x31505358). These identify fixed selection policy version 1: 100 mV / 100 mA,
60 s heartbeat, nominal one-second VI input, known complete SOP routine
suppression. Changing those rules requires a new version. The remaining
layout is the existing journal layout: 16-byte sector headers, 16-byte records,
little-endian fields and the existing checksum/commit word. A packet stores
one raw-header/SOP record and one record per data object. Full raw headers,
all objects, original event IDs, timestamps and signed VI values survive
export. CRC32 and commit rules are those of `charging_log`, not host XSC1.
VI and no-object PD each cost 16 B; a two-object Request costs 48 B and a
seven-object packet costs 128 B. Reusing a stream tail for a later session
adds a 16 B session marker. Existing u32 elapsed-time and 24-bit event ID
limits remain; routine PD still consumes IDs even when not stored.

The firmware uses the existing six-sector bounds `[0x62000, 0x6e000)`,
available-sector checks, no-erase append operation and packet space preflight.
Only the newest valid tail of the **same policy and stream** is reusable.
Unknown contents and legacy screenshot padding remain protected. Native
selective and full journals can coexist and are exported by both new logger
builds; session numbering considers both. Clear ownership recognizes the two
new signatures through the existing confirmed-clear path. No automatic erase,
wraparound or storage enlargement was added. Actual available capacity may
be smaller than six sectors.

The exported `STATUS.TXT` labels selective sectors and describes policy 1.
Older firmware is not assumed to recognize the new signatures. Downgrade,
clear and recovery behavior on hardware remain untested.

Like the existing full journal, native selective logs expose a CRC-checked
prefix without a durable clean-stop marker. A host test checks a torn record
seals the tail; it is not a physical power-cut test. Clean completion cannot
be inferred from the exported prefix. The strict host XSC1 decoder instead
requires its end marker; the two recovery/completeness models differ.

### Replay of existing captures (host only)

| Capture | Retained VI / PD | Native committed bytes | Native sector allocation | Host XSC1 committed / allocated |
| --- | ---: | ---: | ---: | ---: |
| Observer v0.7, about 55 s | 12 / 24 | 864 B | 16,384 B | 1,060 / 8,192 B |
| Logger v0.4, about 64 s | 14 / 24 | 896 B | 16,384 B | 1,108 / 8,192 B |

The native numbers are from an in-memory append replay in
`firmware-core/tests/selective_logger.rs`, using the production Rust selector,
header/record encoders and actual USB block exporter. The selected events are
compared with both archived Python `events.json` files, including PD IDs,
headers and complete payloads. USB CSV rows are checked against the native
records. These are **not measurements of on-device writes**.

Each capture contains 40 PD fragments (640 B), VI records (192 / 224 B), and
two 16-byte headers. VI and PD use separate sectors. Unused tails can be
shared with later sessions of the same policy, unlike the one-file-per-session
host XSC1 prototype. The native totals omit the host JSON metadata/end record;
fixed policy metadata is identified by the versioned native header. A smaller
committed byte count here does not mean a smaller allocation or an equivalent
format. Short controlled sweeps do not estimate real charging duration.

## Controlled ARM comparison

All three ARM release builds succeeded. Each ELF was saved separately before
the next build replaced Cargo's output. Source hashes matched before and after
the sequence. The comparison is full logger versus
selective logger from the **same working-tree source**, with default screenshot
as a separate reference. The shared ownership/export support for the selective
format is present in both logger builds; their difference isolates enabling
the policy/state, not every shared-format change relative to the old HEAD.

| Metric (bytes) | Default screenshot | Full logger | Selective logger | Selective minus full |
| --- | ---: | ---: | ---: | ---: |
| `.text` | 179,436 | 174,508 | 174,644 | +136 |
| `.rodata` | 26,052 | 26,500 | 26,500 | 0 |
| `.data` (RAM plus flash load copy) | 3,752 | 3,752 | 3,752 | 0 |
| `.upper` | 122,648 | 122,664 | 122,664 | 0 |
| Upper remaining | 232 | 216 | 216 | 0 |
| Main flash load bytes | 209,880 | 205,400 | 205,536 | +136 |
| Total main + upper flash load bytes | 332,528 | 328,064 | 328,200 | +136 |
| Main flash span, including alignment | 209,888 | 205,408 | 205,544 | +136 |
| Main linker region remaining | 28 | 4,508 | 4,372 | -136 |
| `.bss` | 41,616 | 42,252 | 42,268 | +16 |
| `.uninit` | 0 | 0 | 0 | 0 |
| Main static RAM (`.data + .bss + .uninit`) | 45,368 | 46,004 | 46,020 | +16 |
| `.sram2` frame buffer | 64,800 | 64,800 | 64,800 | 0 |
| Total static RAM | 110,168 | 110,804 | 110,820 | +16 |
| Linker stack reservation | 44,488 | 43,852 | 43,836 | -16 |

The meter task's static future pool grows from 11,096 to 11,112 B, accounting
for the measured 16 B RAM increase. The PD task pool remains 9,768 B. The
136 B flash delta is the **net linked difference with LTO**, not the sum of
standalone selector function sizes. Both logger builds contain the new shared
format ownership/export support. Relative to the archived pre-change full
logger, the new full logger costs 316 B more total flash (308 B main + 8 B
upper), and the selective logger costs 452 B more total flash and 16 B RAM.
The old saved ELF is a historical reference, not another build in this run.

The default ELF's SHA-256 exactly matches the previously saved default
(`006d4c724e744afa1e79e654eb641acbcfca92fcc7220b3006ec22c30c30e3bd`).
The new full/selective hashes are in the reports. All builds pass the existing
linker/40 KiB minimum stack assertions. **Only 216 B of upper space remains**
in both logger builds. Default screenshot main space has only 28 B remaining;
the logger's smaller main image replaces screenshot capture/export. These
results do not imply room to add both features simultaneously.

Saved local evidence is under `build/logger-firmware-study-20261117/`.
Reviewable reports (without ELFs) are under
[`tests/data/logger-selective-study/firmware-20261117`](../tests/data/logger-selective-study/firmware-20261117).
The source manifest identifies the exact working-tree inputs measured before
publication by file hashes and base HEAD. ELFs and extracted analysis images
are kept locally; the reports and reproduction tools accompany this source.

Reproduce from the repository root with PowerShell:

```powershell
& ./tools/logger_firmware_study.ps1
```

The script uses bundled Rust/Cargo 1.99.1 and the cached Cargo home, offline
locked builds for `thumbv7em-none-eabihf`, release optimization `z`, LTO and
one codegen unit. It creates a fresh output directory and saves build logs,
ELFs, source/toolchain manifest and `footprint.json`. It neither packages an
update nor accesses any meter. The default output directory contains a unique
suffix so it cannot overwrite previous evidence. Interpreter/target/output
paths can be supplied with the script parameters.

`tools/logger_firmware_footprint.py` reads ELF32 PT_LOAD segments and symbols
using Python's standard library. Flash load byte counts include `.data`'s
flash copy; main flash span also includes alignment gaps. Static RAM includes
`.data`, `.bss`, `.uninit` and `.sram2`; linker stack reservation is
`_stack_start - _stack_end`. These are not runtime stack maxima, heap usage,
flash-programming timing, compressed package sizes or installer erase margins.
The main flash span and upper load size for all three saved ELFs were also
independently checked by extracting the relevant sections with bundled
`rust-objcopy`; the lengths match (`objcopy-crosscheck.json`). Those `.bin`
files are analysis outputs without an install container; do not flash them.
Although the two logger upper sections have equal lengths, their extracted
bytes/hashes differ; equal section size does not make their main/upper images
interchangeable.

## Verification and remaining limits

```powershell
$env:PATH = (Join-Path (Get-Location) 'target\capture-toolchain\bin-dist\bin') + ';' + $env:PATH
$env:CARGO_HOME = Join-Path (Get-Location) 'target\capture-toolchain\cargo'
cargo test -p firmware-core --tests --target-dir target/logger-selective-host --locked --offline
& 'C:\Users\James\AppData\Local\Python\pythoncore-3.14-64\python.exe' -m unittest discover -s tools -p 'test_logger*.py' -q
```

All **65 Rust integration tests** passed, including 11 in `selective_logger`
(seven existing native journal tests plus four policy/replay/ownership tests).
Normal charging-control, screenshot, settings and temperature host tests are
included. All **17 Python logger tests** also passed. The native tests cover
selection/heartbeat/reset, signed extremes, conservative routine suppression,
replay equality, CSV export, unknown contents, mixed full/selective ownership
and torn-tail behavior. They do not execute the MCU flash peripheral or the
async writer on hardware.

The screenshot/logger compile-time alternative predates this change and
remains. Neither a successful build nor these host tests establish simultaneous
screenshot logging, active-sink PD recording, runtime reception reliability,
real charging capacity or physical recovery. Ripple and QR are not added.
At the footprint-study stage, no package audit was performed. The later
offline packaging results are linked above. No device write, clearing,
installation or new hardware test has been performed for this integration.

## 日本語要約

通常のPD Sink／QC／SCP／FCP／AFC操作を残し、`selective-logger`でVI変化点と
重要PDを既存のjournal writer／USB CSV exporterへ接続した。機能を削る
observer設定は使わない。スクリーンショットとの排他選択は従来どおり。
PC用XSC1形式の移植ではなく、固定長の実機向けjournal形式を再利用した実装。
既存2ログのPC上の再生では選択結果がPython版と一致し、864／896 Bを使用、
各16 KiB確保だった。Rust 65テストとPython 17テストが成功。
ARM比較値はELFの静的な値であり、実行時stack最大値や実機動作を示さない。
同じ変更後ソースのfull loggerに対し、選択版のflashは136 B増、静的RAMは
16 B増。共通の形式識別・出力対応も含め、保存済みの変更前full loggerからは
flashが452 B増える。選択版のmain空き4,372 B、upper空き216 B、リンカー上の
stack予約43,836 B。defaultは以前のELFと同じSHA-256で、main空きは28 B。
能動PD通信の記録、実充電容量、電断復旧は未検証。実機には書き込んでいない。
