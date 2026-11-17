# Selective charging log: offline capacity study

## Native firmware integration follow-up (2026-11-17)

See [the firmware integration and controlled ARM comparison](LOGGER-FIRMWARE-INTEGRATION.md).
The new `selective-logger` feature connects the selection policy to the
existing bounded journal writer and USB CSV exporter, retaining normal
charging controls. It uses versioned fixed-record native journals, **not host
XSC1**. In fresh same-source ARM release builds, enabling it versus the updated
full logger adds 136 B of flash load image and 16 B of static RAM; both have
216 B upper space remaining. Shared ownership/export additions cost another
316 B compared with the archived pre-change full logger. Native replay uses
864 / 896 B committed and allocates two sectors (16 KiB) per fixture; the host
XSC1 sizes below are unchanged. No hardware write or package test was done.
The earlier host-only review/build statements below describe earlier stages.

## Host review follow-up (2026-11-17)

The latest [upstream response in #30](https://github.com/kralonur/kws-x1-standalone-rs/issues/30#issuecomment-6920330136)
asks for a representative real charging trace and the selective codec's actual
firmware code/RAM footprint with normal charging controls retained. The
previous offline results were already posted; they do not need reposting.
Ripple and QR remain separate. No real charging trace was attached in the
subsequent comment when checked on 2026-11-17 (JST).

Review found and fixed three host-code issues: complete packets with unknown
SOP values now retain the entire original row through the JSON escape;
metadata must be a JSON object in the first record; and an extra sector after
the end marker is rejected even if its header CRC is valid. The CLI also
checks the restored session ID explicitly. These changes leave both existing
fixture binaries, sizes and SHA-256 hashes unchanged.

`python -m unittest discover -s tools -p "test_logger*.py" -q` passed **17 host
tests** (nine selective-codec tests and eight existing decoder/comparison
tests). The added fixture regression checks both captures' input hashes,
selected events, byte-identical re-encoding, output hashes and size reports.
The saved default/full-logger ELF hashes and section reports were rechecked
and match. **No fresh ARM build, package generation, installation or hardware
test was performed in this follow-up.** The ARM results below remain the
previously saved baseline; no selective firmware footprint is available yet.

### Firmware integration gate (design requirements, not implemented)

Before integrating a writer, define a bounded firmware format and allocation
policy. The host format is an experimental reference: its arbitrary JSON
escape is not a bounded embedded event representation. A firmware design must
retain supported unknown/incomplete raw PD and loss diagnostics without
silently truncating them, and explicitly handle values/events it cannot
represent. Any change to the binary format needs an explicit version and a
matching decoder; the host fixture version must not silently change meaning.

The writer must account for headers, metadata, padding, unusable sector tails
and termination/loss records before accepting an event. It must stay inside
the six reserved sectors, use only qualified available sectors, and stop
without wrapping or erasing saved logs, screenshots or unknown contents.
Do not assume all six sectors are available. Session IDs, u32 timestamp/event
limits and allocation failure need explicit stop behavior. Power-cut prefix
recovery requires a separate design and tests; this strict host decoder does
not provide it. Export must distinguish an intact ended session from an
interrupted/invalid one. USB export and flash programming remain unimplemented
for this selective format.

For a meaningful footprint comparison, use the same source and toolchain,
retain normal charging controls, and actually connect the selective encoder,
bounded queue, writer and exporter so the linker cannot discard them. Archive
each ELF separately and compare allocated sections, flash load images,
queue/buffer RAM, stack reservation and the generated update package. Record
any replaced screenshot/logger functionality: a replacement build cannot
establish simultaneous support or the cost of pure addition. The current PD
hook covers the passive sniffer only; active-sink logging needs its own
implemented observation path. Runtime stack use, PD reception and scheduling
still need hardware measurements after that combined implementation exists.
This review does not propose another hardware test yet.

## Measured host codec follow-up (2026-11-16)

The original 624-byte model below is **superseded as an implementation size
claim** by the concrete host codec in `tools/logger_selective_codec.py`.
It is still useful only as an explicitly hypothetical lower-cost model.

| Existing capture | Selected VI / PD | Encoded committed bytes | Allocated bytes | Decode equality |
| --- | ---: | ---: | ---: | --- |
| Observer v0.7, 55.025 s sample span | 12 / 24 | 1,060 | 8,192 | pass |
| Logger v0.4, about 64 s | 14 / 24 | 1,108 | 8,192 | pass |

The stored fixtures and reports are under `tests/data/logger-selective-study`.
Equality covers selected VI time/voltage/current and PD time/event ID/SOP/raw
header/all data objects. Session is in the sector header; message type, ID and
extended flag are derivable from the raw header. Unknown or incomplete events
use a JSON escape and retain all original fields. Routine suppression is
restricted to the known complete SOP patterns. This remains lossy selection:
the removed VI points and routine traffic cannot be recovered.

Reproduce the new results (Python standard library only):

```powershell
$studyPython = 'C:\Users\James\AppData\Local\Python\pythoncore-3.14-64\python.exe'
# Unique output directory preserves previous local runs and hardware evidence.
$studyOut = Join-Path 'build' ('logger-selective-repro-' + [guid]::NewGuid().ToString('N'))
& $studyPython tools/logger_selective_codec.py tests/data/logger-observer-v07-avs-20261113/VI000101.CSV tests/data/logger-observer-v07-avs-20261113/PD000101.CSV --out-dir (Join-Path $studyOut 'v07')
& $studyPython tools/logger_selective_codec.py tests/data/logger-avs-20261113/LOG00001.CSV tests/data/logger-avs-20261113/LOG00002.CSV tests/data/logger-avs-20261113/LOG00003.CSV --out-dir (Join-Path $studyOut 'v04')
& $studyPython -m unittest discover -s tools -p 'test_logger*.py' -q
```

On other hosts, substitute a working Python 3 interpreter. The study scripts
use the standard library only. The full suite also re-encodes and checks the
archived fixtures without modifying them.

The original five host tests passed: public-capture equality; 1,101 events across multiple
sectors with negative current and seven PD data objects; preservation of
unknown/incomplete/loss events; 60-second heartbeat and non-SOP retention;
every single-byte mutation of a small image's committed area, truncation and
a missing end marker rejected. These tests demonstrate file serialization,
not flash programming, recovery after a power cut, or continuous reception.

Concrete format: sectors are 8,192 bytes. The 32-byte header has magic `XSC1`,
version, session, zero-based sequence, sector size, header size, reserved word
and CRC32 over the first 28 bytes (little endian). Each record has a 12-byte
prefix `<BBHII>` (kind, zero flags, payload length, elapsed ms, event ID),
payload, CRC32 over prefix+payload, then erased-byte padding to a four-byte
boundary. Records never cross sectors. Kind 1 VI payload is `<Ii>` mV/mA;
kind 2 PD payload is SOP byte, u16 header, then u32 data objects; kinds 3/5
are JSON metadata/escaped unknown events; kind 4 contains the u32 event count
and marks a complete session. CRC32 is Python zlib's CRC32. Timestamps are u32
milliseconds; overflow is rejected by packing, not wrapped.

Thus VI records are 24 B, no-object PD records 20 B, two-object EPR Requests
28 B, and seven-object packets 48 B. The 55-second image contains 832 B of
event records, 176 B of session metadata, 20 B of end record and 32 B of
sector header. One binary file is a single session; unused tails are not yet
shared across sessions in this host codec. Repeated-session capacity must
therefore not use the old model's shared-tail assumption. There is no firmware
writer, USB exporter or ripple record in this codec yet. It does not enforce
the device's six-sector limit; larger images are useful for host boundary
tests and must not be flashed.

### Fresh baseline builds

Cargo/Rust 1.99.1 were found under `target/capture-toolchain/bin-dist/bin`,
with cached dependencies under `target/capture-toolchain/cargo`. This corrects
the earlier environment limitation below. At source HEAD
`abc4dba2bae404edfdae8d2aedc915eef71e3c7c`, an offline locked ARM release build
with `--features charging-logger` succeeded, without disabling active charging
controls. Its `.upper` is 122,656 B / 122,880 B, leaving **224 B**. This is the
existing full-traffic logger, **not the new selective codec**. The build alone
does not establish active-sink logging coverage, USB export, update packaging,
or simultaneous operation on hardware. Existing screenshot and logger modes
are compile-time alternatives. No firmware functionality was removed here.

The same source/toolchain also built the default screenshot firmware:

| ELF allocated section | Default screenshot | Existing full logger | Difference |
| --- | ---: | ---: | ---: |
| `.upper` | 122,648 B | 122,656 B | +8 B |
| `.text` | 179,436 B | 174,332 B | -5,104 B |
| `.rodata` | 26,052 B | 26,368 B | +316 B |
| `.data` | 3,752 B | 3,752 B | 0 B |
| `.bss` | 41,616 B | 42,252 B | +636 B |
| `.sram2` | 64,800 B | 64,800 B | 0 B |

The decrease in main code reflects replacing screenshot functionality, not
the incremental cost of adding logging alongside screenshots. Both passed
the current linker layout and stack assertions; stack assertions are not
runtime high-water measurements. `.data` also has a flash load image, so
`.text` alone is not the complete application/container size. The PD logging
hook currently sits in the passive sniffer path (`pd_task.rs`); compiling the
active sink does not establish that its traffic is recorded.

Build commands after adding the toolchain bin directory to PATH and setting
`CARGO_HOME` to `target/capture-toolchain/cargo`:

```powershell
$studyPython = 'C:\Users\James\AppData\Local\Python\pythoncore-3.14-64\python.exe'
$env:PATH = (Join-Path (Get-Location) 'target\capture-toolchain\bin-dist\bin') + ';' + $env:PATH
$env:CARGO_HOME = Join-Path (Get-Location) 'target\capture-toolchain\cargo'
$studyElfOut = Join-Path 'build' ('logger-arm-repro-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $studyElfOut | Out-Null
cargo build -p firmware --bin firmware --release --features charging-logger --target thumbv7em-none-eabihf --target-dir target/logger-coexist-study --locked --offline
if ($LASTEXITCODE -ne 0) { throw 'Full logger build failed' }
Copy-Item -LiteralPath 'target/logger-coexist-study/thumbv7em-none-eabihf/release/firmware' -Destination (Join-Path $studyElfOut 'full-logger.elf')
cargo build -p firmware --bin firmware --release --target thumbv7em-none-eabihf --target-dir target/logger-coexist-study --locked --offline
if ($LASTEXITCODE -ne 0) { throw 'Default build failed' }
Copy-Item -LiteralPath 'target/logger-coexist-study/thumbv7em-none-eabihf/release/firmware' -Destination (Join-Path $studyElfOut 'default.elf')
& $studyPython tools/logger_elf_sections.py (Join-Path $studyElfOut 'full-logger.elf') --out (Join-Path $studyElfOut 'full-logger-sections.json')
& $studyPython tools/logger_elf_sections.py (Join-Path $studyElfOut 'default.elf') --out (Join-Path $studyElfOut 'default-sections.json')
```

The ELF section reader `tools/logger_elf_sections.py` and archived section
reports provide the new baseline. The new codec needs a bounded firmware
implementation and integration before its incremental code/RAM and runtime
behavior can be assessed. These host results support a concrete capacity
estimate but do not settle coexistence.

## Original hypothetical model (retained for comparison)

The following is a sizing exercise, not an implemented format, an estimate
of normal charging duration, or a claim that the logger fits the normal build.
Inputs are the observer v0.7 AVS capture in
`tests/data/logger-observer-v07-avs-20261113/`; see its README and SHA256SUMS.
KWS was passive, ASD was the active sink, and the sweep lasted 55.025 seconds
between the first and last of 56 roughly one-second VI readings.

## Reproduce

```powershell
python tools/logger_capacity_study.py tests/data/logger-observer-v07-avs-20261113 --out build/logger-capacity-study.json
python tools/logger_compare.py tests/data/logger-observer-v07-avs-20261113 --session 1
```

The script uses the existing fragment decoder, retains the first VI sample,
then compares each new sample to the last retained sample. A heartbeat, when
enabled, retains the first available sample at or after the deadline; the
timer resets on every retained sample. It cannot invent missing measurements.

| Threshold V/I | Heartbeat | Retained VI | Retained PD | Hypothetical committed bytes |
| --- | --- | ---: | ---: | ---: |
| 100 mV / 100 mA | none | 12 | 24 | 624 |
| 100 mV / 50 mA | none | 12 | 24 | 624 |
| 500 mV / 100 mA | none | 11 | 24 | 608 |
| 100 mV / 100 mA | 10 s or 60 s | 12 | 24 | 624 |

The 10 s heartbeat adds nothing in this particular active sweep; this says
nothing about a stationary charging session. The 100 mV / 100 mA events
include the two observed zero-current readings and subsequent recoveries.
One-second input cannot reveal shorter transients. This threshold is a sizing
parameter, not an accuracy or anomaly-detection guarantee.

The decoder reconstructs 476 complete packets: 238 GoodCRC (control type 1),
107 extended type 16 payload `00038002` and 107 `00048002`, interpreted as
EPR KeepAlive and its ACK, and eight each EPR_Request (data type 9, two data
objects), Accept (control type 3), and PS_RDY (control type 6). The existing
comparison matches all 476 to the contiguous CY4500 overlap. This study
suppresses the first three categories and retains the remaining 24. Unknown,
incomplete and logger diagnostic records are retained by default. A real
implementation needs explicit handling of resets, timeouts, failed requests,
retries, drop notices and errors; the current capture does not prove those
events are observable or representable by the hypothetical record.

## Explicit storage assumptions

For a *hypothetical* unified journal, each selected VI, PD, heartbeat or ripple
event consumes one 16-byte slot including any timestamp, type, value, CRC and
alignment. A session start and end each consume one slot; every 8,192-byte
sector has a 16-byte header and 511 slots. The script reports committed bytes
and full-sector allocation separately. This assumes all relevant event data
fits a slot; multi-object messages, timestamps, integrity scheme, interrupted
writes, sector ownership and recovery require a real design and may cost more.
No compression or this journal has been implemented. Sharing partially used
sectors across sessions is assumed; independent VI/PD streams would cost more.

For 36 selected events the bare payload assumption is 576 bytes; two markers
and one header bring the model to **624 committed bytes**, while one whole
sector is allocated. The actual observer consumed **12,240 bytes**, including
its different PD fragments, management and sector headers. These are not
measured before/after compression figures.

| Illustrative one-hour scenario | VI changes | PD events | Heartbeats | Ripple events | Committed | Allocated |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Quiet | 10 | 10 | 60 | 0 | 1,328 B | 8,192 B |
| Moderate | 60 | 120 | 60 | 0 | 3,888 B | 8,192 B |
| Busy | 360 | 600 | 60 | 0 | 16,384 B | 16,384 B |
| Moderate + ripple every 60 s | 60 | 120 | 60 | 60 | 4,848 B | 8,192 B |
| Moderate + ripple every 1 s | 60 | 120 | 60 | 3,600 | 61,600 B | 65,536 B |

Rates are invented independent examples, including a separate heartbeat each
minute, not measurements or predictions; real events can coincide and be
coalesced. They include one start/end for each one-hour scenario. No inference
of actual charging time or availability of all six sectors follows.

## Ripple investigation at abc4dba2bae404edfdae8d2aedc915eef71e3c7c

`firmware/src/ripple.rs` captures 1,024 ADC1/PA2 samples through PGA gain 2,
DMA1 channel 0. ADC2 measures line/reference, while the panel uses DMA2. The
task runs only on the Ripple view when not held and waits 100 ms after each
capture and analysis; this is not continuous monitoring. Changing timebase or
leaving the view clears the 30-capture peak history. Displayed peak-to-peak is
65% history mean plus 35% history maximum, normalized using the shared ADC
reference; RMS is DC-removed over one complete capture and reported in tenths
of mV. Clipping and missing or invalid reference need explicit validity flags.
The default 50 us/div setting spans about 0.4 ms on screen; the entire 1,024
sample acquisition is longer, approximately 1.13 ms at 30 MHz or 0.90 ms at
37.5 MHz nominal
for that setting. Sampling rate/timebase and analog bandwidth are different
things; bandwidth and absolute ripple accuracy remain uncharacterized.

Background capture needs changes to the page/hold gating **and** the ADC2
reference scan, which currently depends on the active page and trigger state.
Timebase, window, Vpp versus RMS, raw versus smoothed, unit, clipping/reference
validity and acquisition time need recording or session metadata. Suggested
starting option: a clearly labeled periodic single-capture Vpp/RMS summary
every 60 s, optionally a maximum over repeated acquisitions if its actual
sampling schedule is specified. Event-adjacent samples need scheduling and
may miss the event itself; a pre-event sample needs a buffer. Periodic
captures cannot guarantee detection of short spikes or preserve waveform or
frequency components. Two bytes/value alone cost 120 B/hour at 60 s, or 7,200
B/hour at 1 s, before timestamps/metadata; the table instead uses separate
16-byte events. ASD and KWS readings should be compared only with documented
definitions, bandwidth, wiring and timing, not assumed equivalent.

DMA separation does not prove coexistence: ADC2 reference and trigger
handshake, task scheduling, CPU/FFT cost, memory, flash writes, PD receive and
active sink operation all need a combined implementation and measurement.

## Existing build evidence and open work

The previous observer v0.7 audit records main 181,848 B, upper 18,160 B and
21,500 B installer erase margin. It excludes active charging controls. A prior
full logger v0.4 audit records main 199,560 B and upper 122,680 B, leaving
200 B in the 122,880 B upper region; its FLASH guide says not hardware tested.
These are different builds, packaging and source states, not a controlled
comparison. At the original model stage Cargo was not on PATH; the later
toolchain discovery and same-source builds above supersede that limitation.
Link layout in `firmware/memory.x` reserves upper
`[0x44000,0x62000)` and six capture/log sectors `[0x62000,0x6e000)`;
screenshots and this logger therefore compete for that region in this fork.
Code size, RAM, installer and storage layout must be measured on the same
source and actual selected logger design before assessing coexistence.

## 日本語要約

PC用codecで既存2ログを符号化・復元し、約55秒はVI 12件＋PD 24件で
1,060 B、約64秒はVI 14件＋PD 24件で1,108 Bだった。ファイル確保量は
各8 KiB。624 Bは古い仮定モデルであり、実装サイズではない。
2026-11-17のレビューでは未知SOPの元フィールド保持と形式検査を修正し、
既存結果のバイト単位の再現を含む17テストが成功した。保存済みELFと報告値
の一致も確認したが、今回ARMビルドや実機試験は行っていない。
以前のARMビルドは既存のdefault／full loggerで、新codecを含まない。
選択ロガーのコード／RAM／packaging使用量、通常機能との共存、能動PD記録、
電断復旧、実充電時の容量は未検証。#30では実充電ログと実装後の使用量が
次の判断材料とされ、RippleとQRは別扱い。新たな実機試験はまだ提案しない。
