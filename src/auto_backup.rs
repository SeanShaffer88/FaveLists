//! Automatic backups (v0.3 program #19): the app keeps its own
//! rolling set of backup ZIPs — one a week, the newest four kept —
//! under the data folder's `backups/` directory, written with the
//! same backup machinery the manual export uses
//! (`backup::export_backup`, whole library scope). Restoring one is
//! the existing merge import; nothing here invents a second format.
//!
//! The two decision functions ([`is_due`], [`rotation_deletions`])
//! are pure and take plain data, so the schedule and the rotation
//! are testable without touching the filesystem.

use crate::model::Library;
use std::path::Path;

/// How many automatic backups are kept.
pub const AUTO_BACKUP_KEEP: usize = 4;
/// Minimum age of the newest backup before another is written.
pub const AUTO_BACKUP_INTERVAL_SECS: i64 = 7 * 86_400;

const FILE_PREFIX: &str = "auto-";
const FILE_SUFFIX: &str = ".zip";

/// Is a new automatic backup due at `now`, given the newest existing
/// backup's mtime (None = none yet, so one is due immediately)?
pub fn is_due(now: i64, newest: Option<i64>) -> bool {
    match newest {
        None => true,
        Some(t) => now - t >= AUTO_BACKUP_INTERVAL_SECS,
    }
}

/// Which files the rotation deletes: everything past the newest
/// `keep`, newest-first. The sort is stable, so entries with equal
/// mtimes keep their incoming (directory-listing) order — a file
/// listed earlier counts as the older of a tie.
pub fn rotation_deletions(entries: &[(String, i64)], keep: usize) -> Vec<String> {
    let mut sorted: Vec<&(String, i64)> = entries.iter().collect();
    sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
    sorted
        .iter()
        .skip(keep)
        .map(|(name, _)| name.clone())
        .collect()
}

/// File name for a backup taken at `now` (UTC): the timestamp makes
/// names sort in chronological order on their own.
pub fn file_name_for(now: i64) -> String {
    let date = crate::model::iso_date(now).replace('-', "");
    let secs = now.rem_euclid(86_400);
    format!(
        "{FILE_PREFIX}{date}-{:02}{:02}{:02}{FILE_SUFFIX}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// The automatic backups in `dir` as (file name, mtime in unix
/// seconds), sorted by name. A missing directory reads as empty.
pub fn list_auto_backups(dir: &Path) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(FILE_PREFIX) && name.ends_with(FILE_SUFFIX) {
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                out.push((name, mtime));
            }
        }
    }
    out.sort();
    out
}

/// The newest automatic backup's mtime, if any exist.
pub fn newest_auto_backup_time(dir: &Path) -> Option<i64> {
    list_auto_backups(dir).into_iter().map(|(_, t)| t).max()
}

/// Write one whole-library backup into `dir` (created if needed)
/// and apply the rotation. Returns the file name written. Shared by
/// the launch-time automatic backup and Settings' "Back up now" —
/// they differ only in what triggered them.
pub fn write_auto_backup(
    library: &Library,
    covers_dir: &Path,
    dir: &Path,
) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = file_name_for(crate::model::now_unix());
    crate::backup::export_backup(
        library,
        &crate::backup::BackupScope::WholeLibrary,
        covers_dir,
        &dir.join(&name),
    )
    .map_err(|e| e.to_string())?;
    for old in rotation_deletions(&list_auto_backups(dir), AUTO_BACKUP_KEEP) {
        let _ = std::fs::remove_file(dir.join(old));
    }
    Ok(name)
}
