//! Integration tests for the v0.2 Slice 1 foundation: the command registry
//! (`favelists::commands`), the headless JSON channel
//! (`favelists::json_channel`), and the corpus fixtures under
//! `tests/fixtures/commands/`.
//!
//! The equivalence tests are the point of the slice: the same operations
//! driven three ways — direct library calls (what the GUI did before
//! routing), registry dispatch (what the GUI does now), and JSON-lines
//! through the channel — must land on identical store state.

use favelists::commands::{self, CommandContext};
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
        "favelists-commands-{}-{}-{}",
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

/// Zero the wall-clock fields so two libraries built at slightly different
/// moments can be compared exactly. (`added_at` is pinned by the callers;
/// `created_at` is not settable through the model API.)
fn normalised(lib: &Library) -> Library {
    let mut lib = lib.clone();
    for list in &mut lib.lists {
        list.created_at = 0;
    }
    lib
}

fn tiny_png_bytes() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 30, 30, 255]));
    favelists::render::encode(&img, favelists::render::ImageFormat::Png).unwrap()
}

// ---------------------------------------------------------------------------
// Every registered command succeeds on a fixture store
// ---------------------------------------------------------------------------

#[test]
fn every_registered_command_dispatches_successfully() {
    let dir = temp_dir("all");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();

    let r = dispatch(&mut lib, &covers, "commands.list", json!({})).unwrap();
    assert_eq!(
        r["commands"].as_array().unwrap().len(),
        commands::COMMANDS.len()
    );

    // Lists.
    let r = dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Best Sci-Fi", "category": "movies", "ranked": true}),
    )
    .unwrap();
    assert_eq!(r["list_id"], 1);
    let r = dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Sodas", "category": "sodas"}),
    )
    .unwrap();
    assert_eq!(r["list_id"], 2);

    // Items.
    let r = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien",
               "fields": {"year": "1979", "director": "Ridley Scott"},
               "provider_id": "348", "source": "tmdb", "added_at": 1700000000}),
    )
    .unwrap();
    assert_eq!(r["outcome"], "added");
    assert_eq!(r["item_id"], 1);
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Arrival",
               "fields": {"year": "2016", "director": "Denis Villeneuve"},
               "provider_id": "329865", "source": "tmdb", "added_at": 1700000100}),
    )
    .unwrap();
    // Same work again (same provider id): reported duplicate, not added.
    let r = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien (1979)",
               "provider_id": "348", "source": "tmdb", "added_at": 1700000200}),
    )
    .unwrap();
    assert_eq!(r["outcome"], "duplicate");
    assert_eq!(lib.list(1).unwrap().items.len(), 2);
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 2, "title": "Fanta", "fields": {"brand": "Coca-Cola", "flavor": "Orange"}}),
    )
    .unwrap();

    // List mutations.
    dispatch(
        &mut lib,
        &covers,
        "list.rename",
        json!({"list_id": 2, "name": "Best Sodas"}),
    )
    .unwrap();
    assert_eq!(lib.list(2).unwrap().name, "Best Sodas");
    dispatch(
        &mut lib,
        &covers,
        "list.set_ranked",
        json!({"list_id": 2, "ranked": true}),
    )
    .unwrap();
    assert!(lib.list(2).unwrap().ranked);

    // Item edit: wholesale field replace + cover assignment.
    let png = tiny_png_bytes();
    fs::write(covers.join("cover-1.png"), &png).unwrap();
    let r = dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": 1,
               "fields": {"year": "1979", "director": "Ridley Scott", "genre": "Horror"},
               "cover": "cover-1.png"}),
    )
    .unwrap();
    assert_eq!(r["item"]["fields"]["genre"], "Horror");
    assert_eq!(r["item"]["cover"], "cover-1.png");

    // Order ops: move, swap (net no-op pair), then explicit rank.
    dispatch(
        &mut lib,
        &covers,
        "item.move",
        json!({"list_id": 1, "from": 0, "to": 1}),
    )
    .unwrap();
    assert_eq!(lib.list(1).unwrap().items[0].title, "Arrival");
    dispatch(
        &mut lib,
        &covers,
        "item.swap",
        json!({"list_id": 1, "a": 0, "b": 1}),
    )
    .unwrap();
    assert_eq!(lib.list(1).unwrap().items[0].title, "Alien");
    let r = dispatch(
        &mut lib,
        &covers,
        "item.set_rank",
        json!({"list_id": 1, "item_id": 2, "rank": 1}),
    )
    .unwrap();
    assert_eq!(r["rank"], 1);
    assert_eq!(lib.list(1).unwrap().items[0].title, "Arrival");
    dispatch(
        &mut lib,
        &covers,
        "list.sort",
        json!({"list_id": 1, "kind": "title"}),
    )
    .unwrap();
    assert_eq!(lib.list(1).unwrap().items[0].title, "Alien");

    // Queries.
    let r = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "query": "ali"}),
    )
    .unwrap();
    let matches = r["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["item"]["title"], "Alien");
    let r = dispatch(&mut lib, &covers, "library.search", json!({"query": "a"})).unwrap();
    let matches = r["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 3); // Alien, Arrival, Fanta
    assert!(matches.iter().any(|m| m["list_id"] == 2));
    let r = dispatch(&mut lib, &covers, "library.stats", json!({})).unwrap();
    assert_eq!(r["lists"], 2);
    assert_eq!(r["items"], 3);
    let r = dispatch(&mut lib, &covers, "list.get", json!({"list_id": 1})).unwrap();
    assert_eq!(r["name"], "Best Sci-Fi");
    let r = dispatch(&mut lib, &covers, "library.get", json!({})).unwrap();
    assert_eq!(r["lists"].as_array().unwrap().len(), 2);

    // Text export.
    let txt_path = dir.join("sci-fi.txt");
    dispatch(
        &mut lib,
        &covers,
        "export.text",
        json!({"list_id": 1, "path": txt_path.to_str().unwrap()}),
    )
    .unwrap();
    let text = fs::read_to_string(&txt_path).unwrap();
    assert!(text.contains("1. Alien"));
    assert!(text.contains("Director: Ridley Scott"));

    // Markdown export with the cover asset.
    let md_path = dir.join("sci-fi.md");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.markdown",
        json!({"list_id": 1, "path": md_path.to_str().unwrap()}),
    )
    .unwrap();
    assert_eq!(r["assets_written"], 1);
    let md = fs::read_to_string(&md_path).unwrap();
    assert!(md.contains("![cover](sci-fi_assets/cover-1.png)"));
    assert!(dir.join("sci-fi_assets/cover-1.png").exists());

    // Image export: PNG written, the coverless item reported missing.
    let img_path = dir.join("sci-fi.png");
    let r = dispatch(
        &mut lib,
        &covers,
        "export.image",
        json!({"list_id": 1, "path": img_path.to_str().unwrap(), "format": "png"}),
    )
    .unwrap();
    let bytes = fs::read(&img_path).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    assert_eq!(r["missing_indices"], json!([1])); // Arrival has no cover

    // Backup round-trip through the registry: export everything, then
    // import into the same library — all duplicates, nothing added.
    let zip_path = dir.join("backup.zip");
    let r = dispatch(
        &mut lib,
        &covers,
        "backup.export",
        json!({"path": zip_path.to_str().unwrap(), "scope": {"kind": "whole"}}),
    )
    .unwrap();
    assert_eq!(r["lists_written"], 2);
    let r = dispatch(
        &mut lib,
        &covers,
        "backup.import",
        json!({"path": zip_path.to_str().unwrap()}),
    )
    .unwrap();
    assert_eq!(r["lists_merged"], 2);
    assert_eq!(r["items_added"], 0);
    assert_eq!(r["items_skipped_as_duplicates"], 3);

    // Category scope + removal + deletion.
    let zip2 = dir.join("movies.zip");
    let r = dispatch(
        &mut lib,
        &covers,
        "backup.export",
        json!({"path": zip2.to_str().unwrap(),
               "scope": {"kind": "category", "category": "movies"}}),
    )
    .unwrap();
    assert_eq!(r["lists_written"], 1);
    let arrival_id = lib.list(1).unwrap().items[1].id;
    let r = dispatch(
        &mut lib,
        &covers,
        "item.remove",
        json!({"list_id": 1, "item_id": arrival_id}),
    )
    .unwrap();
    assert_eq!(r["removed"], true);
    dispatch(&mut lib, &covers, "list.delete", json!({"list_id": 2})).unwrap();
    assert_eq!(lib.lists.len(), 1);

    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Errors are clean and non-mutating
// ---------------------------------------------------------------------------

#[test]
fn bad_params_and_missing_entities_error_cleanly() {
    let dir = temp_dir("errors");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Films", "category": "movies", "ranked": true}),
    )
    .unwrap();

    let err = dispatch(&mut lib, &covers, "list.get", json!({"list_id": 99})).unwrap_err();
    assert_eq!(err.code, "not_found");
    let err = dispatch(&mut lib, &covers, "item.add", json!({"list_id": 1})).unwrap_err();
    assert_eq!(err.code, "bad_params"); // missing title
    let err = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "  "}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params"); // empty title
    let err = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 42, "title": "Ghost"}),
    )
    .unwrap_err();
    assert_eq!(err.code, "not_found");
    let err = dispatch(
        &mut lib,
        &covers,
        "item.remove",
        json!({"list_id": 1, "item_id": 42}),
    )
    .unwrap_err();
    assert_eq!(err.code, "not_found");
    let err = dispatch(
        &mut lib,
        &covers,
        "item.set_rank",
        json!({"list_id": 1, "item_id": 1, "rank": 0}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params"); // rank is 1-based
    let err = dispatch(&mut lib, &covers, "export.text", json!({"list_id": 1})).unwrap_err();
    assert_eq!(err.code, "bad_params"); // missing path
    let err = dispatch(
        &mut lib,
        &covers,
        "export.image",
        json!({"list_id": 1, "path": "x.png", "arrangement": "spiral-mountain"}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    let err = dispatch(
        &mut lib,
        &covers,
        "backup.export",
        json!({"path": "x.zip", "scope": {"kind": "everything"}}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    let err = dispatch(&mut lib, &covers, "list.delete", json!({"list_id": 99})).unwrap_err();
    assert_eq!(err.code, "not_found");

    // None of the failures above mutated the library.
    assert_eq!(lib.lists.len(), 1);
    assert_eq!(lib.lists[0].items.len(), 0);
    assert_eq!(lib.next_item_id, 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cover_bytes_round_trip_through_item_add() {
    let dir = temp_dir("coverbytes");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Films", "category": "movies"}),
    )
    .unwrap();
    let png = tiny_png_bytes();
    let r = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien",
               "cover_bytes_base64": commands::base64_encode(&png),
               "cover_ext": "png"}),
    )
    .unwrap();
    assert_eq!(r["outcome"], "added");
    let item = &lib.list(1).unwrap().items[0];
    assert_eq!(item.cover.as_deref(), Some("cover-1.png"));
    assert_eq!(fs::read(covers.join("cover-1.png")).unwrap(), png);
    // Invalid base64 is a clean bad_params, not a panic or a partial add.
    let err = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Arrival", "cover_bytes_base64": "***"}),
    )
    .unwrap_err();
    assert_eq!(err.code, "bad_params");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Equivalence: direct library calls (the pre-routing GUI path) vs registry
// ---------------------------------------------------------------------------

fn sample_via_direct_calls() -> Library {
    let mut lib = Library::default();
    let list = lib.new_list("Best Sci-Fi", Category::Movies, true);
    let mut alien = Item::new(lib.alloc_item_id(), "Alien");
    alien.fields.insert("year".into(), "1979".into());
    alien
        .fields
        .insert("director".into(), "Ridley Scott".into());
    alien.provider_id = Some("348".into());
    alien.source = "tmdb".into();
    alien.added_at = 1_700_000_000;
    lib.list_mut(list).unwrap().add_item(alien);
    let mut arrival = Item::new(lib.alloc_item_id(), "Arrival");
    arrival.fields.insert("year".into(), "2016".into());
    arrival.provider_id = Some("329865".into());
    arrival.source = "tmdb".into();
    arrival.added_at = 1_700_000_100;
    lib.list_mut(list).unwrap().add_item(arrival);
    lib.list_mut(list).unwrap().move_item(0, 1);
    lib
}

fn sample_via_dispatch(covers: &Path) -> Library {
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        covers,
        "list.create",
        json!({"name": "Best Sci-Fi", "category": "movies", "ranked": true}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        covers,
        "item.add",
        json!({"list_id": 1, "title": "Alien",
               "fields": {"year": "1979", "director": "Ridley Scott"},
               "provider_id": "348", "source": "tmdb", "added_at": 1700000000}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        covers,
        "item.add",
        json!({"list_id": 1, "title": "Arrival",
               "fields": {"year": "2016"},
               "provider_id": "329865", "source": "tmdb", "added_at": 1700000100}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        covers,
        "item.move",
        json!({"list_id": 1, "from": 0, "to": 1}),
    )
    .unwrap();
    lib
}

#[test]
fn registry_dispatch_matches_direct_library_calls() {
    let dir = temp_dir("equiv-direct");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let direct = sample_via_direct_calls();
    let via_registry = sample_via_dispatch(&covers);
    assert_eq!(normalised(&direct), normalised(&via_registry));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_channel_matches_registry_path() {
    let dir = temp_dir("equiv-channel");
    let base = dir.join("base");
    fs::create_dir_all(&base).unwrap();
    let input = concat!(
        "{\"command\":\"list.create\",\"params\":{\"name\":\"Best Sci-Fi\",\"category\":\"movies\",\"ranked\":true}}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":1,\"title\":\"Alien\",\"fields\":{\"year\":\"1979\",\"director\":\"Ridley Scott\"},\"provider_id\":\"348\",\"source\":\"tmdb\",\"added_at\":1700000000}}\n",
        "{\"command\":\"item.add\",\"params\":{\"list_id\":1,\"title\":\"Arrival\",\"fields\":{\"year\":\"2016\"},\"provider_id\":\"329865\",\"source\":\"tmdb\",\"added_at\":1700000100}}\n",
        "{\"command\":\"item.move\",\"params\":{\"list_id\":1,\"from\":0,\"to\":1}}\n",
    );
    let mut out: Vec<u8> = Vec::new();
    favelists::json_channel::run(&base, Cursor::new(input.as_bytes().to_vec()), &mut out).unwrap();
    let responses: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(responses.len(), 4);
    assert!(responses.iter().all(|r| r["ok"] == true));

    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let via_registry = sample_via_dispatch(&covers);
    let store = Store::open(&base).unwrap();
    let via_channel = store.load_library().unwrap();
    assert_eq!(normalised(&via_registry), normalised(&via_channel));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Corpus fixtures
// ---------------------------------------------------------------------------

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("commands")
}

/// Rebuild a library Value with `created_at` normalised, for comparison
/// against the recorded expected state.
fn normalised_value(lib: &Library) -> Value {
    let mut v = serde_json::to_value(normalised(lib)).unwrap();
    // serde_json::Value equality is field-order insensitive for objects,
    // but make the intent explicit: the fixture pins everything else.
    if let Some(lists) = v.get_mut("lists").and_then(Value::as_array_mut) {
        for list in lists {
            list["created_at"] = json!(0);
        }
    }
    v
}

#[test]
fn fixture_session_reproduces_expected_library() {
    let dir = temp_dir("fixture-session");
    let base = dir.join("base");
    fs::create_dir_all(&base).unwrap();
    let requests = fs::read_to_string(fixture_dir().join("session_requests.jsonl")).unwrap();
    let mut out: Vec<u8> = Vec::new();
    favelists::json_channel::run(&base, Cursor::new(requests.into_bytes()), &mut out).unwrap();
    let responses: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(responses.len(), 5);
    assert!(responses.iter().all(|r| r["ok"] == true));
    // The session's final query sees only Alien for "ali" (post-move index 1).
    let matches = responses[4]["result"]["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["item"]["title"], "Alien");
    assert_eq!(matches[0]["index"], 1);

    let store = Store::open(&base).unwrap();
    let lib = store.load_library().unwrap();
    let expected: Value = serde_json::from_str(
        &fs::read_to_string(fixture_dir().join("session_expected_library.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(normalised_value(&lib), expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fixture_bad_requests_produce_expected_error_codes() {
    let dir = temp_dir("fixture-bad");
    let base = dir.join("base");
    fs::create_dir_all(&base).unwrap();
    let requests = fs::read_to_string(fixture_dir().join("bad_requests.jsonl")).unwrap();
    let mut out: Vec<u8> = Vec::new();
    favelists::json_channel::run(&base, Cursor::new(requests.into_bytes()), &mut out).unwrap();
    let responses: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let codes: Vec<&str> = responses
        .iter()
        .map(|r| r["error"]["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        ["unknown_command", "not_found", "bad_params", "bad_json"]
    );
    assert!(responses.iter().all(|r| r["ok"] == false));
    let _ = fs::remove_dir_all(&dir);
}
