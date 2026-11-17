# Host selective-codec validation, 2026-11-16

The later native package audit is in `package-20261117/`; see the
[offline package report](../../../docs/LOGGER-PACKAGE-AUDIT.md). Its two-stage
containers and synthetic install replay do not establish hardware operation.

For the later native journal firmware implementation and same-source ARM
comparison, see [the integration report](../../../docs/LOGGER-FIRMWARE-INTEGRATION.md)
and `firmware-20261117/`. The native firmware format is distinct from the
host XSC1 images below; do not mix their byte/allocation counts.

See [study and reproduction commands](../../../docs/LOGGER-CAPACITY-STUDY.md).
The source firmware baseline is abc4dba2bae404edfdae8d2aedc915eef71e3c7c.
The codec and its test suite are accompanying host-only study source.

`v07/` and `v04/` contain the actual host-generated sector image, decoded
selected events, input hashes, encoded sizes and binary hash. All original
input CSVs remain in their existing fixture directories. These are offline
serialization artifacts, not firmware and not device flash dumps. **Do not
flash selective.bin.** They use 100 mV / 100 mA thresholds and a 60 s heartbeat.

The two section reports came from new offline locked ARM release builds on
the same source with Rust/Cargo 1.99.1, respectively no feature and
`charging-logger`. ELFs remain in `build/logger-selective-study/`. They do not
contain the Python selective codec. Runtime, installation and active-sink
logging remain untested for these new binaries.

## Review on 2026-11-17

Run `python -m unittest discover -s tools -p "test_logger*.py" -q` from the
repository root (substitute the working interpreter listed in the study on
Windows). All 17 host tests passed, including byte-identical re-encoding of
both fixture images, input/output SHA-256 checks, event equality and report
sizes. This verification does not rewrite the fixtures. The saved baseline
ELFs were separately checked against both section reports, including hashes;
no ARM rebuild or device test was performed during this review.

The host codec now preserves every original field for unknown SOP packets,
requires object metadata as the first record, and rejects extra sectors after
the end marker. Neither existing binary nor its recorded size/hash changed.
