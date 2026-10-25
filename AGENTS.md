# AGENTS.md — FaveLists

Notes for agents working on this project. The Universal Windows Desktop
Master Prompt applies in full:
`~/workspace/your_files/windows-desktop-master-prompt.md`

## Project in one line

A Windows desktop app (Rust + egui) for building single-type lists of
favorite movies, shows, books, music, games, podcasts, board games, and
drinks — with online auto-fill, a bundled offline drinks database, and
text / Markdown / cover-image exports. MIT. Local-only.

## Hard rules

- **Never claim Windows runtime is verified** until the build has run on
  SEAN's Windows machine and he (or his log) confirms it. Local tests and
  cross-checks are *not* Windows verification. Label claims Verified /
  Partially Verified / Unverified. Field test: `docs/FIELD_TEST.md`.
- Screenshots come only from real runs on SEAN's machine — never
  placeholders; the capture list is `docs/screenshots/CAPTURE.md`.
- **No API keys in code, tests, or this repo — ever.** TMDB and Twitch
  credentials belong to the user and live only in `settings.json` in the
  data folder. Provider fixtures in `tests/fixtures/` are key-free
  recorded responses.
- The fixed set is **11 categories** (spec §2). No anime, no software, no
  user-created categories, no synopses. (Personal ratings/notes were
  excluded in v1; SEAN reversed that 2026-10-25 — they are supported
  since v0.2, program item #5.)
- Public documentation must not name or compare competitor software.

## Architecture

- **All decision logic lives in the library** and is testable off-Windows:
  model, store, providers (pure parsers + fixtures), drinks DB, exports,
  image layout geometry, rendering, backup. The GUI renders state and
  collects intent only; network work runs on background threads.
- Storage is versioned JSON written atomically (temp + rename) in a data
  folder that defaults to next to the exe; relocation moves data and is
  recorded in `favelists-location.json` beside the exe.
- Renderer: egui via eframe with the **glow** renderer explicitly selected.
- HTTP: ureq with **native-tls** (Schannel on Windows). Do NOT switch to
  rustls/ring — its build script needs MSVC tools when cross-checking the
  windows-msvc target from Linux (see DECISIONS.md).
- The bundled drinks DB (`data/drinks.json`) is generated, not hand-made:
  `tools/drinks-db/` compiles it from an Open Food Facts export (ODbL;
  images CC BY-SA). Never invent product data for it.

## The gate (run in full before publishing anything)

```sh
cargo fmt --all
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --target x86_64-pc-windows-msvc --all-targets -- -D warnings
cargo test --locked
cargo check --target x86_64-pc-windows-msvc --all-targets
```

- Run the **full** test suite, never a filtered subset.
- CI (`.github/workflows/ci.yml`) mirrors the essentials on
  `windows-latest`.

## License

MIT — see `LICENSE`. Copyright 2026 Sean Shaffer.
