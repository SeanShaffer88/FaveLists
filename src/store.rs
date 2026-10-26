//! Persistence: the library and settings live as JSON in a data folder that
//! defaults to `FaveListsData/` NEXT TO THE EXE (spec §5). A tiny pointer
//! file (`favelists-location.json`) sits next to the exe; when the user
//! relocates the data folder, the pointer is rewritten and all existing data
//! is moved automatically.

use crate::model::{Library, Settings};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DATA_DIR_NAME: &str = "FaveListsData";
pub const LIBRARY_FILE: &str = "library.json";
/// Autosave journal (v0.2 program #4): a shadow copy of the library,
/// rewritten atomically on every [`Store::save_library`]. It is only ever
/// *read* by [`Store::load_library_recovering`], and only when the main
/// library file is missing or fails to parse — a parseable main file
/// always wins, so the journal can never resurrect older state over newer.
pub const JOURNAL_FILE: &str = "library.journal.json";
pub const SETTINGS_FILE: &str = "settings.json";
pub const COVERS_DIR: &str = "covers";
/// Automatic backups (v0.3 #19) live here, under the data folder.
pub const BACKUPS_DIR: &str = "backups";
pub const DRINKS_FILE: &str = "drinks.json";
const POINTER_FILE: &str = "favelists-location.json";

#[derive(Debug, Serialize, Deserialize)]
struct LocationPointer {
    data_dir: String,
}

/// Directory containing the running executable (or the current dir as a
/// fallback, e.g. in tests and `cargo run` from odd places).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve the data folder: pointer file if present and valid, else the
/// default next to the exe.
pub fn resolve_data_dir(base_dir: &Path) -> PathBuf {
    let pointer = base_dir.join(POINTER_FILE);
    if let Ok(text) = fs::read_to_string(&pointer) {
        if let Ok(p) = serde_json::from_str::<LocationPointer>(&text) {
            let dir = PathBuf::from(p.data_dir);
            if !dir.as_os_str().is_empty() {
                return dir;
            }
        }
    }
    base_dir.join(DATA_DIR_NAME)
}

#[derive(Debug)]
pub struct Store {
    pub data_dir: PathBuf,
}

/// The outcome of [`Store::load_library_recovering`]: the library, plus
/// whether it came from the autosave journal because the main library
/// file was missing or corrupt.
#[derive(Debug, Default)]
pub struct LibraryLoad {
    pub library: Library,
    pub recovered_from_journal: bool,
}

impl Store {
    /// Open (creating if needed) the store rooted at `base_dir` — the exe
    /// folder in production, a temp folder in tests.
    pub fn open(base_dir: &Path) -> io::Result<Store> {
        let data_dir = resolve_data_dir(base_dir);
        let store = Store { data_dir };
        store.ensure_dirs()?;
        Ok(store)
    }

    /// Open a store at an explicit data dir (tests, import/export helpers).
    pub fn open_at(data_dir: &Path) -> io::Result<Store> {
        let store = Store {
            data_dir: data_dir.to_path_buf(),
        };
        store.ensure_dirs()?;
        Ok(store)
    }

    fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(self.covers_dir())?;
        Ok(())
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.data_dir.join(COVERS_DIR)
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join(BACKUPS_DIR)
    }

    pub fn library_path(&self) -> PathBuf {
        self.data_dir.join(LIBRARY_FILE)
    }

    pub fn journal_path(&self) -> PathBuf {
        self.data_dir.join(JOURNAL_FILE)
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_dir.join(SETTINGS_FILE)
    }

    pub fn drinks_path(&self) -> PathBuf {
        self.data_dir.join(DRINKS_FILE)
    }

    pub fn load_library(&self) -> io::Result<Library> {
        let mut library = load_json_or_default(&self.library_path())?;
        purge_trash(&mut library);
        Ok(library)
    }

    /// Load the library, recovering from the autosave journal when the
    /// main file is missing or unparseable. The result says whether
    /// recovery happened so the surface can tell the user (the GUI puts it
    /// in the status line). Main-file-wins: when the main file parses, the
    /// journal is never consulted. When both are unusable, the main file's
    /// error is returned (a brand-new store — neither file present — loads
    /// as a default library with no recovery flag).
    pub fn load_library_recovering(&self) -> io::Result<LibraryLoad> {
        let mut main_error: Option<io::Error> = None;
        match fs::read_to_string(self.library_path()) {
            Ok(text) => match serde_json::from_str::<Library>(&text) {
                Ok(mut library) => {
                    purge_trash(&mut library);
                    return Ok(LibraryLoad {
                        library,
                        recovered_from_journal: false,
                    });
                }
                Err(e) => {
                    main_error = Some(io::Error::new(io::ErrorKind::InvalidData, e));
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => main_error = Some(e),
        }
        // The main file is missing or unusable: try the journal.
        if let Ok(text) = fs::read_to_string(self.journal_path()) {
            if let Ok(mut library) = serde_json::from_str::<Library>(&text) {
                purge_trash(&mut library);
                return Ok(LibraryLoad {
                    library,
                    recovered_from_journal: true,
                });
            }
        }
        match main_error {
            Some(e) => Err(e),
            None => Ok(LibraryLoad::default()),
        }
    }

    /// Save the library and, through this same funnel, refresh the
    /// autosave journal with the identical state. Both the GUI's
    /// `persist()` and the JSON channel's post-mutation save come through
    /// here, so the journal can never miss a state the main file has.
    pub fn save_library(&self, lib: &Library) -> io::Result<()> {
        save_json_atomic(&self.library_path(), lib)?;
        save_json_atomic(&self.journal_path(), lib)
    }

    pub fn load_settings(&self) -> io::Result<Settings> {
        load_json_or_default(&self.settings_path())
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        save_json_atomic(&self.settings_path(), settings)
    }

    /// Move the whole data folder to `new_dir`, and record the new location
    /// in the pointer file in `base_dir` (the exe folder). Existing data —
    /// library, settings, covers, drinks DB — is moved, never copied-and-left
    /// (a copy is made only as a fallback when a rename across filesystems
    /// fails, and the original is removed after a successful copy).
    pub fn relocate(&mut self, base_dir: &Path, new_dir: &Path) -> io::Result<()> {
        if new_dir == self.data_dir {
            return Ok(());
        }
        if new_dir.exists() && fs::read_dir(new_dir)?.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "target folder is not empty",
            ));
        }
        fs::create_dir_all(new_dir)?;
        move_dir_contents(&self.data_dir, new_dir)?;
        // Best effort: drop the now-empty old folder.
        let _ = fs::remove_dir(&self.data_dir);
        let pointer = LocationPointer {
            data_dir: new_dir.to_string_lossy().into_owned(),
        };
        save_json_atomic(&base_dir.join(POINTER_FILE), &pointer)?;
        self.data_dir = new_dir.to_path_buf();
        self.ensure_dirs()?;
        Ok(())
    }
}

/// Drop trash records past the 30-day retention window (v0.3 #10)
/// at load time, so an expired record never comes back from disk.
/// The drop persists on the next save; cover files are untouched
/// (a copy of the item may share them).
fn purge_trash(library: &mut Library) {
    crate::trash::purge_expired(&mut library.trash, crate::model::now_unix());
}

fn load_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> io::Result<T> {
    match fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e),
    }
}

/// Write JSON via a temp file + rename so a crash mid-save can't corrupt the
/// previous good file.
pub fn save_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    #[cfg(windows)]
    {
        // rename over an existing file fails on Windows; remove first.
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn move_dir_contents(from: &Path, to: &Path) -> io::Result<()> {
    if !from.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        let ty = entry.file_type()?;
        if ty.is_dir() {
            fs::create_dir_all(&dest)?;
            move_dir_contents(&entry.path(), &dest)?;
            let _ = fs::remove_dir(entry.path());
        } else if fs::rename(entry.path(), &dest).is_err() {
            fs::copy(entry.path(), &dest)?;
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// A unique cover file name for an item: stable, filesystem-safe.
pub fn cover_file_name(item_id: u64, source_url_or_path: &str) -> String {
    let ext = Path::new(source_url_or_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .filter(|e| ["jpg", "jpeg", "png", "webp", "gif"].contains(&e.as_str()))
        .unwrap_or_else(|| "jpg".to_string());
    format!("cover-{item_id}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Category;

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "favelists-test-{}-{}-{}",
            tag,
            std::process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip_library_and_settings() {
        let base = temp_dir("roundtrip");
        let store = Store::open(&base).unwrap();
        let mut lib = Library::default();
        let id = lib.new_list("Films", Category::Movies, true);
        let iid = lib.alloc_item_id();
        let mut it = crate::model::Item::new(iid, "Alien");
        it.fields.insert("year".into(), "1979".into());
        lib.list_mut(id).unwrap().add_item(it);
        store.save_library(&lib).unwrap();
        let settings = Settings {
            tmdb_api_key: "abc".into(),
            ..Default::default()
        };
        store.save_settings(&settings).unwrap();

        let store2 = Store::open(&base).unwrap();
        let loaded = store2.load_library().unwrap();
        assert_eq!(loaded, lib);
        assert_eq!(store2.load_settings().unwrap().tmdb_api_key, "abc");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn default_data_dir_is_next_to_base() {
        let base = temp_dir("defaultdir");
        let store = Store::open(&base).unwrap();
        assert_eq!(store.data_dir, base.join(DATA_DIR_NAME));
        assert!(store.covers_dir().is_dir());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn relocate_moves_data_and_writes_pointer() {
        let base = temp_dir("reloc-base");
        let mut store = Store::open(&base).unwrap();
        let mut lib = Library::default();
        lib.new_list("Sodas", Category::Sodas, false);
        store.save_library(&lib).unwrap();
        fs::write(store.covers_dir().join("cover-1.jpg"), b"img").unwrap();

        let new_dir = temp_dir("reloc-new");
        let _ = fs::remove_dir_all(&new_dir);
        store.relocate(&base, &new_dir).unwrap();

        assert!(!base.join(DATA_DIR_NAME).join(LIBRARY_FILE).exists());
        let reopened = Store::open(&base).unwrap();
        assert_eq!(reopened.data_dir, new_dir);
        assert_eq!(reopened.load_library().unwrap().lists.len(), 1);
        assert!(new_dir.join(COVERS_DIR).join("cover-1.jpg").exists());
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&new_dir);
    }

    #[test]
    fn relocate_refuses_non_empty_target() {
        let base = temp_dir("reloc2-base");
        let mut store = Store::open(&base).unwrap();
        let target = temp_dir("reloc2-target");
        fs::write(target.join("something.txt"), b"x").unwrap();
        assert!(store.relocate(&base, &target).is_err());
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn cover_file_names_are_safe() {
        assert_eq!(cover_file_name(7, "https://x/y/poster.JPG"), "cover-7.jpg");
        assert_eq!(cover_file_name(7, "no-extension"), "cover-7.jpg");
        assert_eq!(cover_file_name(3, "a/b/pic.png"), "cover-3.png");
    }
}
