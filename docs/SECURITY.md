# Security

## The short version

FaveLists is local-only software. There are no accounts, no sync,
no telemetry, and no analytics. Your lists never leave your PC
except when you export them yourself.

## Your data

- Everything the program saves lives in one place: the
  `FaveListsData` folder next to the executable (library, settings,
  covers, backups, autosave journal). The folder can be relocated
  from Settings; nothing is written anywhere else.
- Data files are written atomically (temp file + rename), so a
  crash mid-save cannot corrupt the previous good file, and a
  recovery journal guards the library if its file is ever damaged.
- Removed entries sit in a Trash for 30 days before they are gone,
  and the app keeps its own weekly backup ZIPs (newest four).

## API keys

- TMDB and Twitch/IGDB credentials belong to you. They are stored
  only in `settings.json` inside your data folder — never in code,
  never in this repo, never sent anywhere except to the provider
  they belong to, as part of the lookup you asked for.
- Treat `settings.json` like a password file: don't share it in a
  bug report unless you have removed the keys first.

## Network use

The program makes network requests only for these, each triggered
by you or by a setting you can change:

- **Catalogue lookups** when you search or auto-fill (TMDB, IGDB,
  Open Library, MusicBrainz, iTunes, BoardGameGeek, and Open Food
  Facts for barcode lookups), plus cover-image downloads for
  entries you add.
- **Drinks database updates** — a silent check for a newer
  `drinks.json` pack (on by default; can be switched off or pointed
  elsewhere in Settings).
- **Update check** — a quiet launch-time check that notes in the
  status bar when a newer FaveLists release exists (on by default;
  can be switched off in Settings). It downloads nothing.

The drinks categories and all list management work fully offline.

## Downloads

Release binaries are **unsigned** (no code-signing certificate).
Every release carries `SHA256SUMS.txt` — verify the hash of the
installer or portable zip before running it (see
[INSTALL.md](INSTALL.md)).

## Reporting a problem

Report security concerns through the GitHub repository
(SeanShaffer88/FaveLists) — see [SUPPORT.md](SUPPORT.md).
