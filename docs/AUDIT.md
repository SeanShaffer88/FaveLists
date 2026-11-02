# Audit

What checks exist for this project, and the honest state of each.
Anything not actually run is labelled **Unverified** rather than
scored up. Per-capability status lives in [PARITY.md](PARITY.md),
scored only from test and field evidence.

## The gate (runs in CI on every push and before any release)

- `cargo fmt --all` — formatting
- `cargo clippy --locked --all-targets -- -D warnings` — lints at
  zero warnings, all targets
- `cargo test --locked` — the **full** test suite, never a filtered
  subset: model and store behaviour, provider parsers against
  recorded fixtures, the command registry and JSON-channel
  equivalence, imports/exports, backup merge rules, back-compat
  library fixtures from earlier versions, and the schema-migration
  fixtures below
- `cargo check` / clippy against the Windows target — the Windows
  build must compile, not just the host build

## Conformance audit (v0.4.0, engineering contract v4.0, tier t3)

Checked and fixed in the v0.4.0 pass: classification record
(`.project-tier` + the Project Manifest in
[AGENTS.md](../AGENTS.md)), static C runtime for the Windows build
(`.cargo/config.toml`, `+crt-static` — verified by DLL-import
inspection of the windows-gnu release exe: system DLLs only), the
pinned release profile,
`schema_version` on both persisted files with tested migrations
(pre-versioning fixtures load, migrate, and round-trip; a
newer-version file is refused and never overwritten), the release
pipeline (V4.0 asset names + `SHA256SUMS.txt` on every release),
and this standard docs set.

## Standing checks by hand

- Release assets are re-downloaded after publishing and their
  SHA-256 compared against `SHA256SUMS.txt` before a release is
  called verified.
- README screenshots are real captures from a Windows machine
  only — never mockups or placeholders (capture list:
  `docs/screenshots/CAPTURE.md`).

## Unverified

- **Windows runtime behaviour of each new build** until it has
  actually been run on a Windows PC — see the field test in
  [FIELD_TEST.md](FIELD_TEST.md). CI compiles and tests the code;
  it does not click through the window. At v0.4.0 the still-untested
  field areas are trash restore, barcode lookup, and the cover
  flows.
- **Idle RAM** against the tier's 40 MB guideline — not yet
  measured on a release build.
