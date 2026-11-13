# Logger single-file feasibility — 2026-11-13

English is primary; Japanese summary follows. This is an offline size and
installation-layout study, not a firmware release or a hardware test.

## Findings

The complete v0.4 logger does **not** currently fit. A narrower PD receive-only
logger is feasible at the packaging level, provided the packed streams are
positioned to preserve unread bytes during installation. These are separate
conditions: fitting the bytes alone does not establish a safe erase order.

The application content budget is 209,916 bytes (`APP_LIMIT = 0x333fc`).
The bootloader/container envelope and all code/storage boundaries are unchanged.
The required-content column is unpacker + compressed upper + compressed main,
before placement padding. X1C files are padded containers; comparing their
total file lengths would hide the differences below.

| Configuration | Unpacker | Packed upper | Packed main | Required content | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| Hardware-tested v0.4, all current functions, device-specific recovery enabled | 15,340 | 78,859 | 127,832 | 222,031 | 12,115 bytes over budget |
| Same logger, no embedded backup fingerprints | 15,340 | 78,887 | 127,021 | 221,248 | 11,332 bytes over budget |
| No fingerprints; active PD sink awaits removed from task | 15,340 | 14,794 | 119,973 | 150,107 | Fits by 59,809 bytes; original placement fails erase-margin rehearsal |
| Same reduced firmware; packed group placed at end of budget | 15,340 | 14,794 | 119,973 | 150,107 | Complete package, audit and flasher dry-run pass; minimum erase margin 21,499 bytes |

Removing device-specific recovery alone saved only 783 packed bytes. Twelve
liblzma recompression trials (six lc/lp/pb combinations, nice lengths 32/273,
64 KiB dictionary) did not improve on the current setting. Their best estimate
was 222,030 bytes, just one byte different from the production encoder's
222,031 bytes. The alternate encoder results are estimates, not packer audits.

## What the reduced probe actually changes

All changes were made in a disposable copy under `target/`:

- Build with `charging-logger` and no `KWS_X1_RECLAIM_SHA256` value.
- Force the task's initial sink selection false and syntactically remove its
  active-sink branch, retaining `passthrough()` and `sniff()` plus their logger
  calls. A false condition alone did not remove the async sink state machine:
  that preliminary probe still exceeded the budget by 11,342 bytes.
- Keep the logger measurement/event format, CSV/USB export, flash boundaries,
  normal measurement tasks and other source modules unchanged.
- In the disposable packer, align the entire compressed upper+main group near
  `APP_LIMIT`, rather than immediately after the unpacker. Main-only placement
  retains its existing behavior; upper-only placement is unchanged.

The existing unpacker's record already supplies the compressed-data pointer;
the unpacker and its format were not modified. The unchanged production
`inflate_sectors` and `main_erase_margin` checks accepted the relocated complete
package and verified the inflated output against the matching firmware ELF.

The reduced probe is **not a usable or safety-qualified passive-only product**.
It does not update the slider indication or remove/disable inappropriate UI
and other protocol trigger paths. No user should flash it. A proper variant
needs an explicit feature boundary, clear controls and startup policy, and
repeat measurement/sniffing/export tests. The hardware-tested v0.4 remains the
matching two-stage firmware already installed on the test device.

## Evidence and reproducibility

Local experiment outputs are in `build/logger-single-file-study/`: original
ELFs, size-only ELFs, stage package reports, compression estimates, complete
audit, dry-run output and SHA-256 manifest. No USB device was opened.

`tools/logger-size-study/prepare.py` reproduces the source-only reductions in
a new directory beneath `target/`; it never modifies production sources.
See the adjacent [tool notes](../tools/logger-size-study/README.md).

Probe complete image SHA-256:
`d03513e46d06ef258d728e6fbc8ac096f8d4981068a2a6cfedb3e6fe05c4bda0`.
This identifies size-only evidence, **not an approved flashing image**.

Before upstream discussion, present both conclusions: the full logger still
exceeds the single-file budget, while removing the active PD sink path gives
substantial room for a dedicated observational logger. Do not present this as
single-file support for the full screenshot or full-function logger firmware.
Power interruption, physical installation, recovery, timing and UI behavior
of the reduced probe have not been tested.

## 日本語要約

現行v0.4は単一ファイル上限を12,115バイト超過します。実機固有の回復処理を
外しても11,332バイト超過し、圧縮設定12通りでは改善しませんでした。

PDの能動Sink経路をタスクから外すサイズ試験では、必要量が150,107バイトまで
減りました。ただし従来の配置は展開中の消去安全検査に失敗します。圧縮データ群を
後方へ配置すると同じ検査・監査・dry-runを通り、最小余裕は21,499バイトでした。

これは「受信観測用の専用ロガーなら単一ファイル化の見込みがある」という結果です。
現行全機能を維持した単一ファイル化ではありません。試験版はUI・スイッチ表示等を
未整理のため書き込まないでください。実機のv0.4や製品ソースは変更していません。
ソース差分は隔離フォルダーで作成し、通常の出力位置にあったv0.4 ELFとパッカーも
保存した元のバイト列に戻しました。正式版化には明示的な機能分離と実機試験が必要です。
