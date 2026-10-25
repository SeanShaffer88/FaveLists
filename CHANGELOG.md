# Changelog

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
