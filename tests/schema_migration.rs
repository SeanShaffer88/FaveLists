//! Schema-version migration fixtures (Adaptive Engineering Contract
//! V4.0 universal mandate: `schema_version` + tested upward
//! migrations). The two fixtures are byte-for-byte what v0.3.0
//! writes — recorded from the v0.3.0 code before the version field
//! existed, so neither carries a `schema_version`. They must load,
//! migrate to version 1, and round-trip at version 1; a file written
//! by a newer FaveLists must be refused and left untouched.

use favelists::model::{
    Category, ItemStatus, Library, Settings, LIBRARY_SCHEMA_VERSION, SETTINGS_SCHEMA_VERSION,
};
use favelists::store::Store;
use std::fs;
use std::path::PathBuf;

const SETTINGS_V030: &str = include_str!("fixtures/settings_v030.json");
const LIBRARY_V030: &str = include_str!("fixtures/library_v030.json");

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "favelists-schema-test-{}-{}-{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A copy of the fixture with a `schema_version` stamped in, as a
/// newer FaveLists would write it.
fn with_schema_version(fixture: &str, version: u32) -> String {
    let mut value: serde_json::Value = serde_json::from_str(fixture).unwrap();
    value["schema_version"] = serde_json::json!(version);
    serde_json::to_string_pretty(&value).unwrap()
}

#[test]
fn settings_v030_loads_migrates_and_round_trips_at_v1() {
    let dir = temp_dir("settings");
    let store = Store::open_at(&dir).unwrap();
    fs::write(store.settings_path(), SETTINGS_V030).unwrap();

    let settings = store.load_settings().unwrap();
    assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
    // The user's actual choices survive the migration untouched.
    assert_eq!(settings.cover_scale, 1.25);
    assert!(settings.rows_view);
    assert!(settings.wizard_done);
    assert_eq!(settings.drinks_last_check, 1_761_350_400);
    assert_eq!(
        settings.remembered_export_fields(Category::Movies),
        vec!["year".to_string(), "director".to_string()]
    );

    // The pristine v0.3.0 file is preserved beside the live one.
    let backup = dir.join("settings.json.v0-backup");
    assert_eq!(fs::read_to_string(&backup).unwrap(), SETTINGS_V030);

    // Saving stamps the current version; loading that file is a
    // no-op round-trip and never touches the preserved original.
    store.save_settings(&settings).unwrap();
    let on_disk = fs::read_to_string(store.settings_path()).unwrap();
    assert!(on_disk.contains("\"schema_version\": 1"));
    assert_eq!(store.load_settings().unwrap(), settings);
    assert_eq!(fs::read_to_string(&backup).unwrap(), SETTINGS_V030);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn library_v030_loads_migrates_and_round_trips_at_v1() {
    let dir = temp_dir("library");
    let store = Store::open_at(&dir).unwrap();
    fs::write(store.library_path(), LIBRARY_V030).unwrap();

    let library = store.load_library().unwrap();
    assert_eq!(library.schema_version, LIBRARY_SCHEMA_VERSION);
    assert_eq!(library.lists.len(), 1);
    let list = &library.lists[0];
    assert_eq!(list.name, "Best films");
    assert!(list.ranked);
    assert_eq!(list.items.len(), 2);
    assert_eq!(list.items[0].title, "Alien");
    assert_eq!(list.items[0].rating, Some(9));
    assert_eq!(list.items[0].status, Some(ItemStatus::Owned));
    assert_eq!(list.items[1].title, "The Princess Bride");

    let backup = dir.join("library.json.v0-backup");
    assert_eq!(fs::read_to_string(&backup).unwrap(), LIBRARY_V030);

    store.save_library(&library).unwrap();
    let on_disk = fs::read_to_string(store.library_path()).unwrap();
    assert!(on_disk.contains("\"schema_version\": 1"));
    assert_eq!(store.load_library().unwrap(), library);
    // The recovering load agrees and reports no journal recovery.
    let load = store.load_library_recovering().unwrap();
    assert!(!load.recovered_from_journal);
    assert_eq!(load.library, library);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn newer_settings_are_refused_and_never_overwritten() {
    let dir = temp_dir("settings-newer");
    let store = Store::open_at(&dir).unwrap();
    let newer = with_schema_version(SETTINGS_V030, 99);
    fs::write(store.settings_path(), &newer).unwrap();

    assert!(store.load_settings().is_err());
    // A caller holding defaults must not be able to clobber it.
    assert!(store.save_settings(&Settings::default()).is_err());
    assert_eq!(fs::read_to_string(store.settings_path()).unwrap(), newer);
    assert!(!dir.join("settings.json.v0-backup").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn newer_library_is_refused_by_both_load_paths() {
    let dir = temp_dir("library-newer");
    let store = Store::open_at(&dir).unwrap();
    let newer = with_schema_version(LIBRARY_V030, 99);
    fs::write(store.library_path(), &newer).unwrap();

    assert!(store.load_library().is_err());
    // Not a journal-recovery case either: it must surface, not fall
    // back to older state.
    assert!(store.load_library_recovering().is_err());
    assert!(store.save_library(&Library::default()).is_err());
    assert_eq!(fs::read_to_string(store.library_path()).unwrap(), newer);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn current_version_files_load_without_a_backup_copy() {
    let dir = temp_dir("current");
    let store = Store::open_at(&dir).unwrap();
    store.save_settings(&Settings::default()).unwrap();
    store.save_library(&Library::default()).unwrap();

    assert_eq!(
        store.load_settings().unwrap().schema_version,
        SETTINGS_SCHEMA_VERSION
    );
    assert_eq!(
        store.load_library().unwrap().schema_version,
        LIBRARY_SCHEMA_VERSION
    );
    // Nothing migrated, so nothing was preserved.
    assert!(!dir.join("settings.json.v0-backup").exists());
    assert!(!dir.join("library.json.v0-backup").exists());
    let _ = fs::remove_dir_all(&dir);
}
