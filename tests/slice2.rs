//! Integration tests for v0.2 Slice 2: undo/redo + autosave journal
//! (#4) and filtered global search (#2).
//!
//! The undo tests lean on exact `Library` equality — `Library: PartialEq`
//! covers list set, item ids, order, fields, and the id counters, which is
//! precisely the "restore the EXACT prior state" requirement. The search
//! tests run against the corpus fixture in `tests/fixtures/search/` plus a
//! registry ≡ JSON-channel equivalence check on a filtered search.

use favelists::commands::{self, CommandContext, History};
use favelists::model::Library;
use favelists::store::{save_json_atomic, Store};
use serde_json::{json, Value};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "favelists-slice2-{}-{}-{}",
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

/// A two-list library built entirely through the registry (with history).
fn build_sample(covers: &Path) -> (Library, History) {
    let mut lib = Library::default();
    let mut history = History::new();
    dispatch_h(
        &mut lib,
        &mut history,
        covers,
        "list.create",
        json!({"name": "Best Sci-Fi", "category": "movies", "ranked": true}),
    )
    .unwrap();
    dispatch_h(
        &mut lib,
        &mut history,
        covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien",
               "fields": {"year": "1979", "genre": "Horror"}, "added_at": 1700000000}),
    )
    .unwrap();
    dispatch_h(
        &mut lib,
        &mut history,
        covers,
        "item.add",
        json!({"list_id": 1, "title": "Arrival",
               "fields": {"year": "2016", "genre": "Sci-Fi"}, "added_at": 1700000100}),
    )
    .unwrap();
    dispatch_h(
        &mut lib,
        &mut history,
        covers,
        "item.add",
        json!({"list_id": 1, "title": "Blade Runner",
               "fields": {"year": "1982", "genre": "Sci-Fi"}, "added_at": 1700000200}),
    )
    .unwrap();
    (lib, history)
}

// ---------------------------------------------------------------------------
// #4 — undo / redo
// ---------------------------------------------------------------------------

#[test]
fn undo_add_restores_exact_prior_state() {
    let dir = temp_dir("undo-add");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    let mut history = History::new();
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "list.create",
        json!({"name": "Films", "category": "movies", "ranked": true}),
    )
    .unwrap();
    let before_add = lib.clone();
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien", "fields": {"year": "1979"}}),
    )
    .unwrap();
    assert_eq!(lib.total_items(), 1);
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
    assert_eq!(r["undone"], true);
    assert_eq!(r["command"], "item.add");
    // Exact: item gone AND the item-id counter rolled back.
    assert_eq!(lib, before_add);
    assert_eq!(lib.next_item_id, before_add.next_item_id);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn undo_delete_list_restores_it_fully() {
    let dir = temp_dir("undo-delete");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let (mut lib, mut history) = build_sample(&covers);
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "list.create",
        json!({"name": "Sodas", "category": "sodas"}),
    )
    .unwrap();
    let before_delete = lib.clone();
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "list.delete",
        json!({"list_id": 1}),
    )
    .unwrap();
    assert!(lib.list(1).is_none());
    assert_eq!(lib.lists.len(), 1);
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
    assert_eq!(r["command"], "list.delete");
    assert_eq!(lib, before_delete);
    assert_eq!(lib.list(1).unwrap().items.len(), 3);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn undo_set_rank_restores_order_and_redo_reapplies() {
    let dir = temp_dir("undo-rank");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let (mut lib, mut history) = build_sample(&covers);
    let blade_id = lib.list(1).unwrap().items[2].id;
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "item.set_rank",
        json!({"list_id": 1, "item_id": blade_id, "rank": 1}),
    )
    .unwrap();
    let ranked_order: Vec<&str> = lib
        .list(1)
        .unwrap()
        .items
        .iter()
        .map(|i| i.title.as_str())
        .collect();
    assert_eq!(ranked_order, ["Blade Runner", "Alien", "Arrival"]);
    let after_rank = lib.clone();
    dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
    let restored: Vec<&str> = lib
        .list(1)
        .unwrap()
        .items
        .iter()
        .map(|i| i.title.as_str())
        .collect();
    assert_eq!(restored, ["Alien", "Arrival", "Blade Runner"]);
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.redo", json!({})).unwrap();
    assert_eq!(r["redone"], true);
    assert_eq!(r["command"], "item.set_rank");
    assert_eq!(lib, after_rank);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn new_mutation_clears_the_redo_stack() {
    let dir = temp_dir("undo-clear-redo");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let (mut lib, mut history) = build_sample(&covers);
    dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
    assert!(history.can_redo());
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Dune", "added_at": 1700000300}),
    )
    .unwrap();
    assert!(!history.can_redo());
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.redo", json!({})).unwrap();
    assert_eq!(r["redone"], false);
    // "Blade Runner" (the undone add) stays gone; "Dune" is what's there.
    let titles: Vec<&str> = lib
        .list(1)
        .unwrap()
        .items
        .iter()
        .map(|i| i.title.as_str())
        .collect();
    assert_eq!(titles, ["Alien", "Arrival", "Dune"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn undo_on_empty_history_is_a_clean_no_op() {
    let dir = temp_dir("undo-empty");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    let mut history = History::new();
    // With a history attached but nothing recorded.
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
    assert_eq!(r["undone"], false);
    assert_eq!(r["command"], Value::Null);
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.redo", json!({})).unwrap();
    assert_eq!(r["redone"], false);
    // Without any history attached at all.
    let r = dispatch(&mut lib, &covers, "history.undo", json!({})).unwrap();
    assert_eq!(r["undone"], false);
    assert!(lib.lists.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn history_cap_is_respected() {
    let dir = temp_dir("undo-cap");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    let mut history = History::new();
    dispatch_h(
        &mut lib,
        &mut history,
        &covers,
        "list.create",
        json!({"name": "Films", "category": "movies"}),
    )
    .unwrap();
    for n in 1..=54 {
        dispatch_h(
            &mut lib,
            &mut history,
            &covers,
            "item.add",
            json!({"list_id": 1, "title": format!("Film {n:02}"), "added_at": 1_700_000_000_i64 + n}),
        )
        .unwrap();
    }
    // 55 mutations, but only the newest 50 snapshots are kept.
    assert_eq!(history.undo_len(), commands::HISTORY_CAP);
    for step in 0..50 {
        let r = dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
        assert_eq!(r["undone"], true, "undo #{step} should succeed");
    }
    let r = dispatch_h(&mut lib, &mut history, &covers, "history.undo", json!({})).unwrap();
    assert_eq!(r["undone"], false);
    // State after 50 undos = just after the 5th mutation: list + 4 items,
    // id counters rolled back to match.
    assert_eq!(lib.list(1).unwrap().items.len(), 4);
    assert_eq!(lib.next_item_id, 5);
    assert_eq!(lib.next_list_id, 2);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn channel_undo_redo_session_persists() {
    let dir = temp_dir("channel-history");
    let base = dir.join("base");
    fs::create_dir_all(&base).unwrap();
    let input = concat!(
        "{\"command\":\"history.undo\",\"id\":1}\n",
        "{\"command\":\"list.create\",\"params\":{\"name\":\"Films\",\"category\":\"movies\"},\"id\":2}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":1,\"title\":\"Alien\",\"added_at\":1700000000},\"id\":3}\n",
        "{\"command\":\"history.undo\",\"id\":4}\n",
        "{\"command\":\"library.stats\",\"id\":5}\n",
        "{\"command\":\"history.redo\",\"id\":6}\n",
        "{\"command\":\"library.stats\",\"id\":7}\n",
    );
    let mut out: Vec<u8> = Vec::new();
    favelists::json_channel::run(&base, Cursor::new(input.as_bytes().to_vec()), &mut out).unwrap();
    let responses: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(responses.len(), 7);
    assert!(responses.iter().all(|r| r["ok"] == true));
    assert_eq!(responses[0]["result"]["undone"], false); // fresh session, empty history
    assert_eq!(responses[3]["result"]["undone"], true);
    assert_eq!(responses[3]["result"]["command"], "item.add");
    assert_eq!(responses[4]["result"]["items"], 0); // the add is undone
    assert_eq!(responses[5]["result"]["redone"], true);
    assert_eq!(responses[6]["result"]["items"], 1); // …and redone
                                                    // Undo/redo are mutating commands, so the channel persisted the final
                                                    // state — and the journal shadow was written through the same save.
    let store = Store::open(&base).unwrap();
    let lib = store.load_library().unwrap();
    assert_eq!(lib.total_items(), 1);
    assert!(store.journal_path().exists());
    let journal: Library =
        serde_json::from_str(&fs::read_to_string(store.journal_path()).unwrap()).unwrap();
    assert_eq!(journal, lib);
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #4 — autosave journal
// ---------------------------------------------------------------------------

fn sample_library() -> Library {
    let mut lib = Library::default();
    let id = lib.new_list("Films", favelists::model::Category::Movies, true);
    let iid = lib.alloc_item_id();
    let mut it = favelists::model::Item::new(iid, "Alien");
    it.fields.insert("year".into(), "1979".into());
    lib.list_mut(id).unwrap().add_item(it);
    lib
}

#[test]
fn journal_is_written_on_save_and_matches() {
    let dir = temp_dir("journal-write");
    let store = Store::open(&dir).unwrap();
    let lib = sample_library();
    store.save_library(&lib).unwrap();
    assert!(store.journal_path().exists());
    let journal_text = fs::read_to_string(store.journal_path()).unwrap();
    let journal: Library = serde_json::from_str(&journal_text).unwrap();
    assert_eq!(journal, lib);
    // A normal load does not flag recovery.
    let load = store.load_library_recovering().unwrap();
    assert!(!load.recovered_from_journal);
    assert_eq!(load.library, lib);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn journal_recovers_when_main_is_corrupt() {
    let dir = temp_dir("journal-corrupt");
    let store = Store::open(&dir).unwrap();
    let lib = sample_library();
    store.save_library(&lib).unwrap();
    fs::write(store.library_path(), b"{ this is not json").unwrap();
    let load = store.load_library_recovering().unwrap();
    assert!(load.recovered_from_journal);
    assert_eq!(load.library, lib);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn journal_recovers_when_main_is_missing() {
    let dir = temp_dir("journal-missing");
    let store = Store::open(&dir).unwrap();
    let lib = sample_library();
    store.save_library(&lib).unwrap();
    fs::remove_file(store.library_path()).unwrap();
    let load = store.load_library_recovering().unwrap();
    assert!(load.recovered_from_journal);
    assert_eq!(load.library, lib);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn valid_main_wins_over_a_stale_journal() {
    let dir = temp_dir("journal-main-wins");
    let store = Store::open(&dir).unwrap();
    let older = sample_library();
    store.save_library(&older).unwrap(); // journal now holds `older`
    let mut newer = older.clone();
    newer.new_list("Sodas", favelists::model::Category::Sodas, false);
    // Write ONLY the main file (bypassing save_library), so the journal
    // stays stale: the parseable main file must still win, unflagged.
    save_json_atomic(&store.library_path(), &newer).unwrap();
    let load = store.load_library_recovering().unwrap();
    assert!(!load.recovered_from_journal);
    assert_eq!(load.library, newer);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fresh_store_loads_default_without_recovery_flag() {
    let dir = temp_dir("journal-fresh");
    let store = Store::open(&dir).unwrap();
    let load = store.load_library_recovering().unwrap();
    assert!(!load.recovered_from_journal);
    assert_eq!(load.library, Library::default());
    // And a corrupt main with NO journal is still an honest error.
    fs::write(store.library_path(), b"nope").unwrap();
    assert!(store.load_library_recovering().is_err());
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// #2 — global search
// ---------------------------------------------------------------------------

fn search_fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("search")
        .join("library_and_cases.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn search_corpus_fixture_cases() {
    let dir = temp_dir("search-corpus");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let fixture = search_fixture();
    let mut lib: Library = serde_json::from_value(fixture["library"].clone()).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(cases.len() >= 8);
    for case in cases {
        let result = dispatch(&mut lib, &covers, "library.search", case["params"].clone())
            .unwrap_or_else(|e| panic!("search {:?} failed: {e}", case["params"]));
        let titles: Vec<&str> = result["matches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["title"].as_str().unwrap())
            .collect();
        let expected: Vec<&str> = case["expect_titles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();
        assert_eq!(titles, expected, "params: {:?}", case["params"]);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn search_result_shape_and_filter_param_validation() {
    let dir = temp_dir("search-shape");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let fixture = search_fixture();
    let mut lib: Library = serde_json::from_value(fixture["library"].clone()).unwrap();
    let r = dispatch(&mut lib, &covers, "library.search", json!({"query": "ali"})).unwrap();
    let m = &r["matches"][0];
    // The jump-to fields are top-level: a surface needs no item digging.
    assert_eq!(m["list_id"], 1);
    assert_eq!(m["list_name"], "Best Sci-Fi");
    assert_eq!(m["category"], "movies");
    assert_eq!(m["item_id"], 1);
    assert_eq!(m["title"], "Alien");
    assert_eq!(m["item"]["id"], 1); // Slice 1 shape is retained
                                    // Mistyped filters are clean bad_params, never silently ignored.
    let err = dispatch(
        &mut lib,
        &covers,
        "library.search",
        json!({"query": "", "category": "anime"}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    let err = dispatch(
        &mut lib,
        &covers,
        "library.search",
        json!({"query": "", "year": 1979}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn filtered_search_channel_matches_registry() {
    let dir = temp_dir("search-equiv");
    let base = dir.join("base");
    fs::create_dir_all(&base).unwrap();
    // Build identical data two ways. Via the channel:
    let build = concat!(
        "{\"command\":\"list.create\",\"params\":{\"name\":\"Best Sci-Fi\",\"category\":\"movies\",\"ranked\":true}}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":1,\"title\":\"Alien\",\"fields\":{\"year\":\"1979\",\"genre\":\"Horror\"},\"added_at\":1700000000}}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":1,\"title\":\"Arrival\",\"fields\":{\"year\":\"2016\",\"genre\":\"Sci-Fi\"},\"added_at\":1700000100}}\n",
        "{\"command\":\"list.create\",\"params\":{\"name\":\"Sci-Fi Books\",\"category\":\"books\"}}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":2,\"title\":\"Dune\",\"fields\":{\"year\":\"1965\",\"genre\":\"Sci-Fi\"},\"added_at\":1700000200}}\n",
    );
    let mut out: Vec<u8> = Vec::new();
    favelists::json_channel::run(&base, Cursor::new(build.as_bytes().to_vec()), &mut out).unwrap();
    // Filtered search through the channel (fresh session, recovered load):
    let ask = "{\"command\":\"library.search\",\"params\":{\"query\":\"\",\"genre\":\"sci-fi\",\"year\":\"19\"}}\n";
    let mut out: Vec<u8> = Vec::new();
    favelists::json_channel::run(&base, Cursor::new(ask.as_bytes().to_vec()), &mut out).unwrap();
    let channel_resp: Value = serde_json::from_str(String::from_utf8(out).unwrap().trim()).unwrap();
    assert_eq!(channel_resp["ok"], true);

    // …and via direct registry dispatch on an identical library:
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Best Sci-Fi", "category": "movies", "ranked": true}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien",
               "fields": {"year": "1979", "genre": "Horror"}, "added_at": 1700000000}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Arrival",
               "fields": {"year": "2016", "genre": "Sci-Fi"}, "added_at": 1700000100}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Sci-Fi Books", "category": "books"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 2, "title": "Dune",
               "fields": {"year": "1965", "genre": "Sci-Fi"}, "added_at": 1700000200}),
    )
    .unwrap();
    let registry_result = dispatch(
        &mut lib,
        &covers,
        "library.search",
        json!({"query": "", "genre": "sci-fi", "year": "19"}),
    )
    .unwrap();
    assert_eq!(channel_resp["result"], registry_result);
    // The filter itself: genre sci-fi AND year containing "19" → Dune only
    // (Alien is Horror; Arrival's year is 2016).
    let titles: Vec<&str> = registry_result["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Dune"]);
    let _ = fs::remove_dir_all(&dir);
}
