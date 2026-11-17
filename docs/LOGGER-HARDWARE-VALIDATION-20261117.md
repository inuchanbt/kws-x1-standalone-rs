# Selective logger: first hardware validation (2026-11-17)

This follow-up records actual device observations after the earlier
[ARM integration study](LOGGER-FIRMWARE-INTEGRATION.md),
[offline package audit](LOGGER-PACKAGE-AUDIT.md) and
[staged trial plan](LOGGER-HARDWARE-TEST-PLAN.md).
[Capture data, provenance and reproduction commands](../tests/data/logger-selective-hardware-20261117/README.md).

## Outcome and scope

| Stage | Observation | Scope |
| --- | --- | --- |
| Matching two-stage install | PASS on one device | Upper accepted, two short backlight flashes then steady; main accepted, normal measurement startup |
| Existing capture/log preservation | PASS locally | All 49,152 bytes identical before/after install, before new recording; not whole-MCU/settings comparison |
| Quiet PC 5 V recording | PASS | First VI point + two heartbeat points at 61,026 / 61,016 ms intervals |
| Stopped-log power-off retention | PASS | User observed screen off and waited ≥5 seconds; all re-exported files identical |
| Second short session | PASS | Prior session CSV unchanged, new first point, same VI sector tail reused |
| Passive AVS | PASS in comparison window | 13 VI points, 24 important PD packets, all eight Request/Accept/PS_RDY sequences match CY4500 |
| KWS active-Sink charging with recording | PENDING | Normal charging controls are compiled in; these trials used OFF/PASS only |

Firmware source: `dfbaf84d9aa9da69734fbfeab354c02899b04922`, from checkout
`bf2653a2958b7a5e7efec66132fb6872443127ee`. The existing matched two-stage package
was used, with no feature removal. Both stages accepted 26/26 batches with no
recorded internal/manual retry. Package hashes are in `installation.json` and
the earlier offline audit. This does not establish interrupted-update recovery,
atomic installation or one-file installation for this full-function variant.

## Passive AVS evidence

KWS remained OFF/PASS, external ASD-PD31 was active Sink, CY4500 captured the
same run. The sequence was 20→22→24→26→28→26→24→22→20 V at approximately 0.5 A.
The sweep took about 59 s; logging also included steady operator waiting time.

All 24 important packets matched SOP, complete raw 16-bit header, every data
object and order. The Request-based clock-fit maximum residual was **3.71 ms**.
Of 411 CY SOP packets between the first retained Request and final PS_RDY,
387 were exact known routine patterns omitted by policy: 205 GoodCRC and
182 EPR KeepAlive/ACK. No unexplained important omission, incomplete KWS
packet, queue-drop notice or sniffer-stop notice was found in this comparison.
CY CRC/EOP/IDLE error flags and decoder flag mismatches were zero.

This is a comparison against the observed analyzer records and one clock fit,
not clock calibration, independent physical waveform/CRC validation or general
loss-free reception. One-second VI input sampling cannot recover short or
discarded events. This is not a representative full battery-charge trace.

## Actual committed storage versus allocation

Session 3 added **880 B**: one VI session marker + 13 VI records (224 B), plus
PD header + 40 PD fragments (656 B). The existing VI sector tail was reused and
one new **8 KiB** PD sector was allocated. Total selective allocation was
**16 KiB**, and all three sessions occupied **976 committed bytes**.
Existing UNKNOWN/capture sectors remained protected; no clear/reclaim was used.

Local raw checks passed for all native header/record CRCs, erased tails, every
CSV row and preservation of earlier committed/protected bytes. The raw dumps
contain existing device data and remain private; public data reproduces the
PD comparison, while raw verification is represented by results and hashes.

The native **XSL1/XSP1** format differs from the earlier PC **XSC1** experiment.
It has no durable clean-stop marker. Screenshots and logging remain mutually
exclusive compile-time variants. The prior ARM code/RAM numbers are unchanged
by publishing this evidence; runtime stack high-water remains unmeasured.

## ASD session handling and unsuccessful preparation

The operator observed that closing the ASD COM session turns its load off and
cuts KWS power in this topology. Manual panel AVS setup also required LOAD ON.
An initial instruction to configure with LOAD OFF was therefore incorrect.
Read-only PDO commands still closed COM on exit: the absence of explicit
voltage/load writes did not preserve physical output state.

The first automated setup used the CLI's Fixed-5V zero-load analyzer path
proof, which cleared visible AVS capability; preflight then rejected the sweep.
No KWS recording or eight-transition sweep had started in that attempt.

The successful setup kept one COM session open: fresh PDO/range and measured
V/I checks, an initial **22→20 V / 0.5 A** analyzer path proof, then monitored
operator waits around the normal eight-transition sequence. The setup path
proof occurred **before KWS recording** and is outside the reported 24 packets.
Existing capture-health/PD-evidence and measured protection checks remained.
Limits were 20 W / 0.75 A; operator waits were bounded at 300/180 s. On faults
or timeouts, LOAD_OFF had priority. Normally, LOAD_OFF ran only after the
operator confirmed LOGGER STOPPED. External ASD sources were not modified.

The initial local console wrapper also had an encoding failure, corrected
before the successful run. A CLI default version-metadata file was written
outside the trial folder during initial discovery; its prior contents were
not captured. Later version/measurement/capture outputs were explicitly routed
to the trial folder. Private originals and failed preparation logs remain local.

QR, ripple, long-duration/full-store endurance, active-mode charging controls,
write-interruption and downgrade recovery remain separate work.

## 日本語要約

1台で上位→mainの2段階更新と通常起動を確認し、記録前の既存48 KiBは一致。
5 Vではheartbeat、停止後の完全電断保持、次セッションへの追記を確認。
OFF/PASS＋ASD能動SinkのAVSでは重要PD 24個がCY4500と一致、最大残差3.71 ms。
保存増分880 B、新規確保8 KiB。raw CRC／CSV一致／既存領域保持はローカル検証。

成功試験はASDのCOMを保持し、KWSの記録開始・停止確認後に負荷OFFにした。
5 V確認でAVSが解除された準備失敗も記録。能動充電の同時動作は未検証で、
PC版XSC1とは別形式、スクリーンショットとは排他。長時間実充電の容量や
一般的な無欠落、書込み中の電断復旧を証明する結果ではない。
