//! Integration tests for v0.3 Slice 6 (safety + setup): trash with
//! restore (#10), automatic backups (#19), the app update check
//! (#18), and the first-run wizard's settings flag (#20).
//!
//! Everything runs through the command registry — the same
//! dispatch the GUI and the JSON channel use — plus the pure
//! decision functions of `trash`, `auto_backup`, and
//! `update_check`.

use favelists::auto_backup;
use favelists::commands::{self, CommandContext, History};
use favelists::model::{Library, Settings};
use favelists::store::Store;
use favelists::trash;
use favelists::update_check;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "favelists-safety-setup-{}-{}-{}",
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
    covers: &Path,
    history: &mut History,
    name: &str,
    params: Value,
) -> Result<Value, commands::CommandError> {
    let mut ctx = CommandContext::with_history(lib, covers, history);
    commands::dispatch(&mut ctx, name, &params)
}

fn make_list(lib: &mut Library, covers: &Path, name: &str) -> u64 {
    dispatch(
        lib,
        covers,
        "list.create",
        json!({"name": name, "category": "movies"}),
    )
    .unwrap()["list_id"]
        .as_u64()
        .unwrap()
}

fn add_item(lib: &mut Library, covers: &Path, list_id: u64, title: &str) -> u64 {
    dispatch(
        lib,
        covers,
        "item.add",
        json!({"list_id": list_id, "title": title}),
    )
    .unwrap()["item_id"]
        .as_u64()
        .unwrap()
}

// ------------------------------------------------------------------ #10 trash

#[test]
fn trash_remove_list_restore_round_trip() {
    let dir = temp_dir("round-trip");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Films");
    let rich = dispatch(
        &mut lib,
        &covers,
        "item.add",
        json!({"list_id": list, "title": "Alien",
               "fields": {"year": "1979", "director": "Ridley Scott"},
               "provider_id": "348", "source": "tmdb",
               "rating": 8, "note": "great", "tags": ["classic"],
               "status": "owned", "added_at": 1700000000}),
    )
    .unwrap()["item_id"]
        .as_u64()
        .unwrap();
    let other = add_item(&mut lib, &covers, list, "Arrival");
    let before = lib.list(list).unwrap().item(rich).unwrap().clone();

    let r = dispatch(
        &mut lib,
        &covers,
        "item.remove",
        json!({"list_id": list, "item_id": rich}),
    )
    .unwrap();
    assert_eq!(r["removed"], true);
    assert_eq!(lib.list(list).unwrap().items.len(), 1);
    assert_eq!(lib.trash.len(), 1);
    assert_eq!(lib.trash[0].original_index, 0);
    assert_eq!(lib.trash[0].from_list_name, "Films");

    let r = dispatch(&mut lib, &covers, "trash.list", json!({})).unwrap();
    assert_eq!(r["total"], 1);
    let entry = &r["entries"][0];
    assert_eq!(entry["item_id"], rich);
    assert_eq!(entry["title"], "Alien");
    assert_eq!(entry["from_list_id"], list);
    assert_eq!(entry["from_list_name"], "Films");
    // Deleted a moment ago: the full window is left (29 only if the
    // run straddles a UTC midnight between the two dispatches).
    let days_left = entry["days_left"].as_i64().unwrap();
    assert!((29..=30).contains(&days_left), "days_left={days_left}");

    let r = dispatch(&mut lib, &covers, "trash.restore", json!({"item_id": rich})).unwrap();
    assert_eq!(r["restored_to"], list);
    assert!(lib.trash.is_empty());
    let items = &lib.list(list).unwrap().items;
    assert_eq!(items.len(), 2);
    // Restored at the END, with every personal field intact.
    assert_eq!(items[0].id, other);
    assert_eq!(&items[1], &before);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bulk_remove_trashes_in_one_undo_step() {
    let dir = temp_dir("bulk-undo");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let mut history = History::new();
    let list = make_list(&mut lib, &covers, "Films");
    let a = add_item(&mut lib, &covers, list, "Alien");
    add_item(&mut lib, &covers, list, "Arrival");
    let c = add_item(&mut lib, &covers, list, "Casablanca");

    let r = dispatch_h(
        &mut lib,
        &covers,
        &mut history,
        "items.bulk_remove",
        json!({"list_id": list, "item_ids": [a, c]}),
    )
    .unwrap();
    assert_eq!(r["removed"], json!([a, c]));
    assert_eq!(lib.list(list).unwrap().items.len(), 1);
    assert_eq!(history.undo_len(), 1);

    let r = dispatch(&mut lib, &covers, "trash.list", json!({})).unwrap();
    assert_eq!(r["total"], 2);

    // ONE undo restores both items AND empties the trash.
    dispatch_h(&mut lib, &covers, &mut history, "history.undo", json!({})).unwrap();
    assert_eq!(lib.list(list).unwrap().items.len(), 3);
    assert!(lib.trash.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn restore_after_original_list_deleted() {
    let dir = temp_dir("restore-orphan");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let gone = make_list(&mut lib, &covers, "Doomed");
    let safe = make_list(&mut lib, &covers, "Safe");
    let item = add_item(&mut lib, &covers, gone, "Alien");
    dispatch(
        &mut lib,
        &covers,
        "item.remove",
        json!({"list_id": gone, "item_id": item}),
    )
    .unwrap();
    dispatch(&mut lib, &covers, "list.delete", json!({"list_id": gone})).unwrap();

    // No target and the original list is gone: bad_params, and the
    // record stays in the trash.
    let e = dispatch(&mut lib, &covers, "trash.restore", json!({"item_id": item})).unwrap_err();
    assert_eq!(e.code, "bad_params");
    assert_eq!(lib.trash.len(), 1);

    // With an explicit target it lands there.
    let r = dispatch(
        &mut lib,
        &covers,
        "trash.restore",
        json!({"item_id": item, "list_id": safe}),
    )
    .unwrap();
    assert_eq!(r["restored_to"], safe);
    assert_eq!(lib.list(safe).unwrap().items.len(), 1);
    assert!(lib.trash.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn restore_conflict_is_bad_params_without_mutation() {
    let dir = temp_dir("restore-conflict");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Films");
    let first = add_item(&mut lib, &covers, list, "Alien");
    dispatch(
        &mut lib,
        &covers,
        "item.remove",
        json!({"list_id": list, "item_id": first}),
    )
    .unwrap();
    // The same work is added again while the first copy sits in the
    // trash — restoring now would duplicate it in the list.
    add_item(&mut lib, &covers, list, "Alien");

    let before = lib.clone();
    let e = dispatch(
        &mut lib,
        &covers,
        "trash.restore",
        json!({"item_id": first}),
    )
    .unwrap_err();
    assert_eq!(e.code, "bad_params");
    assert!(e.message.contains("already in that list"), "{}", e.message);
    assert_eq!(lib, before);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn trash_remove_and_empty() {
    let dir = temp_dir("remove-empty");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Films");
    let a = add_item(&mut lib, &covers, list, "Alien");
    let b = add_item(&mut lib, &covers, list, "Arrival");
    for id in [a, b] {
        dispatch(
            &mut lib,
            &covers,
            "item.remove",
            json!({"list_id": list, "item_id": id}),
        )
        .unwrap();
    }
    assert_eq!(lib.trash.len(), 2);

    let r = dispatch(&mut lib, &covers, "trash.remove", json!({"item_id": a})).unwrap();
    assert_eq!(r["removed"], true);
    assert_eq!(lib.trash.len(), 1);

    let r = dispatch(&mut lib, &covers, "trash.empty", json!({})).unwrap();
    assert_eq!(r["removed"], 1);
    assert!(lib.trash.is_empty());
    let r = dispatch(&mut lib, &covers, "trash.empty", json!({})).unwrap();
    assert_eq!(r["removed"], 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn trash_unknown_ids_are_not_found() {
    let dir = temp_dir("unknown");
    let covers = dir.join("covers");
    let mut lib = Library::default();
    let list = make_list(&mut lib, &covers, "Films");
    let item = add_item(&mut lib, &covers, list, "Alien");
    dispatch(
        &mut lib,
        &covers,
        "item.remove",
        json!({"list_id": list, "item_id": item}),
    )
    .unwrap();

    for (name, params) in [
        ("trash.restore", json!({"item_id": 999})),
        ("trash.remove", json!({"item_id": 999})),
        // A real record, but an unknown target list.
        ("trash.restore", json!({"item_id": item, "list_id": 999})),
    ] {
        let e = dispatch(&mut lib, &covers, name, params).unwrap_err();
        assert_eq!(e.code, "not_found", "{name}: {e}");
    }
    assert_eq!(lib.trash.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn purge_drops_only_entries_older_than_30_days() {
    let now: i64 = 1_800_000_000;
    let day = 86_400;
    let record = |id: u64, deleted_at: i64| trash::TrashedItem {
        item: favelists::model::Item::new(id, format!("Item {id}")),
        from_list_id: 1,
        from_list_name: "Films".to_string(),
        deleted_at,
        original_index: 0,
    };
    let mut trash_vec = vec![
        record(1, now - 29 * day),
        record(2, now - 30 * day),
        record(3, now - 31 * day),
    ];
    assert_eq!(trash::purge_expired(&mut trash_vec, now), 1);
    let kept: Vec<u64> = trash_vec.iter().map(|t| t.item.id).collect();
    assert_eq!(kept, [1, 2]);
    // days_left: fresh = the whole window; 29 days in = 1; at the
    // boundary = 0 (still kept — expiry is strictly past 30 days).
    assert_eq!(trash::days_left(now, now), 30);
    assert_eq!(trash::days_left(now - 29 * day, now), 1);
    assert_eq!(trash::days_left(now - 30 * day, now), 0);
}

#[test]
fn library_json_without_trash_key_loads_empty_trash() {
    // A library file written before the trash existed.
    let lib: Library =
        serde_json::from_str(r#"{"lists":[],"next_list_id":1,"next_item_id":1}"#).unwrap();
    assert!(lib.trash.is_empty());

    // And a library WITH a record round-trips through the store
    // shape unchanged.
    let mut lib = Library::default();
    lib.trash.push(trash::TrashedItem {
        item: favelists::model::Item::new(7, "Alien"),
        from_list_id: 1,
        from_list_name: "Films".to_string(),
        deleted_at: 1_700_000_000,
        original_index: 2,
    });
    let text = serde_json::to_string(&lib).unwrap();
    let back: Library = serde_json::from_str(&text).unwrap();
    assert_eq!(back, lib);
}

// ------------------------------------------------------- #19 automatic backups

#[test]
fn auto_backup_due_rule() {
    let now: i64 = 1_800_000_000;
    let day = 86_400;
    assert!(auto_backup::is_due(now, None));
    assert!(!auto_backup::is_due(now, Some(now - 6 * day)));
    assert!(auto_backup::is_due(now, Some(now - 7 * day)));
    assert!(auto_backup::is_due(now, Some(now - 8 * day)));
    // Backup names sort chronologically and carry the UTC time.
    assert_eq!(
        auto_backup::file_name_for(1_761_350_400),
        "auto-20251025-000000.zip"
    );
}

#[test]
fn auto_backup_rotation_keeps_newest_four() {
    let entries: Vec<(String, i64)> = [
        ("auto-a.zip", 100),
        ("auto-b.zip", 200),
        ("auto-c.zip", 300),
        ("auto-d.zip", 400),
        ("auto-e.zip", 500),
        ("auto-f.zip", 600),
    ]
    .iter()
    .map(|(n, t)| (n.to_string(), *t))
    .collect();
    // Six files, keep four: the two oldest go.
    assert_eq!(
        auto_backup::rotation_deletions(&entries, 4),
        ["auto-b.zip", "auto-a.zip"]
    );
    // At or under the keep count nothing is deleted.
    assert!(auto_backup::rotation_deletions(&entries[..4], 4).is_empty());
    // Equal mtimes are stable: the earlier-listed file counts as
    // the older of the tie.
    let tied: Vec<(String, i64)> = [("auto-x.zip", 100), ("auto-y.zip", 100), ("auto-z.zip", 50)]
        .iter()
        .map(|(n, t)| (n.to_string(), *t))
        .collect();
    assert_eq!(auto_backup::rotation_deletions(&tied, 2), ["auto-z.zip"]);
}

// ---------------------------------------------------------- #18 update check

#[test]
fn update_check_version_rules() {
    assert!(update_check::is_newer("0.3.0", "0.3.1"));
    assert!(!update_check::is_newer("0.3.1", "0.3.0"));
    assert!(!update_check::is_newer("0.3.0", "0.3.0")); // equal is not newer
    assert!(update_check::is_newer("0.9.9", "1.0"));
    assert!(update_check::is_newer("v0.3.0", "v0.3.1")); // v-prefix both sides
    assert_eq!(update_check::parse_version("v0.3.1"), Some((0, 3, 1)));
    assert_eq!(update_check::parse_version("1.0"), Some((1, 0, 0)));
    assert_eq!(update_check::parse_version("2"), Some((2, 0, 0)));
    // Junk never parses, and never counts as newer.
    assert_eq!(update_check::parse_version("junk"), None);
    assert_eq!(update_check::parse_version(""), None);
    assert_eq!(update_check::parse_version("1.0.0.1"), None);
    assert_eq!(update_check::parse_version("1.0-beta"), None);
    assert!(!update_check::is_newer("0.3.0", "junk"));
    assert!(!update_check::is_newer("junk", "9.9.9"));
}

#[test]
fn update_check_latest_tag_from_api_json() {
    let payload = r#"{"tag_name":"v0.4.0","name":"FaveLists v0.4.0","draft":false}"#;
    assert_eq!(
        update_check::latest_from_api_json(payload),
        Some("v0.4.0".to_string())
    );
    assert_eq!(update_check::latest_from_api_json(r#"{"name":"x"}"#), None);
    assert_eq!(
        update_check::latest_from_api_json(r#"{"tag_name":""}"#),
        None
    );
    assert_eq!(update_check::latest_from_api_json("not json"), None);
}

// ------------------------------------------------------------- #20 wizard flag

#[test]
fn settings_new_flags_default_and_round_trip() {
    let defaults = Settings::default();
    assert!(defaults.auto_backups);
    assert!(defaults.check_for_updates);
    assert!(!defaults.wizard_done);

    // A settings file written before these flags existed loads
    // with the same defaults.
    let old: Settings = serde_json::from_str(r#"{"tmdb_api_key":"k"}"#).unwrap();
    assert!(old.auto_backups);
    assert!(old.check_for_updates);
    assert!(!old.wizard_done);

    // And the flags persist through the store.
    let dir = temp_dir("settings-flags");
    let store = Store::open_at(&dir).unwrap();
    let settings = Settings {
        auto_backups: false,
        wizard_done: true,
        ..Default::default()
    };
    store.save_settings(&settings).unwrap();
    let loaded = store.load_settings().unwrap();
    assert_eq!(loaded, settings);
    let _ = fs::remove_dir_all(&dir);
}
