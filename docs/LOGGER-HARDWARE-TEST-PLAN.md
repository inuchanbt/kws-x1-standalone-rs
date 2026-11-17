# Selective logger: proposed two-stage hardware trial

Status: **plan only; no device access or writing performed**. Writing this
plan and publishing its sources does not authorize running the device steps.
Execute them only after a later explicit instruction to begin the hardware
trial, with the backup and artifact checks below complete.

English is primary; the [Japanese checklist](#日本語チェックリスト) follows.
Firmware source: `dfbaf84d9aa9da69734fbfeab354c02899b04922`.
[Offline package evidence](LOGGER-PACKAGE-AUDIT.md) and
[implementation limits](LOGGER-FIRMWARE-INTEGRATION.md) apply.

## Scope and evidence to collect

| Phase | Purpose | Required evidence |
| --- | --- | --- |
| A: preparation | Preserve current data and identify the exact candidate | Existing export, hashes, current firmware/installer identity, restore artifacts, offline candidate checks |
| B: install | Check the matching upper/main stages and normal startup | Separate stage terminal logs, backlight/startup observations, pre/post-install exports |
| C: quiet 5 V | Check selective VI recording, heartbeat, stop/start and persistence | UI observations, VI/PD CSVs when present, STATUS.TXT, STORAGE.BIN, repeat export hashes |
| D: passive AVS | Check important PD selection against an independent capture | Same-run CY4500 and ASD logs, selected CSVs/raw storage, wiring/timing notes |
| E: active charging | Check retained charging controls while logging | Same source/load setup with logging off/on, UI and external V/I observations, exported VI |

Complete each phase before moving on. Neither phases B/C nor retained controls
establish recording of active-sink PD: the hook remains in the passive sniffer.
Screenshots and logging are compile-time alternatives. Native XSL1/XSP1
journals differ from the PC XSC1 codec. Physical power-cut, full-store endurance,
downgrade recovery, QR and background ripple are outside this initial trial.

## A. Preparation and data protection

1. Record the unit, current firmware version/build, installer family (existing
   two-stage versus the separately evaluated one-step path), host/tool versions,
   cable and slider position. Do not assume which build is currently installed.
2. Before replacement, export all wanted existing files at PC 5 V using the
   installed firmware's known read-only workflow. Keep BMPs, CSVs, status and
   raw storage when offered. Use a new local trial directory with separate
   `before/`, `after-install/`, `quiet-session/`, `passive-avs/` and `active/`
   subdirectories; never merge exports from different post-clear runs.
3. Hash the copied files and keep them unchanged. Native logger `STORAGE.BIN`
   is 49,152 B for the six sectors. A normal screenshot build may expose BMPs
   without a raw dump; record that gap. Existing BMPs must be copied before
   replacing the screenshot exporter. Raw storage and user backups stay private
   under ignored `build/`, not in a public commit.
4. Have the operator's known working restore build, its matching updater/tool,
   both stages if applicable, and hashes available. Do not substitute the
   selective main into another build's upper image, or assume an old updater
   understands these new journal signatures. Stop before writing if needed data
   cannot be preserved adequately or the recovery artifacts are unidentified.
5. Do not clear/reclaim storage in this trial. Export uses **Left + Right** at
   boot, at PC 5 V. Do not add Back: that is the clear chord. Cancel any clear
   prompt with Back. A full store is a reason to stop and report, not to erase.

The candidate folder is local:
`build/selective-logger-package-20261117/candidate/`. The ZIP is
`build/selective-logger-package-20261117/selective-logger-offline-candidate.zip`.
Its SHA-256 is
`10be12e640f12e596a7766fb1e2f4f1cc8b9672bb711e495b95964ee5bb062ba`.
These artifacts are hardware-untested and are not a one-file installer.

Run this **offline-only** setup from the repository root, with the meter
disconnected. Keep the same PowerShell session for the later device commands:

```powershell
$ErrorActionPreference = 'Stop'
$trialCandidate = (Resolve-Path -LiteralPath 'build/selective-logger-package-20261117/candidate').Path
$trialFlasher = Join-Path $trialCandidate 'flasher.exe'
$trialExpected = @{
    'firmware-upper.X1C' = 'd409ec43812022be07cdeae451207198c4bcdd6a9a17419d382c5d9d990cfbbe'
    'firmware.X1C' = '3c8aeb66f03d1f5c0b1fac87625810a40ef06d1145db4464472e8f6ee51e7eca'
    'flasher.exe' = '3a953717c0cd3cf1564827f4e1d7490db466053f70544c5289c095cab1c3ce97'
}
foreach ($trialName in $trialExpected.Keys) {
    $trialActual = (Get-FileHash -LiteralPath (Join-Path $trialCandidate $trialName) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($trialActual -ne $trialExpected[$trialName]) { throw "Wrong candidate: $trialName" }
}
& (Join-Path $trialCandidate 'CHECK-OFFLINE.ps1')
if ($LASTEXITCODE -ne 0) { throw 'Offline candidate check failed' }
$trialRoot = Join-Path (Get-Location) ('build/selective-logger-hw-trial-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $trialRoot | Out-Null
```

Create and fill the backup subdirectories before executing phase B. Disconnect
chargers, loads and analyzers for installation. Use a data cable from the PC
to the meter's USB-C receptacle, leave its other connector empty, and set the
physical slider to **OFF/PASS before power-up**. This full-function variant
samples the mode at startup; change PASS/Sink only while unpowered and reconnect.

## B. Two-stage install — future device I/O only

**Do not execute these blocks while merely reviewing/preparing this plan.**
`--info` itself performs device I/O; commands without `--dry-run` write flash.
The current request authorizes preparing this document, not running these steps.

### Upper stage

With the meter unplugged, hold **Back + OK** while plugging into PC 5 V,
release after about one second and confirm the bootloader display. Save a
photo/note of the shown version. Only the intended unit should be connected.

```powershell
# FUTURE HARDWARE STEP: run only after explicit trial authorization.
$trialUpperAttempt = Join-Path $trialRoot ('upper-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $trialUpperAttempt | Out-Null
$trialInfo = & $trialFlasher --info 2>&1
$trialInfo | Tee-Object -FilePath (Join-Path $trialUpperAttempt 'info.log')
if ($LASTEXITCODE -ne 0 -or ($trialInfo -join "`n") -notmatch '\bmode=0\b') { throw 'Stop: bootloader mode not confirmed' }
& $trialFlasher --image (Join-Path $trialCandidate 'firmware-upper.X1C') --confirm-sha256 $trialExpected['firmware-upper.X1C'] 2>&1 | Tee-Object -FilePath (Join-Path $trialUpperAttempt 'write.log')
if ($LASTEXITCODE -ne 0) { throw 'Stop: upper stage failed' }
```

Bootloader acceptance is followed by the unpacker running on the meter. Wait
for **two short backlight flashes followed by steady illumination**; that is
the existing upper-only waiting state. It is not normal measurement startup.
Record the observed behavior. Do not proceed if that state was not observed;
terminal success alone does not establish completion of the on-device unpacker.

### Main stage

After the upper waiting state, unplug and re-enter the bootloader using
**Back + OK**. This second connection is intentional; no automatic handoff is
implemented in this branch. Check the intended unit and bootloader mode again.

```powershell
# FUTURE HARDWARE STEP: requires successful, observed upper stage.
$trialMainAttempt = Join-Path $trialRoot ('main-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $trialMainAttempt | Out-Null
$trialInfo = & $trialFlasher --info 2>&1
$trialInfo | Tee-Object -FilePath (Join-Path $trialMainAttempt 'info.log')
if ($LASTEXITCODE -ne 0 -or ($trialInfo -join "`n") -notmatch '\bmode=0\b') { throw 'Stop: bootloader mode not confirmed' }
& $trialFlasher --image (Join-Path $trialCandidate 'firmware.X1C') --confirm-sha256 $trialExpected['firmware.X1C'] 2>&1 | Tee-Object -FilePath (Join-Path $trialMainAttempt 'write.log')
if ($LASTEXITCODE -ne 0) { throw 'Stop: main stage failed' }
```

Wait for normal measurement startup at 5 V with no buttons held. Record any
upper-missing/error indication, repeated blinking, reset or frozen UI. Do not
start recording yet. Reconnect at PC 5 V holding only Left + Right and copy
the complete read-only export into `after-install/`.

If both before/after raw six-sector dumps are available, their hashes must
match **before the first new recording**. STATUS text/owner labels can change
between firmware versions. If no before raw dump exists, record that byte-level
preservation is unverified; retain the exported files and do not claim equality
of the entire store. This logger exporter supplies CSV/raw storage, not the old
BMP exporter. Missing BMP files on the new drive alone do not establish erasure.

## C. Quiet 5 V record, heartbeat and retention

Use PC 5 V, OFF/PASS, the other connector empty and no trigger operations.
Before starting, save STATUS/raw export and confirm `READY - HOLD L+R` in
normal measurement mode. Selective logging needs available selective-stream
tails or new sectors; free capacity is not assumed from a nominal 48 KiB.

1. Hold Left + Right for about half a second, release and confirm
   `SELECT REC 1s - L+R STOP`. Observe V/I, UI heartbeat and System/IRQ values
   without using any reset action. Use a phone/photo for evidence; this build
   cannot save screenshots while logging.
2. Record for about 130 s, so a stable input can exercise two 60 s heartbeats.
   Hold Left + Right again; wait for `LOGGER STOPPED - L+R START`. If STOPPING
   persists for 10 s, treat it as an observation timeout and stop the trial;
   this is an operator bound, not a proven device timing limit.
3. Export into `quiet-session/first-export/` at PC 5 V. Keep every CSV,
   STATUS.TXT and STORAGE.BIN with hashes. A quiet 5 V run can have no PD file.
4. With recording already stopped, fully remove power, reconnect for export
   and save `quiet-session/re-export/`. Raw storage and the prior session CSVs
   should remain byte-identical. This tests stopped-log retention, not a
   power cut during a write.
5. If capacity remains, do one additional short session and export separately.
   Confirm a new session ID, first retained VI reading, unchanged earlier
   session rows and reuse/allocation consistent with the native journal.

For a genuinely stable input, expect the first retained VI reading and the
first available sample at/after each 60,000 ms deadline since the last kept
point. Real changes of at least 100 mV or 100 mA reset the deadline and can
add points; do not require exactly three readings or exactly 60,000 ms spacing.
The input schedule is approximately one second. Selected output alone cannot
prove that every discarded internal reading obeyed the policy.

Decode copies of all files from **one export**, with a new output filename:

```powershell
$trialPython = 'C:\Users\James\AppData\Local\Python\pythoncore-3.14-64\python.exe'
# Example after copying an export; the globs may yield VI only.
& $trialPython tools/logger_decode.py (Join-Path $trialRoot 'quiet-session/first-export/VI*.CSV') (Join-Path $trialRoot 'quiet-session/first-export/PD*.CSV') --out (Join-Path $trialRoot 'quiet-session/decoded.csv')
if ($LASTEXITCODE -ne 0) { throw 'Decode failed; preserve the raw files' }
```

Check session IDs, monotonic per-stream time, signed current, plausible V/I,
heartbeat behavior, complete reconstructed PD where present and all drop/stop
notices. Event ID gaps are expected after routine suppression. CSV decode does
not validate the raw journal CRCs: keep STORAGE.BIN for a native-format prefix/
CRC review against `firmware-core/src/charging_log.rs` before declaring the
storage/CSV correspondence passed. Do not feed native raw storage to the PC
XSC1 decoder or flash any replay `.bin` file.

## D. Passive AVS comparison, after B/C pass

Purpose: verify important PD capture and export on hardware while an external
sink controls the charging process. Reuse the known ASD-PD31/CY4500 topology;
KWS remains OFF/PASS from boot. Remove the PC data/export connection during
the powered AVS run. Record actual wiring, source/cable versions, instrument
settings, and the acquisition start/stop order. Keep the sniffer running and
start the logger before issuing the controlled sweep.

Use the previous approximately 0.5 A, 20 -> 28 -> 20 V sweep with eight voltage
transitions, only with the same rated source/cable/load setup. Capture same-run
CY4500 PD and ASD V/I references. Stop the logger before disconnecting; return
to PC 5 V for export into `passive-avs/`.

The core comparison is the eight EPR_Request/Accept/PS_RDY sequences: request
payload, MessageID, response order and fitted timestamps against CY4500.
Other important packets can also be retained. Suppressed routine traffic is
expected to be missing; unexpected missing important packets or drop/stop/
incomplete notices must be reported. New VI counts need not equal the earlier
12/14 PC replay counts because input timing and measurements differ.

`tools/logger_compare.py` was written for full traffic and reports unmatched
CY packets inside a contiguous overlap. Its all-packet criterion is unsuitable
for selective output, and its narrow message-name mapping can reject newly
retained types. Do not reuse its full-capture PASS claim. Review retained
signatures as an ordered subset, explicitly accounting for routine omissions;
use `logger_decode.py` for reconstruction and preserve the same-run references.
If subset/clock/CRC review has not been completed, mark PD comparison PENDING.
One sweep cannot establish general loss-free reception or real charging capacity.

## E. Active charging coexistence, a separate follow-up phase

Purpose: exercise retained charging controls with recording enabled. Use a
known source/load and compare a short baseline with logging stopped against
the same setup with logging enabled. Start at a confirmed 5 V fixed contract
and about 0.5 A or less, with independent V/I observation. Select Sink while
unpowered, then reconnect; never change the slider as an in-place mode switch.
Use the source's known 5 V default/appropriate saved settings and inspect the
requested contract before permitting higher voltage.

Check explicit UI confirmation, measured voltage/current, response to normal
controls, stable UI, logger stop/export and previous data retention. Only after
the 5 V check passes consider one already-qualified low-voltage 5 -> 9 -> 5 V
sequence on a suitably rated setup. QC/SCP/FCP/AFC and PPS/AVS need their own
source-specific trials before claiming those modes work with logging. Building
them into the firmware is not that evidence. No new active protocol trial is
performed while preparing this plan.

Active PD packets are not guaranteed to appear in this log; the passive hook
does not cover active-sink communications. Use an independent analyzer for
active PD evidence, and limit a successful result to the modes actually run.

## Stop and recovery boundaries

Stop on a hash mismatch, wrong/multiple HID devices, bootloader-mode mismatch,
stage acceptance failure, unobserved upper waiting state, upper-missing/error
UI, unexpected voltage, bus/flash error, freezes or missing existing data.
Save logs and observations; do not clear, try random images or mix stages.
`LOGGER FULL`/`LOGGER SESSION LIMIT` means stop and export, not auto-clear.

The standard flasher can internally perform a second erase/write attempt after
its bootloader final-verification failure; record that as part of the stage
result. Do not stack manual retries onto an unexplained failure. If explicitly
authorized to retry, re-enter Back + OK bootloader mode and use the same
hash-verified stage/pair. If upper completion was uncertain, restart from upper.
Bootloader availability and physical recovery are not proven by the PC model.
If it cannot be reached, preserve the unit and stop for a separate recovery
assessment. No deliberate interruption or downgrade experiment is included.

Report each phase as PASS/FAIL/PENDING with actual filenames, hashes and scope.
Record UI observations separately from PC analysis, ARM build and offline
package audit. Leave unperformed phases PENDING; no hardware result exists yet.

## 日本語チェックリスト

**これは実機試験の計画書です。今回の作成・commit/pushでは、実機への接続や
書込みは行いません。実行は、後で実機試験を始める明示的な指示を受けてからです。**

1. **退避・照合**：現在のファーム／更新方式を記録し、5 Vで既存のBMP・CSV・
   STATUS・raw dumpを取得できる範囲で全部退避してハッシュを保存する。通常の
   スクリーンショット版ではrawを出せない場合があり、その場合はバイト単位の
   保存確認が未検証であることを残す。既知の復帰用ファームと一致する組も準備。
   必要なデータを保護できなければ書込み前に中止。消去・reclaimは行わない。
2. **offline確認**：PCから本体を外し、上記3ファイルの固定SHA-256と
   `CHECK-OFFLINE.ps1`を照合する。候補は2ファイル方式で、単一ファイル版は
   容量超過。別ビルドの上位ファイルやone-step installerを混ぜない。
3. **上位段階**：後日の実行時、OFF/PASS・PC 5 V・反対側端子は空でBack＋OK起動。
   `--info`でmode=0を確認し、上位用だけを書き込む。端末の成功表示に加え、本体の
   短い2回点滅→点灯待機を確認。見えなければmain段階へ進まない。
4. **main段階**：一度外してBack＋OKで再接続し、再びmode=0を確認してmain用を
   書き込む。測定画面まで起動した後、記録を始める前にLeft＋Right起動で再出力。
   前後のrawがあれば一致を確認する。新ロガーのUSBでBMPが見えないことだけを
   消去の証拠にしない。Left＋RightにBackを加えると消去操作なので避ける。
5. **静かな5 V**：OFF/PASSのまま約130秒記録し、STOPPEDを確認して出力。
   変化が小さければ初回と約60秒ごとのheartbeatを確認する。変化で間隔はリセット
   されるため、件数や間隔を固定値に決めつけない。停止後に完全に電源を外して
   再出力し、rawと既存CSVの保持を確認する。容量があれば次の短いセッションも確認。
6. **passive AVS**：前段階合格後、既存のASD能動Sink＋CY4500、KWS OFF/PASSで
   20→28→20 V・約0.5 A・8遷移を同時記録。重要なRequest/Accept/PS_RDYとpayload、
   順序・時刻を照合する。routine省略を考慮し、旧全パケット比較の合格判定を流用
   しない。VI件数12/14やPD総数24を新しい実機ログの固定合格条件にしない。
7. **能動充電は別枠**：電源を外してSink側に切り替え、既知の5 V設定から、記録
   OFF/ONでUI・実測V/I・操作・保存を比較する。PD通信の記録はpassive hookの範囲外。
   スクリーンショット同時保存や未実施の充電方式の動作を確認済みと扱わない。

異常・FULL・既存データ欠落時は消去せず、出力と端末ログを保存して中止する。
電断試験、更新中断、無条件の再試行、downgradeは今回の初回試験に含めない。
CSV復元とraw journalのCRC検証は別で、STORAGE.BINを残して両者の対応を後で検証する。
結果は段階ごとにPASS/FAIL/PENDINGを付け、未実施のものはPENDINGのままにする。
