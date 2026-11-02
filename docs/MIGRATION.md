# Migration

## v0.4.0 — data files gain a `schema_version`

v0.4.0 stamps a format version into both stored JSON files. The
change happens automatically on the first v0.4.0 run; nothing needs
to be done by hand, and nothing is deleted.

### What changes

| File | Before v0.4.0 | v0.4.0 and later |
|---|---|---|
| `FaveListsData/library.json` | no version field | `"schema_version": 1` |
| `FaveListsData/settings.json` | no version field | `"schema_version": 1` |

The data folder itself does not move, and no field inside either
file changes meaning — version 1 is exactly the v0.3.0 format plus
the stamp.

### How it works

- A file **without** the field is version 0 (every release up to
  v0.3.0). It loads exactly as before. Before the migrated state can
  be saved back, the loader keeps a pristine copy of the original
  file beside it — `library.json.v0-backup` /
  `settings.json.v0-backup` — written once and never overwritten, so
  the pre-migration bytes always survive. The migration becomes
  permanent on the next save, which stamps version 1.
- A file with a **higher** version than the running build
  understands was written by a newer FaveLists: the build refuses it
  with a clear error and **never overwrites the file** — not even
  with defaults. Re-running the newer build reads it normally.
- The migration is covered by fixture tests
  (`tests/schema_migration.rs`): the fixtures are byte-for-byte what
  v0.3.0 writes, and the tests prove they load, migrate, round-trip
  at version 1, and that newer files are refused untouched.

### Going back to v0.3.0

v0.3.0 ignores the `schema_version` field, so a library saved by
v0.4.0 still opens in v0.3.0. If anything looks wrong, the
`.v0-backup` copies are the exact files v0.3.0 last wrote — close
the app, copy one back over its live file (dropping the
`.v0-backup` suffix), and reopen.

## Earlier versions

No earlier migrations exist: up to v0.3.0 the data files carried no
version stamp, and new fields were added only with defaults that
older files load through. Backup ZIPs and exports from any earlier
version still import (merge-only) into current builds.
