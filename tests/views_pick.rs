//! Integration tests for v0.3 Slice 2: smart views (#2), the share
//! card for smart views (#3), and "Pick for me" (#1).
//!
//! The view orderings and the seeded picker are pure (`src/views.rs`);
//! this file pins them end to end through the registry: view ordering
//! with ties and cross-list mixing, `library.view` shape + limit
//! clamping + errors, seeded pick determinism per scope, empty-scope
//! nulls, multi-scope rejection, the no-history guarantee for the two
//! new commands, and the `export.share_card` view path (render
//! assertions follow the slice5 pattern: real PNG where a system font
//! exists, a clean `io` error where none does).

use favelists::commands::{self, CommandContext, History};
use favelists::model::Library;
use favelists::share_card;
use favelists::views::{self, SmartView};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "favelists-views-{}-{}-{}",
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

/// The cross-list fixture library, built entirely through the
/// registry (ids are allocation order):
///
/// * list 1 "Films" (movies): Zulu ★9 @300, Alpha ★9 @100, Beta ★7
///   @200, Gamma unrated @400 — rating ties (Alpha/Zulu), titles chosen
///   so insertion order ≠ sorted order.
/// * list 2 "Books" (books): Dune ★10 @50, Emma unrated @500, Beta
///   unrated @150 — a title shared with list 1's Beta.
/// * list 3 "Aardvark" (albums): Gamma unrated @600 — a list name that
///   sorts first, for the Needs-a-Rating list-name ordering.
fn fixture_library(covers: &Path) -> Library {
    let mut lib = Library::default();
    let add = |lib: &mut Library, list_id: u64, title: &str, rating: Option<u8>, added_at: i64| {
        let mut params = json!({
            "list_id": list_id, "title": title, "added_at": added_at,
        });
        if let Some(r) = rating {
            params["rating"] = json!(r);
        }
        dispatch(lib, covers, "item.add", params).unwrap();
    };
    dispatch(
        &mut lib,
        covers,
        "list.create",
        json!({"name": "Films", "category": "movies", "ranked": true}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        covers,
        "list.create",
        json!({"name": "Books", "category": "books"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        covers,
        "list.create",
        json!({"name": "Aardvark", "category": "albums"}),
    )
    .unwrap();
    add(&mut lib, 1, "Zulu", Some(9), 300); // item 1
    add(&mut lib, 1, "Alpha", Some(9), 100); // item 2
    add(&mut lib, 1, "Beta", Some(7), 200); // item 3
    add(&mut lib, 1, "Gamma", None, 400); // item 4
    add(&mut lib, 2, "Dune", Some(10), 50); // item 5
    add(&mut lib, 2, "Emma", None, 500); // item 6
    add(&mut lib, 2, "Beta", None, 150); // item 7
    add(&mut lib, 3, "Gamma", None, 600); // item 8
    lib
}

fn entry_ids(entries: &[views::ViewEntry]) -> Vec<u64> {
    entries.iter().map(|e| e.item.id).collect()
}

// ---------------------------------------------------------------------------
// #2 — view orderings (pure layer)
// ---------------------------------------------------------------------------

#[test]
fn view_items_orderings_ties_and_cross_list_mixing() {
    let dir = temp_dir("orders");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let lib = fixture_library(&covers);

    // Top Rated: rated only; 10 first, then the ★9 tie broken by
    // title (Alpha before Zulu despite insertion order), then ★7.
    let top = views::view_items(&lib, SmartView::TopRated);
    assert_eq!(entry_ids(&top), vec![5, 2, 1, 3]);
    assert!(top.iter().all(|e| e.item.rating.is_some()));
    // Cross-list mixing: the ★10 lives in Books, the rest in Films.
    assert_eq!(top[0].list_name, "Books");
    assert_eq!(top[1].list_name, "Films");

    // Recently Added: added_at descending across all lists.
    let recent = views::view_items(&lib, SmartView::RecentlyAdded);
    assert_eq!(entry_ids(&recent), vec![8, 6, 4, 1, 3, 7, 2, 5]);

    // Needs a Rating: unrated only; list name ascending (Aardvark,
    // Books, Films), then title ascending inside a list.
    let needs = views::view_items(&lib, SmartView::NeedsRating);
    assert_eq!(entry_ids(&needs), vec![8, 7, 6, 4]);
    assert!(needs.iter().all(|e| e.item.rating.is_none()));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #2 — library.view command
// ---------------------------------------------------------------------------

#[test]
fn library_view_shape_limit_clamping_and_errors() {
    let dir = temp_dir("viewcmd");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = fixture_library(&covers);

    let r = dispatch(
        &mut lib,
        &covers,
        "library.view",
        json!({"view": "top_rated"}),
    )
    .unwrap();
    assert_eq!(r["view"], "top_rated");
    assert_eq!(r["name"], "Top Rated");
    let entries = r["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 4);
    // Entry shape mirrors library.search matches.
    let first = &entries[0];
    assert_eq!(first["list_id"], 2);
    assert_eq!(first["list_name"], "Books");
    assert_eq!(first["category"], "books");
    assert_eq!(first["item_id"], 5);
    assert_eq!(first["title"], "Dune");
    assert_eq!(first["item"]["rating"], 10);

    // Limit: present values clamp like share-card counts (1–25).
    let r = dispatch(
        &mut lib,
        &covers,
        "library.view",
        json!({"view": "recently_added", "limit": 3}),
    )
    .unwrap();
    assert_eq!(r["entries"].as_array().unwrap().len(), 3);
    let r = dispatch(
        &mut lib,
        &covers,
        "library.view",
        json!({"view": "recently_added", "limit": 0}),
    )
    .unwrap();
    assert_eq!(
        r["entries"].as_array().unwrap().len(),
        1,
        "limit 0 clamps up to 1"
    );
    let r = dispatch(
        &mut lib,
        &covers,
        "library.view",
        json!({"view": "recently_added", "limit": 100}),
    )
    .unwrap();
    assert_eq!(
        r["entries"].as_array().unwrap().len(),
        8,
        "a big limit returns the whole view"
    );

    // Errors: unknown view, missing view, mistyped limit.
    let err = dispatch(&mut lib, &covers, "library.view", json!({"view": "bogus"})).unwrap_err();
    assert_eq!(err.code, "bad_params");
    let err = dispatch(&mut lib, &covers, "library.view", json!({})).unwrap_err();
    assert_eq!(err.code, "bad_params");
    let err = dispatch(
        &mut lib,
        &covers,
        "library.view",
        json!({"view": "top_rated", "limit": "three"}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #1 — library.pick
// ---------------------------------------------------------------------------

#[test]
fn pick_is_deterministic_for_a_fixed_seed_in_every_scope() {
    let dir = temp_dir("pickseed");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = fixture_library(&covers);

    // For each scope: two seeded dispatches agree, and the winner is
    // the entry pick_index chooses over the scope's own entry vector.
    let scopes: Vec<(Value, Vec<views::ViewEntry>)> = vec![
        (
            json!({"list_id": 1}),
            views::list_entries(lib.list(1).unwrap()),
        ),
        (
            json!({"category": "books"}),
            views::category_entries(&lib, favelists::model::Category::Books),
        ),
        (
            json!({"view": "top_rated"}),
            views::view_items(&lib, SmartView::TopRated),
        ),
        (json!({}), views::all_entries(&lib)),
    ];
    for (scope, entries) in scopes {
        let mut params = scope.clone();
        params["seed"] = json!(42);
        let r1 = dispatch(&mut lib, &covers, "library.pick", params.clone()).unwrap();
        let r2 = dispatch(&mut lib, &covers, "library.pick", params).unwrap();
        assert_eq!(r1, r2, "same seed, same pick ({scope})");
        let expected = &entries[views::pick_index(42, entries.len()).unwrap()];
        assert_eq!(r1["entry"]["item_id"], expected.item.id, "scope {scope}");
        assert_eq!(r1["entry"]["list_id"], expected.list_id);
        assert_eq!(r1["entry"]["title"], expected.item.title);
        // A spread of seeds stays inside the scope.
        for seed in 0..25u64 {
            let mut p = scope.clone();
            p["seed"] = json!(seed);
            let r = dispatch(&mut lib, &covers, "library.pick", p).unwrap();
            let id = r["entry"]["item_id"].as_u64().unwrap();
            assert!(
                entries.iter().any(|e| e.item.id == id),
                "seed {seed} picked outside scope {scope}"
            );
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pick_empty_scopes_return_null_entry() {
    let dir = temp_dir("pickempty");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    // Whole library, empty.
    let mut lib = Library::default();
    let r = dispatch(&mut lib, &covers, "library.pick", json!({"seed": 1})).unwrap();
    assert_eq!(r["entry"], Value::Null);
    // An existing but empty list.
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Empty", "category": "movies"}),
    )
    .unwrap();
    let r = dispatch(
        &mut lib,
        &covers,
        "library.pick",
        json!({"list_id": 1, "seed": 1}),
    )
    .unwrap();
    assert_eq!(r["entry"], Value::Null);
    // A category with no lists at all.
    let r = dispatch(
        &mut lib,
        &covers,
        "library.pick",
        json!({"category": "sodas", "seed": 1}),
    )
    .unwrap();
    assert_eq!(r["entry"], Value::Null);
    // A view with nothing in it (nothing rated yet).
    let r = dispatch(
        &mut lib,
        &covers,
        "library.pick",
        json!({"view": "top_rated", "seed": 1}),
    )
    .unwrap();
    assert_eq!(r["entry"], Value::Null);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pick_rejects_multiple_scopes_and_bad_references() {
    let dir = temp_dir("pickbad");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = fixture_library(&covers);
    let before = lib.clone();
    for params in [
        json!({"list_id": 1, "view": "top_rated"}),
        json!({"list_id": 1, "category": "movies"}),
        json!({"category": "movies", "view": "top_rated"}),
        json!({"list_id": 1, "category": "movies", "view": "top_rated"}),
        json!({"view": "bogus"}),
        json!({"category": "bogus"}),
        json!({"seed": "abc"}),
    ] {
        let err = dispatch(&mut lib, &covers, "library.pick", params.clone()).unwrap_err();
        assert_eq!(err.code, "bad_params", "params {params}");
    }
    let err = dispatch(&mut lib, &covers, "library.pick", json!({"list_id": 999})).unwrap_err();
    assert_eq!(err.code, "not_found");
    assert_eq!(lib, before, "rejected picks mutate nothing");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn view_and_pick_record_no_history() {
    let dir = temp_dir("pickhist");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = fixture_library(&covers);
    let mut history = History::new();
    let run = |lib: &mut Library, history: &mut History, name: &str, params: Value| {
        let mut ctx = CommandContext::with_history(lib, &covers, history);
        commands::dispatch(&mut ctx, name, &params)
    };
    run(
        &mut lib,
        &mut history,
        "library.view",
        json!({"view": "top_rated"}),
    )
    .unwrap();
    run(&mut lib, &mut history, "library.pick", json!({"seed": 3})).unwrap();
    assert_eq!(
        history.undo_len(),
        0,
        "non-mutating commands record nothing"
    );
    assert!(!history.can_undo());
    // A real mutation records exactly one step; a pick after it adds none.
    run(
        &mut lib,
        &mut history,
        "item.edit",
        json!({"list_id": 1, "item_id": 1, "rating": 5}),
    )
    .unwrap();
    assert_eq!(history.undo_len(), 1);
    run(&mut lib, &mut history, "library.pick", json!({"seed": 3})).unwrap();
    assert_eq!(history.undo_len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pick_result_parses_back_into_a_view_entry() {
    let dir = temp_dir("pickparse");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = fixture_library(&covers);
    let r = dispatch(
        &mut lib,
        &covers,
        "library.pick",
        json!({"list_id": 2, "seed": 9}),
    )
    .unwrap();
    let entry = views::entry_from_json(&r["entry"]).expect("pick entry parses");
    assert_eq!(entry.list_id, 2);
    assert_eq!(entry.list_name, "Books");
    assert_eq!(entry.category, favelists::model::Category::Books);
    assert_eq!(entry.item.id, r["entry"]["item_id"].as_u64().unwrap());
    assert_eq!(entry.item.title, r["entry"]["title"].as_str().unwrap());
    // And the null entry parses to None (the GUI's empty-scope path).
    assert_eq!(views::entry_from_json(&Value::Null), None);
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #3 — export.share_card with a view
// ---------------------------------------------------------------------------

#[test]
fn share_card_view_path_renders_view_order_under_view_name() {
    let dir = temp_dir("cardview");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    // Zulu (item 1) gets a real cover file; the rest of Top Rated has none.
    let img = image::RgbaImage::from_pixel(40, 60, image::Rgba([200, 60, 60, 255]));
    img.save(covers.join("zulu.png")).unwrap();
    let mut lib = fixture_library(&covers);
    dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": 1, "cover": "zulu.png"}),
    )
    .unwrap();

    let out = dir.join("view-card.png");
    let params = json!({"view": "top_rated", "path": out.to_string_lossy()});
    if !share_card::system_font_available() {
        // No font: the same clean io error as the list path.
        let err = dispatch(&mut lib, &covers, "export.share_card", params).unwrap_err();
        assert_eq!(err.code, "io");
        let _ = fs::remove_dir_all(&dir);
        return;
    }
    let r = dispatch(&mut lib, &covers, "export.share_card", params).unwrap();
    assert_eq!(r["title"], "Top Rated", "the header is the view's name");
    assert_eq!(r["entries"], 4);
    assert_eq!(r["width"], share_card::CARD_W);
    assert_eq!(r["height"], share_card::CARD_H);
    // View order is Dune, Alpha, Zulu, Beta — only Zulu (index 2) has
    // a cover, so the other three are reported missing.
    assert_eq!(r["missing_indices"], json!([0, 1, 3]));
    let img = image::open(&out).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (share_card::CARD_W, share_card::CARD_H));

    // count applies over view order.
    let out2 = dir.join("view-card-2.png");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.share_card",
        json!({"view": "top_rated", "path": out2.to_string_lossy(), "count": 2}),
    )
    .unwrap();
    assert_eq!(r["entries"], 2);
    assert_eq!(r["missing_indices"], json!([0, 1]));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn share_card_rejects_list_and_view_together_neither_and_empty_view() {
    let dir = temp_dir("cardbad");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = fixture_library(&covers);
    let path = dir.join("x.png").to_string_lossy().into_owned();
    // Both sources.
    let err = dispatch(
        &mut lib,
        &covers,
        "export.share_card",
        json!({"list_id": 1, "view": "top_rated", "path": path.clone()}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    // Neither source.
    let err = dispatch(
        &mut lib,
        &covers,
        "export.share_card",
        json!({"path": path.clone()}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    // An empty view (rate everything, then Needs a Rating is empty)…
    let mut lib2 = Library::default();
    dispatch(
        &mut lib2,
        &covers,
        "list.create",
        json!({"name": "Rated", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib2,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Done", "rating": 8}),
    )
    .unwrap();
    let err = dispatch(
        &mut lib2,
        &covers,
        "export.share_card",
        json!({"view": "needs_rating", "path": path.clone()}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    // …and an unknown view id.
    let err = dispatch(
        &mut lib2,
        &covers,
        "export.share_card",
        json!({"view": "bogus", "path": path}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn share_card_list_path_reports_the_list_name_as_title() {
    if !share_card::system_font_available() {
        eprintln!("skipping: no system font on this machine");
        return;
    }
    let dir = temp_dir("cardtitle");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = fixture_library(&covers);
    let out = dir.join("list-card.png");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.share_card",
        json!({"list_id": 1, "path": out.to_string_lossy()}),
    )
    .unwrap();
    assert_eq!(r["title"], "Films");
    assert_eq!(r["entries"], 4);
    let _ = fs::remove_dir_all(&dir);
}
