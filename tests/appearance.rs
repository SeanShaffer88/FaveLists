//! Integration tests for v0.3 Slice 5: text size (#14), grid
//! density + rows view (#15), high contrast (#16) and drop-a-cover
//! helpers (#17).
//!
//! The pure appearance logic is unit-tested in `src/appearance.rs`;
//! what lives here is the persistence contract — an old settings
//! file loads with the new fields at their defaults (Large keeps the
//! v0.2.1 fixed 1.3x), the cover scale is clamped on load, and a
//! save/load round-trip through the real store preserves every new
//! field — plus the drop-path helpers in `src/app_drop.rs`.

use favelists::app_drop::{cover_ext_for_path, pasted_cover_path, target_at};
use favelists::appearance::{self, TextSize};
use favelists::model::Settings;
use favelists::store::Store;
use std::path::{Path, PathBuf};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "favelists-appearance-{}-{}",
        std::process::id(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn text_size_defaults_to_large_and_round_trips_ids() {
    assert_eq!(TextSize::default(), TextSize::Large);
    assert_eq!(TextSize::default().factor(), 1.3);
    for size in TextSize::ALL {
        assert_eq!(TextSize::from_id(size.id()), Some(size));
        // The settings file stores the id string.
        let json = serde_json::to_string(&size).unwrap();
        assert_eq!(json, format!("\"{}\"", size.id()));
        assert_eq!(serde_json::from_str::<TextSize>(&json).unwrap(), size);
    }
}

#[test]
fn old_settings_json_loads_appearance_defaults() {
    // A v0.2.1-era settings file: none of the Slice 5 fields exist.
    let json = r#"{
        "tmdb_api_key": "key",
        "igdb_client_id": "",
        "igdb_client_secret": "",
        "drinks_update_url": "https://example.invalid/drinks.json",
        "drinks_auto_update": true,
        "drinks_last_check": 42,
        "export_fields": {}
    }"#;
    let settings: Settings = serde_json::from_str(json).unwrap();
    // Large == the fixed 1.3x those users already had.
    assert_eq!(settings.text_size, TextSize::Large);
    assert_eq!(settings.cover_scale, 1.0);
    assert!(!settings.rows_view);
    assert!(!settings.high_contrast);
    // Pre-existing fields are untouched by the new defaults.
    assert_eq!(settings.tmdb_api_key, "key");
    assert_eq!(settings.drinks_last_check, 42);
}

#[test]
fn cover_scale_is_clamped_on_load() {
    // The app's load path applies Settings::sanitized (FaveApp::new).
    let mut settings = Settings {
        cover_scale: 99.0,
        ..Settings::default()
    };
    settings = settings.sanitized();
    assert_eq!(settings.cover_scale, appearance::COVER_SCALE_MAX);

    let mut settings = Settings {
        cover_scale: 0.1,
        ..Settings::default()
    };
    settings = settings.sanitized();
    assert_eq!(settings.cover_scale, appearance::COVER_SCALE_MIN);

    // A hand-edited non-finite value falls back to the default.
    let settings = Settings {
        cover_scale: f32::NAN,
        ..Settings::default()
    }
    .sanitized();
    assert_eq!(settings.cover_scale, appearance::COVER_SCALE_DEFAULT);

    // An in-range value passes through untouched.
    let settings = Settings {
        cover_scale: 1.25,
        ..Settings::default()
    }
    .sanitized();
    assert_eq!(settings.cover_scale, 1.25);
}

#[test]
fn settings_round_trip_through_the_store_preserves_appearance() {
    let base = temp_dir("round-trip");
    let store = Store::open(&base).unwrap();
    let settings = Settings {
        text_size: TextSize::ExtraLarge,
        cover_scale: 1.25,
        rows_view: true,
        high_contrast: true,
        ..Settings::default()
    };
    store.save_settings(&settings).unwrap();

    let store2 = Store::open(&base).unwrap();
    let loaded = store2.load_settings().unwrap().sanitized();
    assert_eq!(loaded.text_size, TextSize::ExtraLarge);
    assert_eq!(loaded.cover_scale, 1.25);
    assert!(loaded.rows_view);
    assert!(loaded.high_contrast);
    assert_eq!(loaded, settings);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn drop_extension_classification() {
    let ok = [
        ("cover.jpg", "jpg"),
        ("cover.JPG", "jpg"),
        ("photo.jpeg", "jpg"),
        ("photo.JPEG", "jpg"),
        ("art.png", "png"),
        ("art.webp", "webp"),
        ("/some/dir/pic.PNG", "png"),
        ("C:\\pics\\poster.webp", "webp"),
    ];
    for (path, expected) in ok {
        assert_eq!(
            cover_ext_for_path(Path::new(path)),
            Some(expected),
            "path: {path}"
        );
    }
    for path in [
        "anim.gif",
        "notes.txt",
        "no-extension",
        "archive.zip",
        "cover.jpg.bak",
        ".hidden",
    ] {
        assert_eq!(cover_ext_for_path(Path::new(path)), None, "path: {path}");
    }
}

#[test]
fn pasted_cover_path_classification() {
    assert_eq!(
        pasted_cover_path("  /tmp/poster.jpg  "),
        Some(PathBuf::from("/tmp/poster.jpg"))
    );
    assert_eq!(
        pasted_cover_path("C:\\pics\\cover.png"),
        Some(PathBuf::from("C:\\pics\\cover.png"))
    );
    // Not single image paths: multi-line text, non-images, blanks.
    assert_eq!(pasted_cover_path("/tmp/a.jpg\n/tmp/b.jpg"), None);
    assert_eq!(pasted_cover_path("/tmp/notes.txt"), None);
    assert_eq!(pasted_cover_path("just some text"), None);
    assert_eq!(pasted_cover_path("   "), None);
}

#[test]
fn drop_target_resolution() {
    let targets = vec![
        (
            7u64,
            eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(100.0, 50.0),
            ),
        ),
        (
            9u64,
            eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(100.0, 0.0),
                eframe::egui::vec2(100.0, 50.0),
            ),
        ),
    ];
    assert_eq!(target_at(&targets, eframe::egui::pos2(50.0, 25.0)), Some(7));
    assert_eq!(
        target_at(&targets, eframe::egui::pos2(150.0, 25.0)),
        Some(9)
    );
    assert_eq!(target_at(&targets, eframe::egui::pos2(50.0, 80.0)), None);
    assert_eq!(target_at(&[], eframe::egui::pos2(0.0, 0.0)), None);
    // Overlapping targets: the first recorded one wins.
    let overlapping = vec![
        (
            1u64,
            eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(100.0, 100.0),
            ),
        ),
        (
            2u64,
            eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(10.0, 10.0),
                eframe::egui::vec2(100.0, 100.0),
            ),
        ),
    ];
    assert_eq!(
        target_at(&overlapping, eframe::egui::pos2(50.0, 50.0)),
        Some(1)
    );
}
