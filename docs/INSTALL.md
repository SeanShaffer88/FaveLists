# Install

## Downloads

Every release on the
[releases page](https://github.com/SeanShaffer88/FaveLists/releases)
carries four files (Windows 10/11, 64-bit):

- **FaveLists-v{VERSION}-x64-Setup.exe** — a per-user installer (no
  admin prompt). Installs for your account only and adds a Start
  menu entry.
- **FaveLists-v{VERSION}-x64-Portable.zip** — the portable build:
  the exe, the license, and a short PORTABLE note in one zip. Unzip
  anywhere and run.
- **drinks.json** — the drinks-database update pack the app fetches
  by itself for its silent auto-update. Not a download you need.
- **SHA256SUMS.txt** — SHA-256 checksums for the other three files,
  so a download can be verified before it is run.

## Checking a download

In PowerShell, in the folder you downloaded to:

```powershell
Get-FileHash .\FaveLists-v0.4.0-x64-Setup.exe -Algorithm SHA256
```

Compare the result with the matching line in `SHA256SUMS.txt`.

## First run

Both builds are **unsigned**, so Windows SmartScreen may warn on
first run — choose **More info → Run anyway**.

No install is needed for the portable build, and nothing is written
outside the program's own folder: your lists live in a
`FaveListsData` folder created next to the exe (library, settings,
covers, backups). Moving the folder moves the whole app and all
your data together.

A short first-run setup wizard opens on a genuinely fresh data
folder; it can be skipped and revisited from Settings.

## Uninstalling

Uninstalling (or deleting the portable folder) keeps your
`FaveListsData` folder, so your lists survive a reinstall. Delete
that folder yourself if you want everything gone.

## Building from source

Rust 1.80+ (the crate's `rust-version`):

```sh
cargo build --release
cargo test --locked
```

The full developer gate is in [AGENTS.md](../AGENTS.md).
