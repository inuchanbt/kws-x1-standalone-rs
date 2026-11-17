# Capture filenames grouped by boot

Branch: `codex/capture-boot-filenames`, based on
`e9fab2e50ebf11649e7b0b7c12c48928dfc29d74` (`charging-logger`).
**Implemented and PC/ARM-tested; not installed or tested on hardware.**
No device access, flash writing, clearing or firmware-package creation was
performed for this implementation. The existing normal memory layout is used.

## User behavior

The default screenshot build identifies new images by the boot in which they
were saved, keeping a flat read-only USB volume:

```text
PD01.BMP                       existing untagged capture, unchanged
00000001_OVERVIEW09.BMP         first boot with new captures
00000001_CHARGER10.BMP          another image in that same boot
00000002_PD11.BMP               a later boot with a saved capture
```

The trailing image number still follows capture order across all menus; it does
not restart per boot. The prefix is eight decimal digits. Only the first
successful save in a boot allocates the next ID; later images reuse it.
Export-only boots and boots without saved images make no flash writes and use
no ID. A boot ID therefore counts boots **with saved captures**, not all power
cycles. It does not detect a charger model or equipment changes while KWS stays
powered. Record which target was used for each ID when that distinction matters.

Old images cannot be assigned their historical boot: they keep their original
names in the root. Clearing all images resets the next ID to 1. This is not a
lifetime unique filename guarantee; keep separate PC archives across clears.
IDs stop at 99,999,999 without wrapping. The display's save/full/clear controls
are unchanged. This behavior is not in the previously published capture releases.

## Storage and old-data protection

New records have a 12-byte X1B1 envelope: magic, little-endian u32 boot ID and
its complement. The existing compressed image and its CRC stay inside it.
The normal journal length, sequence/complement and final commit still govern
the record. The ID complement detects single-bit header corruption; it is not
a separate CRC or authentication mechanism.

Old X1S1/X1S2/X1S3 images and X1SC/X1SR detail reports remain readable. No
migration rewrites, compression changes or automatic erases are introduced.
Appends use existing verified erased tails or entirely erased sectors; unknown
occupied sectors and the existing spare/settings/energy exclusions remain.
No flash boundaries, charging controls or screenshot modes were removed.

Each new image costs 12 B more. The maximum inner image record is 8,160 B
instead of 8,172 B; oversize images are rejected before programming. Existing
larger untagged images remain readable. The boot cache is set after successful
commit, and the next boot derives the next ID from valid saved records.

Older firmware does not understand X1B1. Downgrade behavior and recovery have
not been qualified. Export wanted files before any future firmware change;
do not assume old firmware can display newly tagged images.

## Implementation and fitting the normal ARM layout

`firmware-core/src/capture_boot.rs` handles old/tagged records.
`firmware/src/screenshot.rs` persists IDs and returns a packed capture/ID table.
`firmware-core/src/screenshot_disk.rs` adds the prefix to FAT long names, while
preserving distinct existing short aliases and original diagnostic filenames.
The longest new filename occupies at most the existing two long-name entries,
so no subdirectory, additional FAT chain or larger root region is needed.

To recover code space, the sector scanner is shared across its three callbacks
instead of being instantiated for each callback/table shape. Export uses a
bounded insertion sort with the same wrap-aware sequence key. It sorts at most
160 records, only during isolated USB export. These are code-sharing changes;
the normal measurement, charging, capture and flash-protection features remain.

The initial isolated filename prototype exceeded main by 1,004 B. With those
shared implementations, the actual default ARM release build passes the
**original** linker boundaries and stack assertion. No temporary region
extension, special linker optimization flags or feature removal is used.

Same base/source path/toolchain/profile, Rust/Cargo 1.99.1,
`thumbv7em-none-eabihf`, `--release --locked --offline`, normal
`opt-level=z`, LTO and one codegen unit:

| Metric | Original default | Boot filenames | Difference |
| --- | ---: | ---: | ---: |
| Main flash load bytes | 209,880 | 208,424 | −1,456 |
| Upper flash load bytes | 122,648 | 122,656 | +8 |
| Total flash load bytes | 332,528 | 331,080 | −1,448 |
| Main-region space remaining | 28 | 1,484 | +1,456 |
| Upper-region space remaining | 232 | 224 | −8 |
| Total static RAM | 110,168 | 110,208 | +40 |
| Linker stack reservation | 44,488 | 44,448 | −40 |

These are net linked ELF sizes, not compressed package size or standalone
function cost. The 160-entry ID export table adds 640 B of local data; compiler
temporaries, sorting and call frames also use stack. Static RAM and the passing
linker reservation are not a measured runtime stack high-water mark. The change
in callback/sorting implementation still needs device timing/USB validation.

The `selective-logger` ARM release build also passed. Its total flash load
(328,200 B) and static RAM (110,820 B) match the previously published selective
build, with zero size delta. This is a build/size regression check, not renewed
hardware validation of either variant. Screenshot and logger remain exclusive.

## PC checks

- All 67 firmware-core integration tests passed, including unchanged earlier
  format/export checks and new prefix, long-name/alias and pixel-byte checks.
- With `capture-backup`, all 16 screenshot integration tests passed, including
  unchanged raw-snapshot filename/content and image numbering.
- The actual firmware writer was adapted to RAM flash, immediate fake timers
  and a dummy bus. Five serial model tests passed: old-format/multiple boots,
  wrap-aware sequence order, exhausted IDs/protected full storage, interrupted
  programming and an optional private-backup regression.
- The writer model exercised 1,356 word/partial-word interruption cases across
  append and new-sector allocation, with erase operations forbidden. Existing
  records/protected bytes survived; safe append or blank-sector retry remained.
- A private 49,152 B backup was read into RAM: eight old images remained valid
  after one new boot-tagged append; original nonblank words and other regions
  were unchanged. The original backup file was not modified or published.

The model is bounded PC evidence, not physical power-cut recovery or a real USB
test. No hardware stack, receive timing, capture during active negotiation,
installation or downgrade result is claimed for this branch.

[Recorded build/check summary](../tests/data/capture-boot-filenames-20261117/README.md)
includes source hashes and ELF measurements without private backups or binaries.

## Reproduce PC/build checks without device access

```powershell
cargo test -p firmware-core --tests --locked --offline
cargo test -p firmware-core --test screenshot --features capture-backup --locked --offline
python tools/capture-boot-filenames/check_storage.py
cargo build -p firmware --bin firmware --release --target thumbv7em-none-eabihf --locked --offline
```

The writer model prints an explicit omission if no private backup is supplied.
Use `--backup PRIVATE_CAPFLASH.BIN` to include that read-only check. See the
[tool notes](../tools/capture-boot-filenames/README.md) for Windows host linker
setup and unique output paths. Existing saved ELFs must be copied under distinct
names before another build overwrites Cargo's firmware artifact.

## 日本語要約

新しい画像に `00000001_`のような起動番号を付け、同一起動の画像を区別する。
旧画像の名前・内容は保持。USB出力だけの起動や撮影なしの起動ではflashを書かない。
番号は撮影成功した起動ごとで、画像全消去後は1に戻る。機器名の自動判別ではない。

共通処理の重複を減らし、通常のARM配置でビルド成功。元のdefaultよりflash全体が
1,448 B減り、main空き1,484 B、upper空き224 B。静的RAMは40 B増だが、USB出力時の
番号表640 Bなど別途stackを使うため、実機stack最大使用量の検証ではない。
新画像は1枚12 B増。旧形式・名前・BMP内容・1,356件の書込み途中モデルと、
実データ8画像を使うPC検証は成功。実機書込み・USB・動作確認はまだ行っていない。
