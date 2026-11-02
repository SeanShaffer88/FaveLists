# Compatibility

## Operating system

- **Windows 10 or Windows 11, 64-bit (x64)** — the only supported
  platform for the shipped binaries.
- Releases are x64 only. There are no x86 or ARM64 builds.

## Runtime requirements

- **None.** The executable links its C runtime statically: it needs
  no .NET, no Visual C++ redistributable, no Java, and no Electron.
- No account or licence server. An internet connection is used only
  for the optional online lookups, the drinks-database update, and
  the update check (see [SECURITY.md](SECURITY.md)) — the drinks
  categories and all list work function fully offline.
- The installer is per-user and needs **no administrator rights**.
  The portable build needs no install at all.
- The binaries are **unsigned** (no code-signing certificate), so
  Windows SmartScreen may warn on first run — choose *More info →
  Run anyway* if you trust the build, and verify the download
  against the release's `SHA256SUMS.txt` (see
  [INSTALL.md](INSTALL.md)).

## Data compatibility

- **File format**: `library.json` and `settings.json` are versioned
  (`schema_version` 1 since v0.4.0). Files from v0.3.0 and earlier
  (no version field) load unchanged and are upgraded on the next
  save; a file written by a newer version is refused with a clear
  message and never overwritten by an older build. See
  [MIGRATION.md](MIGRATION.md).
- **Data location**: everything lives in the `FaveListsData` folder
  next to the executable (or wherever it was relocated to — the
  location is recorded in `favelists-location.json` beside the exe).
- Both data files are plain JSON and can be inspected or backed up
  with any text editor.
- Backup ZIPs from earlier versions still import (import is
  merge-only and tolerant of older payloads).

## Building from source

- Rust 1.80+ (the crate's `rust-version`).
- Development, the test suite, and the windows-gnu cross-checks also
  run on Linux; the shipped Windows binary is built by the release
  workflow on a Windows runner.
