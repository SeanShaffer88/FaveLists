# Troubleshooting

## Windows SmartScreen warns on first run

The binaries are unsigned (no code-signing certificate). Choose
**More info → Run anyway**. To be sure the download arrived intact,
check it against the release's `SHA256SUMS.txt` first — see
[INSTALL.md](INSTALL.md).

## Search says to add a key, or finds nothing

Movies/TV search needs your own TMDB key and games need Twitch
(IGDB) credentials — paste them once in Settings (the README's
"Auto-fill sources & keys" section says where to get them). Without
a key, that category falls back to manual entry and everything else
keeps working. Books, music, podcasts, and board games need no key.

## Where did my lists go?

Your data lives in `FaveListsData` next to the exe you ran. If you
unzip the portable build somewhere new, that copy starts with its
own fresh `FaveListsData` — your old folder is still next to the
old exe. If you relocated the data folder from Settings, the
location is recorded in `favelists-location.json` beside the exe.

## The library file was damaged

On startup the app falls back to its autosave journal
(`library.journal.json`) when `library.json` is missing or
unreadable, and says so in the status bar. The weekly automatic
backups in `FaveListsData/backups/` can also be restored from
Settings.

## "declares schema_version …, which is newer than this build understands"

The data file was written by a newer FaveLists than the one you are
running. The older build refuses to load or overwrite it — nothing
has been changed. Run the newer build (or reinstall the latest
release) and your data opens normally. See
[MIGRATION.md](MIGRATION.md).

## A cover is missing in an image export

The export warns before producing an image when a cover is missing.
Add the entry's cover by hand (Details → Replace cover, or drag an
image file onto the cover) and export again.

## Drinks search is empty or out of date

Drinks are looked up in the bundled offline database. It updates
itself silently when online; you can also update it by hand in
Settings, or turn the automatic update off there.
