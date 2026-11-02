# Features

What FaveLists does today. The per-capability verification status is
in [PARITY.md](PARITY.md); the short version is in the
[README](../README.md).

## Lists

- **11 fixed list types, one type per list:** movies, TV shows,
  books, music artists, albums, songs, video games, podcasts, board
  games, energy drinks, and sodas.
- **Ranked or unranked** per list, switchable at any time; switching
  never reorders entries. Drag to reorder in ranked lists.
- **Undo/redo** (Ctrl+Z, 50 steps) for library changes, plus an
  autosave journal that recovers the library if its file is damaged.

## Adding entries

- **Auto-fill search** with cover art, year, and a hint line
  (director, author, artist, platform) so remakes are easy to tell
  apart; every filled field can be edited afterwards.
- Sources: TMDB (movies/TV), IGDB via Twitch (games), Open Library
  (books), MusicBrainz (music), iTunes (podcasts), BoardGameGeek
  (board games). TMDB and IGDB need the user's own free keys, pasted
  into Settings; the rest need none. Anything can also be added by
  hand, with a user-supplied cover image.
- **Drinks work fully offline** against the bundled drinks database
  (compiled from Open Food Facts data), including **add by barcode**.
- **Paste-a-list import:** paste titles, one per line, review the
  matched / manual / already-present split, then add in one undo
  step. CSV import (several known formats) and diary/ratings imports
  merge the same way.

## Organising

- **Tags** on every entry (filter by tag, tag counts across the
  library, tags in search and exports).
- **Personal status** per entry — want / done / own-it, with wording
  that adapts per category — plus a personal **1–10 rating** and a
  free-text **note**.
- **Smart views:** Top Rated, Recently Added, and Needs a Rating,
  pulled from across all lists; **Pick for me** chooses a random
  favorite from a list, category, view, or the whole library.
- **Global search** across every list at once; **bulk actions**
  (multi-select move/copy between lists); a **duplicate finder**
  that also groups near-identical titles and merges them.
- **Trash:** removed entries wait 30 days with one-click restore.

## Exporting

- **Text and Markdown exports** with a header and only the detail
  fields you tick (choices are remembered per category); Markdown
  exports carry the cover images in a companion folder.
- **Cover-image exports** (PNG/JPG) made only of cover art: ten
  arrangements (uniform grid, topster, mosaic, shelf rows, scattered
  collage, spiral, honeycomb, ring, filmstrip, masonry), a preview
  with drag-to-swap, and a missing-cover warning before exporting.
- **Share cards:** a designed picture (list name, ranks, covers,
  titles, years) for a list or a smart view.
- **CSV and JSON export** of a list or the whole library.

## Backup and data safety

- **Backup & restore:** export one list, selected lists, a category,
  or everything to a single ZIP (data + covers). Import merges —
  it adds what is in the file and never deletes or overwrites.
- **Automatic weekly backups** (newest four kept), restorable from
  Settings.
- **Relocatable data folder:** all data lives in `FaveListsData`
  next to the program; Settings can move it, taking existing lists
  and covers along.
- **Versioned data files** with safe migration — see
  [MIGRATION.md](MIGRATION.md).

## Appearance

- Text size (Comfortable / Large / Extra large), a cover-size
  slider, a compact rows view, and a high-contrast option.
- Drag an image onto a cover to replace the artwork.
- A quiet launch-time check notes in the status bar when a newer
  version is available; nothing is ever downloaded automatically
  (the drinks database pack is the one exception, and it can be
  switched off in Settings).
