# Firmware branches

English is primary; see [Japanese](BRANCHES.ja.md).

| Branch | Purpose | Firmware build |
| --- | --- | --- |
| `main` | Screenshot firmware baseline and established releases | Default build |
| `charging-logger` | Experimental voltage/current and passive PD logging, CSV export and AVS validation | `--features charging-logger` |

The logger branch starts from the shared screenshot baseline. Select the branch
and build feature explicitly; switching a Git branch alone does not change
firmware already installed on a meter. The logger replaces screenshot capture
in its firmware image. Always use a matching upper/main pair from one build.

Keep future independent firmware variants on descriptive, purpose-specific
branches. Document each branch's build command, storage use, test evidence and
release status here when that variant is actually created. Share common fixes
through reviewed commits; merging an entire experimental variant into main is
a separate decision. Existing main history (including the opt-in v0.2 logger)
and published screenshot releases are preserved.

Logger test data includes the [v0.4 capture](../tests/data/logger-avs-20261113)
and [v0.7 observer capture](../tests/data/logger-observer-v07-avs-20261113).
Publishing the source/test data does not publish a firmware Release or certify
untested hardware. Device-specific reclaim fingerprints must come from the
operator's own verified backup, never be copied to unrelated meters.

The `charging-logger` branch also offers the dedicated observation build via
`--features logger-observer`. This one-file candidate excludes active protocol
controls; see [scope and validation](LOGGER-OBSERVER.md). It is not a replacement
for the full logger build or an approved screenshot firmware Release.
