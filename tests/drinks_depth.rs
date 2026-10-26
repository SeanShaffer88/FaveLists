//! Integration tests for v0.3 Slice 4 (drinks depth + covers):
//! #11 barcode add (the response parser, on synthetic fixtures in the
//! real response shape), #12 tried-it for drinks (status toggle +
//! per-list progress), #13 caffeine sort + filter, and #9 the
//! missing-cover sweep (`library.missing_covers` + the `item.edit`
//! cover-bytes path the sweep sets covers through).
//!
//! The live barcode lookup and provider cover fetches are network
//! seams and stay untested here by design — what is pinned is every
//! pure layer they stand on.

use favelists::barcode::{self, BarcodeLookup};
use favelists::commands::{self, CommandContext};
use favelists::drinks;
use favelists::model::{ItemStatus, Library};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "favelists-drinks-depth-{}-{}-{}",
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

fn lib_with_energy_list() -> (Library, PathBuf) {
    let dir = temp_dir("cmds");
    let covers = dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    let mut lib = Library::default();
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Energy", "category": "energy_drinks"}),
    )
    .unwrap();
    (lib, covers)
}

fn add_drink(lib: &mut Library, covers: &Path, title: &str, caffeine: Option<&str>) -> u64 {
    let fields = match caffeine {
        Some(label) => json!({"brand": "ZapCo", "caffeine": label}),
        None => json!({"brand": "ZapCo"}),
    };
    let r = dispatch(
        lib,
        covers,
        "item.add",
        json!({"list_id": 1, "title": title, "fields": fields, "source": "drinks-db"}),
    )
    .unwrap();
    r["item_id"].as_u64().unwrap()
}

fn titles(lib: &Library, list_id: u64) -> Vec<String> {
    lib.list(list_id)
        .unwrap()
        .items
        .iter()
        .map(|i| i.title.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Registry surface: library.missing_covers joined in v0.3 Slice 4
// ---------------------------------------------------------------------------

#[test]
fn registry_includes_missing_covers() {
    let names = commands::command_names();
    assert_eq!(names.len(), 42);
    assert!(names.contains(&"library.missing_covers"));
    assert!(!commands::is_mutating("library.missing_covers"));
}

// ---------------------------------------------------------------------------
// #11 barcode parsing (synthetic fixtures, real response shape)
// ---------------------------------------------------------------------------

#[test]
fn barcode_fixture_parses_to_add_fields() {
    let p = match barcode::parse_product_response("5000000000017", &fixture("off_product.json"))
        .unwrap()
    {
        BarcodeLookup::Found(p) => p,
        BarcodeLookup::NotFound => panic!("the found fixture must parse as a product"),
    };
    assert_eq!(p.code, "5000000000017");
    assert_eq!(p.title, "ZapCo Wild Berry");
    assert_eq!(p.brand, "ZapCo"); // first of the comma list
    assert_eq!(p.flavor, "Wild Berry"); // mined by the drinks extractor
                                        // caffeine_100g 0.032 → 32 mg/100ml, via the drinks conversion.
    assert_eq!(p.caffeine_mg_per_100ml, Some(32.0));
    assert_eq!(p.caffeine_label.as_deref(), Some("32 mg/100ml"));
    let fields = p.add_fields();
    assert_eq!(fields["brand"], "ZapCo");
    assert_eq!(fields["flavor"], "Wild Berry");
    assert_eq!(fields["caffeine"], "32 mg/100ml");
}

#[test]
fn barcode_not_found_fixture() {
    assert_eq!(
        barcode::parse_product_response("5000000000017", &fixture("off_product_not_found.json"))
            .unwrap(),
        BarcodeLookup::NotFound
    );
}

#[test]
fn barcode_malformed_response_is_a_clean_error() {
    assert!(barcode::parse_product_response("5000000000017", "not json").is_err());
    assert!(barcode::parse_product_response("5000000000017", "").is_err());
}

// ---------------------------------------------------------------------------
// #13 caffeine sort + filter
// ---------------------------------------------------------------------------

#[test]
fn sort_by_caffeine_desc_missing_last_stable() {
    let (mut lib, covers) = lib_with_energy_list();
    add_drink(&mut lib, &covers, "Low", Some("10 mg/100ml"));
    add_drink(&mut lib, &covers, "High A", Some("32 mg/100ml"));
    add_drink(&mut lib, &covers, "Plain", None);
    add_drink(&mut lib, &covers, "Mid", Some("31.5 mg/100ml"));
    add_drink(&mut lib, &covers, "High B", Some("32 mg/100ml"));
    dispatch(
        &mut lib,
        &covers,
        "list.sort",
        json!({"list_id": 1, "kind": "caffeine"}),
    )
    .unwrap();
    // Highest first; the two 32s keep their relative order (stable);
    // the caffeine-less item goes last.
    assert_eq!(titles(&lib, 1), ["High A", "High B", "Mid", "Low", "Plain"]);
}

#[test]
fn list_query_caffeine_min_filters() {
    let (mut lib, covers) = lib_with_energy_list();
    add_drink(&mut lib, &covers, "Low", Some("10 mg/100ml"));
    add_drink(&mut lib, &covers, "High", Some("32 mg/100ml"));
    add_drink(&mut lib, &covers, "Plain", None);
    add_drink(&mut lib, &covers, "Mid", Some("31.5 mg/100ml"));

    let r = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "query": "", "caffeine_min": 20}),
    )
    .unwrap();
    let matched: Vec<&str> = r["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["item"]["title"].as_str().unwrap())
        .collect();
    // At-or-above the minimum, in list order; the caffeine-less item
    // never matches a caffeine filter.
    assert_eq!(matched, ["High", "Mid"]);

    // A boundary value is inclusive.
    let r = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "query": "", "caffeine_min": 32}),
    )
    .unwrap();
    assert_eq!(r["matches"].as_array().unwrap().len(), 1);

    // No filter: everything matches, as before this slice.
    let r = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "query": ""}),
    )
    .unwrap();
    assert_eq!(r["matches"].as_array().unwrap().len(), 4);

    // A mistyped filter is a clean bad_params, never silently ignored.
    let e = dispatch(
        &mut lib,
        &covers,
        "list.query",
        json!({"list_id": 1, "query": "", "caffeine_min": "lots"}),
    )
    .unwrap_err();
    assert_eq!(e.code, "bad_params");
}

// ---------------------------------------------------------------------------
// #12 tried-it: status toggle round-trip + per-list progress
// ---------------------------------------------------------------------------

#[test]
fn drink_status_toggle_round_trip_and_progress() {
    let (mut lib, covers) = lib_with_energy_list();
    let a = add_drink(&mut lib, &covers, "Zap Berry", Some("32 mg/100ml"));
    add_drink(&mut lib, &covers, "Zap Citrus", None);
    let list = lib.list(1).unwrap();
    assert_eq!(drinks::tried_progress(list), (0, 2));

    // The toggle's dispatch: status "tried".
    dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": a, "status": "tried"}),
    )
    .unwrap();
    let item = lib.list(1).unwrap().item(a).unwrap();
    assert_eq!(item.status, Some(ItemStatus::Tried));
    assert_eq!(drinks::tried_progress(lib.list(1).unwrap()), (1, 2));

    // And back off: status null clears.
    dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": a, "status": null}),
    )
    .unwrap();
    let item = lib.list(1).unwrap().item(a).unwrap();
    assert_eq!(item.status, None);
    assert_eq!(drinks::tried_progress(lib.list(1).unwrap()), (0, 2));
}

// ---------------------------------------------------------------------------
// #9 missing-cover sweep
// ---------------------------------------------------------------------------

#[test]
fn missing_covers_shape_order_total_and_non_mutating() {
    let dir = temp_dir("missing");
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
    dispatch(
        &mut lib,
        &covers,
        "list.create",
        json!({"name": "Shelf", "category": "books"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Coverless Film"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 1, "title": "Covered Film", "cover": "cover-2.jpg"}),
    )
    .unwrap();
    dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": 2, "title": "Coverless Book"}),
    )
    .unwrap();

    let before = serde_json::to_string(&lib).unwrap();
    let r = dispatch(&mut lib, &covers, "library.missing_covers", json!({})).unwrap();
    assert_eq!(r["total"], 2);
    let entries = r["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    // Library order: list 1's coverless item, then list 2's.
    assert_eq!(entries[0]["list_id"], 1);
    assert_eq!(entries[0]["list_name"], "Films");
    assert_eq!(entries[0]["category"], "movies");
    assert_eq!(entries[0]["title"], "Coverless Film");
    assert_eq!(entries[1]["list_id"], 2);
    assert_eq!(entries[1]["category"], "books");
    assert_eq!(entries[1]["title"], "Coverless Book");
    assert!(entries[0]["item_id"].as_u64().unwrap() > 0);
    // Non-mutating: the library is byte-identical afterwards.
    assert_eq!(serde_json::to_string(&lib).unwrap(), before);
}

#[test]
fn item_edit_cover_bytes_sets_cover_through_shared_write_path() {
    let (mut lib, covers) = lib_with_energy_list();
    let id = add_drink(&mut lib, &covers, "Coverless Cola", None);
    // The item starts in the sweep…
    let r = dispatch(&mut lib, &covers, "library.missing_covers", json!({})).unwrap();
    assert_eq!(r["total"], 1);

    // …and the sweep's set path — item.edit with cover bytes, exactly
    // what item.add accepts — writes the cover file under the item's
    // stable name and stores it on the item.
    let bytes = b"\xff\xd8\xff\xe0fake-jpeg-bytes";
    let r = dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": id,
               "cover_bytes_base64": commands::base64_encode(bytes),
               "cover_ext": "jpg"}),
    )
    .unwrap();
    assert_eq!(r["item"]["cover"], format!("cover-{id}.jpg"));
    let written = fs::read(covers.join(format!("cover-{id}.jpg"))).unwrap();
    assert_eq!(written, bytes);

    // …so it leaves the sweep.
    let r = dispatch(&mut lib, &covers, "library.missing_covers", json!({})).unwrap();
    assert_eq!(r["total"], 0);

    // Bad bytes are rejected before any mutation.
    let e = dispatch(
        &mut lib,
        &covers,
        "item.edit",
        json!({"list_id": 1, "item_id": id, "cover_bytes_base64": "not*base64"}),
    )
    .unwrap_err();
    assert_eq!(e.code, "bad_params");
    assert_eq!(
        lib.list(1).unwrap().item(id).unwrap().cover.as_deref(),
        Some(format!("cover-{id}.jpg").as_str())
    );
}
