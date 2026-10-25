# FaveLists — Capability Parity & Verification Matrix

Baseline: **v0.1.1** feature set plus the v0.2 Slice 1 foundation
(command registry + JSON channel), Slice 2 (undo/redo + autosave
journal, filtered global search), Slice 3 (bulk actions +
drag-to-list, duplicate finder + merge), Slice 4 (personal
rating + note, artist cover fallback, drinks DB quality pass), and
Slice 5 (share-card export, CSV import) — the complete v0.2 program.
Scored **only** from test evidence in
this tree and field evidence from SEAN's machine — never from intention.
Update rule (program gate): a row changes only when a test (or a field
test) changes what it proves.

**Labels** — *Verified*: covered by passing tests at library level (or
field-verified on SEAN's machine where the row says so). *Partially
Verified*: some layers tested, some not (the gap is named). *Unverified*:
no test or field evidence yet.

**Evidence base**: 156 tests in tree — 55 pre-existing (`model` 9,
`providers` 11, `drinks` 8, `layout` 7, `render` 6, `backup` 5, `store`
5, `export_text` 4) + 13 added by Slice 1 (`commands` unit 4,
`json_channel` unit 2, `tests/commands.rs` integration 7) + 20 added by
Slice 2 (`commands` unit 3 for history, `search` unit 2,
`tests/slice2.rs` integration 15: undo/redo, journal, search, channel
equivalence) + 21 added by Slice 3 (`duplicates` unit 6,
`tests/slice3.rs` integration 15: bulk actions, finder, merge, channel
equivalence) + 19 added by Slice 4 (`model` unit 1 for rating sort,
`export_text` unit 1, `artist_image` unit 5, `drinks` unit 4,
`tests/slice4.rs` integration 8: back-compat fixtures, rating/note
commands + sort + exports, artist parsers, drinks pack tolerance) +
28 added by Slice 5 (`csv_import` unit 10: reader edge cases,
detection signatures, per-format mapping + rating conversions;
`share_card` unit 5: font-free layout/truncation + a font-gated render
test; `tests/slice5.rs` integration 13: per-format import counts,
split-list creation, merge-only/duplicate/invalid accounting,
wrong-category rejection, one-undo-step, channel equivalence,
share-card PNG canvas + count clamping + missing-cover report) —
plus the corpus under `tests/fixtures/` (provider responses incl. the
Slice 4 Deezer/Wikipedia artist fixtures; `commands/` session +
bad-request scripts; `search/` library-and-cases fixture;
`duplicates/` library fixture; `csv/` import fixtures; `library_v011.json` and
`drinks_pack_v1.json` back-compat fixtures) and SEAN's 2026-10-25
v0.1.1 screenshot.

| Capability | Status | Evidence / gap |
|---|---|---|
| 11 fixed categories, exact per-category field sets (spec §2) | Verified | `model::tests::category_field_sets_match_spec`; registry rejects unknown category ids (`commands` bad-params tests) |
| List create / rename / delete / ranked switch / one-shot sorts | Partially Verified | Registry dispatch covered end-to-end (`tests/commands.rs::every_registered_command_dispatches_successfully`, model sort/move tests). GUI create/delete/ranked/sort route through the registry; the header inline rename edits the model directly (per-keystroke) — its command exists and is tested headlessly, the GUI path is not |
| Item add (manual + auto-fill pending add) with same-work duplicate blocking | Partially Verified | Model duplicate tests (provider-id and normalised-title keys); registry `item.add` added/duplicate outcomes + cover-bytes round-trip tested. GUI add paths route through the registry; provider detail/cover fetch is network code (see auto-fill row) |
| Item edit (title/fields/cover/rating/note) and remove | Partially Verified | Registry `item.edit`/`item.remove` tested incl. not-found errors and the #5 rating/note params; GUI detail panel routes through the registry. GUI flow itself field-Unverified |
| Reorder: move / swap / set-rank | Partially Verified | Model `move_and_swap_reorder`; registry move/swap/set-rank tested. GUI grid drag routes `item.move` through the registry; drag interaction itself field-Unverified beyond first launch |
| In-list filter query (title contains, case-insensitive) | Partially Verified | Registry `list.query` uses the GUI's exact predicate and is tested, incl. via the corpus session fixture. The GUI grid still applies its own inline copy of the predicate |
| Auto-fill suggestion parsers (TMDB, Open Library, MusicBrainz/CAA, IGDB, iTunes, BGG) | Partially Verified | 11 fixture tests on recorded key-free responses in `tests/fixtures/`. Live provider calls Unverified by design (user keys live only in the data folder) |
| Offline drinks DB: bundled pack, search, dedupe, flavour parse | Partially Verified | 12 `drinks` tests on the real bundled pack (v2, 5,078 entries — see the #7 row for the quality pass and its measured stats) |
| Drinks DB update (validate + silent/manual refresh) | Partially Verified | Validation rules tested in `drinks`; the live fetch against the release-asset URL is Unverified |
| Settings: keys storage, remembered export fields | Partially Verified | Model remembered-fields test; settings round-trip in `store` tests. GUI settings dialog field-Unverified |
| Persistence: versioned JSON, atomic save, data-next-to-exe, relocation auto-move | Partially Verified | 5 `store` tests (round-trip, default dir, relocate + pointer, non-empty refusal) + Slice 2 journal cases below. Windows path behaviour Unverified; SEAN's screenshot shows the resolved data path in the status bar |
| Text (.txt) export with field selection + ranked numbering | Verified | 4 `export_text` tests + registry `export.text` writes and content asserted in `tests/commands.rs` |
| Markdown (.md) export with cover assets folder | Verified | `export_text` asset test + registry `export.markdown` (assets written, relative refs asserted) |
| Cover-image export: 10 arrangements, layout geometry | Verified | 7 `layout` tests across arrangements (deterministic) |
| Cover-image export: rendering, crop-fill vs whole-cover, PNG/JPEG encode, missing-cover detection | Verified | 6 `render` tests + registry `export.image` writes a real PNG and reports the coverless item index |
| Image-export GUI flow (subset checklist, preview drag-to-swap, save) | Partially Verified | Final render now routes through `export.image`; the preview still computes layout inline in the GUI, and the dialog flow is field-Unverified beyond first launch |
| Library backup export (single/selected/whole/category scopes) | Verified | 5 `backup` tests (scope filtering) + registry scope tests incl. category scope count |
| Library backup import (merge-only, duplicate skip, id re-allocation, covers restored) | Verified | `backup` merge/partial-overlap/non-FaveLists-rejection tests + registry round-trip (import into same library adds 0, skips all) |
| **Command registry dispatch (Slice 1 #1)** | Verified | All 31 registered commands covered: the original 22 dispatched successfully end-to-end on a fixture store (`tests/commands.rs`), the 2 history commands by the Slice 2 tests below, the 5 Slice 3 commands (bulk ×3, finder, merge) by `tests/slice3.rs`, the 2 Slice 5 commands (share card, CSV import) by `tests/slice5.rs`; bad params / unknown command / missing entities error cleanly without mutating; base64 cover path round-trips (`commands` unit + `tests/commands.rs`) |
| **Surface equivalence: direct calls ≡ registry ≡ JSON channel (Slice 1 #1)** | Verified | `tests/commands.rs::registry_dispatch_matches_direct_library_calls` and `::json_channel_matches_registry_path` land on identical store state; corpus session fixture reproduces the recorded expected library exactly |
| **JSON channel protocol (Slice 1 #1)** | Partially Verified | Session + error-continuation tests (`json_channel` unit + corpus bad-request fixture: `bad_json`/`bad_request`/`unknown_command`/`not_found` in order). Verified on this platform; behaviour under the Windows GUI subsystem (console-less stdio) is Unverified until a field test |
| **Undo / redo (Slice 2 #4)** | Verified | Exact-state tests (`tests/slice2.rs` + `commands` unit): add/delete-list/set-rank undone to byte-equal prior state (ids, order, id counters); redo reapplies; a new mutation clears redo; failed and no-change commands record nothing; the 50-entry cap holds (55 mutations → exactly 50 undos, counters rolled back); a JSON-channel session undoes/redoes and the result persists. The GUI's Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z wiring dispatches through this same layer; keystroke handling itself is GUI behaviour (see Windows GUI rows). Known limit: mutations that bypass dispatch (the header inline rename) are not undoable, and cover files written by an undone add/import stay in the cache as orphans |
| **Autosave journal recovery (Slice 2 #4)** | Verified | `tests/slice2.rs` journal cases: every `save_library` rewrites `library.journal.json` identically; corrupt main + valid journal recovers the journal state with the recovery flag set; missing main + journal recovers; parseable main beats a stale journal (no flag); fresh store loads default unflagged; corrupt main with no journal is an honest error. The GUI surfaces the flag as a status-line message on launch (compile-checked; see Windows GUI rows) |
| **Global search command (Slice 2 #2)** | Verified | `library.search` filters — title (case-insensitive contains), `category`, `year` (year/years field contains), `genre` (contains, case-insensitive), combined, and empty-query-matches-all semantics — are asserted case-by-case by the corpus fixture `tests/fixtures/search/library_and_cases.json` (10 cases) plus shape/validation tests (jump-to fields `item_id`/`title` top-level; mistyped filters are `bad_params`); registry ≡ JSON channel equivalence on a filtered search (`tests/slice2.rs::filtered_search_channel_matches_registry`) |
| **Global search GUI view + jump-to (Slice 2 #2)** | Partially Verified | The sidebar box (Ctrl+K focus) routes the query through `library.search` dispatch (no inline filtering) and renders `search::hits_from_result`/`grouped` groupings — those pure functions are unit-tested (`search` unit 2). The results interaction (grouped view, click-to-jump selection, Esc-to-clear) is field-Unverified |
| **Bulk item actions: remove / move / copy (Slice 3 #9)** | Verified | `tests/slice3.rs`: remove reports exactly the ids removed and ignores unknown ones; move preserves item ids and source order, skips (and reports) works already in the target under the model's `contains_work` rule, same-list move is a no-op, missing-list errors mutate nothing and record no history; copy allocates fresh ids from the library counter and shares the cover file reference (no byte duplication); any bulk op is exactly one undo step restoring the whole prior state; `items.bulk_move` registry ≡ JSON channel equivalence incl. persisted state |
| **Drag-to-list (Slice 3 #9)** | Partially Verified | The drop dispatches the Verified `items.bulk_move` with a single id, so skip/undo semantics are the tested ones, and the skip surfaces the status line "Already in that list — skipped."; the sidebar-row rect/drop interaction itself is GUI behaviour — field-Unverified. (Grid drag reorder pre-dates this slice; see the reorder row) |
| **Multi-select grid + bulk action bar (Slice 3 #9)** | Partially Verified | Ctrl+click (any mode) and plain click in Select mode toggle the selection; the bar's Move/Copy/Remove dispatch the Verified bulk commands and clear the selection on success — dispatch behaviour is test-proven, the click/highlight interaction is field-Unverified |
| **Duplicate finder (Slice 3 #3)** | Verified | `duplicates` unit tests + the corpus fixture `tests/fixtures/duplicates/library.json` driven through both the pure function and `library.find_duplicates`: article stripping ("The Matrix" ≡ "Matrix"), punctuation-insensitivity ("Spider-Man" ≡ "Spider Man"), exact vs possible by year agreement/conflict, category separation (the Books "Dune" never joins the Movies group), within-list copies reported, no singleton false positives; registry results round-trip losslessly into the GUI's group structs (`groups_from_result`) |
| **Duplicate merge (Slice 3 #3)** | Verified | `tests/slice3.rs`: the kept copy adopts only fields it lacks (first non-empty in drop order), a cover, and a provider identity; kept fields are never overwritten; every invalid shape (keep ∈ drop, empty drop, missing list/item) errors `bad_params`/`not_found` with zero mutation and no history step; a merge is exactly one undo step; registry ≡ JSON channel equivalence incl. the persisted merged library |
| **Duplicates GUI view (Slice 3 #3)** | Partially Verified | The view renders `groups_from_result` output (round-trip unit-tested), offers a per-copy Keep choice, and dispatches the Verified `library.merge_duplicates`; the interaction (group sections, radio choice, refresh after merge) is field-Unverified |
| **Personal rating + note (Slice 4 #5)** | Verified | Library level: `Item.rating`/`note` with serde defaults — the v0.1.1-shaped fixture `tests/fixtures/library_v011.json` (no rating/note keys) loads with `None`/`""`; `item.add`/`item.edit` accept `rating` (1–10, null clears) and `note` (trimmed, empty clears), out-of-range ratings are `bad_params` with zero mutation (`tests/slice4.rs`); sort kind `rating` is highest-first with unrated last and stable (`model` unit + integration); exports render `Rating: n/10` / `Note: …` only when the pseudo-fields are selected and present (`export_text` unit + integration for .txt and .md); remembered export-field sets accept the pseudo keys. The detail-panel rating selector + note field dispatch through the same `item.edit` (undoable); the GUI interaction itself is field-Unverified |
| **Artist cover fallback (Slice 4 #6)** | Partially Verified | Parsers for both stages (Deezer `picture_xl` with exact-name preference, Wikipedia `originalimage`) are unit-tested on recorded key-free fixtures (`deezer_artist.json`, `wikipedia_artist_summary.json`) and replayed in `tests/slice4.rs`; the chain is exposed as `providers::artist_image::fetch_artist_image` and wired into the artist add path's background worker, Artists category only. Live calls are Unverified by design (no keys involved, but no field evidence yet); a miss leaves the item coverless as before |
| **Drinks DB quality pass (Slice 4 #7)** | Partially Verified | The generator (Python `tools/drinks-db/build_drinks_db.py` + Rust twin `src/drinks.rs`) runs a shared 30-vector flavour table in both suites (Python `--selftest` passes; Rust unit mirrors it). Pack regenerated from a fresh OFF export → v2, 5,078 entries (old: 5,091; −13 is export drift — 5,067 of the new entries match an old one by name+brand). Measured: flavour-empty 80.9% → 57.1% (1,240 fills on matched entries), malformed leading-comma flavours 560 → 0, kind changes on matched entries 277 soda→energy / 0 energy→soda (positive evidence only: "energy" name token or documented brand list), caffeine on 317 entries (OFF `caffeine_100g` ×1000 → mg/100 ml, plausibility-filtered; calibration: Red Bull rows land at 31.9–32.2) and surfaced as a `caffeine` field when a drink is added. Two consecutive generator runs are byte-identical. Old packs without the caffeine key still load (`tests/slice4.rs` + `drinks` unit). The on-device add-a-drink flow showing the caffeine field is field-Unverified |
| **Share card export (Slice 5 #8)** | Partially Verified | Layout geometry (single column ≤10 entries, two columns beyond, count clamped 1–25) and pixel-width title truncation are unit-tested WITHOUT a font via a measure abstraction (`share_card` unit 5). The full render is proven where a system font exists: `tests/slice5.rs` dispatches `export.share_card` and asserts an exact 1200×630 PNG, non-blank content, and `missing_indices` for the coverless entry (rendered as an initial tile); the in-module render test asserts the same canvas + placeholder behaviour. Text uses a pure-Rust rasteriser (one added dependency) loading fonts from OS font locations at runtime — no font file is bundled; with no font installed the command fails with a clean `io` error, and the render tests skip cleanly. The GUI dialog (count choice 5/10/25, save flow, status summary) dispatches the same command but is field-Unverified, as is Windows font behaviour |
| **CSV import (Slice 5 #10)** | Verified | Library level: the hand-rolled RFC-4180 reader (quoted commas, `""` escapes, CRLF/LF/CR, quoted newlines, BOM, unterminated-quote error), header-signature detection for the three supported export shapes with an unknown-header error listing what was found, and per-format row mapping are unit-tested (`csv_import` unit 10) — incl. 5-scale ratings ×2 rounded (0/blank → unrated), 1–10 ratings as-is, fields stored only where the destination category defines them. `tests/slice5.rs` (13 tests) drives `import.csv` on the three synthetic corpus fixtures: per-format counts, the split format creating one list per category in new mode, existing-list mode with wrong-category rows skipped and counted, re-import counting every row as a duplicate, empty-title rows counted `invalid`, wrong-category single-format imports rejected without mutation, the whole import as exactly ONE undo step restoring the pre-import library byte-for-byte, and registry ≡ JSON-channel equivalence. The GUI import dialog (file picker → detection readout → target choice → summary) dispatches the same command; its interaction is field-Unverified |
| Windows GUI: first launch + empty state (v0.1.1) | Partially Verified | SEAN's screenshot 2026-10-25: app running from the release folder, no console window (the v0.1.0 defect, fixed), all 11 categories in the sidebar, status bar shows the data path beside the exe |
| Windows GUI: deeper flows (add/edit/export/backup dialogs end-to-end) | Unverified | No field evidence yet — `docs/FIELD_TEST.md` is the checklist |
| Release ZIP (installer + portable + SHA256SUMS) | Verified | v0.1.1 release assets re-downloaded and hash-verified; shipped portable is a GUI-subsystem PE (see MEMORY/release notes) |

## Notes

- "Verified" for export/backup/library rows means the *library behaviour*
  is test-proven off-Windows; it never claims the Windows GUI flow was
  exercised — GUI rows carry their own status.
- All ten v0.2 program items now have rows above. The program is
  complete at library level; the v0.2.0 release is a separate step
  (SEAN's call — the stack stays local until then).
