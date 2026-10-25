# FaveLists — decisions log

Kept per the house pattern: choices that affect the format, stack, or
behaviour, with the why. The full product spec is
`~/workspace/favorites-app-brief/FaveLists-SPEC.md` (outside this repo).

- **Storage: plain JSON files** (library.json / settings.json in the data
  folder), not SQLite. The library is small, lists are edited one at a time,
  and JSON keeps backups human-inspectable and the store trivially testable.
  Writes are atomic (temp file + rename).
- **Data folder next to the exe** with a pointer file
  (`favelists-location.json` beside the exe) for relocation — the installed
  location is per-user so this works without admin rights.
- **Renderer: egui/eframe with glow**, never wgpu (house lesson: wgpu with
  no backend feature panics on real Windows PCs).
- **HTTP: ureq with native-tls** (Schannel on Windows). Deliberately not
  rustls/ring: ring's build script needs MSVC tools when cross-checking
  the windows-msvc target from Linux, which breaks the local gate.
- **One entry per work; duplicates defined by work key** — provider id when
  known, else the normalised title. Same work twice in one list is blocked;
  across lists is fine.
- **Providers are parsers first**: every provider's response parsing is a
  pure function tested against recorded fixtures in `tests/fixtures/`, so
  the suite runs offline. Network code is a thin wrapper.
- **Drinks DB compiled from the Open Food Facts `en` CSV export**, streamed
  through `tools/drinks-db/build_drinks_db.py` (the full export never lands
  on disk uncompressed). Images are referenced by URL, downloaded on add —
  bundling images would grow the pack by orders of magnitude.
- **Image export layouts are pure geometry** (`src/layout.rs`), separated
  from rendering (`src/render.rs`), so all ten arrangements are unit-tested
  without pixels. Scattered/spiral randomness is a fixed-seed LCG:
  deterministic output for the same inputs.
- **Markdown exports copy covers into a `<stem>_assets/` folder** with
  relative references — a single .md file with remote-looking links would
  break offline, which is the app's home turf.
- **Backup import merges by list name + category** and re-allocates item
  ids; it never deletes or overwrites. Duplicate detection is the work key.
- **API keys live only in settings.json in the user's data folder.** None
  in code, none in the repo, none in tests (fixtures are key-free API
  responses).
