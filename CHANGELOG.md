# Changelog

## v0.3.0 — 2026-10-25

The v0.3 program: 20 enhancements in six slices (tracker in
`docs/PROGRAM-v0.3.md`), stacked on the v0.2 foundation. This release
also folds in the three v0.2.1 field-test fixes, which were built but
never released separately (listed at the end). Existing libraries
load unchanged — nothing about your lists or data folder moves.

- **Tags on every entry:** give any entry free-form tags, filter a
  list by tag, see every tag in your library with its count, and find
  entries by their tags in global search. Tags also ride along when
  duplicate entries are merged, and can be included in text and
  Markdown exports.
- **A status on every entry:** mark entries as wanted, done, or
  owned — the words adapt to the category ("Want to watch / Watched /
  Own it" for movies, "Want to read / Read / Own it" for books, and so
  on) — and filter lists and search results by status.
- **Smart views:** three new sidebar views — Top Rated, Recently
  Added, and Needs a Rating — pull entries from across all your
  lists; click any entry to jump straight to it in its list.
- **Share cards for smart views:** any smart view can be exported
  as a share card, headed with the view's name.
- **Pick for me:** can't decide? One click picks a random favorite
  from a list, a whole category, a smart view, or your entire
  library, with a short reveal before it settles.
- **CSV and JSON export:** export any list to a spreadsheet-ready
  CSV file with every detail included, or export the whole library
  as one CSV file or as a JSON file in the app's own format (the
  round-trip partner of CSV import).
- **Paste-a-list import:** paste a plain list of titles, one per
  line; the app matches each line against the online catalogs and
  shows a review screen — matched, manual, or already present —
  before anything is added, all in a single undo step.
- **Smarter duplicate finding:** the duplicate finder now also
  groups near-identical titles — "Part II" next to "Part 2",
  edition variants, and small typos — as possible duplicates
  (sequels are never grouped by mistake).
- **Add drinks by barcode:** on a drinks list, type or scan a
  barcode and the app looks the product up and fills in its name,
  brand, flavour, and caffeine for you.
- **Tried it, for drinks:** mark a drink as tried with one click in
  its details, and watch the "Tried X of Y" progress at the top of
  each drinks list.
- **Caffeine sort and filter:** sort a drinks list by caffeine
  content — strongest first — or filter it down to the drinks at
  or above a caffeine level you set.
- **Missing-cover sweep:** a new sidebar screen lists every entry
  that has no cover; fetch the missing covers one at a time or all
  at once, without leaving the screen.
- **Text size setting:** Settings now offers Comfortable, Large,
  and Extra large text; the choice applies immediately and is
  remembered. Existing users keep Large — the size the app already
  used.
- **Cover size + rows view:** a slider in Settings scales the
  cover grid up or down, and a Rows toggle (in Settings or right
  in the list header) switches a list to compact rows — cover,
  title, rating, and status on one line — for long lists.
- **High-contrast option:** a Settings switch keeps the black/red
  look but pushes text, accents, and borders brighter so edges
  read clearly.
- **Drop a cover on it:** drag an image file onto a cover and it
  becomes that entry's cover (dropping elsewhere in the window
  uses the selected entry); pasting an image file's path while an
  entry is selected works too.
- **Trash with restore:** removing an entry no longer deletes it
  outright — it waits in a Trash view for 30 days, with everything
  (rating, note, tags, status) intact, and one click puts it back.
  Emptying the trash is explicit, and undo still brings a removal
  back instantly.
- **Automatic backups:** the app now keeps its own rolling
  backups — a full backup once a week, the newest four kept — and
  any of them can be restored from Settings (restoring merges,
  never replaces). A "Back up now" button lives there too.
- **Update check:** on launch the app quietly checks the project's
  releases page; when a newer version exists, the status bar shows
  a small "vX available" link. Nothing is ever downloaded
  automatically, and a failed check shows nothing.
- **First-run setup wizard:** a fresh install opens a short
  welcome walkthrough — optional API keys (with links to get
  them), a look at where your data lives — that can be skipped in
  one click and reopened any time from Settings. Existing users
  never see it unasked.

Also folded in — the v0.2.1 field-test fixes (built after the first
v0.2.0 field test, released here for the first time):

- **Get API key links in Settings:** each API key field — TMDB, Twitch
  Client ID, and Twitch Client Secret — now has a "Get API key ↗" link
  beside it that opens the right sign-up page in your browser.
- **Move data folder note:** Settings now says plainly, next to the Move
  data folder button, that moving is safe — everything in the data
  folder moves with it and nothing is left behind or lost.
- **Larger text:** the whole app renders larger by default (this
  became the Large choice of the new Text size setting above).

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
