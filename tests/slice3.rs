//! Integration tests for v0.2 Slice 3: bulk actions (#9) and the
//! duplicate finder + merge (#3).
//!
//! The bulk/merge tests lean on exact `Library` equality for the
//! one-undo-step and no-mutation-on-error requirements, and on the
//! corpus fixture in `tests/fixtures/duplicates/` for the finder's
//! grouping/confidence semantics. Registry ≡ JSON-channel equivalence
//! is asserted for one bulk op and one merge, mirroring Slices 1–2.

use favelists::commands::{self, CommandContext, History};
use favelists::model::{Category, Item, Library};
use favelists::store::Store;
use serde_json::{json, Value};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "favelists-slice3-{}-{}-{}",
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

fn titles(lib: &Library, list_id: u64) -> Vec<&str> {
    lib.list(list_id)
        .unwrap()
        .items
        .iter()
        .map(|i| i.title.as_str())
        .collect()
}

fn ids(lib: &Library, list_id: u64) -> Vec<u64> {
    lib.list(list_id)
        .unwrap()
        .items
        .iter()
        .map(|i| i.id)
        .collect()
}

/// Two movie lists built entirely through the registry (with history):
/// list 1 "Films A" = Alien(1), Arrival(2), Blade Runner(3), Dune(4);
/// list 2 "Films B" = Alien(5, a second copy of the work), Heat(6).
fn build_sample(covers: &Path) -> (Library, History) {
    let mut lib = Library::default();
    let mut history = History::new();
    dispatch_h(
        &mut lib,
        &mut history,
        covers,
        "list.create",
        json!({"name": "Films A", "category": "movies", "ranked": true}),
    )
    .unwrap();
    for (title, year, added) in [
        ("Alien", "1979", 1_700_000_000_i64),
        ("Arrival", "2016", 1_700_000_100),
        ("Blade Runner", "1982", 1_700_000_200),
        ("Dune", "2021", 1_700_000_300),
    ] {
        dispatch_h(
            &mut lib,
            &mut history,
            covers,
            "item.add",
            json!({"list_id": 1, "title": title,
                   "fields": {"year": year}, "added_at": added}),
        )
        .unwrap();
    }
    dispatch_h(
        &mut lib,
        &mut history,
        covers,
        "list.create",
        json!({"name": "Films B", "category": "movies"}),
    )
    .unwrap();
    dispatch_h(
        &mut lib,
        &mut history,
        covers,
        "item.add",
        json!({"list_id": 2, "title": "Alien",
               "fields": {"year": "1979"}, "added_at": 1_700_000_400}),
    )
    .unwrap();
    dispatch_h(
        &mut lib,
        &mut history,
        covers,
        "item.add",
        json!({"list_id": 2, "title": "Heat",
               "fields": {"year": "1995"}, "added_at": 1_700_000_500}),
    )
    .unwrap();
    (lib, history)
}

// ---------------------------------------------------------------------------
// #9 — bulk actions
// ---------------------------------------------------------------------------

#[test]
fn bulk_remove_removes_present_ids_and_reports() {
    let dir = temp_dir("bulk-remove");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let (mut lib, _history) = build_sample(&covers);
    // Request order is scrambled and includes an unknown id: the result
    // reports what actually left (in request order), the rest is ignored.
    let r = dispatch(
        &mut lib,
        &covers,
        "items.bulk_remove",
        json!({"list_id": 1, "item_ids": [4, 2, 999]}),
    )
    .unwrap();
    assert_eq!(r["removed"], json!([4, 2]));
    assert_eq!(titles(&lib, 1), ["Alien", "Blade Runner"]);
    // Unknown list: clean not_found.
    let err = dispatch(
        &mut lib,
        &covers,
        "items.bulk_remove",
        json!({"list_id": 99, "item_ids": [1]}),
    )
    .unwrap_err();
    assert_eq!(err.code, "not_found");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bulk_move_preserves_ids_and_source_order_and_skips_duplicates() {
    let dir = temp_dir("bulk-move");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let (mut lib, _history) = build_sample(&covers);
    // Scrambled request [Dune(4), Alien(1), Blade Runner(3)]: processing
    // follows SOURCE order (Alien, Blade Runner, Dune). Alien's work is
    // already in list 2, so it is skipped and reported; the other two
    // move with their ids intact, appended in source order.
    let r = dispatch(
        &mut lib,
        &covers,
        "items.bulk_move",
        json!({"from_list_id": 1, "to_list_id": 2, "item_ids": [4, 1, 3]}),
    )
    .unwrap();
    assert_eq!(r["moved"], json!([3, 4]));
    assert_eq!(r["skipped"], json!([1]));
    assert_eq!(titles(&lib, 1), ["Alien", "Arrival"]);
    assert_eq!(titles(&lib, 2), ["Alien", "Heat", "Blade Runner", "Dune"]);
    assert_eq!(ids(&lib, 2), [5, 6, 3, 4]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bulk_move_same_list_is_a_clean_noop() {
    let dir = temp_dir("bulk-move-noop");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let (mut lib, _history) = build_sample(&covers);
    let before = lib.clone();
    let r = dispatch(
        &mut lib,
        &covers,
        "items.bulk_move",
        json!({"from_list_id": 1, "to_list_id": 1, "item_ids": [1, 2]}),
    )
    .unwrap();
    assert_eq!(r["moved"], json!([]));
    assert_eq!(r["skipped"], json!([]));
    assert_eq!(r["noop"], true);
    assert_eq!(lib, before);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bulk_move_to_missing_list_errors_without_mutation() {
    let dir = temp_dir("bulk-move-missing");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let (mut lib, mut history) = build_sample(&covers);
    let before = lib.clone();
    let undo_before = history.undo_len();
    let err = dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "items.bulk_move",
        json!({"from_list_id": 1, "to_list_id": 99, "item_ids": [1]}),
    )
    .unwrap_err();
    assert_eq!(err.code, "not_found");
    assert_eq!(lib, before);
    assert_eq!(history.undo_len(), undo_before); // failed command: no undo step
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bulk_copy_allocates_new_ids_and_shares_the_cover_reference() {
    let dir = temp_dir("bulk-copy");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Source", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Covered", "cover": "covered.jpg",
               "fields": {"year": "2001"}, "added_at": 1_700_000_000}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Target", "category": "movies"}),
    )
    .unwrap();
    let next_before = lib.next_item_id;
    let r = dispatch(
        &mut lib,
        &covers,
        "items.bulk_copy",
        json!({"from_list_id": 1, "to_list_id": 2, "item_ids": [1]}),
    )
    .unwrap();
    let new_id = r["copied"][0]["item_id"].as_u64().unwrap();
    assert_eq!(r["copied"][0]["source_item_id"], 1);
    assert_ne!(new_id, 1);
    assert_eq!(new_id, next_before); // allocated from the library counter
    let copy = lib.list(2).unwrap().item(new_id).unwrap();
    assert_eq!(copy.title, "Covered");
    // The cover file reference is shared, not duplicated.
    assert_eq!(copy.cover.as_deref(), Some("covered.jpg"));
    assert_eq!(lib.list(1).unwrap().items.len(), 1); // source untouched
                                                     // Copying the same work again: now a duplicate in the target.
    let r = dispatch(
        &mut lib,
        &covers,
        "items.bulk_copy",
        json!({"from_list_id": 1, "to_list_id": 2, "item_ids": [1]}),
    )
    .unwrap();
    assert_eq!(r["copied"], json!([]));
    assert_eq!(r["skipped"], json!([1]));
    // Copying within the same list skips everything (the work is there).
    let r = dispatch(
        &mut lib,
        &covers,
        "items.bulk_copy",
        json!({"from_list_id": 1, "to_list_id": 1, "item_ids": [1]}),
    )
    .unwrap();
    assert_eq!(r["skipped"], json!([1]));
    assert_eq!(lib.list(1).unwrap().items.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bulk_action_is_exactly_one_undo_step() {
    let dir = temp_dir("bulk-undo");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let (mut lib, mut history) = build_sample(&covers);
    let before = lib.clone();
    let undo_before = history.undo_len();
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "items.bulk_move",
        json!({"from_list_id": 1, "to_list_id": 2, "item_ids": [3, 4]}),
    )
    .unwrap();
    assert_eq!(history.undo_len(), undo_before + 1); // ONE step for the whole batch
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
    assert_eq!(r["command"], "items.bulk_move");
    assert_eq!(lib, before); // everything restored at once
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #3 — duplicate finder
// ---------------------------------------------------------------------------

/// Hand-built library (direct pushes, so within-list copies can exist):
/// list A: "The Matrix" (1999, provider), "Dune" (2021);
/// list B: "Matrix" (1999, manual), "Dune" (1984);
/// list C (books): "Dune" (1965).
fn duplicate_library() -> Library {
    let mut lib = Library::default();
    let a = lib.new_list("Films", Category::Movies, true);
    let b = lib.new_list("More Films", Category::Movies, false);
    let c = lib.new_list("Books", Category::Books, false);
    let push = |lib: &mut Library, list: u64, title: &str, year: &str, pid: Option<&str>| {
        let mut item = Item::new(lib.alloc_item_id(), title);
        let key = lib.list(list).unwrap().category.year_key().unwrap();
        item.fields.insert(key.to_string(), year.to_string());
        if let Some(p) = pid {
            item.provider_id = Some(p.to_string());
            item.source = "tmdb".to_string();
        }
        lib.list_mut(list).unwrap().items.push(item);
    };
    push(&mut lib, a, "The Matrix", "1999", Some("603"));
    push(&mut lib, a, "Dune", "2021", Some("438631"));
    push(&mut lib, b, "Matrix", "1999", None);
    push(&mut lib, b, "Dune", "1984", None);
    push(&mut lib, c, "Dune", "1965", None);
    lib
}

#[test]
fn find_duplicates_groups_with_confidence() {
    let dir = temp_dir("find-dups");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = duplicate_library();
    let r = dispatch(&mut lib, &covers, "library.find_duplicates", json!({})).unwrap();
    let groups = r["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    let matrix = groups.iter().find(|g| g["key"] == "matrix").unwrap();
    assert_eq!(matrix["confidence"], "exact"); // article stripped, years agree
    assert_eq!(matrix["category"], "movies");
    assert_eq!(matrix["copies"].as_array().unwrap().len(), 2);
    let dune = groups.iter().find(|g| g["key"] == "dune").unwrap();
    assert_eq!(dune["confidence"], "possible"); // 2021 vs 1984
                                                // The Books "Dune" never joins the movie group.
    assert!(groups
        .iter()
        .all(|g| g["copies"].as_array().unwrap().len() == 2));
    assert!(groups.iter().all(|g| g["category"] == "movies"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn duplicates_corpus_fixture() {
    let dir = temp_dir("dups-corpus");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("duplicates")
        .join("library.json");
    let fixture: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let mut lib: Library = serde_json::from_value(fixture["library"].clone()).unwrap();
    // Via the pure library function…
    let direct = favelists::duplicates::find_duplicates(&lib);
    // …and via the registry command: identical groups.
    let r = dispatch(&mut lib, &covers, "library.find_duplicates", json!({})).unwrap();
    let via_cmd = favelists::duplicates::groups_from_result(&r);
    assert_eq!(via_cmd, direct);
    let expected = fixture["expect_groups"].as_array().unwrap();
    assert_eq!(direct.len(), expected.len());
    for exp in expected {
        let group = direct
            .iter()
            .find(|g| g.key == exp["key"].as_str().unwrap())
            .unwrap_or_else(|| panic!("missing group {}", exp["key"]));
        assert_eq!(group.category_id, exp["category"].as_str().unwrap());
        assert_eq!(
            group.confidence.as_str(),
            exp["confidence"].as_str().unwrap()
        );
        let got_ids: Vec<u64> = group.copies.iter().map(|c| c.item_id).collect();
        let want_ids: Vec<u64> = exp["copy_item_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        assert_eq!(got_ids, want_ids, "group {}", exp["key"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #3 — merge
// ---------------------------------------------------------------------------

/// List A holds a sparse manual copy of "Obscure Film" (year only);
/// list B holds the rich provider copy (genre, director, cover, pid).
fn merge_library(covers: &Path) -> Library {
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        covers,
        "list.create",
        json!({"name": "Kept List", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        covers,
        "item.add",
        json!({"list_id": 1, "title": "Obscure Film",
               "fields": {"year": "1977"}, "added_at": 1_700_000_000}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        covers,
        "list.create",
        json!({"name": "Rich List", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        covers,
        "item.add",
        json!({"list_id": 2, "title": "Obscure Film",
               "fields": {"year": "1977", "genre": "Drama", "director": "A. Director"},
               "provider_id": "42", "source": "tmdb",
               "cover": "obscure.jpg", "added_at": 1_700_000_100}),
    )
    .unwrap();
    lib
}

#[test]
fn merge_adopts_missing_fields_cover_and_provider() {
    let dir = temp_dir("merge-adopt");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = merge_library(&covers);
    let r = dispatch(
        &mut lib,
        &covers,
        "library.merge_duplicates",
        json!({"keep": {"list_id": 1, "item_id": 1},
               "drop": [{"list_id": 2, "item_id": 2}]}),
    )
    .unwrap();
    assert_eq!(r["kept"], json!({"list_id": 1, "item_id": 1}));
    assert_eq!(r["removed"], json!([{"list_id": 2, "item_id": 2}]));
    assert_eq!(r["adopted_cover"], true);
    assert_eq!(r["adopted_provider"], true);
    let adopted: Vec<&str> = r["adopted_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(adopted, ["director", "genre"]); // year was already present
    let kept = lib.list(1).unwrap().item(1).unwrap();
    assert_eq!(kept.field("genre"), Some("Drama"));
    assert_eq!(kept.field("director"), Some("A. Director"));
    assert_eq!(kept.field("year"), Some("1977"));
    assert_eq!(kept.cover.as_deref(), Some("obscure.jpg"));
    assert_eq!(kept.provider_id.as_deref(), Some("42"));
    assert_eq!(kept.source, "tmdb");
    assert!(lib.list(2).unwrap().items.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn merge_never_overwrites_kept_fields() {
    let dir = temp_dir("merge-keep-wins");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "A", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Some Film",
               "fields": {"year": "1990", "genre": "Comedy"}, "added_at": 1_700_000_000}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "B", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 2, "title": "Some Film",
               "fields": {"year": "1990", "genre": "Drama", "director": "D"},
               "added_at": 1_700_000_100}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "library.merge_duplicates",
        json!({"keep": {"list_id": 1, "item_id": 1},
               "drop": [{"list_id": 2, "item_id": 2}]}),
    )
    .unwrap();
    let kept = lib.list(1).unwrap().item(1).unwrap();
    assert_eq!(kept.field("genre"), Some("Comedy")); // kept wins
    assert_eq!(kept.field("director"), Some("D")); // missing one adopted
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn merge_invalid_inputs_error_without_mutation() {
    let dir = temp_dir("merge-invalid");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = merge_library(&covers);
    let mut history = History::new();
    let cases: Vec<(Value, &str)> = vec![
        // keep also in drop
        (
            json!({"keep": {"list_id": 1, "item_id": 1},
                   "drop": [{"list_id": 1, "item_id": 1}]}),
            "bad_params",
        ),
        // empty drop
        (
            json!({"keep": {"list_id": 1, "item_id": 1}, "drop": []}),
            "bad_params",
        ),
        // drop item does not exist
        (
            json!({"keep": {"list_id": 1, "item_id": 1},
                   "drop": [{"list_id": 2, "item_id": 99}]}),
            "not_found",
        ),
        // keep list does not exist
        (
            json!({"keep": {"list_id": 99, "item_id": 1},
                   "drop": [{"list_id": 2, "item_id": 2}]}),
            "not_found",
        ),
        // keep item does not exist
        (
            json!({"keep": {"list_id": 1, "item_id": 99},
                   "drop": [{"list_id": 2, "item_id": 2}]}),
            "not_found",
        ),
    ];
    for (params, code) in cases {
        let before = lib.clone();
        let undo_before = history.undo_len();
        let err = dispatch_h(
            &mut lib,
            &mut history,
            &covers,
            "library.merge_duplicates",
            params.clone(),
        )
        .unwrap_err();
        assert_eq!(err.code, code, "params: {params}");
        assert_eq!(lib, before, "invalid merge mutated state: {params}");
        assert_eq!(
            history.undo_len(),
            undo_before,
            "failed merge recorded history"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn merge_is_one_undo_step() {
    let dir = temp_dir("merge-undo");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = merge_library(&covers);
    let mut history = History::new();
    // The fixture was built without history, so the stack starts empty
    // and the merge must be the one and only recorded step.
    let before = lib.clone();
    let undo_before = history.undo_len();
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "library.merge_duplicates",
        json!({"keep": {"list_id": 1, "item_id": 1},
               "drop": [{"list_id": 2, "item_id": 2}]}),
    )
    .unwrap();
    assert_eq!(history.undo_len(), undo_before + 1);
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
    assert_eq!(r["command"], "library.merge_duplicates");
    assert_eq!(lib, before); // dropped copy back, kept copy sparse again
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Registry surface + channel equivalence
// ---------------------------------------------------------------------------

#[test]
fn registry_includes_the_slice3_commands() {
    let names = commands::command_names();
    // 29 through Slice 4; Slice 5 added export.share_card + import.csv.
    assert_eq!(names.len(), 31);
    for expected in [
        "items.bulk_remove",
        "items.bulk_move",
        "items.bulk_copy",
        "library.find_duplicates",
        "library.merge_duplicates",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
    assert!(commands::is_mutating("items.bulk_move"));
    assert!(commands::is_mutating("library.merge_duplicates"));
    assert!(!commands::is_mutating("library.find_duplicates"));
}

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
fn bulk_move_channel_matches_registry() {
    let dir = temp_dir("bulk-equiv");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    // Identical build + bulk move through the channel:
    let base = dir.join("base");
    fs::create_dir_all(&base).unwrap();
    let input = concat!(
        "{\"command\":\"list.create\",\"params\":{\"name\":\"A\",\"category\":\"movies\"}}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":1,\"title\":\"Alien\",\"fields\":{\"year\":\"1979\"},\"added_at\":1700000000}}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":1,\"title\":\"Arrival\",\"fields\":{\"year\":\"2016\"},\"added_at\":1700000100}}\n",
        "{\"command\":\"list.create\",\"params\":{\"name\":\"B\",\"category\":\"movies\"}}\n",
        "{\"command\":\"items.bulk_move\",\"params\":{\"from_list_id\":1,\"to_list_id\":2,\"item_ids\":[1,2]},\"id\":99}\n",
    );
    let responses = run_channel(&base, input);
    assert!(responses.iter().all(|r| r["ok"] == true));
    let channel_result = responses[4]["result"].clone();
    // …and through direct registry dispatch on an identical library:
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "A", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien",
               "fields": {"year": "1979"}, "added_at": 1_700_000_000}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Arrival",
               "fields": {"year": "2016"}, "added_at": 1_700_000_100}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "B", "category": "movies"}),
    )
    .unwrap();
    let registry_result = dispatch(
        &mut lib,
        &covers,
        "items.bulk_move",
        json!({"from_list_id": 1, "to_list_id": 2, "item_ids": [1, 2]}),
    )
    .unwrap();
    assert_eq!(channel_result, registry_result);
    // The channel persisted the moved state to the normal data path.
    let store = Store::open(&base).unwrap();
    let persisted = store.load_library().unwrap();
    assert_eq!(titles(&persisted, 2), ["Alien", "Arrival"]);
    assert!(persisted.list(1).unwrap().items.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn merge_channel_matches_registry() {
    let dir = temp_dir("merge-equiv");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    // Same sparse/rich pair built both ways; with added_at pinned, the
    // merge results (ids, adopted fields) must be identical.
    let build = concat!(
        "{\"command\":\"list.create\",\"params\":{\"name\":\"A\",\"category\":\"movies\"}}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":1,\"title\":\"Obscure Film\",\"fields\":{\"year\":\"1977\"},\"added_at\":1700000000}}\n",
        "{\"command\":\"list.create\",\"params\":{\"name\":\"B\",\"category\":\"movies\"}}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":2,\"title\":\"Obscure Film\",\"fields\":{\"year\":\"1977\",\"genre\":\"Drama\"},\"cover\":\"obscure.jpg\",\"added_at\":1700000100}}\n",
    );
    let base = dir.join("base");
    fs::create_dir_all(&base).unwrap();
    let mut session = build.to_string();
    session.push_str(
        "{\"command\":\"library.merge_duplicates\",\"params\":{\"keep\":{\"list_id\":1,\"item_id\":1},\"drop\":[{\"list_id\":2,\"item_id\":2}]},\"id\":7}\n",
    );
    let responses = run_channel(&base, &session);
    assert!(responses.iter().all(|r| r["ok"] == true));
    let channel_result = responses[4]["result"].clone();

    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "A", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Obscure Film",
               "fields": {"year": "1977"}, "added_at": 1_700_000_000}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "B", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 2, "title": "Obscure Film",
               "fields": {"year": "1977", "genre": "Drama"},
               "cover": "obscure.jpg", "added_at": 1_700_000_100}),
    )
    .unwrap();
    let registry_result = dispatch(
        &mut lib,
        &covers,
        "library.merge_duplicates",
        json!({"keep": {"list_id": 1, "item_id": 1},
               "drop": [{"list_id": 2, "item_id": 2}]}),
    )
    .unwrap();
    assert_eq!(channel_result, registry_result);
    assert_eq!(registry_result["adopted_fields"], json!(["genre"]));
    assert_eq!(registry_result["adopted_cover"], true);
    // The channel's persisted library has the merged kept copy.
    let store = Store::open(&base).unwrap();
    let persisted = store.load_library().unwrap();
    let kept = persisted.list(1).unwrap().item(1).unwrap();
    assert_eq!(kept.field("genre"), Some("Drama"));
    assert_eq!(kept.cover.as_deref(), Some("obscure.jpg"));
    let _ = fs::remove_dir_all(&dir);
}
