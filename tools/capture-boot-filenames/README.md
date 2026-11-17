# Boot-filename writer model (no device access)

[Feature/storage/build scope](../../docs/CAPTURE-BOOT-FILENAMES.md).
`check_storage.py` compiles the current `firmware/src/screenshot.rs` against a
RAM-only flash/bus/timer model. It changes only raw flash pointer reads, task
attributes, test visibility and reset of the boot cache. It preserves the
actual scanner, allocation, sorting, journal framing and save implementation.
Erase calls panic. Tests must run serially because the flash model is shared.

```powershell
python tools/capture-boot-filenames/check_storage.py
python tools/capture-boot-filenames/check_storage.py --backup PRIVATE_CAPFLASH.BIN --out-dir build/NEW_MODEL_RUN
```

The backup is optional and read-only. Without it the test reports omission;
synthetic tests still run. Generated adapters, logs and executable are kept in
a new unique directory under `build/`, never written to the device or source
test directories. An existing output directory is rejected. Cargo builds only
the host library, and rustc builds/runs the model. No flasher or USB operation
is used. A failed check is a failed PC model, not a physical hardware result.

On this Windows workspace the usable Python is
`%LOCALAPPDATA%/Python/pythoncore-3.14-64/python.exe`. Set up the
local Rust cache and GNU host linker in PowerShell before host commands:

```powershell
$studyRoot = (Get-Location).Path
$studyRust = Join-Path $studyRoot 'target/capture-toolchain/bin-dist'
$studyCC = Join-Path $studyRoot 'build/one-step-windows-20261116/toolchain/w64devkit/bin'
$env:PATH = $studyCC + ';' + (Join-Path $studyRust 'bin') + ';' + $env:PATH
$env:CARGO_HOME = Join-Path $studyRoot 'target/capture-toolchain/cargo'
$studyDlltool = (Join-Path $studyCC 'dlltool.exe').Replace('\','/')
$studyLinker = (Join-Path $studyRust 'lib/rustlib/x86_64-pc-windows-gnu/bin/self-contained/x86_64-w64-mingw32-gcc.exe').Replace('\','/')
$env:RUSTFLAGS = '-C dlltool=' + $studyDlltool + ' -C linker=' + $studyLinker + ' -C link-self-contained=yes'
```

Do not add an explicit host `--target` to these host Cargo commands: with this
environment, keeping the host implicit applies the linker flags to host build
scripts too. Before an **ARM** build, remove the host-only `RUSTFLAGS` and
`CARGO_ENCODED_RUSTFLAGS`; keep the normal firmware linker layout unchanged.

日本語：現在の実装をPCのRAM flashへ接続して検証する補助ツール。
実機操作はなく、既存バックアップは読むだけ。実機のUSB・電断復旧を証明しない。
