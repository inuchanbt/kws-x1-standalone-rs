# Native selective logger ARM footprint, 2026-11-17

See [implementation, results and limits](../../../../docs/LOGGER-FIRMWARE-INTEGRATION.md).

- `manifest.json`: base HEAD, hashes of the pre-publication working-tree source,
  toolchain, exact build arguments and each saved ELF's SHA-256.
- `footprint.json`: ELF sections, PT_LOAD sizes, static RAM, linker stack
  reservation, largest static symbols and selective-minus-full deltas.
- `objcopy-crosscheck.json`: independent `rust-objcopy` extraction lengths and
  hashes of main/upper load images for all three ELFs.

All three builds used the same source, toolchain and release configuration.
The full logger includes shared support for reading selective ownership and
exporting policy information. Enabling the actual selector adds 136 B flash
and 16 B static RAM. Compared with the older archived full logger, the total
new selective implementation adds 452 B flash and 16 B RAM, including shared
format support. These are net linked sizes, not standalone function totals.

The ELFs, build logs and extracted analysis images remain local under
`build/logger-firmware-study-20261117/`. They were not packaged or written to a
meter. No hardware run, stack high-water measurement or installer audit was
performed. The default ELF exactly matches the previously saved default.

This implementation uses native fixed-record selective journals, not the host
XSC1 codec. Its host replay uses 864 / 896 committed bytes, with two 8 KiB
sectors allocated per fixture. The separate `v07/` and `v04/` host XSC1 images
are unchanged. Neither replay result establishes real charging capacity.
