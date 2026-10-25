# FaveLists test corpus

The corpus is the growing set of recorded, key-free scenarios that the
test suite replays as evidence. PARITY.md scores capabilities from tests;
corpus entries are what make those tests representative rather than
synthetic one-offs.

## Layout

- `tests/fixtures/commands/` — JSON-channel / command-registry scenarios:
  - `session_requests.jsonl` — a scripted JSON-lines session (one request
    per line) replayed through the real channel by
    `tests/commands.rs::fixture_session_reproduces_expected_library`.
  - `session_expected_library.json` — the exact library state that session
    must produce (`created_at` normalised to 0; everything else exact,
    including ids and `added_at`, which the requests pin explicitly).
  - `bad_requests.jsonl` — malformed/unknown/missing-entity requests whose
    error codes are asserted in order by
    `tests/commands.rs::fixture_bad_requests_produce_expected_error_codes`.
- `tests/fixtures/search/` — global-search scenarios:
  - `library_and_cases.json` — a fixed three-list library plus
    `library.search` cases (title-only, category, year, genre, combined,
    empty-query, no-match) with the exact expected match titles in order,
    replayed by `tests/slice2.rs::search_corpus_fixture_cases`.
- `tests/fixtures/duplicates/` — duplicate-finder scenarios:
  - `library.json` — a fixed three-list library plus the exact expected
    `library.find_duplicates` groups (article/punctuation normalisation,
    exact vs possible by year agreement, a within-list pair, and a
    same-title different-category non-match), replayed by
    `tests/slice3.rs::duplicates_corpus_fixture`.
- `tests/fixtures/csv/` — CSV import scenarios (Slice 5, #10): three
  small synthetic export files (invented titles only — no real user
  data) replayed by `tests/slice5.rs`, with their expected mappings:
  - `letterboxd_diary.csv` — diary shape (Date, Name, Year, URI,
    Rating): 5 rows → 3 movies added, 1 duplicate diary row (the same
    film logged again with a higher rating — merge-only means it must
    NOT overwrite), 1 empty-title row counted invalid. 5-scale ratings
    convert ×2 rounded: 4.5 → 9, 3 → 6, blank → unrated.
  - `goodreads_library.csv` — library shape (Title, Author, My Rating,
    Year Published, Bookshelves): 3 books; the quoted author
    `"Iversen, Mara"` stays ONE field; My Rating 0 → unrated, 5 → 10,
    4 → 8; author lands in the `author` field.
  - `imdb_ratings.csv` — ratings shape (Const, Title, Year, Your
    Rating, Title Type, Genres, Directors): 4 rows split by Title Type
    into Movies (2) and TV Shows (2, incl. a mini-series); 1–10
    ratings pass through as-is, blank → unrated; TV years land under
    the `years` field; the Const id becomes the item's provider id;
    genres/directors land in `genre`/`director` where the category
    defines them.
- `tests/fixtures/` (top level) — recorded provider responses (key-free),
  consumed by the parser tests in `src/providers.rs`. Slice 4 added:
  - `deezer_artist.json` + `wikipedia_artist_summary.json` — recorded
    key-free responses for the #6 artist cover fallback, consumed by the
    parser tests in `src/providers/artist_image.rs` and replayed at
    integration level by `tests/slice4.rs`.
  - `library_v011.json` — a v0.1.1-shaped library (items WITHOUT the
    v0.2 rating/note keys) pinning #5 back-compat: loads with rating
    `None` / note `""` (`tests/slice4.rs`).
  - `drinks_pack_v1.json` — a v1 drinks pack (entries WITHOUT the
    caffeine key) pinning #7 old-pack tolerance (`tests/slice4.rs`).
  - The #7 flavour-extraction vector table (30 name/brand → flavour
    cases) lives in code, mirrored exactly by
    `src/drinks.rs::flavor_extraction_vectors_match_python_twin` and
    `tools/drinks-db/build_drinks_db.py --selftest`.

## Adding a corpus entry

1. Add a scenario file (requests `.jsonl` + expected state `.json`) under
   `tests/fixtures/commands/`, or a recorded provider response under
   `tests/fixtures/`. No API keys, ever — recorded responses are scrubbed
   fixtures, per AGENTS.md.
2. Wire it into a test that replays it end-to-end and asserts on the
   resulting state, not just on "no error".
3. Update `docs/PARITY.md` only when the new entry changes what a test
   actually proves.

An entry that no test consumes is documentation, not evidence, and must
not be cited in PARITY.md.
