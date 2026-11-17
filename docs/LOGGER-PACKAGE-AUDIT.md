# Selective logger package and offline installer audit (2026-11-17)

**Later hardware follow-up:** the matching package was subsequently installed
on one device. [Installation and scoped runtime results](LOGGER-HARDWARE-VALIDATION-20261117.md).
This document remains the earlier offline-only audit record.

The selective logger at `dfbaf84d9aa9da69734fbfeab354c02899b04922` was rebuilt
with its matching unpacker and packaged using the current branch's existing
installer. No firmware features, flash boundaries or updater implementation
were changed. **No device was discovered, opened, queried, erased or written.**
This report covers PC-side builds, package audits, dry runs and synthetic
decoder/install replay. It does not establish hardware installation/recovery.
The [proposed hardware trial](LOGGER-HARDWARE-TEST-PLAN.md) separately documents
backup, two-stage install and staged runtime checks as proposed before that trial.

## Result

The complete single-file package was rejected for exceeding the 209,916 B
application content budget by **15,773 B**. Its required content would be
225,689 B: unpacker 15,340 B + compressed upper 78,921 B + compressed main
131,428 B. No complete image was emitted. No functions were removed to fit it.

The existing **two-stage** form passed with the matching `firmware-upper.X1C`
and `firmware.X1C` from the same firmware/unpacker ELF pair:

| Property | Upper stage | Main stage |
| --- | ---: | ---: |
| X1C file bytes (padded container) | 210,498 | 210,498 |
| Payload bytes | 210,496 | 210,496 |
| Application content used in container | 94,261 | 209,916 |
| Compressed upper stream | 78,921 B | omitted |
| Compressed main stream | omitted | 131,428 B |
| Structural audit | pass | pass |
| Flasher `--dry-run` | pass | pass |
| Bootloader erase plan | 26 sectors | 26 sectors |
| Bootloader erase range | `[0x10000, 0x44000)` | `[0x10000, 0x44000)` |

Both containers have fixed padding; equal file sizes do not indicate equal
content. The main package's zero unused *container* content space comes from
placing its compressed stream at the end of the budget. The running main
firmware still has the previously measured 4,372 B linker headroom, and upper
code has 216 B free. These are different measurements.

Image SHA-256 values:

```text
d409ec43812022be07cdeae451207198c4bcdd6a9a17419d382c5d9d990cfbbe  firmware-upper.X1C
3c8aeb66f03d1f5c0b1fac87625810a40ef06d1145db4464472e8f6ee51e7eca  firmware.X1C
```

Local outputs are under `build/selective-logger-package-20261117/`:

- `firmware.elf`, `unpacker.elf` and build/test logs.
- `checked/`: pack/audit JSON, immutable tool/input copies, exact commands,
  dry-run logs, deliberate negative-test copies and `SHA256SUMS`.
- `candidate/`: only the two update containers, matching flasher, licenses,
  hashes, offline-only check script and explanatory README.
- `selective-logger-offline-candidate.zip`: the clean candidate folder.
- `install-replay.json`: per-sector synthetic install trace.

The candidate and ZIP are **hardware-untested offline outputs**. They are not
a single-file/one-command installer. This branch does not incorporate the
separately evaluated one-step installer from #28. Actual write commands are
not provided or executed by this workflow.

## Source and toolchain identity

Fresh ARM release builds used Rust/Cargo 1.99.1, `thumbv7em-none-eabihf`,
`selective-logger`, `--locked --offline`, and the unchanged release profile.
The firmware ELF exactly matches the previous footprint-study selective ELF:

```text
ce35bfca3f1d64780c208dc84ee5101c3861a34b46a4622d00b3694fc73b45f5  firmware.elf
50467b4bb90aecc4493122b86cc33a2c193083a7d1f3006b881938e25ad40384  unpacker.elf
```

The packer and flasher were freshly built from the same source revision for
Windows GNU using the cached w64devkit C compiler/dlltool and Rust's matching
self-contained linker. The Windows C HIDAPI backend was retained. Initial
host build failures were toolchain-path/linker configuration issues; they did
not require source or feature changes. No toolchain was downloaded.

The exact tool/input hashes and audit reports are recorded in
[`tests/data/logger-selective-study/package-20261117`](../tests/data/logger-selective-study/package-20261117).
The offline helper/example sources accompany this follow-up; their measured
bytes are identified by the manifest. They were added after the firmware
commit above and do not change that firmware's code or updater.

## Audit and replay scope

`tools/logger_package_audit.py` creates a new directory, copies its inputs,
and runs the production packer and flasher. It requires `--dry-run` and
rejects `--info` for every flasher invocation. The current flasher loads and
validates the image before returning from the dry-run branch, ahead of any
`HidApi::new()`/device selection.

For each image, canonical audit rebuilds the container from the exact ELF
pair and checks its hash, layout, vectors, footer/checksum, region sizes and
decoder/erase-order rehearsal. Three deliberate negative cases were rejected:

- Wrong confirmed SHA-256 on an otherwise valid package.
- Corrupted container prefix with the matching SHA-256 of that corrupt file.
- Auditing the selective package against the full-logger ELF.

The existing `firmware-pack` CLI regression suite passed **10 tests**, including
refusing to overwrite output, layout limits, filtered decompression, final
sector completion/tail preservation, malformed files and canonical-audit
tampering checks. These tests use synthetic files and do not access USB.

`firmware-pack/examples/logger_install_replay.rs` additionally replays the
actual two stage files against a patterned synthetic 512 KiB flash array,
using the production `inflate_sectors`, `main_erase_margin` and final-sector
tail API. Expected region bytes were independently extracted from the matching
ELF with `rust-objcopy`; the main upper-identity record is patched as required
by the package format. The replay checks:

- Upper/main stage identity and decoded bytes match their expected regions.
- Each main erase leaves the still-unread compressed bytes intact.
- While compressed bytes remain unread, the smallest erase margin is **5,115 B**.
- The final main sector is erased only after all compressed bytes are consumed,
  and its unwritten tail, including the container footer, is preserved.
- Bootloader bytes and every byte at or above `0x62000` remain unchanged in the
  synthetic model, covering log/capture, factory and settings regions.

The production audit's `erase_margin_bytes: 0` includes the final sector:
`main_erase_margin` explicitly returns zero there only when all stream bytes
have been consumed. It is not a measured positive unread-data margin. The
5,115 B value above excludes callbacks with no unread compressed bytes.

This replay models the upload range and erase-required path in RAM. It does
not execute the MCU, the EFM peripheral, USB transport or power interruptions;
it cannot establish physical recovery, flash timing or reliable reception.
An actual install would be non-atomic. Device operation remains untested for
this selective build. Its PD hook remains passive, its native journal differs
from PC XSC1, and logging remains exclusive with screenshot capture/export.

## Reproduction

Build firmware and unpacker with the same locked offline command, preserving
both ELFs in a new folder:

```powershell
$studyRust = Join-Path (Get-Location) 'target/capture-toolchain/bin-dist'
$env:PATH = (Join-Path $studyRust 'bin') + ';' + $env:PATH
$env:CARGO_HOME = Join-Path (Get-Location) 'target/capture-toolchain/cargo'
cargo build -p firmware --bin firmware --bin unpacker --release --features selective-logger --target thumbv7em-none-eabihf --target-dir target/logger-package-study-arm --locked --offline
```

For the tested Windows GNU host tools, use the cached C toolchain with an
explicit dlltool and Rust's self-contained linker. These flags apply to host
tools, not the ARM firmware command above:

```powershell
$studyCC = Join-Path (Get-Location) 'build/one-step-windows-20261116/toolchain/w64devkit/bin'
$env:PATH = $studyCC + ';' + $env:PATH
$studyDlltool = (Join-Path $studyCC 'dlltool.exe').Replace('\','/')
$studyLinker = (Join-Path $studyRust 'lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained/x86_64-w64-mingw32-gcc.exe').Replace('\','/')
$env:RUSTFLAGS = '-C dlltool=' + $studyDlltool + ' -C linker=' + $studyLinker + ' -C link-self-contained=yes'
cargo build -p firmware-pack -p flasher --release --target-dir target/logger-package-study-host --locked --offline
cargo test -p firmware-pack --test cli --release --target-dir target/logger-package-study-host --locked --offline
cargo build -p firmware-pack --example logger_install_replay --release --target-dir target/logger-package-study-host --locked --offline
```

Run the offline pack/audit helper with a fresh output path. The command below
uses the saved matching ELFs and current-source tools. The helper refuses an
existing output directory:

```powershell
$studyPython = 'C:\Users\James\AppData\Local\Python\pythoncore-3.14-64\python.exe'
$studyOut = Join-Path 'build' ('selective-package-repro-' + [guid]::NewGuid().ToString('N'))
& $studyPython tools/logger_package_audit.py --firmware build/selective-logger-package-20261117/firmware.elf --unpacker build/selective-logger-package-20261117/unpacker.elf --packer target/logger-package-study-host/release/firmware-pack.exe --flasher target/logger-package-study-host/release/flasher.exe --mismatched-firmware build/logger-firmware-study-20261117/full-logger.elf --out $studyOut
& target/logger-package-study-host/release/examples/logger_install_replay.exe --upper-image (Join-Path $studyOut 'firmware-upper.X1C') --main-image (Join-Path $studyOut 'firmware.X1C') --expected-upper build/logger-firmware-study-20261117/selective-logger.upper-load.bin --expected-main build/logger-firmware-study-20261117/selective-logger.main-load.bin --out (Join-Path $studyOut 'install-replay.json')
```

Use a fresh PowerShell process for these setup blocks so host `RUSTFLAGS` do
not leak into later ARM builds. Paths to cached tools are local prerequisites;
other hosts need matching Rust and a suitable Windows C toolchain.

## 日本語要約

選択版と一致するunpackerを新規ビルドし、既存の2段階更新形式でパッケージを
作成した。選択版ELFは前回のコード／RAM測定時と同じSHA-256。単一ファイルは
15,773 Bの容量超過で拒否されたため作成されていない。機能削減はしていない。

上位用・main用の2ファイル（各210,498 B）は構造監査とflasherのdry runに成功。
不正ハッシュ、破損コンテナー、別ELFの組合せも拒否された。既存10テスト成功。
合成flashモデルでは全展開データ、一致する上位ID、未読圧縮データ、最終セクター
末尾、bootloaderおよび0x62000以降の保持を確認。未読データが残る区間の最小
消去余裕は5,115 Bだった。監査値0 Bは、最終セクターで全圧縮データを読み切った
場合の仕様値であり、途中区間の余裕と区別する。

すべてPC上の検証で、実機の探索・接続・問合せ・消去・書込みは行っていない。
実機インストール、電断復旧、動作は未検証。今回の候補は2ファイル方式で、
本家#28で別途検討中のone-step installerを取り込んだものではない。
