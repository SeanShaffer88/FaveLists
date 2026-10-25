//! Integration tests for v0.2 Slice 5: the share-card export (#8) and
//! the CSV import (#10).
//!
//! The CSV reader, detection signatures and row mapping are unit-tested
//! inside `src/csv_import.rs`; the share-card geometry and truncation
//! are unit-tested inside `src/share_card.rs`. This file covers the two
//! commands end to end: import counts per format fixture, the split-
//! format list creation, merge-only semantics, the one-undo-step rule,
//! registry ≡ JSON-channel equivalence, and the share-card PNG itself
//! (render assertions skip cleanly on machines with no system font).

use favelists::commands::{self, CommandContext, History};
use favelists::model::{Category, Library};
use favelists::share_card;
use favelists::store::Store;
use serde_json::{json, Value};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "favelists-slice5-{}-{}-{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn dispatch(
    lib: &mut Library,
    covers: &Path,
    name: &str,
    params: Value,
) -> Result<Value, commands::CommandError> {
    let mut ctx = CommandContext::new(lib, covers);
    commands::dispatch(&mut ctx, name, &params)
}

fn dispatch_h(
    lib: &mut Library,
    history: &mut History,
    covers: &Path,
    name: &str,
    params: Value,
) -> Result<Value, commands::CommandError> {
    let mut ctx = CommandContext::with_history(lib, covers, history);
    commands::dispatch(&mut ctx, name, &params)
}

fn fixture_csv(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/csv")
        .join(name)
}

fn titles(lib: &Library, list_id: u64) -> Vec<&str> {
    lib.list(list_id)
        .unwrap()
        .items
        .iter()
        .map(|i| i.title.as_str())
        .collect()
}

// ---------------------------------------------------------------------------
// #10 — import.csv: diary format (single category, new list)
// ---------------------------------------------------------------------------

#[test]
fn import_diary_fixture_new_list_counts_ratings_and_duplicate() {
    let dir = temp_dir("lb-new");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    let path = fixture_csv("letterboxd_diary.csv");
    let r = dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": path.to_string_lossy(),
               "target": {"mode": "new", "name": "Diary 2026", "category": "movies"}}),
    )
    .unwrap();
    assert_eq!(r["format"], "letterboxd");
    assert_eq!(r["added"], 3);
    assert_eq!(r["skipped_duplicates"], 1, "the repeated diary row");
    assert_eq!(r["skipped_wrong_category"], 0);
    assert_eq!(r["invalid"], 1, "the empty-title row");
    assert_eq!(r["skipped"], 1);
    assert_eq!(r["lists"].as_array().unwrap().len(), 1);
    assert_eq!(r["lists"][0]["name"], "Diary 2026");
    assert_eq!(r["lists"][0]["category"], "movies");
    assert_eq!(r["lists"][0]["added"], 3);

    let list = &lib.lists[0];
    assert_eq!(list.category, Category::Movies);
    assert!(!list.ranked, "imported lists start unranked");
    assert_eq!(
        titles(&lib, list.id),
        [
            "The Clockwork Harbor",
            "Winter, With Teeth",
            "A Field of Paper Cranes"
        ]
    );
    // 5-scale ratings: 4.5 -> 9, 3 -> 6, blank -> None. The duplicate
    // row's 5-star rating must NOT overwrite the first row's.
    assert_eq!(list.items[0].rating, Some(9));
    assert_eq!(list.items[0].field("year"), Some("2019"));
    assert_eq!(list.items[1].rating, Some(6));
    assert_eq!(list.items[2].rating, None);
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #10 — import.csv: library format (books, author field, 0 rating)
// ---------------------------------------------------------------------------

#[test]
fn import_library_fixture_maps_author_and_zero_rating() {
    let dir = temp_dir("gr-new");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    let path = fixture_csv("goodreads_library.csv");
    let r = dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": path.to_string_lossy(),
               "target": {"mode": "new", "name": "Shelf", "category": "books"}}),
    )
    .unwrap();
    assert_eq!(r["format"], "goodreads");
    assert_eq!(r["added"], 3);
    assert_eq!(r["invalid"], 0);
    let list = &lib.lists[0];
    assert_eq!(list.category, Category::Books);
    // The quoted author "Iversen, Mara" is ONE field, comma and all.
    assert_eq!(list.items[0].field("author"), Some("Iversen, Mara"));
    assert_eq!(list.items[0].rating, Some(10), "5 stars -> 10");
    assert_eq!(list.items[0].field("year"), Some("2016"));
    assert_eq!(list.items[1].rating, None, "My Rating 0 means unrated");
    assert_eq!(list.items[2].rating, Some(8), "4 stars -> 8");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #10 — import.csv: ratings format splits by category in new mode
// ---------------------------------------------------------------------------

#[test]
fn import_ratings_fixture_new_mode_splits_into_two_lists() {
    let dir = temp_dir("imdb-split");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    let path = fixture_csv("imdb_ratings.csv");
    let r = dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": path.to_string_lossy(),
               "target": {"mode": "new", "name": "My Ratings"}}),
    )
    .unwrap();
    assert_eq!(r["format"], "imdb");
    assert_eq!(r["added"], 4);
    assert_eq!(r["lists"].as_array().unwrap().len(), 2);
    assert_eq!(r["lists"][0]["name"], "My Ratings — Movies");
    assert_eq!(r["lists"][0]["category"], "movies");
    assert_eq!(r["lists"][0]["added"], 2);
    assert_eq!(r["lists"][1]["name"], "My Ratings — TV Shows");
    assert_eq!(r["lists"][1]["category"], "tv_shows");
    assert_eq!(r["lists"][1]["added"], 2);

    let movies = lib
        .lists
        .iter()
        .find(|l| l.category == Category::Movies)
        .unwrap();
    let film = &movies.items[0];
    assert_eq!(film.title, "The Clockwork Harbor");
    assert_eq!(film.provider_id.as_deref(), Some("tt9000001"));
    assert_eq!(film.rating, Some(9), "1-10 ratings pass through as-is");
    assert_eq!(film.field("genre"), Some("Adventure, Drama"));
    assert_eq!(film.field("director"), Some("R. Calloway"));
    assert_eq!(film.field("year"), Some("2019"));

    let shows = lib
        .lists
        .iter()
        .find(|l| l.category == Category::TvShows)
        .unwrap();
    assert_eq!(shows.items.len(), 2);
    assert_eq!(shows.items[0].title, "Harbor Lights");
    assert_eq!(shows.items[0].rating, None, "blank Your Rating -> unrated");
    assert_eq!(
        shows.items[0].field("years"),
        Some("2022"),
        "TV years land under the years field"
    );
    // The mini-series row mapped to TV too, genres kept, 8 as-is.
    assert_eq!(shows.items[1].rating, Some(8));
    assert_eq!(shows.items[1].field("genre"), Some("Drama, Mystery"));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #10 — import.csv: existing-list mode
// ---------------------------------------------------------------------------

#[test]
fn import_ratings_into_existing_movies_list_skips_tv_rows() {
    let dir = temp_dir("imdb-existing");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Films", "category": "movies"}),
    )
    .unwrap();
    let path = fixture_csv("imdb_ratings.csv");
    let r = dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": path.to_string_lossy(),
               "target": {"mode": "existing", "list_id": 1}}),
    )
    .unwrap();
    assert_eq!(r["added"], 2);
    assert_eq!(r["skipped_wrong_category"], 2, "the two TV rows");
    assert_eq!(r["skipped"], 2);
    assert_eq!(r["lists"].as_array().unwrap().len(), 1);
    assert_eq!(r["lists"][0]["list_id"], 1);
    assert_eq!(r["lists"][0]["added"], 2);
    assert_eq!(
        titles(&lib, 1),
        ["The Clockwork Harbor", "The Last Cartographer"]
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn import_twice_into_same_list_counts_every_row_as_duplicate() {
    let dir = temp_dir("lb-twice");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    let path = fixture_csv("letterboxd_diary.csv");
    let target = json!({"mode": "new", "name": "Diary", "category": "movies"});
    dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": path.to_string_lossy(), "target": target}),
    )
    .unwrap();
    let list_id = lib.lists[0].id;
    let r = dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": path.to_string_lossy(),
               "target": {"mode": "existing", "list_id": list_id}}),
    )
    .unwrap();
    assert_eq!(r["added"], 0);
    assert_eq!(r["skipped_duplicates"], 4, "all four titled rows");
    assert_eq!(r["invalid"], 1);
    assert_eq!(lib.lists[0].items.len(), 3, "nothing was added or replaced");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn import_single_category_file_into_wrong_category_list_is_rejected() {
    let dir = temp_dir("gr-wrong-cat");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Films", "category": "movies"}),
    )
    .unwrap();
    let before = lib.clone();
    let path = fixture_csv("goodreads_library.csv");
    let err = dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": path.to_string_lossy(),
               "target": {"mode": "existing", "list_id": 1}}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    assert_eq!(lib, before, "a rejected import must not mutate");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn import_unknown_headers_error_lists_what_was_found() {
    let dir = temp_dir("unknown");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let csv_path = dir.join("mystery.csv");
    fs::write(&csv_path, "Foo,Bar,Baz\n1,2,3\n").unwrap();
    let mut lib = Library::default();
    let err = dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": csv_path.to_string_lossy(),
               "target": {"mode": "new", "name": "X"}}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    assert!(
        err.message.contains("Foo, Bar, Baz"),
        "error should list the found headers, got: {}",
        err.message
    );
    assert!(lib.lists.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn import_missing_file_is_an_io_error() {
    let dir = temp_dir("missing-file");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    let err = dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": dir.join("nope.csv").to_string_lossy(),
               "target": {"mode": "new", "name": "X"}}),
    )
    .unwrap_err();
    assert_eq!(err.code, "io");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #10 — one dispatch = one undo step
// ---------------------------------------------------------------------------

#[test]
fn import_is_exactly_one_undo_step() {
    let dir = temp_dir("undo");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    let mut history = History::new();
    let before = lib.clone();
    let path = fixture_csv("imdb_ratings.csv");
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "import.csv",
        json!({"path": path.to_string_lossy(),
               "target": {"mode": "new", "name": "My Ratings"}}),
    )
    .unwrap();
    assert_eq!(lib.lists.len(), 2, "the split created two lists");
    assert_eq!(history.undo_len(), 1, "the whole import is ONE step");
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
    assert_eq!(r["undone"], true);
    assert_eq!(r["command"], "import.csv");
    assert_eq!(lib, before, "undo restores the exact pre-import state");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #10 — registry ≡ JSON channel equivalence
// ---------------------------------------------------------------------------

fn run_channel(base: &Path, input: &str) -> Vec<Value> {
    let mut out: Vec<u8> = Vec::new();
    favelists::json_channel::run(base, Cursor::new(input.as_bytes().to_vec()), &mut out).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn import_csv_channel_matches_registry() {
    let dir = temp_dir("equiv");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    // The channel reads the CSV by path too, so both surfaces point at
    // the same fixture file.
    let path = fixture_csv("letterboxd_diary.csv");
    let line = format!(
        "{{\"command\":\"import.csv\",\"params\":{{\"path\":{},\"target\":{{\"mode\":\"new\",\"name\":\"Diary 2026\",\"category\":\"movies\"}}}}}}\n",
        serde_json::to_string(&path.to_string_lossy().into_owned()).unwrap()
    );
    let base = dir.join("base");
    fs::create_dir_all(&base).unwrap();
    let responses = run_channel(&base, &line);
    assert_eq!(responses.len(), 1);
    assert_eq!(
        responses[0]["ok"], true,
        "channel import failed: {responses:?}"
    );
    let channel_result = responses[0]["result"].clone();

    let mut lib = Library::default();
    let registry_result = dispatch(
        &mut lib,
        &covers,
        "import.csv",
        json!({"path": path.to_string_lossy(),
               "target": {"mode": "new", "name": "Diary 2026", "category": "movies"}}),
    )
    .unwrap();
    assert_eq!(channel_result, registry_result);
    // The channel persisted the imported library to the normal data path.
    let store = Store::open(&base).unwrap();
    let persisted = store.load_library().unwrap();
    assert_eq!(persisted.lists.len(), 1);
    assert_eq!(persisted.lists[0].items.len(), 3);
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #8 — export.share_card
// ---------------------------------------------------------------------------

/// Build a two-item Movies list; item 1 has a real cover file in the
/// covers dir, item 2 has none.
fn share_card_library(covers: &Path) -> Library {
    let img = image::RgbaImage::from_pixel(40, 60, image::Rgba([200, 60, 60, 255]));
    img.save(covers.join("harbor.png")).unwrap();
    let mut lib = Library::default();
    let covers_path = covers.to_path_buf();
    dispatch(
        &mut lib,
        &covers_path,
        "list.create",
        json!({"name": "Top Films", "category": "movies", "ranked": true}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers_path,
        "item.add",
        json!({"list_id": 1, "title": "The Clockwork Harbor",
               "fields": {"year": "2019"}, "cover": "harbor.png",
               "added_at": 1_700_000_000}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers_path,
        "item.add",
        json!({"list_id": 1, "title": "A Field of Paper Cranes",
               "fields": {"year": "2008"}, "added_at": 1_700_000_100}),
    )
    .unwrap();
    lib
}

#[test]
fn share_card_writes_exact_canvas_png_and_reports_missing_cover() {
    if !share_card::system_font_available() {
        eprintln!("skipping: no system font on this machine");
        return;
    }
    let dir = temp_dir("card");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = share_card_library(&covers);
    let out = dir.join("card.png");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.share_card",
        json!({"list_id": 1, "path": out.to_string_lossy()}),
    )
    .unwrap();
    assert_eq!(r["entries"], 2);
    assert_eq!(r["missing_indices"], json!([1]), "item 2 has no cover");
    assert_eq!(r["width"], share_card::CARD_W);
    assert_eq!(r["height"], share_card::CARD_H);
    let img = image::open(&out).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (share_card::CARD_W, share_card::CARD_H));
    assert!(
        img.pixels().any(|p| *p != favelists::render::BG),
        "the card must not be a blank background"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn share_card_count_clamps_to_card_range() {
    if !share_card::system_font_available() {
        eprintln!("skipping: no system font on this machine");
        return;
    }
    let dir = temp_dir("card-clamp");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = share_card_library(&covers);
    // count above the list length renders the whole list.
    let out = dir.join("all.png");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.share_card",
        json!({"list_id": 1, "path": out.to_string_lossy(), "count": 100}),
    )
    .unwrap();
    assert_eq!(r["entries"], 2);
    // count 0 clamps up to a single entry.
    let out1 = dir.join("one.png");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.share_card",
        json!({"list_id": 1, "path": out1.to_string_lossy(), "count": 0}),
    )
    .unwrap();
    assert_eq!(r["entries"], 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn share_card_empty_list_is_a_clean_error() {
    let dir = temp_dir("card-empty");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Empty", "category": "movies"}),
    )
    .unwrap();
    let err = dispatch(
        &mut lib,
        &covers,
        "export.share_card",
        json!({"list_id": 1, "path": dir.join("x.png").to_string_lossy()}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    let _ = fs::remove_dir_all(&dir);
}
