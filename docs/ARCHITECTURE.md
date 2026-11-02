# Architecture

FaveLists is a single Rust crate (`favelists`) with a library, a GUI
binary, and a small drinks-database build tool. The design rule
(AGENTS.md): **all decision logic lives in the library** and is
testable off-Windows; the GUI renders state and collects intent only.

## Layout

- `src/model.rs` — the data model: the 11 fixed categories and their
  exact field sets, items, lists, the library, and user settings.
  Pure data + logic; no I/O beyond (de)serialisation.
- `src/store.rs` — persistence: the library and settings as
  versioned JSON in the data folder, written atomically (temp file +
  rename), plus the autosave journal, the location pointer, and data
  folder relocation.
- `src/commands.rs` — the command registry: every library mutation
  is a command, dispatched identically from the GUI and from the
  headless channel, with undo/redo history.
- `src/json_channel.rs` — the headless `--json-channel` mode: the
  same commands over stdin/stdout JSON, no window.
- `src/providers/` (+ `src/providers.rs`) — online catalogues as
  **pure parsers first**, tested against recorded fixtures in
  `tests/fixtures/`; network code is a thin wrapper. Network work
  runs on background threads, never the UI thread.
- `src/drinks.rs` — the bundled offline drinks database (compiled
  from Open Food Facts by `tools/drinks-db/`; the database itself is
  generated, never hand-made).
- `src/layout.rs` / `src/render.rs` — image-export geometry (pure,
  unit-tested without pixels) and rasterisation.
- `src/share_card.rs` — share-card layout and rendering.
- `src/backup.rs` / `src/auto_backup.rs` — backup ZIP export and
  merge-only import; the weekly automatic backup rotation.
- `src/app*.rs` — the egui GUI: one module per surface (settings,
  covers, drinks, import, trash, wizard, …) over the shared state.

## Platform choices (see [DECISIONS.md](../DECISIONS.md))

- **Renderer:** egui via eframe with the **glow** renderer,
  explicitly selected.
- **HTTP:** ureq with **native-tls** (Schannel on Windows).
- **Storage:** plain JSON files, not a database — the library is
  small, edited one list at a time, and JSON keeps backups
  human-inspectable. Both files carry a `schema_version` (see
  [MIGRATION.md](MIGRATION.md)).

## Data flow

The GUI and the JSON channel both load through `Store`, mutate only
through the command registry, and save through `Store::save_library`
— which also refreshes the autosave journal with the identical
state, so the journal can never miss a state the main file has.
