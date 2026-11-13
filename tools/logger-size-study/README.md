# Offline logger size probes

See [findings](../../docs/LOGGER-SINGLE-FILE-STUDY.md). Never flash these probes.
They are not production features or release assets.

Prepare new disposable source copies from the current checkout:

```powershell
python tools/logger-size-study/prepare.py target/logger-size-full
python tools/logger-size-study/prepare.py target/logger-size-passive --passive
python tools/logger-size-study/prepare.py target/logger-size-relocated --passive --relocate-packer
```

For each copy, clear any `KWS_X1_RECLAIM_SHA256` environment value and build
the firmware with `--features charging-logger`, the locked dependencies and the
ARM target. Use separate build directories so normal release artifacts stay
untouched. For example:

```powershell
Remove-Item Env:KWS_X1_RECLAIM_SHA256 -ErrorAction SilentlyContinue
cargo build --manifest-path target/logger-size-relocated/Cargo.toml --target-dir target/logger-size-arm -p firmware --release --target thumbv7em-none-eabihf --features charging-logger --locked --offline
cargo build --manifest-path target/logger-size-relocated/Cargo.toml --target-dir target/logger-size-host -p firmware-pack --release --locked --offline
```

Run that copy's firmware-pack with `pack --stage complete`, both matching
ELFs and a new output filename. Follow with its `audit --stage complete` and
the flasher's `--dry-run`, never a hardware write. Omitting `--relocate-packer`
reproduces the original layout's erase-margin rejection for the reduced probe.
The original full-function logger remains over budget even with relocation.

The passive probe removes only the active PD sink awaits before async task
generation. It does not adapt UI, slider behavior or other trigger controls.
No runtime behavior of these variants has been hardware-validated.

日本語：正式な製品版ではなくサイズ試験用です。結果は上記の英語正本・日本語要約を
参照してください。隔離コピーだけを変更し、実機へは書き込まないでください。

## Production Writer host tests

`writer_harness.py` adapts the actual `firmware/src/logger.rs` Writer: only
flash, channel, bus lock and clock I/O are replaced. `writer_tests.rs` exercises
append/restart, reboot, capacity, torn markers and whole-packet rollover.
The flash model allows only blank-word programming and performs no erase.
It cannot establish device flash timing or real interrupt/USB behavior.

First build firmware-core for the host into target/logger-host (locked/offline),
then run `python tools/logger-size-study/writer_harness.py`. The script uses the
workspace-local toolchain under target/capture-toolchain, writes generated
harness/executable files into target, and makes no hardware calls.
