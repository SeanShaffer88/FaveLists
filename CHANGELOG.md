# Changelog

## v0.2.0 — 2026-10-25

The v0.2 program: 10 enhancements (adopted 2026-10-25; tracker in
`docs/PROGRAM-v0.2.md`), built on a new command foundation. Existing
libraries load unchanged — nothing about your lists or data folder
moves.

- **Command registry + headless JSON channel (Slice 1, #1):** every core
  operation (list/item CRUD, move/copy/rank, search, export, backup) is a
  registered command, dispatched identically from the GUI and a headless
  `--json-channel` mode.
- **Undo/redo + autosave journal (Slice 2, #4):** mutations are undoable
  (50-step history); every save also writes a recovery journal, and a
  missing/corrupt library file is recovered from it.
- **Global search (Slice 2, #2):** one search across all lists and
  categories, with result counts per list.
- **Bulk actions + drag ranking (Slice 3, #9):** multi-select
  move/copy/remove between lists as single undoable operations; drag to
  reorder ranked lists.
- **Duplicate finder + merge (Slice 3, #3):** finds the same work in
  multiple lists (including near-name matches) and merges entries,
  keeping the richest fields and covers.
- **Personal rating + note (Slice 4, #5):** every entry can carry a 1–10
  rating and a free-text note — edited in the detail panel, undoable,
  sortable (highest first, unrated last), and selectable in text and
  Markdown exports. (This reverses the v1 "no personal ratings" rule at
  SEAN's direction, 2026-10-25.)
- **Artist cover fallback (Slice 4, #6):** Artists/Bands entries, which
  start coverless because MusicBrainz has no artist images, now try two
  key-free public image sources in the background when added; a miss
  leaves the entry coverless exactly as before.
- **Drinks DB quality pass (Slice 4, #7):** the bundled drinks pack is
  regenerated from a fresh Open Food Facts export (version 2; 5,078
  entries). Flavours are extracted from product names — empty flavours
  drop from 80.9% to 57.1% and all 560 malformed leading-comma flavours
  are fixed; 277 entries are reclassified soda → energy drink on
  positive name/brand evidence (none move the other way); 317 entries
  now carry caffeine content, surfaced when a drink is added.
- **Share card export (Slice 5, #8):** any list can be exported as a
  designed 1200×630 PNG — the list name as header, then rank numbers,
  cover thumbnails, titles and years for the top 5, top 10, or up to 25
  entries. Entries without covers appear as initial tiles and are
  reported in the result. Card text is rasterised at runtime from the
  operating system's own fonts; no font file is bundled.
- **CSV import (Slice 5, #10):** diary / ratings / library CSV exports
  are recognised by their header shape and imported merge-only — into a
  new list, or an existing one. A mixed movie/TV ratings file imported
  as new splits into one list per category. Titles, years, authors,
  genres and directors are carried across, and ratings are converted
  onto the 1–10 scale. Works already in the destination list are
  skipped and counted, never overwritten; rows without a title are
  counted separately; the whole import is a single undo step.

## v0.1.1 — 2026-10-25

Bug-fix release.

- Fixed: on Windows the app now runs in the Windows GUI subsystem, so
  launching FaveLists no longer opens a console window alongside the
  GUI. In v0.1.0 the binary was built as a console-subsystem program,
  so Windows showed a console window next to the app (and closing that
  console would close the app). No other behaviour changes.
- Builds are unsigned. Windows GUI behaviour remains **Unverified**
  until the maintainer's on-machine field test (`docs/FIELD_TEST.md`)
  is complete.

## v0.1.0 — 2026-10-25

First release.

- 11 single-type list categories: Movies, TV Shows, Books, Music
  Artists/Bands, Albums, Songs, Video Games, Podcasts, Board Games,
  Energy Drinks, Sodas — each with its own basic detail fields (year,
  genre, director/author equivalents, main cast for movies and shows;
  no synopses, no personal ratings).
- Auto-fill search with cover thumbnails: TMDB (movies/shows, bring your
  own free API key), Open Library (books), MusicBrainz + Cover Art
  Archive (artists/albums/songs), IGDB via Twitch credentials (games),
  iTunes (podcasts), BoardGameGeek (board games). Anything not found can
  be added manually with your own details and cover.
- Bundled offline drinks database: 5,091 sodas and energy drinks compiled
  from Open Food Facts, with silent auto-update from this repo's releases
  plus a manual update button in Settings.
- Ranked or unranked lists (switchable), drag to reorder, data stored
  next to the exe with an option to relocate it (existing data moves
  automatically).
- Exports: plain text and Markdown (covers included, detail checklist
  remembered), cover-only images in PNG or JPG with 10 arrangements, a
  preview with drag-to-swap, and per-export crop/fit choice; library
  backup to a single zip (one list, selected lists, a category, or
  everything) with merge-only import.
- Windows runtime is **Unverified** until the maintainer's on-machine
  field test (`docs/FIELD_TEST.md`) is complete. Builds are unsigned.
