# Support

## Getting help

- **Bugs and feature requests**: open an issue on the GitHub
  repository (SeanShaffer88/FaveLists). Include the version (shown
  in Settings / on the release you downloaded), what you did, and
  what happened instead.
- **How things work**: the [README](../README.md) walks through the
  features, and [FEATURES.md](FEATURES.md) lists them in full.

## Before you file

- Check [TROUBLESHOOTING.md](TROUBLESHOOTING.md) — the common cases
  (SmartScreen warning, missing API keys, finding your data folder,
  the schema-version message) are answered there.
- Per-capability status is tracked honestly in
  [PARITY.md](PARITY.md).

## What support can and can't do

FaveLists is free, local-only software with no accounts and no
telemetry: there is no server-side data to look up, reset, or
recover. Everything the program saves is in the `FaveListsData`
folder next to the executable (see [MIGRATION.md](MIGRATION.md)).
If you attach a data file to a bug report, remove the API keys from
`settings.json` first — they are yours and are never required to
diagnose a problem.
