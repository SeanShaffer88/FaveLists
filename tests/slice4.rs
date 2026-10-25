//! Integration tests for v0.2 Slice 4: personal rating + note (#5),
//! artist cover fallback parsers (#6, fixture level — live calls are
//! untested by design), and the drinks quality pass (#7: old-pack
//! tolerance + caffeine round-trip at the library level).
//!
//! Back-compat is pinned by recorded fixtures: a v0.1.1-shaped library
//! (`tests/fixtures/library_v011.json`, items WITHOUT rating/note keys)
//! and a v1 drinks pack (`tests/fixtures/drinks_pack_v1.json`, entries
//! WITHOUT the caffeine key).

use favelists::commands::{self, CommandContext};
use favelists::drinks::{DrinkEntry, DrinkKind, DrinksDb};
use favelists::model::Library;
use favelists::providers::artist_image;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "favelists-slice4-{}-{}-{}",
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

// ---------------------------------------------------------------------------
// #5 back-compat: v0.1.1 library files load unchanged
// ---------------------------------------------------------------------------

#[test]
fn v011_library_without_rating_note_keys_loads_with_defaults() {
    let lib: Library = serde_json::from_str(&fixture("library_v011.json")).unwrap();
    let list = &lib.lists[0];
    assert_eq!(list.items.len(), 2);
    for item in &list.items {
        assert_eq!(item.rating, None, "{} should be unrated", item.title);
        assert_eq!(item.note, "", "{} should have no note", item.title);
    }
    // And a re-serialised library round-trips through the same shape.
    let text = serde_json::to_string(&lib).unwrap();
    let lib2: Library = serde_json::from_str(&text).unwrap();
    assert_eq!(lib2.lists[0].items.len(), 2);
}

// ---------------------------------------------------------------------------
// #5 commands: add/edit rating + note
// ---------------------------------------------------------------------------

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

#[test]
fn item_add_and_edit_rating_and_note() {
    let (mut lib, covers) = lib_with_list();
    let r = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien", "rating": 9, "note": "  Director's cut is best  "}),
    )
    .unwrap();
    let id = r["item_id"].as_u64().unwrap();
    let item = &lib.lists[0].items[0];
    assert_eq!(item.rating, Some(9));
    assert_eq!(
        item.note, "Director's cut is best",
        "note is trimmed on save"
    );

    // Edit: change rating, clear note with whitespace-only text.
    dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": id, "rating": 7, "note": "   "}),
    )
    .unwrap();
    let item = &lib.lists[0].items[0];
    assert_eq!(item.rating, Some(7));
    assert_eq!(item.note, "");

    // Edit: clear rating with explicit null.
    dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": id, "rating": null}),
    )
    .unwrap();
    assert_eq!(lib.lists[0].items[0].rating, None);
}

#[test]
fn out_of_range_rating_is_bad_params_without_mutation() {
    let (mut lib, covers) = lib_with_list();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien", "rating": 5}),
    )
    .unwrap();
    let before = lib.clone();
    for bad in [json!(0), json!(11), json!(-1), json!("eight")] {
        let err = dispatch(
            &mut lib,
            &covers,
            "item.edit",
            json!({"list_id": 1, "item_id": 1, "rating": bad}),
        )
        .unwrap_err();
        assert_eq!(err.code, "bad_params", "rating {bad} must be rejected");
        assert_eq!(lib, before, "rejected edit must not mutate the library");
    }
    // And on add: no item is created.
    let err = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Nope", "rating": 42}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    assert_eq!(lib.lists[0].items.len(), 1);
}

// ---------------------------------------------------------------------------
// #5 sort by rating + exports
// ---------------------------------------------------------------------------

#[test]
fn sort_by_rating_highest_first_unrated_last_stable() {
    let (mut lib, covers) = lib_with_list();
    for (title, rating) in [
        ("Low", json!(3)),
        ("Unrated A", Value::Null),
        ("Top", json!(10)),
        ("Mid", json!(7)),
        ("Unrated B", Value::Null),
        ("Also Top", json!(10)),
    ] {
        let mut params = json!({"list_id": 1, "title": title});
        if !rating.is_null() {
            params["rating"] = rating;
        }
        dispatch(&mut lib, &covers, "item.add", params).unwrap();
    }
    dispatch(
        &mut lib,
        &covers,
        "list.sort",
        json!({"list_id": 1, "kind": "rating"}),
    )
    .unwrap();
    let titles: Vec<&str> = lib.lists[0]
        .items
        .iter()
        .map(|i| i.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["Top", "Also Top", "Mid", "Low", "Unrated A", "Unrated B"],
        "highest first, ties stable, unrated last in original order"
    );
}

#[test]
fn exports_render_rating_and_note_only_when_selected() {
    let (mut lib, covers) = lib_with_list();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien", "rating": 8, "note": "Scary in a good way"}),
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

    // With the pseudo-fields selected: both render for Alien, and the
    // unrated/noteless Arrival gains no extra lines.
    let txt = dir.join("out.txt");
    dispatch(
        &mut lib,
        &covers,
        "export.text",
        json!({"list_id": 1, "path": txt.to_str().unwrap(), "fields": ["rating", "note"]}),
    )
    .unwrap();
    let text = fs::read_to_string(&txt).unwrap();
    assert!(text.contains("Rating: 8/10"), "text export:\n{text}");
    assert!(
        text.contains("Note: Scary in a good way"),
        "text export:\n{text}"
    );
    assert_eq!(
        text.matches("Rating:").count(),
        1,
        "only the rated item renders one"
    );

    let md_path = dir.join("out.md");
    dispatch(
        &mut lib,
        &covers,
        "export.markdown",
        json!({"list_id": 1, "path": md_path.to_str().unwrap(), "fields": ["rating", "note"]}),
    )
    .unwrap();
    let md = fs::read_to_string(&md_path).unwrap();
    assert!(md.contains("Rating: 8/10"), "markdown export:\n{md}");
    assert!(
        md.contains("Note: Scary in a good way"),
        "markdown export:\n{md}"
    );

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
        !text.contains("Rating:"),
        "unselected rating must not render:\n{text}"
    );
    assert!(
        !text.contains("Note:"),
        "unselected note must not render:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// #6 artist cover fallback parsers (fixtures)
// ---------------------------------------------------------------------------

#[test]
fn artist_image_parsers_read_their_fixtures() {
    let deezer = fixture("deezer_artist.json");
    let url = artist_image::parse_deezer_artist_image(&deezer, "Radiohead").unwrap();
    assert!(
        url.contains("dzcdn.net"),
        "Deezer picture_xl expected: {url}"
    );
    // A name not among the results still falls back to the first hit.
    assert!(artist_image::parse_deezer_artist_image(&deezer, "Unknown Act").is_some());
    assert!(artist_image::parse_deezer_artist_image("{\"data\":[]}", "X").is_none());

    let wiki = fixture("wikipedia_artist_summary.json");
    let url = artist_image::parse_wikipedia_artist_image(&wiki).unwrap();
    assert!(
        url.contains("wikimedia.org"),
        "Wikipedia original image expected: {url}"
    );
    assert!(artist_image::parse_wikipedia_artist_image("{}").is_none());
}

// ---------------------------------------------------------------------------
// #7 drinks: old pack tolerance + caffeine round-trip
// ---------------------------------------------------------------------------

#[test]
fn v1_drinks_pack_without_caffeine_key_still_loads() {
    let db: DrinksDb = serde_json::from_str(&fixture("drinks_pack_v1.json")).unwrap();
    assert_eq!(db.entries.len(), 2);
    assert!(db.entries.iter().all(|e| e.caffeine_mg_per_100ml.is_none()));
    assert!(db.entries.iter().all(|e| e.caffeine_label().is_none()));
}

#[test]
fn caffeine_field_round_trips_and_labels() {
    let e = DrinkEntry {
        name: "Zoom Energy".into(),
        brand: "Zoom".into(),
        flavor: "Citrus".into(),
        kind: DrinkKind::EnergyDrink,
        image_url: None,
        caffeine_mg_per_100ml: Some(32.0),
    };
    assert_eq!(e.caffeine_label().as_deref(), Some("32 mg/100ml"));
    let text = serde_json::to_string(&e).unwrap();
    assert!(
        text.contains("\"caffeine_mg_per_100ml\":32.0"),
        "caffeine serialises when present: {text}"
    );
    let back: DrinkEntry = serde_json::from_str(&text).unwrap();
    assert_eq!(back, e);
}
