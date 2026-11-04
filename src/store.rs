//! Persistence: the library and settings live as JSON in a data folder that
//! defaults to `FaveListsData/` NEXT TO THE EXE (spec §5). Both files carry
//! a `schema_version` (v0.4.0): files written before the stamp are version
//! 0, migrate up on load with their original bytes preserved beside them
//! (`<file>.v0-backup`), and a file from a newer FaveLists is refused
//! rather than mangled. A tiny pointer
//! file (`favelists-location.json`) sits next to the exe; when the user
//! relocates the data folder, the pointer is rewritten and all existing data
//! is moved automatically.

use crate::model::{Library, Settings, LIBRARY_SCHEMA_VERSION, SETTINGS_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
pub fn resolve_data_dir(base_dir: &Path) -> io::Result<PathBuf> {
    match fs::read(base_dir.join(POINTER_FILE)) {
        Ok(bytes) => {
            let pointer: LocationPointer = serde_json::from_slice(&bytes).map_err(invalid_data)?;
            let path = PathBuf::from(pointer.data_dir);
            if !path.is_absolute() {
                return Err(invalid_data("invalid data location pointer"));
            }
            if !path.is_dir() {
                return Err(invalid_data(
                    "recorded data folder is unavailable; restore it before opening FaveLists",
                ));
            }
            Ok(path)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(base_dir.join(DATA_DIR_NAME)),
        Err(e) => Err(e),
    }
}

const OWNERSHIP_FILE: &str = ".favelists-owner";

#[derive(Debug, Clone)]
/// Clones share one serialized storage session and retain exclusive ownership.
/// Open acquires ownership before loading; operations resolve paths under the
/// session guard. Dropping the last clone releases ownership without deleting
/// the persistent ownership file. Importing this module starts no work.
pub struct Store {
    session: Arc<Mutex<StorageState>>,
}

#[derive(Debug)]
pub(crate) struct StorageState {
    data_dir: PathBuf,
    base_dir: PathBuf,
    ownership: fs::File,
    library_error: Option<String>,
    settings_error: Option<String>,
}

#[derive(Debug)]
pub struct RelocationOutcome {
    pub cleanup_warning: Option<String>,
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn acquire_ownership(path: &Path) -> io::Result<fs::File> {
    reject_links(&path.join(OWNERSHIP_FILE))?;
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let file = options.open(path.join(OWNERSHIP_FILE)).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("Cannot own data folder; close the other FaveLists GUI or JSON channel: {e}"),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        extern "C" {
            fn flock(
                fd: std::os::raw::c_int,
                operation: std::os::raw::c_int,
            ) -> std::os::raw::c_int;
        }
        // LOCK_EX | LOCK_NB; the kernel releases ownership with the last handle.
        if unsafe { flock(file.as_raw_fd(), 2 | 4) } != 0 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Data folder is owned by another FaveLists GUI or JSON channel",
            ));
        }
    }
    #[cfg(not(any(windows, unix)))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "exclusive storage ownership unavailable",
    ));
    Ok(file)
}

impl Store {
    pub fn open(base_dir: &Path) -> io::Result<Self> {
        Self::open_at(&resolve_data_dir(base_dir)?, base_dir)
    }

    pub fn open_at(data_dir: &Path, base_dir: &Path) -> io::Result<Self> {
        let base_dir = fs::canonicalize(base_dir)?;
        reject_links(data_dir)?;
        fs::create_dir_all(data_dir)?;
        let data_dir = fs::canonicalize(data_dir)?;
        let ownership = acquire_ownership(&data_dir)?;
        fs::create_dir_all(data_dir.join(COVERS_DIR))?;
        Ok(Self {
            session: Arc::new(Mutex::new(StorageState {
                data_dir,
                base_dir,
                ownership,
                library_error: None,
                settings_error: None,
            })),
        })
    }

    pub(crate) fn with_storage<R>(&self, operation: impl FnOnce(&mut StorageState) -> R) -> R {
        operation(&mut self.session.lock().expect("storage session poisoned"))
    }

    pub fn dispatch(
        &self,
        library: &mut Library,
        history: Option<&mut crate::commands::History>,
        name: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, crate::commands::CommandError> {
        self.with_storage(|state| {
            state
                .require_library_writable()
                .map_err(crate::commands::CommandError::io)?;
            let covers = state.covers_dir();
            let mut context = crate::commands::CommandContext::new(library, &covers);
            context.history = history;
            context.attach_storage(state);
            crate::commands::dispatch(&mut context, name, params)
        })
    }

    pub fn data_dir_label(&self) -> String {
        self.with_storage(|state| state.data_dir.display().to_string())
    }

    pub fn load_library(&self) -> io::Result<Library> {
        self.load_library_recovering().map(|load| load.library)
    }

    pub fn load_library_recovering(&self) -> io::Result<LibraryLoad> {
        self.with_storage(|state| {
            let result = state.load_library_recovering();
            state.library_error = match &result {
                Ok(load) => load.write_error.clone(),
                Err(error) => Some(error.to_string()),
            };
            result
        })
    }

    pub fn load_settings(&self) -> io::Result<Settings> {
        self.with_storage(|state| {
            let result = state.load_settings();
            state.settings_error = result.as_ref().err().map(ToString::to_string);
            result
        })
    }

    pub fn save_library(&self, library: &Library) -> io::Result<()> {
        self.with_storage(|state| state.save_library(library))
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        self.with_storage(|state| state.save_settings(settings))
    }

    pub fn relocate(&mut self, base_dir: &Path, new_dir: &Path) -> io::Result<RelocationOutcome> {
        self.with_storage(|state| state.relocate(base_dir, new_dir))
    }
}

/// The outcome of [`Store::load_library_recovering`]: the library, plus
/// whether it came from the autosave journal because the main library
/// file was missing or corrupt.
#[derive(Debug, Default)]
pub struct LibraryLoad {
    pub library: Library,
    pub recovered_from_journal: bool,
    pub write_error: Option<String>,
}

// The borrow keeps validation and all writes inside the storage guard. External
// filesystem races remain possible; this is not a pinned-handle filesystem API.
pub(crate) struct ValidatedOutputs<'a> {
    state: &'a StorageState,
    paths: Vec<PathBuf>,
}

impl ValidatedOutputs<'_> {
    pub(crate) fn write(&self, index: usize, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        self.with_file(index, |file| file.write_all(bytes.as_ref()))
    }

    pub(crate) fn with_file<R>(
        &self,
        index: usize,
        write: impl FnOnce(&mut fs::File) -> io::Result<R>,
    ) -> io::Result<R> {
        self.state.check_external_outputs(&self.paths)?;
        let mut file = fs::File::create(&self.paths[index])?;
        write(&mut file)
    }

    pub(crate) fn create_dir(&self, index: usize) -> io::Result<()> {
        self.state.check_external_outputs(&self.paths)?;
        fs::create_dir_all(&self.paths[index])
    }

    pub(crate) fn copy(&self, index: usize, source: &Path) -> io::Result<()> {
        self.with_file(index, |dest| {
            let mut source = fs::File::open(source)?;
            io::copy(&mut source, dest).map(|_| ())
        })
    }
}

#[derive(PartialEq, Eq)]
struct FileIdentity(u64, u64);

#[cfg(windows)]
fn file_identity(file: &fs::File) -> io::Result<FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct Information {
        attributes: u32,
        times: [u32; 6],
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut Information)
            -> i32;
    }
    let mut info = std::mem::MaybeUninit::<Information>::uninit();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    Ok(FileIdentity(
        info.volume as u64,
        ((info.index_high as u64) << 32) | info.index_low as u64,
    ))
}

#[cfg(unix)]
fn file_identity(file: &fs::File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(FileIdentity(metadata.dev(), metadata.ino()))
}

#[cfg(not(any(windows, unix)))]
fn file_identity(_: &fs::File) -> io::Result<FileIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native file identity unavailable",
    ))
}

fn path_identity(path: &Path) -> io::Result<Option<FileIdentity>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0)
            .share_mode(7)
            .custom_flags(0x02000000); // FILE_FLAG_BACKUP_SEMANTICS
    }
    match options.open(path) {
        Ok(file) => file_identity(&file).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn same_component(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn CompareStringOrdinal(
                a: *const u16,
                an: i32,
                b: *const u16,
                bn: i32,
                ignore_case: i32,
            ) -> i32;
        }
        let a: Vec<u16> = a.encode_wide().collect();
        let b: Vec<u16> = b.encode_wide().collect();
        unsafe {
            CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1) == 2
        }
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

fn within(path: &Path, root: &Path) -> bool {
    let mut components = path.components();
    root.components().all(|part| {
        components
            .next()
            .is_some_and(|c| same_component(c.as_os_str(), part.as_os_str()))
    })
}

fn resolved_output(path: &Path, cwd: &Path) -> io::Result<PathBuf> {
    use std::path::Component;
    #[cfg(windows)]
    for part in path.components() {
        if let Component::Normal(name) = part {
            let name = name
                .to_str()
                .ok_or_else(|| invalid_data("non-Unicode Windows export path"))?;
            if name.contains(':') || name.ends_with(['.', ' ']) {
                return Err(invalid_data("ambiguous Windows export component"));
            }
        }
        if let Component::Prefix(prefix) = part {
            if !matches!(
                prefix.kind(),
                std::path::Prefix::Disk(_)
                    | std::path::Prefix::UNC(_, _)
                    | std::path::Prefix::VerbatimDisk(_)
                    | std::path::Prefix::VerbatimUNC(_, _)
            ) {
                return Err(invalid_data("unsupported Windows export namespace"));
            }
        }
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let components: Vec<_> = absolute.components().collect();
    if let Some(index) = components
        .iter()
        .position(|c| matches!(c, Component::ParentDir))
    {
        let prefix: PathBuf = components[..index].iter().map(|c| c.as_os_str()).collect();
        let mut resolved = resolve_existing_ancestor(prefix)?;
        for part in &components[index..] {
            match part {
                Component::ParentDir => {
                    resolved.pop();
                }
                Component::CurDir => (),
                Component::Normal(name) => {
                    resolved.push(name);
                    resolved = resolve_existing_ancestor(resolved)?;
                }
                _ => return Err(invalid_data("unsupported export suffix")),
            }
        }
        return Ok(resolved);
    }
    resolve_existing_ancestor(absolute)
}

fn resolve_existing_ancestor(absolute: PathBuf) -> io::Result<PathBuf> {
    use std::path::Component;
    let mut ancestor = absolute;
    let mut suffix = Vec::new();
    let mut result = loop {
        match fs::canonicalize(&ancestor) {
            Ok(canonical) => break canonical,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let component = ancestor
                    .components()
                    .next_back()
                    .ok_or_else(|| invalid_data("unresolved export ancestor"))?;
                suffix.push(component.as_os_str().to_os_string());
                if !ancestor.pop() {
                    return Err(invalid_data("unresolved export root"));
                }
                if suffix.len() > 256 {
                    return Err(invalid_data("export path exceeds resolution limit"));
                }
            }
            Err(e) => {
                return Err(invalid_data(format!(
                    "resolve export {}: {e}",
                    ancestor.display()
                )))
            }
        }
    };
    for part in suffix.iter().rev() {
        match Path::new(part).components().next() {
            Some(Component::ParentDir) => {
                result.pop();
            }
            Some(Component::CurDir) => (),
            Some(Component::Normal(_)) => result.push(part),
            _ => return Err(invalid_data("unsupported missing export suffix")),
        }
    }
    Ok(result)
}

fn collect_managed_identities(
    path: &Path,
    identities: &mut Vec<FileIdentity>,
    budget: &mut usize,
) -> io::Result<()> {
    if *budget == 0 {
        return Err(invalid_data(
            "managed alias inventory exceeds 10000 entries",
        ));
    }
    *budget -= 1;
    if let Some(identity) = path_identity(path)? {
        identities.push(identity);
        let metadata = fs::symlink_metadata(path)?;
        // Refuse unresolved managed links rather than making claims about their contents.
        reject_links(path)?;
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                collect_managed_identities(&entry?.path(), identities, budget)?;
            }
        }
    }
    Ok(())
}

impl StorageState {
    pub(crate) fn validate_external_outputs(
        &self,
        outputs: &[PathBuf],
    ) -> io::Result<ValidatedOutputs<'_>> {
        let cwd = std::env::current_dir()?;
        let paths = outputs
            .iter()
            .map(|p| resolved_output(p, &cwd))
            .collect::<io::Result<Vec<_>>>()?;
        self.check_external_outputs(&paths)?;
        Ok(ValidatedOutputs { state: self, paths })
    }

    fn check_external_outputs(&self, outputs: &[PathBuf]) -> io::Result<()> {
        let roots = [self.covers_dir(), self.backups_dir()];
        let mut reserved: Vec<PathBuf> = [
            LIBRARY_FILE,
            JOURNAL_FILE,
            SETTINGS_FILE,
            DRINKS_FILE,
            OWNERSHIP_FILE,
            "library.json.v0-backup",
            "library.journal.json.v0-backup",
            "settings.json.v0-backup",
            "library.json.corrupt-backup",
        ]
        .iter()
        .map(|name| self.data_dir.join(name))
        .collect();
        reserved.push(self.base_dir.join(POINTER_FILE));
        let mut identities = vec![file_identity(&self.ownership)?];
        let mut budget = 10000;
        for path in reserved
            .iter()
            .filter(|p| p.file_name() != Some(std::ffi::OsStr::new(OWNERSHIP_FILE)))
            .chain(roots.iter())
        {
            collect_managed_identities(path, &mut identities, &mut budget).map_err(|e| {
                invalid_data(format!("managed alias inventory {}: {e}", path.display()))
            })?;
        }
        // Include existing atomic temporary files, including hardlinks outside data.
        for dir in [&self.data_dir, &self.base_dir] {
            for entry in fs::read_dir(dir)? {
                let path = entry?.path();
                if reserved.iter().any(|r| temporary_for(&path, r)) {
                    collect_managed_identities(&path, &mut identities, &mut budget)?;
                }
            }
        }
        for output in outputs {
            let current = resolved_output(output, Path::new(""))?;
            if !within(&current, output) || !within(output, &current) {
                return Err(invalid_data("export destination changed since validation"));
            }
            if roots
                .iter()
                .any(|root| within(output, root) || within(root, output))
                || reserved
                    .iter()
                    .any(|p| within(p, output) || temporary_for(output, p))
            {
                return Err(invalid_data(
                    "export destination is reserved for managed storage",
                ));
            }
            if let Some(identity) = path_identity(output)? {
                if identities.contains(&identity) {
                    return Err(invalid_data("export destination aliases managed storage"));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn internal_backup_path(&self, name: &str) -> io::Result<PathBuf> {
        let digits = name
            .strip_prefix("auto-")
            .and_then(|s| s.strip_suffix(".zip"));
        if !digits.is_some_and(|s| {
            s.len() == 15
                && s.as_bytes()[8] == b'-'
                && s.bytes()
                    .enumerate()
                    .all(|(i, c)| i == 8 || c.is_ascii_digit())
        }) {
            return Err(invalid_data("invalid internal backup basename"));
        }
        Ok(self.backups_dir().join(name))
    }

    pub(crate) fn write_internal_backup(&self, library: &Library, name: &str) -> io::Result<()> {
        self.require_library_writable()?;
        let path = self.internal_backup_path(name)?;
        reject_links(&self.backups_dir())?;
        fs::create_dir_all(self.backups_dir())?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        crate::backup::encode_backup(
            library,
            &crate::backup::BackupScope::WholeLibrary,
            &self.covers_dir(),
            &mut file,
        )?;
        Ok(())
    }
}

fn temporary_for(path: &Path, reserved: &Path) -> bool {
    if path
        .parent()
        .zip(reserved.parent())
        .map_or(true, |(a, b)| !within(a, b) || !within(b, a))
    {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let Some(base) = reserved.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let parts: Vec<_> = name.rsplitn(4, '.').collect();
    parts.len() == 4
        && parts[0] == "tmp"
        && !parts[1].is_empty()
        && parts[1].bytes().all(|b| b.is_ascii_digit())
        && !parts[2].is_empty()
        && parts[2].bytes().all(|b| b.is_ascii_digit())
        && same_component(parts[3].as_ref(), base.as_ref())
}

impl StorageState {
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

    /// Parse library JSON at any schema version this build
    /// understands. A missing `schema_version` is version 0 (every
    /// release up to v0.3.0): the data migrates up in memory, the
    /// original file is first preserved beside itself (see
    /// [`preserve_pre_migration_copy`]), and the migration becomes
    /// permanent on the next save. A version newer than this build
    /// understands is refused, never mangled.
    fn parse_library(&self, text: &str, path: &Path) -> io::Result<Library> {
        let declared = check_not_newer(text, LIBRARY_SCHEMA_VERSION, LIBRARY_FILE)?;
        let mut library: Library = parse_object(text)?;
        if declared < LIBRARY_SCHEMA_VERSION {
            preserve_pre_migration_copy(path)?;
            library.schema_version = LIBRARY_SCHEMA_VERSION;
        }
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
        let main = self.library_path();
        let journal = self.journal_path();
        let main_error = match fs::read_to_string(&main) {
            Ok(text) => {
                check_not_newer(&text, LIBRARY_SCHEMA_VERSION, LIBRARY_FILE)?;
                match parse_object::<Library>(&text) {
                    Ok(mut library) => {
                        if check_not_newer(&text, LIBRARY_SCHEMA_VERSION, LIBRARY_FILE)?
                            < LIBRARY_SCHEMA_VERSION
                        {
                            preserve_pre_migration_copy(&main)?;
                            library.schema_version = LIBRARY_SCHEMA_VERSION;
                        }
                        purge_trash(&mut library);
                        return Ok(LibraryLoad {
                            library,
                            recovered_from_journal: false,
                            write_error: preflight::<Library>(
                                &journal,
                                LIBRARY_SCHEMA_VERSION,
                                JOURNAL_FILE,
                            )
                            .err()
                            .map(|e| format!("Journal is write protected: {e}")),
                        });
                    }
                    Err(e) => Some(e),
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => Some(e),
        };
        match fs::read_to_string(&journal) {
            Ok(text) => {
                let mut library = self.parse_library(&text, &journal)?;
                if main_error.is_some() {
                    preserve_original(&main, "corrupt-backup")?;
                }
                // Repair only after preserving the original, under session ownership.
                save_json_atomic(&main, &library)?;
                purge_trash(&mut library);
                Ok(LibraryLoad {
                    library,
                    recovered_from_journal: true,
                    write_error: None,
                })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => match main_error {
                Some(e) => Err(e),
                None => Ok(LibraryLoad::default()),
            },
            Err(e) => Err(e),
        }
    }

    /// Save the library and, through this same funnel, refresh the
    /// autosave journal with the identical state. Both the GUI's
    /// `persist()` and the JSON channel's post-mutation save come through
    /// here, so the journal can never miss a state the main file has.
    pub fn save_library(&self, lib: &Library) -> io::Result<()> {
        self.require_library_writable()?;
        preflight::<Library>(&self.library_path(), LIBRARY_SCHEMA_VERSION, LIBRARY_FILE)?;
        preflight::<Library>(&self.journal_path(), LIBRARY_SCHEMA_VERSION, JOURNAL_FILE)?;
        save_json_atomic(&self.library_path(), lib)?;
        save_json_atomic(&self.journal_path(), lib)
    }

    pub fn load_settings(&self) -> io::Result<Settings> {
        let path = self.settings_path();
        match fs::read_to_string(&path) {
            Ok(text) => {
                let declared = check_not_newer(&text, SETTINGS_SCHEMA_VERSION, SETTINGS_FILE)?;
                let mut settings: Settings = parse_object(&text)?;
                if declared < SETTINGS_SCHEMA_VERSION {
                    preserve_pre_migration_copy(&path)?;
                    settings.schema_version = SETTINGS_SCHEMA_VERSION;
                }
                Ok(settings)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        if let Some(error) = &self.settings_error {
            return Err(invalid_data(error));
        }
        preflight::<Settings>(
            &self.settings_path(),
            SETTINGS_SCHEMA_VERSION,
            SETTINGS_FILE,
        )?;
        save_json_atomic(&self.settings_path(), settings)
    }

    pub(crate) fn require_library_writable(&self) -> io::Result<()> {
        if let Some(error) = &self.library_error {
            return Err(invalid_data(error));
        }
        preflight::<Library>(&self.library_path(), LIBRARY_SCHEMA_VERSION, LIBRARY_FILE)?;
        preflight::<Library>(&self.journal_path(), LIBRARY_SCHEMA_VERSION, JOURNAL_FILE)
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn relocate(&mut self, base_dir: &Path, new_dir: &Path) -> io::Result<RelocationOutcome> {
        if fs::canonicalize(base_dir)? != self.base_dir {
            return Err(invalid_data(
                "relocation pointer base differs from session base",
            ));
        }
        reject_links(new_dir)?;
        let old = self.data_dir.clone();
        if new_dir.exists() && fs::canonicalize(new_dir)? == old {
            return Ok(RelocationOutcome {
                cleanup_warning: None,
            });
        }
        let target = absolute_target(new_dir)?;
        if target.starts_with(&old) || old.starts_with(&target) {
            return Err(invalid_data("source and target folders must be disjoint"));
        }
        let existed = target.exists();
        if existed && fs::read_dir(&target)?.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "target folder is not empty",
            ));
        }
        fs::create_dir(&target).or_else(|e| {
            if existed && e.kind() == io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(e)
            }
        })?;
        let target_owner = acquire_ownership(&target)?;
        let mut created = Vec::new();
        let staged = (|| {
            copy_tree(&old, &target, &mut created, true)?;
            fault("verify")?;
            verify_tree(&old, &target, true)?;
            fault("pointer")?;
            save_json_atomic(
                &base_dir.join(POINTER_FILE),
                &LocationPointer {
                    data_dir: target.to_string_lossy().into_owned(),
                },
            )
        })();
        if let Err(error) = staged {
            // Only paths created by this operation are eligible for cleanup.
            for path in created.iter().rev() {
                if path.is_dir() {
                    let _ = fs::remove_dir(path);
                } else {
                    let _ = fs::remove_file(path);
                }
            }
            drop(target_owner);
            // The persistent ownership file is deliberately never deleted.
            return Err(error);
        }
        self.data_dir = target;
        let old_owner = std::mem::replace(&mut self.ownership, target_owner);
        // Keep old ownership through cleanup. Its persistent lock is excluded.
        let cleanup =
            fault("cleanup").and_then(|()| remove_originals(&old, &self.data_dir, &created));
        drop(old_owner);
        Ok(RelocationOutcome {
            cleanup_warning: cleanup
                .err()
                .map(|e| format!("Data moved; old files remain at {}: {e}", old.display())),
        })
    }
}

/// Drop trash records past the 30-day retention window (v0.3 #10)
/// at load time, so an expired record never comes back from disk.
/// The drop persists on the next save; cover files are untouched
/// (a copy of the item may share them).
fn purge_trash(library: &mut Library) {
    crate::trash::purge_expired(&mut library.trash, crate::model::now_unix());
}

/// The `schema_version` a stored JSON text declares, or `None` when
/// the field is absent or not a whole number (a hand-edited file) —
/// loaders treat `None` as version 0, the pre-versioning format.
pub fn declared_schema_version(text: &str) -> Option<u32> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

/// Check a stored file's declared schema version against what this
/// build understands, returning the declared version (0 when the
/// field is absent). A file from a newer FaveLists is refused with a
/// plain error: loading it anyway would silently drop whatever the
/// newer format added, and the next save would overwrite it.
fn check_not_newer(text: &str, current: u32, file_label: &str) -> io::Result<u32> {
    let declared = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| {
            value
                .get("schema_version")
                .and_then(serde_json::Value::as_u64)
        })
        .unwrap_or(0);
    if declared > u64::from(current) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{file_label} declares schema_version {declared}, which is newer than this build understands ({current}) — it was written by a newer FaveLists and has NOT been changed"
            ),
        ));
    }
    Ok(declared as u32)
}

/// Keep the original bytes of a pre-versioning file safe before its
/// contents are migrated up and (on the next save) rewritten at the
/// current version: a one-time copy beside the file named
/// `<file>.v0-backup`. Written once — an existing copy is the
/// pristine original and is never overwritten.
fn preserve_pre_migration_copy(path: &Path) -> io::Result<()> {
    preserve_original(path, "v0-backup")
}

fn preserve_original(path: &Path, suffix: &str) -> io::Result<()> {
    let bytes = fs::read(path)?;
    let backup = path.with_file_name(format!(
        "{}.{suffix}",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
    {
        Ok(mut file) => {
            let result = (|| {
                fault("preserve_write")?;
                file.write_all(&bytes)?;
                fault("preserve_sync")?;
                file.sync_all()
            })();
            drop(file);
            if result.is_err() {
                let _ = fs::remove_file(&backup);
            }
            result
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            if fs::read(&backup)? == bytes {
                Ok(())
            } else {
                Err(invalid_data(
                    "existing original backup differs; repair refused",
                ))
            }
        }
        Err(e) => Err(e),
    }
}

fn preflight<T: serde::de::DeserializeOwned>(
    path: &Path,
    current: u32,
    label: &str,
) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(text) => {
            check_not_newer(&text, current, label)?;
            parse_object::<T>(&text)?;
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn parse_object<T: serde::de::DeserializeOwned>(text: &str) -> io::Result<T> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(invalid_data)?;
    if !value.is_object() {
        return Err(invalid_data("stored JSON must be an object"));
    }
    serde_json::from_value(value).map_err(invalid_data)
}

#[cfg(test)]
thread_local! { static FAILURE: std::cell::RefCell<Option<&'static str>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
thread_local! { static TEMP_COLLISION: std::cell::RefCell<Option<u64>> = const { std::cell::RefCell::new(None) }; }

fn fault(stage: &str) -> io::Result<()> {
    #[cfg(test)]
    if FAILURE.with(|value| *value.borrow() == Some(stage)) {
        return Err(io::Error::other(format!("injected {stage} failure")));
    }
    let _ = stage;
    Ok(())
}

pub fn save_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(invalid_data)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let (tmp, mut file) = loop {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(test)]
        let n = TEMP_COLLISION
            .with(|value| value.borrow_mut().take())
            .unwrap_or(n);
        let tmp = path.with_file_name(format!(
            "{}.{}.{n}.tmp",
            path.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        fault("write")?;
        file.write_all(&bytes)?;
        fault("flush")?;
        file.flush()?;
        fault("sync")?;
        file.sync_all()
    })();
    drop(file);
    let result = result
        .and_then(|()| fault("replace"))
        .and_then(|()| replace_file(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Windows replacement calls on the same destination can otherwise collide.
    static REPLACEMENTS: Mutex<()> = Mutex::new(());
    let _replacement = REPLACEMENTS
        .lock()
        .map_err(|_| io::Error::other("replacement lock poisoned"))?;
    // REPLACE_EXISTING | WRITE_THROUGH, with no delete-before-replace window.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

fn reject_links(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let link = metadata.file_type().is_symlink();
        #[cfg(windows)]
        let link = {
            use std::os::windows::fs::MetadataExt;
            link || metadata.file_attributes() & 0x400 != 0
        };
        if link {
            return Err(invalid_data(
                "storage reparse points and symlinks are refused",
            ));
        }
    }
    Ok(())
}

fn absolute_target(path: &Path) -> io::Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path);
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid_data("missing target parent"))?;
    Ok(fs::canonicalize(parent)?.join(
        path.file_name()
            .ok_or_else(|| invalid_data("invalid target"))?,
    ))
}

fn copy_tree(from: &Path, to: &Path, created: &mut Vec<PathBuf>, root: bool) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if root && entry.file_name() == OWNERSHIP_FILE {
            continue;
        }
        reject_links(&entry.path())?;
        let dest = to.join(entry.file_name());
        fault("copy")?;
        if entry.file_type()?.is_dir() {
            fs::create_dir(&dest)?;
            created.push(dest.clone());
            copy_tree(&entry.path(), &dest, created, false)?;
        } else if entry.file_type()?.is_file() {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)?;
            created.push(dest.clone());
            let mut source = fs::File::open(entry.path())?;
            io::copy(&mut source, &mut file)?;
            fault("copy_sync")?;
            file.sync_all()?;
        } else {
            return Err(invalid_data("unsupported storage entry"));
        }
    }
    Ok(())
}

fn verify_tree(from: &Path, to: &Path, root: bool) -> io::Result<()> {
    let names = |path: &Path| -> io::Result<std::collections::BTreeSet<std::ffi::OsString>> {
        fs::read_dir(path)?
            .map(|e| e.map(|e| e.file_name()))
            .filter(|e| !root || e.as_ref().map_or(true, |n| n != OWNERSHIP_FILE))
            .collect()
    };
    if names(from)? != names(to)? {
        return Err(invalid_data("relocation inventory differs"));
    }
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if root && entry.file_name() == OWNERSHIP_FILE {
            continue;
        }
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            verify_tree(&entry.path(), &dest, false)?;
        } else if fs::read(entry.path())? != fs::read(dest)? {
            return Err(invalid_data("relocation bytes differ"));
        }
    }
    Ok(())
}

fn remove_originals(root: &Path, target: &Path, copied: &[PathBuf]) -> io::Result<()> {
    for dest in copied.iter().rev() {
        reject_links(dest)?;
        let source = root.join(dest.strip_prefix(target).map_err(invalid_data)?);
        reject_links(&source)?;
        if dest.is_dir() {
            fs::remove_dir(&source)?;
        } else {
            if fs::read(&source)? != fs::read(dest)? {
                return Err(invalid_data(
                    "source changed after commitment; retained original",
                ));
            }
            fs::remove_file(&source)?;
        }
    }
    for entry in fs::read_dir(root)? {
        if entry?.file_name() != OWNERSHIP_FILE {
            return Err(invalid_data("uncopied source content retained"));
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
    fn atomic_failures_preserve_every_destination_and_foreign_temp() {
        let dir = temp_dir("atomic-faults");
        for name in [LIBRARY_FILE, JOURNAL_FILE, SETTINGS_FILE, POINTER_FILE] {
            let path = dir.join(name);
            let foreign = path.with_extension("json.tmp");
            fs::write(&foreign, b"foreign").unwrap();
            for stage in ["write", "flush", "sync", "replace"] {
                fs::write(&path, b"original").unwrap();
                FAILURE.with(|value| *value.borrow_mut() = Some(stage));
                let result = save_json_atomic(&path, &serde_json::json!({"new": true}));
                FAILURE.with(|value| *value.borrow_mut() = None);
                assert!(result.is_err(), "{name}: {stage}");
                assert_eq!(fs::read(&path).unwrap(), b"original");
                assert_eq!(fs::read(&foreign).unwrap(), b"foreign");
                assert_eq!(
                    fs::read_dir(&dir)
                        .unwrap()
                        .filter_map(Result::ok)
                        .filter(|entry| entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(&format!("{name}.")))
                        .count(),
                    1
                );
            }
            save_json_atomic(&path, &serde_json::json!({"new": true})).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&fs::read(path).unwrap()).unwrap()
                    ["new"],
                true
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exclusive_temp_collision_and_concurrent_replacement_preserve_foreign_bytes() {
        let dir = temp_dir("temp-collision");
        let path = dir.join("state.json");
        let foreign = dir.join(format!(
            "state.json.{}.{}.tmp",
            std::process::id(),
            u64::MAX
        ));
        fs::write(&foreign, b"foreign").unwrap();
        TEMP_COLLISION.with(|value| *value.borrow_mut() = Some(u64::MAX));
        save_json_atomic(&path, &serde_json::json!({"first": true})).unwrap();
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign");
        let workers: Vec<_> = (0..4)
            .map(|index| {
                let path = path.clone();
                std::thread::spawn(move || {
                    save_json_atomic(&path, &serde_json::json!({"writer": index})).unwrap()
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(value["writer"].as_u64().unwrap() < 4);
        assert_eq!(fs::read(foreign).unwrap(), b"foreign");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_matrix_is_write_protected_and_original_bytes_survive() {
        for bytes in [
            b"bad JSON".as_slice(),
            b"\xff".as_slice(),
            b"[]".as_slice(),
            br#"{"lists":"invalid"}"#.as_slice(),
            br#"{"schema_version":99}"#.as_slice(),
        ] {
            for name in [LIBRARY_FILE, JOURNAL_FILE] {
                let dir = temp_dir("damage");
                fs::write(dir.join(name), bytes).unwrap();
                let store = Store::open_at(&dir, &dir).unwrap();
                assert!(store.load_library_recovering().is_err());
                assert!(store.save_library(&Library::default()).is_err());
                assert_eq!(fs::read(dir.join(name)).unwrap(), bytes);
                drop(store);
                fs::remove_dir_all(dir).unwrap();
            }
        }
        for bytes in [
            b"bad JSON".as_slice(),
            b"\xff".as_slice(),
            b"[]".as_slice(),
            br#"{"wizard_done":"invalid"}"#.as_slice(),
            br#"{"schema_version":99}"#.as_slice(),
        ] {
            let dir = temp_dir("settings-damage");
            fs::write(dir.join(SETTINGS_FILE), bytes).unwrap();
            let store = Store::open_at(&dir, &dir).unwrap();
            assert!(store.load_settings().is_err());
            assert!(store
                .save_settings(&Settings {
                    wizard_done: true,
                    ..Settings::default()
                })
                .is_err());
            assert_eq!(fs::read(dir.join(SETTINGS_FILE)).unwrap(), bytes);
            // The library remains independently writable.
            store.save_library(&Library::default()).unwrap();
            drop(store);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn valid_main_wins_but_bad_journal_blocks_both_writes() {
        let dir = temp_dir("journal-preflight");
        let store = Store::open_at(&dir, &dir).unwrap();
        let mut original = Library::default();
        original.new_list("Keep me", Category::Movies, true);
        store.save_library(&original).unwrap();
        let before = fs::read(dir.join(LIBRARY_FILE)).unwrap();
        fs::write(dir.join(JOURNAL_FILE), b"corrupt").unwrap();
        let load = store.load_library_recovering().unwrap();
        assert_eq!(load.library, original);
        assert!(load.write_error.is_some());
        assert!(store.save_library(&Library::default()).is_err());
        assert_eq!(fs::read(dir.join(LIBRARY_FILE)).unwrap(), before);
        assert_eq!(fs::read(dir.join(JOURNAL_FILE)).unwrap(), b"corrupt");
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_main_never_recovers_from_an_older_journal() {
        for version in [99, u64::from(u32::MAX) + 1] {
            let dir = temp_dir("newer-main");
            let store = Store::open_at(&dir, &dir).unwrap();
            store.save_library(&Library::default()).unwrap();
            let bytes = format!(r#"{{"schema_version":{version}}}"#);
            fs::write(dir.join(LIBRARY_FILE), &bytes).unwrap();
            assert!(store.load_library_recovering().is_err());
            assert!(store.save_library(&Library::default()).is_err());
            assert_eq!(fs::read(dir.join(LIBRARY_FILE)).unwrap(), bytes.as_bytes());
            assert!(!dir.join("library.json.corrupt-backup").exists());
            drop(store);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn recovery_preserves_corrupt_bytes_and_refuses_failed_preservation() {
        for conflict in [false, true] {
            let dir = temp_dir("recovery-preserve");
            let store = Store::open_at(&dir, &dir).unwrap();
            store.save_library(&Library::default()).unwrap();
            fs::write(dir.join(LIBRARY_FILE), b"\xffdamaged").unwrap();
            if conflict {
                fs::write(dir.join("library.json.corrupt-backup"), b"foreign").unwrap();
            }
            let result = store.load_library_recovering();
            if conflict {
                assert!(result.is_err());
                assert!(store.save_library(&Library::default()).is_err());
                assert_eq!(fs::read(dir.join(LIBRARY_FILE)).unwrap(), b"\xffdamaged");
            } else {
                assert!(result.unwrap().recovered_from_journal);
                assert_eq!(
                    fs::read(dir.join("library.json.corrupt-backup")).unwrap(),
                    b"\xffdamaged"
                );
                store.save_library(&Library::default()).unwrap();
            }
            drop(store);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn unreadable_entries_never_authorize_writes() {
        for name in [LIBRARY_FILE, JOURNAL_FILE, SETTINGS_FILE] {
            let dir = temp_dir("unreadable");
            fs::create_dir(dir.join(name)).unwrap();
            let store = Store::open_at(&dir, &dir).unwrap();
            if name == SETTINGS_FILE {
                assert!(store.load_settings().is_err());
                assert!(store.save_settings(&Settings::default()).is_err());
            } else {
                assert!(store.load_library_recovering().is_err());
                assert!(store.save_library(&Library::default()).is_err());
            }
            assert!(dir.join(name).is_dir());
            drop(store);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn failed_migration_preservation_never_falls_back_to_an_older_journal() {
        for stage in ["preserve_write", "preserve_sync"] {
            let dir = temp_dir("migration-preserve-fault");
            let store = Store::open_at(&dir, &dir).unwrap();
            let legacy = br#"{"lists":[],"next_list_id":42}"#;
            fs::write(dir.join(LIBRARY_FILE), legacy).unwrap();
            save_json_atomic(&dir.join(JOURNAL_FILE), &Library::default()).unwrap();
            FAILURE.with(|value| *value.borrow_mut() = Some(stage));
            let load = store.load_library_recovering();
            FAILURE.with(|value| *value.borrow_mut() = None);
            assert!(load.is_err());
            assert_eq!(fs::read(dir.join(LIBRARY_FILE)).unwrap(), legacy);
            assert!(store.save_library(&Library::default()).is_err());
            assert!(!dir.join("library.json.v0-backup").exists());
            drop(store);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn bad_pointer_and_missing_recorded_folder_fail_closed() {
        let base = temp_dir("bad-pointer");
        fs::write(base.join(POINTER_FILE), b"damaged").unwrap();
        assert!(Store::open(&base).is_err());
        assert!(!base.join(DATA_DIR_NAME).exists());
        save_json_atomic(
            &base.join(POINTER_FILE),
            &LocationPointer {
                data_dir: base.join("missing").display().to_string(),
            },
        )
        .unwrap();
        assert!(Store::open(&base).is_err());
        assert!(!base.join("missing").exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn gui_and_channel_refuse_a_live_owner_before_loading() {
        let base = temp_dir("entrypoint-ownership");
        let store = Store::open(&base).unwrap();
        assert!(crate::app::FaveApp::new(base.clone()).is_err());
        let mut output = Vec::new();
        assert!(crate::json_channel::run(&base, std::io::Cursor::new(b""), &mut output).is_err());
        assert!(output.is_empty());
        drop(store);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn startup_does_not_mark_corrupt_settings_as_wizard_done() {
        let base = temp_dir("startup-settings");
        let root = base.join(DATA_DIR_NAME);
        fs::create_dir(&root).unwrap();
        fs::write(root.join(SETTINGS_FILE), b"corrupt settings").unwrap();
        let app = crate::app::FaveApp::new(base.clone()).unwrap();
        assert_eq!(
            fs::read(root.join(SETTINGS_FILE)).unwrap(),
            b"corrupt settings"
        );
        drop(app);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn relocation_faults_keep_complete_originals_and_pointer_authority() {
        for stage in [
            "copy",
            "copy_sync",
            "verify",
            "pointer",
            "write",
            "flush",
            "sync",
            "replace",
            "cleanup",
        ] {
            let base = temp_dir("relocation-fault");
            let old = base.join(DATA_DIR_NAME);
            let target = base.join("target");
            let mut store = Store::open(&base).unwrap();
            let mut library = Library::default();
            let list_id = library.new_list("Keep", Category::Movies, true);
            let mut item = crate::model::Item::new(library.alloc_item_id(), "Keep cover");
            item.cover = Some("cover-1.jpg".into());
            library.list_mut(list_id).unwrap().add_item(item);
            store.save_library(&library).unwrap();
            store.save_settings(&Settings::default()).unwrap();
            for name in [
                "covers/cover-1.jpg",
                "drinks.json",
                "library.json.v0-backup",
                "backups/backup.zip",
            ] {
                let path = old.join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, name.as_bytes()).unwrap();
            }
            let inventory: Vec<_> = [
                LIBRARY_FILE,
                JOURNAL_FILE,
                SETTINGS_FILE,
                "covers/cover-1.jpg",
                "drinks.json",
                "library.json.v0-backup",
                "backups/backup.zip",
            ]
            .into_iter()
            .map(|name| (name, fs::read(old.join(name)).unwrap()))
            .collect();
            FAILURE.with(|value| *value.borrow_mut() = Some(stage));
            let outcome = store.relocate(&base, &target);
            FAILURE.with(|value| *value.borrow_mut() = None);
            let committed = stage == "cleanup";
            if committed {
                assert!(outcome.unwrap().cleanup_warning.is_some());
            } else {
                assert!(outcome.is_err(), "{stage}");
            }
            for (name, bytes) in &inventory {
                assert_eq!(fs::read(old.join(name)).unwrap(), *bytes, "{stage}: {name}");
                if committed {
                    assert_eq!(fs::read(target.join(name)).unwrap(), *bytes);
                }
            }
            assert_eq!(
                resolve_data_dir(&base).unwrap(),
                if committed {
                    fs::canonicalize(&target).unwrap()
                } else {
                    old.clone()
                }
            );
            store.save_library(&library).unwrap();
            drop(store);
            let reopened = Store::open(&base).unwrap();
            assert_eq!(reopened.load_library().unwrap(), library);
            drop(reopened);
            fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn relocation_rejects_nested_alias_and_foreign_targets_without_deletion() {
        let base = temp_dir("relocate-boundaries");
        let mut store = Store::open(&base).unwrap();
        let old = base.join(DATA_DIR_NAME);
        assert!(store.relocate(&base, &old.join("nested")).is_err());
        assert!(store.relocate(&base, &base).is_err());
        let foreign = base.join("foreign");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join(OWNERSHIP_FILE), b"foreign ownership").unwrap();
        assert!(store.relocate(&base, &foreign).is_err());
        assert_eq!(
            fs::read(foreign.join(OWNERSHIP_FILE)).unwrap(),
            b"foreign ownership"
        );
        drop(store);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn backup_barrier_serializes_relocation_and_queued_workers_use_new_root() {
        use std::sync::mpsc;
        use std::time::Duration;
        let base = temp_dir("backup-barrier");
        let mut store = Store::open(&base).unwrap();
        let library = Library::default();
        store.save_library(&library).unwrap();
        let worker = store.clone();
        let queued = store.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let backup = std::thread::spawn(move || {
            worker.with_storage(|state| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                crate::auto_backup::write_auto_backup(&library, state).unwrap();
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let target = base.join("new");
        let relocation_base = base.clone();
        let relocation_target = target.clone();
        let (moved_tx, moved_rx) = mpsc::channel();
        let relocation = std::thread::spawn(move || {
            let outcome = store
                .relocate(&relocation_base, &relocation_target)
                .unwrap();
            moved_tx.send(()).unwrap();
            (store, outcome)
        });
        assert!(moved_rx.recv_timeout(Duration::from_millis(100)).is_err());
        release_tx.send(()).unwrap();
        backup.join().unwrap();
        let (store, outcome) = relocation.join().unwrap();
        assert!(outcome.cleanup_warning.is_none());
        queued.with_storage(|state| {
            fs::write(state.covers_dir().join("queued.jpg"), b"queued cover").unwrap();
            save_json_atomic(&state.drinks_path(), &serde_json::json!({"version": 2})).unwrap();
        });
        assert!(target.join("covers/queued.jpg").is_file());
        assert!(target.join(DRINKS_FILE).is_file());
        assert!(!base.join(DATA_DIR_NAME).join("covers").exists());
        assert!(!crate::auto_backup::list_auto_backups(&target.join(BACKUPS_DIR)).is_empty());
        drop(store);
        assert!(
            Store::open(&base).is_err(),
            "queued worker retains ownership"
        );
        drop(queued);
        let reopened = Store::open(&base).unwrap();
        assert_eq!(reopened.load_library().unwrap(), Library::default());
        drop(reopened);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn ownership_child() {
        use std::io::BufRead;
        let Some(dir) = std::env::var_os("FAVELISTS_OWNERSHIP_FIXTURE") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let _store = Store::open_at(&dir, &dir).unwrap();
        fs::write(dir.join("ready"), b"ready").unwrap();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).unwrap();
    }

    #[test]
    fn second_process_is_refused_and_abandoned_ownership_is_released() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let dir = temp_dir("ownership-process");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "store::tests::ownership_child", "--nocapture"])
            .env("FAVELISTS_OWNERSHIP_FIXTURE", &dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while !dir.join("ready").exists() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let ready = dir.join("ready").exists();
        let refused = Store::open_at(&dir, &dir).is_err();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(ready, "ownership child did not become ready");
        assert!(refused);
        let store = Store::open_at(&dir, &dir).unwrap();
        store.save_library(&Library::default()).unwrap();
        drop(store);
        fs::remove_dir_all(dir).unwrap();
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

        drop(store);
        let store2 = Store::open(&base).unwrap();
        let loaded = store2.load_library().unwrap();
        assert_eq!(loaded, lib);
        assert_eq!(store2.load_settings().unwrap().tmdb_api_key, "abc");
        drop(store2);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn default_data_dir_is_next_to_base() {
        let base = temp_dir("defaultdir");
        let store = Store::open(&base).unwrap();
        assert_eq!(
            store.data_dir_label(),
            fs::canonicalize(base.join(DATA_DIR_NAME))
                .unwrap()
                .display()
                .to_string()
        );
        assert!(store.with_storage(|state| state.covers_dir()).is_dir());
        drop(store);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn relocate_moves_data_and_writes_pointer() {
        let base = temp_dir("reloc-base");
        let mut store = Store::open(&base).unwrap();
        let mut lib = Library::default();
        let list_id = lib.new_list("Sodas", Category::Sodas, false);
        let mut item = crate::model::Item::new(lib.alloc_item_id(), "Soda");
        item.cover = Some("cover-1.jpg".into());
        lib.list_mut(list_id).unwrap().add_item(item);
        store.save_library(&lib).unwrap();
        fs::write(
            store
                .with_storage(|state| state.covers_dir())
                .join("cover-1.jpg"),
            b"img",
        )
        .unwrap();

        let new_dir = temp_dir("reloc-new");
        fs::remove_dir_all(&new_dir).unwrap();
        store.relocate(&base, &new_dir).unwrap();

        assert!(!base.join(DATA_DIR_NAME).join(LIBRARY_FILE).exists());
        drop(store);
        let reopened = Store::open(&base).unwrap();
        assert_eq!(
            reopened.data_dir_label(),
            fs::canonicalize(&new_dir).unwrap().display().to_string()
        );
        assert_eq!(reopened.load_library().unwrap(), lib);
        assert!(new_dir.join(COVERS_DIR).join("cover-1.jpg").exists());
        drop(reopened);
        fs::remove_dir_all(&base).unwrap();
        fs::remove_dir_all(&new_dir).unwrap();
    }

    #[test]
    fn relocate_refuses_non_empty_target() {
        let base = temp_dir("reloc2-base");
        let mut store = Store::open(&base).unwrap();
        let target = temp_dir("reloc2-target");
        fs::write(target.join("something.txt"), b"x").unwrap();
        assert!(store.relocate(&base, &target).is_err());
        drop(store);
        fs::remove_dir_all(&base).unwrap();
        fs::remove_dir_all(&target).unwrap();
    }

    #[test]
    fn cover_file_names_are_safe() {
        assert_eq!(cover_file_name(7, "https://x/y/poster.JPG"), "cover-7.jpg");
        assert_eq!(cover_file_name(7, "no-extension"), "cover-7.jpg");
        assert_eq!(cover_file_name(3, "a/b/pic.png"), "cover-3.png");
    }
    fn export_requests(path: &Path, list_id: u64) -> Vec<serde_json::Value> {
        ["export.text", "export.markdown", "export.image", "export.share_card", "export.csv", "export.library", "backup.export"]
            .into_iter().map(|command| serde_json::json!({"command": command, "params": {"path": path, "list_id": list_id, "format": if command == "export.library" { "json" } else { "png" }}})).collect()
    }

    #[test]
    fn gui_and_channel_exports_preserve_managed_destinations_and_allow_external_outputs() {
        for settings_bytes in [
            b"damaged settings".as_slice(),
            br#"{"schema_version":999}"#.as_slice(),
        ] {
            let base = temp_dir("export-boundary-dispatch");
            let store = Store::open(&base).unwrap();
            let root = store.with_storage(|state| state.data_dir.clone());
            let mut library = Library::default();
            let list_id = library.new_list("Films", Category::Movies, false);
            library
                .list_mut(list_id)
                .unwrap()
                .add_item(crate::model::Item::new(1, "Alien"));
            store.save_library(&library).unwrap();
            fs::write(root.join(SETTINGS_FILE), settings_bytes).unwrap();
            fs::write(root.join(DRINKS_FILE), b"drinks").unwrap();
            let mut paths: Vec<PathBuf> = [
                LIBRARY_FILE,
                JOURNAL_FILE,
                SETTINGS_FILE,
                DRINKS_FILE,
                OWNERSHIP_FILE,
                "library.json.v0-backup",
                "settings.json.v0-backup",
                "library.json.corrupt-backup",
                "library.json.77.88.tmp",
            ]
            .iter()
            .map(|name| root.join(name))
            .collect();
            paths.extend([
                base.join(POINTER_FILE),
                root.join(COVERS_DIR),
                root.join("backups/new.zip"),
                root.join("covers/new.jpg"),
            ]);
            for name in [
                "library.json.v0-backup",
                "settings.json.v0-backup",
                "library.json.corrupt-backup",
            ] {
                fs::write(root.join(name), b"preserved original").unwrap();
            }
            fs::write(root.join("covers/cover.jpg"), b"cover").unwrap();
            fs::create_dir(root.join(BACKUPS_DIR)).unwrap();
            fs::write(root.join("backups/auto-20000101-000000.zip"), b"backup").unwrap();
            let hardlink = base.join("hardlink-settings.json");
            fs::hard_link(root.join(SETTINGS_FILE), &hardlink).unwrap();
            paths.push(hardlink);
            let cover_link = base.join("hardlink-cover.jpg");
            fs::hard_link(root.join("covers/cover.jpg"), &cover_link).unwrap();
            paths.push(cover_link);
            paths.push(root.join("covers/../settings.json"));
            paths.push(root.join("missing/../settings.json"));
            let alias = base.join("managed-alias");
            #[cfg(windows)]
            {
                let result = std::process::Command::new("cmd.exe")
                    .args(["/d", "/c", "mklink", "/J"])
                    .arg(&alias)
                    .arg(root.join(COVERS_DIR))
                    .output()
                    .unwrap();
                assert!(result.status.success(), "junction fixture failed");
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(root.join(COVERS_DIR), &alias).unwrap();
            paths.push(alias.join("new-cover.jpg"));
            paths.push(alias.join("../settings.json"));
            #[cfg(windows)]
            paths.extend([
                root.join("SETTINGS.JSON"),
                root.join("settings.json:stream"),
                root.join("settings.json."),
            ]);
            let before: Vec<_> = [
                LIBRARY_FILE,
                JOURNAL_FILE,
                SETTINGS_FILE,
                DRINKS_FILE,
                "library.json.v0-backup",
                "settings.json.v0-backup",
                "library.json.corrupt-backup",
            ]
            .iter()
            .map(|name| (root.join(name), fs::read(root.join(name)).unwrap()))
            .collect();
            drop(store);
            let mut app = crate::app::FaveApp::new(base.clone()).unwrap();
            let mut requests = Vec::new();
            for path in &paths {
                for request in export_requests(path, list_id) {
                    let result = app.dispatch(
                        request["command"].as_str().unwrap(),
                        request["params"].clone(),
                    );
                    assert!(result.is_err(), "GUI accepted {request}");
                    requests.push(request.to_string());
                }
            }
            let external = base.join("ordinary.json");
            app.dispatch(
                "export.library",
                serde_json::json!({"path": external, "format": "json"}),
            )
            .unwrap();
            app.store
                .with_storage(|state| {
                    state.write_internal_backup(&app.library, "auto-20261103-000000")
                })
                .unwrap_err();
            app.store
                .with_storage(|state| {
                    state.write_internal_backup(&app.library, "auto-20261103-000000.zip")
                })
                .unwrap();
            assert!(root.join("backups/auto-20261103-000000.zip").is_file());
            assert!(app
                .dispatch(
                    "backup.export",
                    serde_json::json!({"internal_backup": "../settings.json"})
                )
                .is_err());
            drop(app);
            let mut response = Vec::new();
            crate::json_channel::run(
                &base,
                std::io::Cursor::new(requests.join("\n")),
                &mut response,
            )
            .unwrap();
            for line in String::from_utf8(response).unwrap().lines() {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(value["ok"], false, "channel accepted {value}");
            }
            for (path, bytes) in &before {
                assert_eq!(fs::read(path).unwrap(), *bytes);
            }
            assert!(!base.join(POINTER_FILE).exists());
            assert!(!root.join("library.json.77.88.tmp").exists());
            assert!(!root.join("backups/new.zip").exists());
            assert!(!root.join("covers/new.jpg").exists());
            assert!(!root.join("settings_assets").exists());
            assert!(external.is_file());
            #[cfg(windows)]
            fs::remove_dir(&alias).unwrap();
            #[cfg(unix)]
            fs::remove_file(&alias).unwrap();
            fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn markdown_validates_all_assets_and_contexts_require_storage() {
        let base = temp_dir("markdown-boundary");
        let store = Store::open(&base).unwrap();
        let root = store.with_storage(|state| state.data_dir.clone());
        let mut library = Library::default();
        let id = library.new_list("Films", Category::Movies, false);
        let mut item = crate::model::Item::new(1, "Alien");
        item.cover = Some("cover.jpg".into());
        library.list_mut(id).unwrap().add_item(item);
        fs::write(root.join("covers/cover.jpg"), b"cover").unwrap();
        let output = base.join("film.md");
        let assets = base.join("film_assets");
        fs::create_dir(&assets).unwrap();
        store.save_library(&library).unwrap();
        fs::hard_link(root.join(LIBRARY_FILE), assets.join("cover.jpg")).unwrap();
        let original = fs::read(root.join(LIBRARY_FILE)).unwrap();
        assert!(store
            .dispatch(
                &mut library,
                None,
                "export.markdown",
                &serde_json::json!({"path": output, "list_id": id})
            )
            .is_err());
        assert!(!output.exists());
        assert_eq!(fs::read(root.join(LIBRARY_FILE)).unwrap(), original);
        fs::remove_file(assets.join("cover.jpg")).unwrap();
        for cover in [
            "../settings.json",
            "/absolute.jpg",
            "C:\\outside.jpg",
            "bad:stream",
            "sub/cover.jpg",
        ] {
            library.list_mut(id).unwrap().items[0].cover = Some(cover.into());
            assert!(store
                .dispatch(
                    &mut library,
                    None,
                    "export.markdown",
                    &serde_json::json!({"path": output, "list_id": id})
                )
                .is_err());
            assert!(!output.exists());
            assert_eq!(fs::read_dir(&assets).unwrap().count(), 0);
        }
        library.list_mut(id).unwrap().items[0].cover = Some("cover.jpg".into());
        store
            .dispatch(
                &mut library,
                None,
                "export.markdown",
                &serde_json::json!({"path": output, "list_id": id}),
            )
            .unwrap();
        assert_eq!(fs::read(assets.join("cover.jpg")).unwrap(), b"cover");
        let covers = root.join(COVERS_DIR);
        let mut context = crate::commands::CommandContext::new(&mut library, &covers);
        for request in export_requests(&base.join("refused"), id) {
            assert!(crate::commands::dispatch(
                &mut context,
                request["command"].as_str().unwrap(),
                &request["params"]
            )
            .is_err());
        }
        assert!(!base.join("refused").exists());
        drop(store);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn output_revalidation_preserves_late_hardlinks_and_pointer_after_relocation() {
        let base = temp_dir("export-revalidate");
        let mut store = Store::open(&base).unwrap();
        store.save_library(&Library::default()).unwrap();
        let target = base.join("relocated");
        store.relocate(&base, &target).unwrap();
        let original_pointer = fs::read(base.join(POINTER_FILE)).unwrap();
        let output = base.join("late-alias");
        store.with_storage(|state| {
            let outputs = state
                .validate_external_outputs(std::slice::from_ref(&output))
                .unwrap();
            fs::hard_link(state.library_path(), &output).unwrap();
            assert!(outputs.write(0, b"must not truncate").is_err());
            assert!(state
                .validate_external_outputs(&[base.join(POINTER_FILE)])
                .is_err());
            assert!(state
                .validate_external_outputs(&[base.join("favelists-location.json.12.34.tmp")])
                .is_err());
        });
        assert_eq!(fs::read(base.join(POINTER_FILE)).unwrap(), original_pointer);
        drop(store);
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn journal_migration_backup_refuses_gui_and_channel_exports() {
        for recovered in [false, true] {
            let base = temp_dir("journal-backup-exports");
            let store = Store::open(&base).unwrap();
            let root = store.with_storage(|state| state.data_dir.clone());
            let backup = root.join("library.journal.json.v0-backup");
            let original = include_bytes!("../tests/fixtures/library_v030.json");
            let mut library = if recovered {
                fs::write(root.join(JOURNAL_FILE), original).unwrap();
                let load = store.load_library_recovering().unwrap();
                assert!(load.recovered_from_journal);
                assert_eq!(fs::read(&backup).unwrap(), original);
                load.library
            } else {
                Library::default()
            };
            let id = library.new_list("Films", Category::Movies, false);
            let mut paths = vec![backup.clone()];
            if recovered {
                let alias = base.join("journal-original.json");
                fs::hard_link(&backup, &alias).unwrap();
                paths.push(alias);
            }
            let requests: Vec<_> = paths.iter().flat_map(|p| export_requests(p, id)).collect();
            for request in &requests {
                assert!(
                    store
                        .dispatch(
                            &mut library,
                            None,
                            request["command"].as_str().unwrap(),
                            &request["params"]
                        )
                        .is_err(),
                    "GUI accepted {request}"
                );
            }
            let external = base.join("ordinary.json");
            store
                .dispatch(
                    &mut library,
                    None,
                    "export.library",
                    &serde_json::json!({"path": external, "format": "json"}),
                )
                .unwrap();
            store.save_library(&library).unwrap();
            drop(store);
            let mut response = Vec::new();
            let mut channel_requests = requests.clone();
            channel_requests.push(serde_json::json!({"command": "export.library",
                "params": {"path": external, "format": "json"}}));
            crate::json_channel::run(
                &base,
                std::io::Cursor::new(
                    channel_requests
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                &mut response,
            )
            .unwrap();
            let responses: Vec<serde_json::Value> = String::from_utf8(response)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(responses.len(), requests.len() + 1);
            for response in &responses[..requests.len()] {
                assert_eq!(response["ok"], false, "channel accepted {response}");
            }
            assert_eq!(responses.last().unwrap()["ok"], true);
            assert!(external.is_file());
            for path in paths {
                if recovered {
                    assert_eq!(fs::read(&path).unwrap(), original);
                } else {
                    assert!(!path.exists());
                }
                assert!(!path
                    .with_file_name(format!(
                        "{}_assets",
                        path.file_stem().unwrap().to_str().unwrap()
                    ))
                    .exists());
            }
            fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn damaged_and_newer_libraries_and_journals_refuse_production_exports() {
        for name in [LIBRARY_FILE, JOURNAL_FILE] {
            for bytes in [
                b"damaged".as_slice(),
                br#"{"schema_version":999}"#.as_slice(),
            ] {
                let base = temp_dir("export-damaged-library");
                let store = Store::open(&base).unwrap();
                let root = store.with_storage(|state| state.data_dir.clone());
                let mut library = Library::default();
                let id = library.new_list("Films", Category::Movies, false);
                store.save_library(&library).unwrap();
                if name == LIBRARY_FILE {
                    fs::remove_file(root.join(JOURNAL_FILE)).unwrap();
                }
                fs::write(root.join(name), bytes).unwrap();
                drop(store);
                let mut app = crate::app::FaveApp::new(base.clone()).unwrap();
                for request in export_requests(&root.join(name), id) {
                    assert!(app
                        .dispatch(
                            request["command"].as_str().unwrap(),
                            request["params"].clone()
                        )
                        .is_err());
                }
                drop(app);
                let requests = export_requests(&root.join(name), id)
                    .iter()
                    .map(|r| r.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut output = Vec::new();
                let result =
                    crate::json_channel::run(&base, std::io::Cursor::new(requests), &mut output);
                if result.is_ok() {
                    for line in String::from_utf8(output).unwrap().lines() {
                        assert_eq!(
                            serde_json::from_str::<serde_json::Value>(line).unwrap()["ok"],
                            false
                        );
                    }
                }
                assert_eq!(fs::read(root.join(name)).unwrap(), bytes);
                fs::remove_dir_all(base).unwrap();
            }
        }
    }

    #[test]
    fn directory_aliases_resolve_before_parent_components() {
        let base = temp_dir("export-directory-alias");
        let store = Store::open(&base).unwrap();
        let root = store.with_storage(|state| state.data_dir.clone());
        store.save_library(&Library::default()).unwrap();
        let alias = base.join("junction");
        #[cfg(windows)]
        {
            let result = std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(&alias)
                .arg(root.join(COVERS_DIR))
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "junction creation: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join(COVERS_DIR), &alias).unwrap();
        store.with_storage(|state| {
            assert!(state
                .validate_external_outputs(&[alias.join("../settings.json")])
                .is_err());
            assert!(state
                .validate_external_outputs(&[alias.join("new-cover.jpg")])
                .is_err());
            for name in [
                "library.json.v0-backup",
                "settings.json.v0-backup",
                "library.json.corrupt-backup",
                "settings.json.1.2.tmp",
            ] {
                assert!(state.validate_external_outputs(&[root.join(name)]).is_err());
                assert!(!root.join(name).exists());
            }
            let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
            let cwd_parts: Vec<_> = cwd.components().collect();
            let root_parts: Vec<_> = root.components().collect();
            let common = cwd_parts
                .iter()
                .zip(&root_parts)
                .take_while(|(a, b)| same_component(a.as_os_str(), b.as_os_str()))
                .count();
            if common > 0 {
                let mut relative = PathBuf::new();
                for _ in common..cwd_parts.len() {
                    relative.push("..");
                }
                for part in &root_parts[common..] {
                    relative.push(part.as_os_str());
                }
                relative.push(SETTINGS_FILE);
                assert!(state.validate_external_outputs(&[relative]).is_err());
            }
        });
        #[cfg(windows)]
        fs::remove_dir(&alias).unwrap();
        #[cfg(unix)]
        fs::remove_file(&alias).unwrap();
        drop(store);
        fs::remove_dir_all(base).unwrap();
    }
}
