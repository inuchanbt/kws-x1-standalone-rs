# KWS logger AVS comparison launcher

[English (primary)](README.md) · [日本語](README.ja.md)

This adapter loads `D:\ASD-PD31\src\current\asd_pd31_cli.py` and reuses its
transport, PDO decoder, measurement protections and CY4500 CLI capture lifecycle.
It replaces the hardware workflow only inside its own Python process. The ASD
project's files are unchanged. The reviewed CLI is v5.8.20, SHA-256
`860f4b18702376e9283cf60d74efbc69e516dbcbfd088480f5f4c9fab778eb99`.
A changed CLI is rejected before connecting; review the adapter before updating
this fingerprint. Companion ASD modules must remain compatible with that version.

## Procedure

1. Use the existing setup: SRC → cable → CY4500-EPR → KWS-X1 → ASD-PD31.
   Close other software using the ASD serial port or CY4500 USB interface.
2. **Manually establish touch-panel OBJ9 EPR AVS, 20 V, 0.5 A, LOAD ON on ASD first.**
   Let KWS boot and confirm PD SNIFF is receiving. This launcher does not establish
   a fresh contract or recover a stopped sniffer. Do not start KWS logging yet.
3. Run from an interactive PowerShell using the Python installation already used
   for the ASD CLI:

   ```powershell
   python D:\kws_x1_standalone_rs\tools\logger-sweep\kws_avs_sweep.py --port COM3
   ```

4. At `[READY]`, start the KWS logger, then press Enter in PowerShell.
5. The sweep observes the existing 20 V point, then requests
   22, 24, 26, 28, 26, 24, 22 and 20 V. Load/request current is 0.5 A.
   Each point is held for at least three seconds after request/settling work.
6. At `[DONE]`, output remains at 20 V / 0.5 A. Stop the KWS logger and wait for
   `LOGGER STOPPED`; then press Enter in PowerShell. Only then does normal cleanup
   send LOAD_OFF and stop/export CY4500.
7. Copy the matching KWS `VI*.CSV` / `PD*.CSV` files (or legacy `LOG*.CSV`) into the newly printed run folder after USB export, or use the separate reference-directory option below.

Every run creates a unique `CY4500_logs/kws_avs_*` directory under the KWS project,
containing `ASD.csv`, `CY4500.csv`, and the CY4500 backend's diagnostic sidecars.
No timestamp variable needs to be refreshed, and old runs are not overwritten.
Use `--output-dir PATH` to choose another parent directory, `--cable-set cc2` if
the established setup uses CC2, or `--plan` for a hardware-free sequence preview.
Automatic CC switching is intentionally absent.

## Behavior and limits

- Preflight sends one explicit read-only PDO-buffer query and a measurement read.
  It requires visible Object 11 EPR AVS coverage across the full route and measured
  19–21 V, 0.4–0.6 A, and no more than 20 W. Invalid preflight leaves the existing
  output unchanged; it does not send an automatic recovery/reset request.
- The initial measurement is **not proof of the current contract's PDO**. The
  operator must establish touch-panel OBJ9 EPR AVS beforehand. The first request changes to
  22 V, avoiding a redundant 20 V request as the initial PD-evidence check.
- No protocol entry, fixed-5-V probe, initial LOAD_OFF, LOAD_ON toggle, or PDO
  handoff occurs in the successful sequence. Measurement voltage-limit sync is
  retained from the ASD CLI; it is not a new PD negotiation.
- CY4500 capture starts before opening the ASD connection. The zero-load 5 V
  `verify_path()` probe is omitted, but the live-capture guard remains installed:
  changed requests must still produce fresh Request/EPR_Request and PS_RDY evidence.
- Both Enter checkpoints continue checking measurements and capture health about
  every half second plus I/O time. The existing voltage/current checks remain;
  measured power/current ceilings are 20 W / 0.7 A. Condition telemetry is disabled
  to reduce unrelated serial traffic.
- Once preflight passes and the session adopts the load, **errors or Ctrl+C still
  trigger LOAD_OFF immediately**, even if KWS has not finished saving. This is not
  an unconditional keep-power-on mode. Failed cleanup/export does not replace the
  original error. Software cannot guarantee uninterrupted physical VBUS.

## Offline verification

Seven tests exercise the reviewed CLI with fake serial responses, measurements
and capture control: normal command order, read-only rejection of bad initial
conditions, interruption cleanup, missing PD evidence, faults during the final
hold, parser/capture lifecycle and unique output paths on rerun.

```powershell
python -m unittest discover -s tools/logger-sweep -p test_kws_avs_sweep.py
```

No COM/USB device is opened by these tests. Hardware validation remains pending.

## Panel numbering

`--panel-object 9` (default) selects the ninth nonempty PDO in the decoded table.
For this source, panel OBJ9 maps to native Object 11; native slots 6 and 7 are empty.
Preflight prints both numbers and the internal gear, and verifies EPR AVS coverage.
ASD.csv retains the native PDO number. Internal gear remains `0x12`, as before;
this does not send native Object 9 or gear 9. Nine offline tests pass, including
resolution and transmitted gear checks. Hardware validation is pending.
See the automatic-start option below for updated checkpoint handling.

## Automatic start and console input

Use `--start-delay 10` to start the sweep after a monitored ten-second countdown,
without an initial Enter. Start KWS recording before the countdown finishes.
The countdown begins after preflight; serial I/O can extend the wait.
Normal completion still holds 20 V / 0.5 A until you stop KWS, see LOGGER STOPPED,
and press Enter. Checkpoints now read normal stdin without discarding queued input.
EOF is an error and triggers the existing cleanup. Raw serial traffic is hidden
by default; use `--verbose` to restore it. `[SWEEP START]`, `[REQUEST]`, and
Enter acknowledgments show progress. Twelve offline tests pass; hardware validation
is pending. The previous trace showed only READY measurements, no sweep request;
the reason Enter did not advance that run has not been established.

## Three-instrument comparison

Every current run already saves measured voltage/current/power and requested
setpoints to ASD.csv. `--no-condition-log` suppresses condition/temperature
telemetry, not these measurement rows. No launcher/FW update or repeated
measurement is needed to enable the offline ASD comparison.

`tools/logger_compare.py` reports both the existing KWS/CY4500 packet match and
KWS/ASD measurement differences in its `asd` object. With KWS CSVs and both
reference CSVs in one directory:

```powershell
python tools/logger_compare.py PATH_TO_SAME_RUN --session 11
```

For separately copied KWS files, select the analyzer folder printed by the
**same physical sweep**, replacing RUN_ID with that exact folder suffix:

```powershell
python tools/logger_compare.py log_backup/v07-sweep --session 11 --reference-dir CY4500_logs/kws_avs_RUN_ID
```

Session selection is required if the export contains multiple recording IDs;
this prevents earlier short recordings from being mixed into the sweep.
Different runs can use identical voltage routes: do not substitute older
ASD/CY CSVs just because the setpoints agree.

The analog comparison keeps outbound and return steps separate. KWS snapshots
are grouped by the eight recorded EPR_Request boundaries; ASD rows are grouped
by ordered logger-outbound/logger-return phase and target, excluding READY and
STOP-wait rows. Each changed KWS step excludes its first 1,000 ms. Snapshots
outside ±0.5 V or ±0.05 A of their target are counted and excluded from the
steady median; these are selection windows, not accuracy/compliance limits.
Report fields include per-step sample counts, exclusions, instrument medians
and signed KWS-minus-ASD differences in mV/mA. Missing steady data is reported
as partial; a different/missing route is unavailable rather than fabricated.

These are independent per-step medians, not simultaneous sample pairs.
The public fixture has blank timestamp values, and original ASD timestamps
have second resolution; no cross-device timestamp fit is claimed. Differences
include measurement-position/wiring drop, offsets and remaining timing effects.
They do not establish absolute accuracy, calibration or transition response.

The published v0.4 fixture yields nine compared steps with voltage median
differences +12 to +15 mV and current differences +2 to +3 mA, while retaining
516 matching PD packets. The local v0.5 run kws_avs_k98fuz7e, paired with the
backed-up session 1, yields +14 to +39 mV and +2 mA over nine steps; its PD
comparison remains 456 packets. Neither is the pending v0.7 sweep result.

```powershell
python -m unittest discover -s tools -p "test_logger*.py"
```

Seven tests cover legacy/typed filenames, missing ASD, the published analog
comparison, separate reference folders and session selection, known synthetic
meter differences, current transients, incomplete routes and missing steady data.
They open no hardware.
