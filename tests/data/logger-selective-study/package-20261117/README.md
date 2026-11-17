# Selective logger offline package audit, 2026-11-17

See [the report and reproduction commands](../../../../docs/LOGGER-PACKAGE-AUDIT.md).

Firmware source: `dfbaf84d9aa9da69734fbfeab354c02899b04922`. The fresh firmware
ELF is byte-identical to the previously measured selective build. The matching
unpacker and current-source Windows packer/flasher were built offline.

- `summary.json`: single-file size rejection, two-stage file hashes, dry-run
  results, negative checks and the synthetic replay summary.
- `upper-audit.json`, `main-audit.json`: production canonical audits against
  the exact firmware/unpacker ELF pair.
- `install-replay.json`: per-main-sector decode/erase trace on synthetic flash.
- `manifest.json`: source/tool/input hashes, toolchain and build arguments.

The complete single-file layout exceeds its content budget by 15,773 B.
Two-stage packages pass audit and dry run. The smallest margin while compressed
bytes remain unread is 5,115 B; the final callback returns the defined zero
margin only after the stream is fully consumed. Patterned bootloader and all
bytes at or above 0x62000 remain unchanged in the synthetic model.

Packages, ZIP, executables and detailed logs remain local under
`build/selective-logger-package-20261117/`. The clean `candidate/` folder has
an offline-only hash/dry-run checker. Neither these artifacts nor the replay
establish hardware operation, physical flash behavior or recovery. No USB
device was discovered/opened and no erase/write was requested.

The replay example and audit helper are follow-up additions, identified by
hashes. No new firmware functionality or installer change was introduced.
The [hardware test plan](../../../../docs/LOGGER-HARDWARE-TEST-PLAN.md) describes
future execution only; all hardware phases remain PENDING.
