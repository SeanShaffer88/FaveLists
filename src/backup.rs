//! Library backup (spec §7): export a scope of lists to a single ZIP
//! (a `library.json` payload plus the cached cover files), and import by
//! MERGING — import adds everything from the file, never deletes or
//! replaces existing data, and skips a work that is already in the same
//! list (matched by list name + category for the target list).

use crate::model::{Category, Library};
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

/// What a backup export includes.
#[derive(Debug, Clone, PartialEq)]
pub enum BackupScope {
    SingleList(u64),
    SelectedLists(Vec<u64>),
    WholeLibrary,
    Category(Category),
}

impl BackupScope {
    pub fn label(&self) -> String {
        match self {
            BackupScope::SingleList(_) => "Single list".to_string(),
            BackupScope::SelectedLists(ids) => format!("{} selected lists", ids.len()),
            BackupScope::WholeLibrary => "Entire library".to_string(),
            BackupScope::Category(c) => format!("All lists in {}", c.name()),
        }
    }

    fn includes(&self, list_id: u64, category: Category) -> bool {
        match self {
            BackupScope::SingleList(id) => *id == list_id,
            BackupScope::SelectedLists(ids) => ids.contains(&list_id),
            BackupScope::WholeLibrary => true,
            BackupScope::Category(c) => *c == category,
        }
    }
}

/// The payload stored inside the ZIP.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct BackupPayload {
    app: String,
    version: u32,
    library: Library,
}

const PAYLOAD_NAME: &str = "library.json";

/// Export the scoped lists (with their covers) from `library` to a ZIP at
/// `zip_path`. `covers_dir` is the data folder's cover cache. Returns how
/// many lists were written.
pub fn export_backup(
    library: &Library,
    scope: &BackupScope,
    covers_dir: &Path,
    zip_path: &Path,
) -> io::Result<usize> {
    let chosen: Vec<&crate::model::FavList> = library
        .lists
        .iter()
        .filter(|l| scope.includes(l.id, l.category))
        .collect();
    // Build a payload library containing only the chosen lists.
    let payload_lib = Library {
        lists: chosen.iter().map(|l| (*l).clone()).collect(),
        next_list_id: library.next_list_id,
        next_item_id: library.next_item_id,
    };
    let payload = BackupPayload {
        app: "FaveLists".to_string(),
        version: 1,
        library: payload_lib,
    };
    let text = serde_json::to_string_pretty(&payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let file = fs::File::create(zip_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(PAYLOAD_NAME, options)?;
    zip.write_all(text.as_bytes())?;
    let mut cover_names: Vec<&str> = Vec::new();
    for list in &chosen {
        for item in &list.items {
            if let Some(cover) = &item.cover {
                if !cover_names.contains(&cover.as_str()) {
                    cover_names.push(cover.as_str());
                }
            }
        }
    }
    for cover in cover_names {
        let src = covers_dir.join(cover);
        if let Ok(bytes) = fs::read(&src) {
            zip.start_file(format!("covers/{cover}"), options)?;
            zip.write_all(&bytes)?;
        }
    }
    zip.finish()?;
    Ok(chosen.len())
}

/// A parsed backup: the payload library plus `(file name, bytes)` covers.
pub type BackupContents = (Library, Vec<(String, Vec<u8>)>);

/// Read a backup ZIP into its payload library + cover bytes.
fn read_backup(zip_path: &Path) -> io::Result<BackupContents> {
    let file = fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut payload: Option<Library> = None;
    let mut covers = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_string();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        if name == PAYLOAD_NAME {
            let parsed: BackupPayload = serde_json::from_slice(&buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if parsed.app != "FaveLists" {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "not a FaveLists backup",
                ));
            }
            payload = Some(parsed.library);
        } else if let Some(rest) = name.strip_prefix("covers/") {
            covers.push((rest.to_string(), buf));
        }
    }
    let payload = payload
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "backup has no library.json"))?;
    Ok((payload, covers))
}

/// Outcome of a merge import.
#[derive(Debug, Default, PartialEq)]
pub struct ImportReport {
    pub lists_added: usize,
    pub lists_merged: usize,
    pub items_added: usize,
    pub items_skipped_as_duplicates: usize,
    pub covers_written: usize,
}

/// Import a backup ZIP by merging into `library` (spec §7):
/// * a backup list whose name+category matches an existing list merges into
///   it; otherwise it is added as a new list (with a fresh id);
/// * items already in the target list (same work) are skipped;
/// * item ids are re-allocated from the target library's counter;
/// * covers are written into `covers_dir` when missing;
/// * existing data is NEVER deleted or replaced.
pub fn import_backup(
    library: &mut Library,
    covers_dir: &Path,
    zip_path: &Path,
) -> io::Result<ImportReport> {
    let (incoming, covers) = read_backup(zip_path)?;
    let mut report = ImportReport::default();
    fs::create_dir_all(covers_dir)?;
    for (name, bytes) in &covers {
        let dest = covers_dir.join(name);
        if !dest.exists() {
            fs::write(&dest, bytes)?;
            report.covers_written += 1;
        }
    }
    for mut list in incoming.lists {
        let target_id = library
            .lists
            .iter()
            .find(|l| l.name == list.name && l.category == list.category)
            .map(|l| l.id);
        let target_id = match target_id {
            Some(id) => {
                report.lists_merged += 1;
                id
            }
            None => {
                let id = library.next_list_id;
                library.next_list_id += 1;
                let mut new_list =
                    crate::model::FavList::new(id, list.name.clone(), list.category, list.ranked);
                new_list.created_at = list.created_at;
                library.lists.push(new_list);
                report.lists_added += 1;
                id
            }
        };
        // Decide what joins the target list without holding a borrow on the
        // library: track the work keys already present (plus the ones we
        // accept), and hand out fresh ids from a local counter.
        let mut seen_keys: std::collections::HashSet<String> = library
            .list(target_id)
            .expect("target list exists")
            .items
            .iter()
            .map(|i| i.work_key())
            .collect();
        let mut next_id = library.next_item_id;
        let mut accepted: Vec<crate::model::Item> = Vec::new();
        for mut item in list.items.drain(..) {
            if !seen_keys.insert(item.work_key()) {
                report.items_skipped_as_duplicates += 1;
                continue;
            }
            item.id = next_id;
            next_id += 1;
            accepted.push(item);
        }
        library.next_item_id = next_id;
        let target = library.list_mut(target_id).expect("target list exists");
        for item in accepted {
            // Cannot be a duplicate after the key check above, but keep the
            // accounting honest if that ever changes.
            match target.add_item(item) {
                crate::model::AddOutcome::Added => report.items_added += 1,
                crate::model::AddOutcome::Duplicate => {
                    report.items_skipped_as_duplicates += 1;
                }
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FavList, Item};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        // Unique per call: several tests reuse the same tags in parallel.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "favelists-backup-{}-{}-{}",
            tag,
            std::process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_library() -> (Library, std::path::PathBuf) {
        let covers = temp_dir("covers");
        let mut lib = Library::default();
        let films = lib.new_list("Films", Category::Movies, true);
        let mut alien = Item::new(lib.alloc_item_id(), "Alien");
        alien.provider_id = Some("348".into());
        alien.source = "tmdb".into();
        alien.cover = Some("cover-1.jpg".into());
        lib.list_mut(films).unwrap().add_item(alien);
        let sodas = lib.new_list("Sodas", Category::Sodas, false);
        let fanta_id = lib.alloc_item_id();
        lib.list_mut(sodas)
            .unwrap()
            .add_item(Item::new(fanta_id, "Fanta"));
        fs::write(covers.join("cover-1.jpg"), b"img").unwrap();
        (lib, covers)
    }

    #[test]
    fn scope_filtering() {
        let (lib, covers) = sample_library();
        let dir = temp_dir("scope");
        let out = dir.join("all.zip");
        let n = export_backup(&lib, &BackupScope::WholeLibrary, &covers, &out).unwrap();
        assert_eq!(n, 2);
        let out2 = dir.join("movies.zip");
        let n2 = export_backup(
            &lib,
            &BackupScope::Category(Category::Movies),
            &covers,
            &out2,
        )
        .unwrap();
        assert_eq!(n2, 1);
        let (payload, payload_covers) = read_backup(&out2).unwrap();
        assert_eq!(payload.lists.len(), 1);
        assert_eq!(payload.lists[0].category, Category::Movies);
        assert_eq!(payload_covers.len(), 1);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&covers);
    }

    #[test]
    fn import_merges_and_skips_duplicates() {
        let (lib, covers) = sample_library();
        let dir = temp_dir("merge");
        let zip_path = dir.join("backup.zip");
        export_backup(&lib, &BackupScope::WholeLibrary, &covers, &zip_path).unwrap();

        // Importing into the SAME library: everything is a duplicate.
        let mut same = lib.clone();
        let report = import_backup(&mut same, &covers, &zip_path).unwrap();
        assert_eq!(report.items_added, 0);
        assert_eq!(report.items_skipped_as_duplicates, 2);
        assert_eq!(report.lists_merged, 2);
        assert_eq!(same.total_items(), 2);

        // Importing into an empty library adds all.
        let mut fresh = Library::default();
        let fresh_covers = temp_dir("fresh-covers");
        let report2 = import_backup(&mut fresh, &fresh_covers, &zip_path).unwrap();
        assert_eq!(report2.lists_added, 2);
        assert_eq!(report2.items_added, 2);
        assert_eq!(report2.covers_written, 1);
        assert_eq!(fresh.total_items(), 2);
        // Ids were re-allocated, unique within the fresh library.
        let ids: Vec<u64> = fresh
            .lists
            .iter()
            .flat_map(|l| l.items.iter().map(|i| i.id))
            .collect();
        let mut dedup = ids.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(ids.len(), dedup.len());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&covers);
        let _ = fs::remove_dir_all(&fresh_covers);
    }

    #[test]
    fn import_partial_overlap_adds_only_new_items() {
        let (lib, covers) = sample_library();
        let dir = temp_dir("partial");
        let zip_path = dir.join("b.zip");
        export_backup(&lib, &BackupScope::WholeLibrary, &covers, &zip_path).unwrap();

        let mut target = Library::default();
        let films = target.new_list("Films", Category::Movies, true);
        let mut alien = Item::new(target.alloc_item_id(), "Alien");
        alien.provider_id = Some("348".into());
        alien.source = "tmdb".into();
        target.list_mut(films).unwrap().add_item(alien);
        // Films list already has Alien -> skip; Sodas list is new -> add.
        let report = import_backup(&mut target, &covers, &zip_path).unwrap();
        assert_eq!(report.lists_merged, 1);
        assert_eq!(report.lists_added, 1);
        assert_eq!(report.items_added, 1);
        assert_eq!(report.items_skipped_as_duplicates, 1);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&covers);
    }

    #[test]
    fn import_rejects_non_favelists_zip() {
        let dir = temp_dir("bad");
        let path = dir.join("bad.zip");
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("library.json", options).unwrap();
        zip.write_all(br#"{"app":"Other","version":1,"library":{"lists":[],"next_list_id":1,"next_item_id":1}}"#).unwrap();
        zip.finish().unwrap();
        let mut lib = Library::default();
        assert!(import_backup(&mut lib, &dir, &path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn single_list_scope() {
        let (mut lib, covers) = sample_library();
        let extra = lib.new_list("Games", Category::VideoGames, false);
        let hades_id = lib.alloc_item_id();
        lib.list_mut(extra)
            .unwrap()
            .add_item(Item::new(hades_id, "Hades"));
        let dir = temp_dir("single");
        let zip_path = dir.join("one.zip");
        let n = export_backup(&lib, &BackupScope::SingleList(extra), &covers, &zip_path).unwrap();
        assert_eq!(n, 1);
        let (payload, _) = read_backup(&zip_path).unwrap();
        assert_eq!(payload.lists[0].name, "Games");
        let _ = FavList::new(0, "x", Category::Books, false); // type sanity
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&covers);
    }
}
