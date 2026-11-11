# Capture validation / キャプチャの動作確認記録

## English

The `firmware-v0.12.0-capture.1` pair below was tested on a KWS-X1 on
2026-11-11. Use both files from the same release and follow
[Two-stage flashing](FLASHING.md).

| File | SHA-256 |
| --- | --- |
| `firmware-upper.X1C` | `d52a18631aa530c4472284444806775fb215d67e6822edec0851a9409bc42607` |
| `firmware.X1C` | `9a29c6b66f936da878e9ede547b69fd1067b36ec538c72a7ccde8db815c38078` |

The earlier capture build could leave voltage, current and power displayed
as `---` until a restart. A captured SYSTEM report recorded an I2C software
timeout. Compression and journal validation could occupy the thread executor
while an I2C transaction was suspended there.

The fix waits for that transaction before CPU-only capture preparation, and
acquires the bus before each short compression/write batch. Long preflight
work releases the bus so the interrupt-driven PD task can still use it.
Full-page report preparation follows the same ordering. The fault latch and
the capture storage format remain in place.

Validation of the exact pair:

- Both flashing stages passed the bootloader's checksum checks, followed by
  the two-flash stage-1 indication and normal measurement startup.
- After erasing captures, the user captured repeatedly until `CAPTURE FULL`;
  the V/A/W freeze did not recur.
- The user confirmed USB export. `OVERVIEW19.BMP` (240×135) and
  `SYSTEM17.BMP` (240×324) were copied with matching SHA-256 hashes and decoded
  on the PC. The SYSTEM report showed `I2C FAULT OK` and zero fault counters.
- Two deterministic simulations of the actual save task exposed the old
  scheduling conflicts and passed with the fix. Capture/compatibility/export
  tests (13 cases), firmware compilation, static checks, image audits and
  flasher dry runs also passed.

This records the observed results on the tested unit; it does not establish
compatibility with every hardware revision. Screenshot examples are in the
[README](../README.md#screen-capture).

## 日本語

2026年11月11日、上記ハッシュの2本をKWS-X1実機で確認しました。
同じリリースの2本を、[2段階書き込み手順](FLASHING.md#日本語) の順番で使います。

以前のキャプチャ版には、撮影後に電圧・電流・電力が `---` になり、
再起動まで復帰しないことがありました。SYSTEMの診断では測定通信の
タイムアウトが記録されていました。圧縮計算や既存画像の検査が、
同じ実行枠で進行中の通信を待たせる経路を修正しています。
縦長画像の準備と、小分けに圧縮・書き込みする部分にも待ち合わせを入れています。

- 2段階とも書き込みとブートローダーの照合が通り、1段階目の2回点滅と、
  2段階目の測定画面への復帰を確認しました。
- 本体のキャプチャを全消去し、FULLになるまで撮影してもVAW停止は再発しませんでした。
- USB取り出しを確認し、通常画像と縦長画像をPCへコピーして読み込みました。
  SYSTEMには `I2C FAULT OK` と表示され、エラーの記録もゼロでした。
- 通信との競合テスト2件、画像保存・互換性・取り出しテスト13件、ビルドと静的検査、
  X1Cの構造・元のビルドとの照合、書き込みツールの事前検査も通っています。

既存画像の形式、保存区画、設定・工場データの扱い、通信異常時の停止機構は従来どおりです。
この記録は確認した個体での結果です。実際の保存画像は
[日本語README](../README.ja.md#画面キャプチャ) に掲載しています。
