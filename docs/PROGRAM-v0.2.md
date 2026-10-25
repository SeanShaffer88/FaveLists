# FaveLists v0.2 Program — 10 Enhancements

Adopted by SEAN 2026-10-25 ("Adopt all 10 as the FaveLists v0.2 program").
Ships as v0.2.0 under the standing release rule: release ZIP contains BOTH
the installer .exe AND the portable .exe, plus SHA256SUMS, built by CI.

Sequencing follows the Storytold orchestration rules: the foundation slice
lands ALONE first (interfaces frozen), then feature slices build on it.

## Slice 1 — Foundation (lands alone)

- [x] **#1 Command registry + headless CLI/JSON channel** — every core
  operation (list CRUD, item add/remove/move/rank, search, export, backup)
  registered as a command, dispatched identically from the GUI, a headless
  CLI, and a JSON channel. Plus `docs/PARITY.md` (capability matrix scored
  from test evidence) and a starter test corpus.

## Slice 2 — Safety + findability

- [x] **#4 Undo + autosave journal** — undo deletes/moves/rank changes;
  crash recovery from a journal.
- [x] **#2 Global search** — one search across all lists/categories with
  year/genre filters and keyboard jump-to.

## Slice 3 — List management power

- [x] **#9 Drag-and-drop ranking + bulk actions** — drag reorder; multi-
  select move/copy/remove between lists.
- [x] **#3 Duplicate finder** — same work in multiple lists (incl. near-
  name matches) with a merge action.

## Slice 4 — Entry richness + data quality

- [x] **#5 Personal rating + note per entry** — 1–10 score and short note;
  does NOT change the locked per-category field sets.
- [x] **#6 Artist cover fallback** — second image source for artists
  (MusicBrainz has none) before manual upload.
- [x] **#7 Drinks DB quality pass** — flavor extracted from product names
  (~92% empty today), soda↔energy cross-tag fixes, caffeine surfaced for
  energy drinks.

## Slice 5 — Sharing + import

- [x] **#8 Top-10 share card export** — designed PNG (covers + ranks +
  titles) for a ranked list.
- [x] **#10 CSV import** — Letterboxd / Goodreads / IMDb exports as new
  lists, merge-only (never overwrites existing data).

**All five slices are implemented** (local tree; gate + PARITY evidence
in `docs/PARITY.md`). The v0.2.0 release itself is a separate step and,
per SEAN's direction, happens only when he calls it — the v0.2 stack
stays local until then.

## Gate (every slice)

fmt clean · clippy clean (host + windows-msvc) · FULL `cargo test --locked`
(all targets) run personally by the integrator on the slice tree and again
on the merged tree · PARITY.md updated from test evidence only.
