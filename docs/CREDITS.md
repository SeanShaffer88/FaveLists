# Credits

- **Sean Shaffer** — design and development.
- **Open-source libraries** (MIT/Apache-2.0 licensed):
  [egui / eframe](https://github.com/emilk/egui) for the window,
  [serde](https://github.com/serde-rs/serde) and
  [serde_json](https://github.com/serde-rs/json) for the data
  files, [ureq](https://github.com/algesten/ureq) for HTTP,
  [image](https://github.com/image-rs/image) for cover handling,
  [fontdue](https://github.com/mooman219/fontdue) for share-card
  text, [zip](https://github.com/zip-rs/zip) for backups, and
  [rfd](https://github.com/PolyMeilex/rfd) for file dialogs — all
  built on the Rust standard library and toolchain.
- **Catalogue data**: [TMDB](https://www.themoviedb.org),
  [IGDB](https://www.igdb.com) (via Twitch), Open Library,
  MusicBrainz, iTunes, and BoardGameGeek — looked up live, at the
  user's request, with the user's own keys where a key is needed.
- **Drinks data**: compiled from
  [Open Food Facts](https://world.openfoodfacts.org) data under the
  Open Database License (ODbL); product images © their
  contributors, CC BY-SA. The compile pipeline lives in
  `tools/drinks-db/`.
- **Inno Setup** for the installer.
