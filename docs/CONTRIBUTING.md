# Contributing

Thanks for helping. FaveLists is a single Rust crate; the layout is
described in [ARCHITECTURE.md](ARCHITECTURE.md).

## The hard rules

- **All decision logic lives in the library** (`src/` modules behind
  `lib.rs`) and is testable off-Windows. The GUI renders state and
  collects intent only; library mutations go through the command
  registry (`src/commands.rs`) so the GUI and the headless
  `--json-channel` can never disagree.
- **No API keys in code, tests, or this repo — ever.** Provider
  fixtures in `tests/fixtures/` are key-free recorded responses.
- **Screenshots come only from real runs on a Windows PC** — never
  placeholders.

## Before you open a pull request

Run the full gate — the whole suite, never a filtered subset (the
gate is listed in [AGENTS.md](../AGENTS.md)):

```sh
cargo fmt --all
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo check --target x86_64-pc-windows-msvc --all-targets
```

CI mirrors the essentials and must be green. Keep changes surgical:
complete files when new, small diffs when editing, and update the
docs your change touches (README / docs / CHANGELOG) in the same
change. A persisted-format change must bump the `schema_version`
and ship a migration fixture (see [MIGRATION.md](MIGRATION.md)).

## House rules

- No telemetry, no accounts, no new dependency without a stated
  reason.
- No winget packaging. Releases are cut by the maintainer from
  tags; don't add release automation beyond `.github/workflows/`.
- Public docs state product facts only.
