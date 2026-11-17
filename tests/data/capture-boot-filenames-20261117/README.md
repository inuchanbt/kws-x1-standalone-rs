# Boot-filename build and PC checks — 2026-11-17

[Implementation and limits](../../../docs/CAPTURE-BOOT-FILENAMES.md).
Base: `e9fab2e50ebf11649e7b0b7c12c48928dfc29d74`.

- `footprint.json`: original/default, final boot-filename screenshot and
  selective-logger ELF hashes, flash load/region margins and static RAM. All
  final builds use the original linker memory.x, Rust 1.99.1, locked/offline
  ARM release settings. No measurement-only extended region is used here.
- `verification.json`: PC test counts, normalized source hashes, private backup
  hash and limitations. No private raw dump, BMP, device identifier, ELF or
  flashable update file is included. Hardware remains untested for this change.
- `SHA256SUMS`: hashes of the two JSON records, preserved without newline
  conversion by fixture-local attributes.

Reproduce without hardware using the commands in the implementation guide.
For new saved ELFs, use `tools/logger_firmware_footprint.py --elf LABEL=ELF
--out NEW_FILE.json`. Host/model tests live with the branch's implementation;
the optional private backup check cannot be repeated from these JSONs alone.

Measured default changes: total flash −1,448 B, main margin 1,484 B, upper
margin 224 B, static RAM +40 B. A local 640 B boot-ID export table also uses
stack; runtime stack maximum has not been measured. Normal screenshot and
selective-logger builds remain alternatives.

日本語：通常配置のARMビルドとPC検証の記録。実機試験のデータではない。
既存8画像の確認は非公開バックアップをRAMモデルに読み込んだ結果であり、
公開JSONだけから元のraw検証は再実行できない。
