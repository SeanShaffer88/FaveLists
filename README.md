# FaveLists

## TL;DR

FaveLists is a Windows program for making lists of your favorite things —
movies, TV shows, books, music artists, albums, songs, video games, podcasts,
board games, energy drinks, and sodas. Start typing a title and it suggests
matches with cover art, so adding a favorite takes seconds. When a list is
done, save it as a tidy text or Markdown file, or turn it into a picture made
only of the covers/posters — pick from lots of arrangements, shuffle the
covers around in a preview until it looks right, and share the image anywhere.
Everything is stored on your own PC, in a folder next to the program.

## What it does

- **11 list types, one type per list.** A movie list holds only movies, an
  album list only albums, and so on.
- **Auto-fill search.** Type a few letters and pick from a dropdown showing
  the cover, year, and a hint line (director, author, artist, platform) so
  remakes are easy to tell apart. Details like year, genre, director, and
  main cast are filled in for you — and every field can be edited afterwards.
- **Ranked or not, your choice.** Lists can be ranked (numbered, drag covers
  to reorder) or unranked, and you can switch a list at any time.
- **Undo, search, and tidy-up tools.** Ctrl+Z undoes changes (50 steps),
  one search covers every list at once, multi-select moves or copies
  entries between lists in bulk, a duplicate finder spots the same work
  in several lists and merges it, and every entry can carry your own
  1–10 rating and a short note. A recovery journal guards your library
  if its file is ever damaged.
- **Tags, status, and smart views.** Every entry can carry free-form
  tags and a want/done/own-it status (the words adapt per category);
  lists filter by both, global search includes them, and three
  sidebar smart views — Top Rated, Recently Added, Needs a Rating —
  pull entries from across all your lists. Stuck for a choice?
  "Pick for me" picks a random favorite for you.
- **Text exports** (`.txt` and Markdown) with a header and only the details
  you tick — your tick choices are remembered. Markdown exports include the
  cover images in a companion folder.
- **Image exports** (PNG or JPG) made only of cover art — no words in the
  picture at all. Ten arrangements (uniform grid, topster, mosaic, shelf
  rows, scattered collage, spiral, honeycomb, ring, filmstrip, masonry),
  a preview where you drag covers to swap places, and a crop-to-fill or
  whole-cover choice per export. If a cover is missing you get a warning
  before exporting.
- **Share cards.** Turn a list into a designed picture (list name, rank
  numbers, covers, titles, years) sized for sharing, straight from the app.
- **Backup & restore.** Export one list, some lists, a whole category, or
  everything into a single ZIP (data + covers). Importing merges: it adds
  what's in the file and never deletes or overwrites what you already have.
  Diary, ratings, and library CSV exports can be imported the same
  merge-only way. Removed entries wait 30 days in a Trash view with a
  one-click restore, and the app keeps its own weekly backups (the
  newest four) that can be restored from Settings.
- **Make it yours.** Text size (Comfortable / Large / Extra large), a
  cover-size slider, a compact rows view, a high-contrast option,
  and drag-an-image-onto-a-cover to set artwork. A quiet launch-time
  check tells you in the status bar when a newer version is out —
  nothing is ever downloaded automatically.
- **Energy drinks & sodas work offline.** They are looked up in a drinks
  database built into the program (compiled from Open Food Facts data),
  which can silently update itself and can also be updated by hand in
  Settings.
- **Your data stays yours.** Lists live in a `FaveListsData` folder next to
  the program file. Settings can move that folder elsewhere and takes your
  existing lists and covers with it. No accounts, no sync, no tracking.

## Auto-fill sources & keys

Movies and TV shows come from TMDB, and video games from IGDB. Both need a
free key of your own, pasted once into Settings:

- TMDB: sign in at themoviedb.org → your avatar → Settings → API → create a
  key and copy the short "API Key".
- IGDB: this runs through Twitch. Sign in at the Twitch Developer Console,
  register an application (Redirect URL: `localhost`, Client Type:
  Confidential), then copy the Client ID and generate a Client Secret.

Without a key, that category simply falls back to manual entry; everything
else keeps working. Books (Open Library), music (MusicBrainz), podcasts
(iTunes), and board games (BoardGameGeek) need no key. Anything that can't
be found can always be added by hand, with your own cover image.

## Drinks data attribution

The drinks database is compiled from [Open Food Facts](https://world.openfoodfacts.org)
data, available under the Open Database License (ODbL). Product images are
© their contributors, Creative Commons Attribution-ShareAlike (CC BY-SA).
The compile pipeline lives in `tools/drinks-db/`.

## Download

Two downloads on the [latest release](https://github.com/SeanShaffer88/FaveLists/releases/latest)
(Windows 10/11, 64-bit) — pick whichever suits you:

- **FaveLists-Setup-v0.3.0-x64.exe** — a per-user installer (no admin prompt).
  Installs for your account only and adds a Start menu entry.
- **FaveLists-v0.3.0-portable.zip** — the portable build: the exe, the
  license, and a short PORTABLE note in one zip. Unzip anywhere and run;
  the drinks database is built in, and your lists are stored in a
  `FaveListsData` folder next to the exe.

Both are **unsigned**, so Windows SmartScreen may warn on first run — choose
"More info" → "Run anyway". Uninstalling keeps your `FaveListsData` folder,
so your lists survive a reinstall. The release page also carries a
standalone `drinks.json` — that's the drinks-database update pack the app
fetches by itself for its silent auto-update, not a download you need.

## Building

Rust + egui. Storage is plain JSON files (library, settings) in the data
folder — no database server, nothing to install.

```sh
cargo build --release
cargo test --locked
```

## Screenshots

Screenshots will be added from real runs on the maintainer's Windows PC —
see `docs/screenshots/CAPTURE.md` for the capture list. (First launch on
Windows was field-verified on v0.1.1; the full field test lives in
`docs/FIELD_TEST.md`, and the per-capability status is in `docs/PARITY.md`.)

## License

MIT — see `LICENSE`. Copyright 2026 Sean Shaffer.
