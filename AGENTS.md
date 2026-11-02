# AGENTS.md — FaveLists

Notes for agents working on this project. The Universal Windows Desktop
Master Prompt applies in full:
`~/workspace/your_files/windows-desktop-master-prompt.md`

## Project in one line

A Windows desktop app (Rust + egui) for building single-type lists of
favorite movies, shows, books, music, games, podcasts, board games, and
drinks — with online auto-fill, a bundled offline drinks database, and
text / Markdown / cover-image exports. MIT. Local-only.

## Project Manifest

- tier: t3 (rich Windows GUI) — **by SEAN's decision, 2026-11-02**
- tier rationale: an egui/eframe rich-render app — cover grids, share-card
  rendering, image-export layouts — matching the DevTextTools/CalcDex egui
  precedent (both settled t3 by SEAN's decision at their passes). The
  contract's classification companion listed this project under t1b; that
  listing is **superseded for this project** by SEAN's 2026-11-02 decision.
  Note: egui is not named in the contract's §6 renderer list (Slint,
  Direct2D, Godot, raylib); the t3 placement follows the companion's own
  richness test (a rich multi-pane editor or render-driven surface → t3)
  plus SEAN's decision.
- targets: windows-x64
- toolchain: Rust 2021, eframe/egui 0.33 (glow renderer); static C runtime
  for the gnu target via `.cargo/config.toml` (`target-feature=+crt-static`)
  — verified in the v0.4.0 pass: the windows-gnu release exe's DLL imports
  (`x86_64-w64-mingw32-objdump -p`) are Windows system DLLs only, plus
  `msvcrt.dll` (a Windows OS component); no `libgcc` / `libstdc++` /
  `libwinpthread` imports
- renderer: egui immediate-mode GUI over glow
- release assets: FaveLists-v{VERSION}-x64-Setup.exe +
  FaveLists-v{VERSION}-x64-Portable.zip + SHA256SUMS.txt (the exe installer
  ships with every desktop release; no MSI, ever — SEAN's amendment). The
  release also carries the standalone `drinks.json` drinks-database pack,
  which the app's own drinks auto-update fetches.
- tier overrides: idle RAM cap 40 MB (t3 rule); LICENSE kept under that
  exact name at the repo root for GitHub license detection — a documented
  deviation from the standard docs set's LICENSE.md
- data: `library.json` and `settings.json` carry `schema_version` (v1
  since v0.4.0); loaders migrate upward explicitly, preserve the
  pre-migration original beside the file, and refuse a newer version
  rather than overwrite it (see docs/MIGRATION.md)
- project records: DECISIONS.md and docs/PARITY.md remain the project's
  own records and are not replaced by this file
- condensed from: Adaptive Engineering Contract V4.0 (2026-11-01, with
  SEAN's five amendments)

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
