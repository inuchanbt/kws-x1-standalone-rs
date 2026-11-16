# Automatic capture / PD timing experiment

This experimental branch publishes the instrumented firmware and evidence for
[the 2026-11-16 comparison](../../docs/CAPTURE-PD-VALIDATION-20261116.md).
It is based on nieldk's `one-step-install` commit
`6201a9ce407e8bd72dba99e885612186d7b72fe6`, with the normal screenshot and
trigger features retained. The test feature is `capture-autotest`.

## Recheck the published evidence

Python 3.10+ and its standard library are sufficient. This command checks every
distributed file hash, reconstructs the three trace comparisons, and checks the
exported image counts/dimensions. It does not open a serial port or USB device.

```powershell
python tools/capture-autotest/replay.py
python -m unittest discover -s tools/capture-autotest -p test_analyze.py -v
```

Results go to `build/capture-pd-replay-20261116/`. The data directory includes
verbatim CY4500/ASD CSV exports, the instrumented traces, and all sixteen BMPs
with pixel-verified PNG previews. The BASE snapshot contains only its original
committed trace; see [provenance](../../tests/data/capture-pd-20261116/PROVENANCE.json).

## Hardware controls

The experiment requires **slider OFF/PASS and ENERGY MEMORY OFF**. KWS observes
traffic passively; ASD-PD31 performs the active EPR AVS negotiation. The start
guard refuses an active KWS sink configuration. The device mode resets to BASE
after every power loss. The host's `--mode` argument does not set the meter.

| Runtime control | Action |
| --- | --- |
| Hold Back + OK for one second, then release | Cycle BASE -> SCREEN -> TALL while READY |
| Hold Back + Left + Right for one second, then release | Start; repeat to finish |

Connect measurement wiring before selecting the desired READY mode. Confirm the
matching RUNNING label before Enter at the host's READY prompt. At DONE, finish
on the meter and wait for TEST SAVED before Enter, so the RAM trace is committed
before LOAD_OFF. TEST SAVED is a persistence result; offline analysis determines
whether the experiment has usable evidence.

One 8 KiB sector is reserved before observation. PD events and capture markers
are buffered in RAM; the trace is flushed only after observation ends. Ordinary
GoodCRC and EPR keepalive messages are counted rather than individually logged.
Eight Request/EPR_Request events trigger eight captures in SCREEN/TALL.

Copy evidence before clearing. At PC USB 5 V, hold Back + Left + Right during
connection, release all buttons, then hold OK for two seconds until CAPTURES
CLEARED. Export by reconnecting with Left + Right held. Check readiness with:

```powershell
python tools/capture-autotest/check_clear.py F:\CAPFLASH.BIN
```

The eight-capture plan requires the entire six-sector pool blank at the start.
Old logger data can remain outside the ordinary capture-clear ownership rules.
The local pilot needed backup-matched maintenance to reclaim three such sectors;
the published branch makes no general erase assumption about unfamiliar data.

## ASD/CY4500 controller

The sweep adapter reuses `tools/logger-sweep/kws_avs_sweep.py`. Hardware runs need
the separately installed, reviewed ASD-PD31 CLI/protocol dictionary and CY4500 CLI.
The adapter pins the ASD CLI SHA-256 and retains its load/voltage protections.
Offline replay above does not need these external controllers.

```powershell
python tools/capture-autotest/run_sweep.py --mode screen --port COM3 --plan
python tools/capture-autotest/run_sweep.py --mode screen --port COM3
python tools/capture-autotest/run_sweep.py --mode tall --port COM3
```

Run one condition at a time after selecting the matching device mode. ASD must
already be on panel OBJ9/native Object 11, EPR AVS, 20 V / 0.5 A, LOAD ON.
Per-step holds are randomized within 3.0-4.4 s and recorded in the run plan.
Successful completion prints the exact export command for that new run folder.

The collector copies the raw snapshot and BMPs, refuses overwrites, and runs the
analyzer. Older traces are selected only when expected mode, all 24 ordered PD
signatures and request timing identify a unique candidate. A failed capture is
retained as a failure. Ambiguous/corrupt/mismatched evidence requires review.

## Build the instrumented firmware

The recorded build used Rust/Cargo 1.99.1, Windows GNU x64, and locked dependencies.
The full instrumented application requires identical-code folding and machine
outlining to fit the original memory bounds. The normal trigger functions were
retained. Other toolchain versions can produce different sizes and hashes.

```powershell
cargo +1.99.1 build --release -p firmware --bin unpacker --features capture-autotest --target thumbv7em-none-eabihf --locked
cargo +1.99.1 rustc --release -p firmware --bin firmware --features capture-autotest --target thumbv7em-none-eabihf --locked -- -C link-arg=--icf=all -C llvm-args=-enable-machine-outliner=always
cargo +1.99.1 build --release -p firmware-pack -p flasher --locked
New-Item -ItemType Directory -Force build/capture-autotest | Out-Null
$elf='target/thumbv7em-none-eabihf/release'
target/release/firmware-pack.exe pack --stage usb --firmware "$elf/firmware" --unpacker "$elf/unpacker" --out build/capture-autotest/firmware.X1C
$imageHash=(Get-FileHash build/capture-autotest/firmware.X1C -Algorithm SHA256).Hash.ToLowerInvariant()
target/release/firmware-pack.exe audit --stage usb --image build/capture-autotest/firmware.X1C --confirm-sha256 $imageHash --firmware "$elf/firmware" --unpacker "$elf/unpacker"
target/release/flasher.exe --dry-run --image build/capture-autotest/firmware.X1C --confirm-sha256 $imageHash
```

These commands build/audit only. See [the one-step installation guide](../../docs/ONE-STEP-INSTALL.md)
for device installation and the distinction between host transfer completion and
the meter finishing its internal installation. This validation publication does
not create a firmware release or qualify Windows power-loss/downgrade recovery.

## 日本語補足

公開データの再解析は最初のreplay.pyだけで実行でき、機器には接続しない。
実機試験ではPASS/OFF・ENERGY MEMORY OFFを使い、測定配線を接続した後で
本体モードを選ぶ。電源断でBASEへ戻る。試験結果の範囲は受動観測であり、
KWS自身のアクティブなトリガ動作中や、より長い縦長画像は別の検証対象。
