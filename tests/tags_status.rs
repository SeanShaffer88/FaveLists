//! Integration tests for v0.3 Slice 1 (foundation): item tags (#4) and
//! item status (#5).
//!
//! Back-compat is pinned by a recorded fixture: a v0.2-shaped library
//! (`tests/fixtures/library_v020.json`, items WITH rating/note keys
//! but WITHOUT tags/status keys) loads with empty tags and no status,
//! and its rating/note values survive intact.

use favelists::commands::{self, CommandContext};
use favelists::model::Library;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "favelists-tags-status-{}-{}-{}",
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

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn lib_with_list() -> (Library, PathBuf) {
    let dir = temp_dir("cmds");
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
    (lib, covers)
}

// ---------------------------------------------------------------------------
// Back-compat: a v0.2 library loads with tag/status defaults
// ---------------------------------------------------------------------------

#[test]
fn v020_library_without_tags_status_keys_loads_with_defaults() {
    let lib: Library = serde_json::from_str(&fixture("library_v020.json")).unwrap();
    let list = &lib.lists[0];
    assert_eq!(list.items.len(), 2);
    for item in &list.items {
        assert!(item.tags.is_empty(), "{} should be untagged", item.title);
        assert_eq!(item.status, None, "{} should have no status", item.title);
    }
    // And the v0.2 values themselves survive: rating and note intact.
    let alien = &list.items[0];
    assert_eq!(alien.rating, Some(8));
    assert_eq!(alien.note, "Scary in a good way");
    // A re-serialised library carries the new keys with the defaults.
    let text = serde_json::to_string(&lib).unwrap();
    let lib2: Library = serde_json::from_str(&text).unwrap();
    assert_eq!(lib2.lists[0].items[0].tags, Vec::<String>::new());
    assert_eq!(lib2.lists[0].items[0].status, None);
    assert!(text.contains("\"status\":null"), "serialised item:\n{text}");
    assert!(text.contains("\"tags\":[]"), "serialised item:\n{text}");
}

// ---------------------------------------------------------------------------
// Registry surface: library.tags joined in v0.3 Slice 1 (count since grown)
// ---------------------------------------------------------------------------

#[test]
fn registry_includes_library_tags() {
    let names = commands::command_names();
    assert_eq!(names.len(), 42);
    assert!(names.contains(&"library.tags"));
    assert!(!commands::is_mutating("library.tags"));
}

// ---------------------------------------------------------------------------
// Commands: add/edit tags + status, normalisation, rejection
// ---------------------------------------------------------------------------

#[test]
fn item_add_and_edit_tags_and_status() {
    let (mut lib, covers) = lib_with_list();
    let r = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien",
               "tags": [" sci-fi ", "Horror", "sci-fi", "", "Comfort"],
               "status": "tried"}),
    )
    .unwrap();
    let id = r["item_id"].as_u64().unwrap();
    let item = &lib.lists[0].items[0];
    // Trimmed, deduped case-insensitively (first spelling wins), sorted.
    assert_eq!(item.tags, ["Comfort", "Horror", "sci-fi"]);
    assert_eq!(item.status, Some(favelists::model::ItemStatus::Tried));

    // Edit: replace tags wholesale, clear status with null.
    dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": id,
               "tags": ["New Tag"], "status": null}),
    )
    .unwrap();
    let item = &lib.lists[0].items[0];
    assert_eq!(item.tags, ["New Tag"]);
    assert_eq!(item.status, None);

    // Edit: clear tags with null.
    dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": id, "tags": null}),
    )
    .unwrap();
    assert!(lib.lists[0].items[0].tags.is_empty());

    // Edit without either param leaves both alone.
    dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": id, "note": "kept"}),
    )
    .unwrap();
    let item = &lib.lists[0].items[0];
    assert!(item.tags.is_empty());
    assert_eq!(item.status, None);
}

#[test]
fn invalid_tags_and_status_are_bad_params_without_mutation() {
    let (mut lib, covers) = lib_with_list();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien", "tags": ["ok"], "status": "want"}),
    )
    .unwrap();
    let before = lib.clone();
    let bad_tags: Vec<Value> = vec![
        json!({"tags": ["x".repeat(41)]}),
        json!({"tags": (0..21).map(|i| format!("t{i}")).collect::<Vec<_>>()}),
        json!({"tags": "not-an-array"}),
        json!({"tags": ["ok", 42]}),
    ];
    for params in bad_tags {
        let mut p = params;
        p["list_id"] = json!(1);
        p["item_id"] = json!(1);
        let err = dispatch(&mut lib, &covers, "item.edit", p).unwrap_err();
        assert_eq!(err.code, "bad_params");
        assert_eq!(lib, before, "rejected edit must not mutate the library");
    }
    for bad in ["bogus", "WANT", ""] {
        let err = dispatch(
            &mut lib,
            &covers,
            "item.edit",
            json!({"list_id": 1, "item_id": 1, "status": bad}),
        )
        .unwrap_err();
        assert_eq!(err.code, "bad_params", "status {bad} must be rejected");
        assert_eq!(lib, before);
    }
    // And on add: an invalid status creates no item.
    let err = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Nope", "status": "watched"}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    assert_eq!(lib.lists[0].items.len(), 1);
}

// ---------------------------------------------------------------------------
// Filters: list.query + library.search (tag/status), tag text in search
// ---------------------------------------------------------------------------

fn lib_with_tagged_items() -> (Library, PathBuf) {
    let (mut lib, covers) = lib_with_list();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien", "tags": ["sci-fi"], "status": "tried"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Arrival", "tags": ["Sci-Fi", "Drama"], "status": "want"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "The Thing", "tags": ["Horror"], "status": "tried"}),
    )
    .unwrap();
    (lib, covers)
}

#[test]
fn list_query_filters_by_tag_and_status() {
    let (mut lib, covers) = lib_with_tagged_items();
    // Tag match is case-insensitive.
    let r = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "tag": "SCI-FI"}),
    )
    .unwrap();
    let titles: Vec<&str> = r["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["item"]["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Alien", "Arrival"]);
    // Status filter.
    let r = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "status": "tried"}),
    )
    .unwrap();
    assert_eq!(r["matches"].as_array().unwrap().len(), 2);
    // Combined: tag AND status.
    let r = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "tag": "sci-fi", "status": "want"}),
    )
    .unwrap();
    let titles: Vec<&str> = r["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["item"]["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Arrival"]);
    // Title query still composes with the filters.
    let r = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "query": "the", "status": "tried"}),
    )
    .unwrap();
    let titles: Vec<&str> = r["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["item"]["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["The Thing"]);
    // A mistyped status filter is a clean error, not a silent no-match.
    let err = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "status": "bogus"}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
}

#[test]
fn library_search_tag_filter_status_filter_and_tag_text() {
    let (mut lib, covers) = lib_with_tagged_items();
    // The query text matches tag text, not just titles: no title here
    // contains "horror", and only "The Thing" carries it as a tag —
    // the tag-text match finds exactly that item.
    let r = dispatch(
        &mut lib,
        &covers,
        "library.search",
        json!({"query": "horror"}),
    )
    .unwrap();
    let titles: Vec<&str> = r["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["The Thing"]);
    // Tag filter (exact, case-insensitive) + status filter.
    let r = dispatch(
        &mut lib,
        &covers,
        "library.search",
        json!({"tag": "drama", "status": "want"}),
    )
    .unwrap();
    let titles: Vec<&str> = r["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Arrival"]);
    // Empty query + tag filter still narrows.
    let r = dispatch(
        &mut lib,
        &covers,
        "library.search",
        json!({"query": "", "tag": "sci-fi"}),
    )
    .unwrap();
    assert_eq!(r["matches"].as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// library.tags counts
// ---------------------------------------------------------------------------

#[test]
fn library_tags_counts_sorted_and_case_grouped() {
    let (mut lib, covers) = lib_with_tagged_items();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Sodas", "category": "sodas"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 2, "title": "Cola", "tags": ["SCI-FI", "Fizzy"]}),
    )
    .unwrap();
    let r = dispatch(&mut lib, &covers, "library.tags", json!({})).unwrap();
    let tags: Vec<(String, u64)> = r["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["tag"].as_str().unwrap().to_string(),
                t["count"].as_u64().unwrap(),
            )
        })
        .collect();
    // Sorted case-insensitively; "sci-fi" groups across spellings and
    // lists, showing the first-seen spelling.
    assert_eq!(
        tags,
        [
            ("Drama".to_string(), 1),
            ("Fizzy".to_string(), 1),
            ("Horror".to_string(), 1),
            ("sci-fi".to_string(), 3),
        ]
    );
}

// ---------------------------------------------------------------------------
// Merge: tag union + status adoption
// ---------------------------------------------------------------------------

#[test]
fn merge_adopts_tag_union_and_status_without_overwriting() {
    let (mut lib, covers) = lib_with_list();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "The Matrix", "tags": ["sci-fi"], "status": null}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "More films", "category": "movies"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 2, "title": "Matrix", "tags": ["Sci-Fi", "Mind-bending"], "status": "owned"}),
    )
    .unwrap();
    // The kept copy already has a status: it must NOT be overwritten.
    dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": 1, "status": "want"}),
    )
    .unwrap();
    let r = dispatch(
        &mut lib,
        &covers,
        "library.merge_duplicates",
        json!({"keep": {"list_id": 1, "item_id": 1},
               "drop": [{"list_id": 2, "item_id": 2}]}),
    )
    .unwrap();
    assert_eq!(r["adopted_tags"], 1, "only Mind-bending is new");
    assert_eq!(r["adopted_status"], false, "kept status survives");
    let kept = &lib.lists[0].items[0];
    assert_eq!(kept.tags, ["Mind-bending", "sci-fi"]);
    assert_eq!(
        kept.status,
        Some(favelists::model::ItemStatus::Want),
        "kept status is never overwritten"
    );

    // Now a kept copy with NO status adopts the dropped copy's status.
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Dune"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 2, "title": "Dune II", "status": "tried"}),
    )
    .unwrap();
    let r = dispatch(
        &mut lib,
        &covers,
        "library.merge_duplicates",
        json!({"keep": {"list_id": 1, "item_id": 3},
               "drop": [{"list_id": 2, "item_id": 4}]}),
    )
    .unwrap();
    assert_eq!(r["adopted_status"], true);
    assert_eq!(
        lib.lists[0].items[1].status,
        Some(favelists::model::ItemStatus::Tried)
    );
}

// ---------------------------------------------------------------------------
// Exports: tags + status pseudo-fields
// ---------------------------------------------------------------------------

#[test]
fn exports_render_tags_and_status_only_when_selected() {
    let (mut lib, covers) = lib_with_list();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien", "tags": ["sci-fi", "comfort"], "status": "tried"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Arrival"}),
    )
    .unwrap();
    let dir = temp_dir("export");

    let txt = dir.join("out.txt");
    dispatch(
        &mut lib,
        &covers,
        "export.text",
        json!({"list_id": 1, "path": txt.to_str().unwrap(), "fields": ["tags", "status"]}),
    )
    .unwrap();
    let text = fs::read_to_string(&txt).unwrap();
    assert!(
        text.contains("Tags: comfort, sci-fi"),
        "text export:\n{text}"
    );
    assert!(text.contains("Status: tried"), "text export:\n{text}");
    assert_eq!(
        text.matches("Tags:").count(),
        1,
        "only the tagged item renders one"
    );
    assert_eq!(
        text.matches("Status:").count(),
        1,
        "only the statused item renders one"
    );

    let md_path = dir.join("out.md");
    dispatch(
        &mut lib,
        &covers,
        "export.markdown",
        json!({"list_id": 1, "path": md_path.to_str().unwrap(), "fields": ["tags", "status"]}),
    )
    .unwrap();
    let md = fs::read_to_string(&md_path).unwrap();
    assert!(
        md.contains("Tags: comfort, sci-fi"),
        "markdown export:\n{md}"
    );
    assert!(md.contains("Status: tried"), "markdown export:\n{md}");

    // Without selection: neither renders.
    let plain = dir.join("plain.txt");
    dispatch(
        &mut lib,
        &covers,
        "export.text",
        json!({"list_id": 1, "path": plain.to_str().unwrap(), "fields": []}),
    )
    .unwrap();
    let text = fs::read_to_string(&plain).unwrap();
    assert!(
        !text.contains("Tags:"),
        "unselected tags must not render:\n{text}"
    );
    assert!(
        !text.contains("Status:"),
        "unselected status must not render:\n{text}"
    );
}
