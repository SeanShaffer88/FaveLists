# FaveLists v0.3 Program — 20 Enhancements

Adopted by SEAN 2026-10-25 ("adopt all 20"). Ships as v0.3.0 under the
standing (clarified) release rule: installer .exe loose + ONE portable
.zip (portable .exe + LICENSE + PORTABLE note) + drinks.json (the app's
drinks-update pack), all built by CI/the release workflow.

The stack builds on top of the held v0.2.1 commit (6724103 — field-test
fixes, gated green, unpublished). Shipping order (v0.2.1 first, or folded
into v0.3.0) is SEAN's call when the program is done.

Sequencing follows the Storytold orchestration rules: the foundation
slice lands ALONE first (data model + command interfaces frozen), then
feature slices build on it. Every new capability is a registry command
first (GUI + JSON channel parity), per the v0.2 foundation.

## Slice 1 — Foundation: richer items (lands alone)

- [x] **#4 Tags on items** — free-form tags per item; filter any list
  by tag; tags searchable in global search.
- [x] **#5 Status per item** — Want to try / Tried / Own it (wording
  adapts per category); status filter in list view and search.

## Slice 2 — Discovery

- [x] **#2 Smart views** — Top Rated (across all lists), Recently
  Added, Needs a Rating; sidebar views, not lists (single-type list
  rule untouched).
- [x] **#3 Share card for smart views** — the Top Rated view renders
  through the existing share-card command.
- [x] **#1 "Pick for me"** — random favorite from a list, category,
  or smart view, with a reveal in the GUI; a command underneath, so
  the JSON channel can pick too.

## Slice 3 — Data in / out

- [x] **#6 CSV + JSON export** — per-list CSV and full-library
  CSV/JSON export (round-trip out for the v0.2 CSV import).
- [x] **#7 Paste-a-list import** — one title per line; provider
  matching with a review screen before anything is added;
  merge-only, one undo step.
- [x] **#8 Fuzzy duplicate pass** — the finder also groups
  near-identical titles (subtitles, punctuation, "Part II" vs
  "Part 2") as possible duplicates.

## Slice 4 — Drinks depth + covers

- [x] **#11 Barcode add** — enter a UPC; Open Food Facts lookup
  fills the drink (same source as the bundled pack).
- [x] **#12 "Tried it" for drinks** — tried checkmark per drink
  item plus a progress line. Uses the #5 status model where it
  fits. *(Slice 4 wording adjustment: the progress line counts
  per list — "Tried X of Y" in each drinks list's header — not
  pack-wide. Drink entries carry no pack key, so there is nothing
  to count the original "128 of 5,078" pack figure against;
  recorded here rather than faked.)*
- [x] **#13 Caffeine sort + filter** — sort drinks by caffeine and
  a high-caffeine filter, from the pack v2 caffeine data.
- [x] **#9 Missing-cover sweep** — one screen listing every
  coverless item; one click fetches covers via the existing
  providers and fallbacks.

## Slice 5 — Look & feel

- [x] **#14 Text size setting** — Comfortable / Large / Extra large
  in Settings, remembered; generalises the fixed 1.3× zoom from
  v0.2.1 into a choice.
- [x] **#15 Grid density + list view** — cover-size slider and a
  compact rows view for long lists.
- [x] **#16 High-contrast option** — brighter text/accents on the
  black/red family look.
- [x] **#17 Drop a cover on it** — drag an image file onto a cover
  tile, or paste from the clipboard, to set that item's cover.
  *(Slice 5 wording adjustment: a pasted* image *cannot reach the
  app — egui delivers pasted text only, and no clipboard crate is
  in the tree, so true image paste would need a dependency
  decision. What shipped: drag-and-drop onto a tile/row (or onto
  the window for the selected item), plus pasting an image
  file's* path *while an item is selected sets its cover.
  Recorded here rather than faked.)*

## Slice 6 — Safety + setup

- [x] **#10 Trash with restore** — deleted items kept in a trash
  view for 30 days, restorable; emptying is explicit.
- [x] **#19 Automatic backups** — the app keeps the last few backup
  ZIPs on its own (weekly), restorable from Settings.
- [x] **#18 App update check** — on launch, quietly check GitHub
  Releases; show "vX available" in the status bar (same pattern
  as the drinks update). No auto-download.
- [x] **#20 First-run setup wizard** — welcome → API keys (with the
  Get API links) → optional data-folder pick → done. Reopenable
  from Settings.

## Program status

All 20 enhancements are implemented (Slices 1–6, on top of the held
v0.2.1 commit), each behind the full gate. Shipping decision (SEAN,
2026-10-25): **v0.2.1 folds into v0.3.0** — one release containing the
whole stack; v0.2.1 was never released separately.

## Gate (every slice)

fmt clean · clippy clean (host + windows-msvc) · FULL `cargo test
--locked` (all targets) run personally by the integrator on the slice
tree and again on the merged tree · PARITY.md updated from test
evidence only.
