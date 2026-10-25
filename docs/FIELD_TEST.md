# FaveLists — on-machine field test (SEAN's Windows PC)

Everything below is **Unverified** until run on a real Windows machine.
Local tests/CI on Linux do not verify Windows GUI runtime. Work top to
bottom; note failures with the step number.

## Setup
1. Unzip the release ZIP. Run the per-user installer (no admin prompt
   expected) AND, separately, the portable exe from a plain folder.
2. Confirm `FaveListsData\` appears next to the exe after first run, with
   `library.json`, `settings.json`, and a `covers\` folder inside.

## Core flows
3. Create a list of each of the 11 categories. Restart the app; all lists
   persist.
4. Movies: paste your TMDB API key in Settings, save, search "alien" —
   dropdown shows posters + years; pick Alien (1979). Check the details:
   year, genre, director (Ridley Scott), runtime, main cast.
5. Without the TMDB key (clear it, restart): the search box shows the
   "add your key / use Add manually" hint and manual entry still works.
6. TV Shows: add Firefly; years show a range, seasons = 1, main cast present.
7. Books: search "left hand of darkness" — author, year, pages, cover.
8. Music: add an Artist (genre + country), an Album (cover art appears),
   and a Song (album name filled). Artist entries may have no image —
   upload one by hand from Details → Replace cover.
9. Games: paste Twitch Client ID + Secret in Settings; search "witcher" —
   covers, developer, platforms present.
10. Podcasts: search a podcast; host + category filled; artwork appears.
11. Board Games: search "gloomhaven" — year + publisher (Cephalofair) and
    cover appear.
12. Energy Drinks + Sodas (no network needed): search "coca" in a Sodas
    list and "monster" in an Energy Drinks list; entries come from the
    bundled offline database; covers download once and then show offline
    (disconnect Wi-Fi and restart to prove it).
13. Add the same work twice to one list — the second add is refused with a
    message. Add it to a different list — allowed.
14. Switch a list Ranked ↔ Unranked both ways; order does not change.
    Drag covers to reorder; restart; order persists.
15. Sort buttons (A–Z, year, added) work. Filter box narrows the grid.
16. Edit an item's fields, save, restart — edits persist. Replace a cover
    with a local image file.

## Exports
17. Text export: header shows list name, category, count, date; ranked list
    is numbered. Untick some details, export again — the unticked details
    are gone, and the dialog remembers your ticks next time it opens.
18. Markdown export: the .md renders with cover images from the
    `<name>_assets` folder next to it.
19. Image export: pick a subset of items, Uniform grid, PNG — the image has
    NO text anywhere. Repeat with JPG, Scattered collage, and
    crop-to-fill vs whole-cover; compare.
20. In the preview, drag one cover onto another — they swap in the export.
21. Remove a cover (delete the file from `covers\` after removing the
    item's cover via a fresh manual entry without one) — the export dialog
    warns by name before exporting.
22. Backup: export Entire library to ZIP. Delete a list. Import the ZIP:
    the deleted list returns; nothing else was duplicated or lost.
    Import the same ZIP again: report shows duplicates skipped.

## Settings & data
23. Move the data folder (Settings → Move data folder…) to a new empty
    folder: lists + covers are all there, the app keeps working, and the
    old folder no longer holds the current data.
24. Drinks database: Settings shows the bundled pack version and counts.
    With no update URL set, "Update drinks database now" explains itself
    (no crash).
25. Resize the window down to its minimum; sidebar, grid, and dialogs stay
    usable. Tab through the main window: controls are reachable by keyboard.

## Sign-off
- [ ] All steps passed, or failures noted with step numbers.
- [ ] Screenshots captured per `docs/screenshots/CAPTURE.md`.
