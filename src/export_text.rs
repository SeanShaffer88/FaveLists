//! Text (.txt) and Markdown (.md) list exports (spec §7).
//!
//! Both formats share one content model: a header (list name, category,
//! item count, export date) followed by one block per item — numbered for
//! ranked lists, in list order otherwise — with only the detail fields the
//! user ticked. Markdown additionally writes the covers into an
//! `<name>_assets/` folder next to the .md and references them with relative
//! paths.

use crate::model::{iso_date, now_unix, FavList, Item};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The export pseudo-fields: the per-entry personal rating and note
/// (v0.2 program #5) and tags and status (v0.3 program #4/#5). They are
/// not category fields — they read from the item itself — and they always
/// sort AFTER the category fields in the export checklist and the
/// rendered document.
pub const PSEUDO_FIELDS: [crate::model::FieldDef; 4] = [
    crate::model::FieldDef {
        key: "rating",
        label: "Rating",
    },
    crate::model::FieldDef {
        key: "note",
        label: "Note",
    },
    crate::model::FieldDef {
        key: "tags",
        label: "Tags",
    },
    crate::model::FieldDef {
        key: "status",
        label: "Status",
    },
];

/// Every field an export checklist may offer for this list: the category
/// fields followed by the pseudo-fields.
pub fn all_field_defs(list: &FavList) -> Vec<crate::model::FieldDef> {
    let mut out: Vec<crate::model::FieldDef> = list.category.fields().to_vec();
    out.extend(PSEUDO_FIELDS);
    out
}

/// The fields to include in a text export, in category order with the
/// pseudo-fields last. Built by the GUI from the checklist (and persisted
/// via Settings), consumed here. (`FieldDef` holds only `&'static str`, so
/// no lifetime is needed.)
pub fn selected_field_defs(
    list: &FavList,
    selected_keys: &[String],
) -> Vec<crate::model::FieldDef> {
    all_field_defs(list)
        .into_iter()
        .filter(|f| selected_keys.iter().any(|k| k == f.key))
        .collect()
}

fn item_lines(
    item: &Item,
    fields: &[crate::model::FieldDef],
    markdown: bool,
    cover_ref: Option<&str>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if markdown {
        if let Some(r) = cover_ref {
            lines.push(format!("![cover]({r})"));
            lines.push(String::new());
        }
    }
    for f in fields {
        // Pseudo-fields render from the item, not the field map, and only
        // when the item actually carries them.
        match f.key {
            "rating" => {
                if let Some(r) = item.rating {
                    lines.push(format!("Rating: {r}/10"));
                }
            }
            "note" => {
                let note = item.note.trim();
                if !note.is_empty() {
                    lines.push(format!("Note: {note}"));
                }
            }
            "tags" => {
                if !item.tags.is_empty() {
                    lines.push(format!("Tags: {}", item.tags.join(", ")));
                }
            }
            "status" => {
                if let Some(s) = item.status {
                    lines.push(format!("Status: {}", s.id()));
                }
            }
            _ => {
                if let Some(v) = item.field(f.key) {
                    lines.push(format!("{}: {}", f.label, v));
                }
            }
        }
    }
    lines
}

/// Shared core: returns the document text. `cover_ref` maps an item id to
/// the relative asset path for Markdown mode (None in .txt mode).
fn build_document(
    list: &FavList,
    selected_keys: &[String],
    markdown: bool,
    cover_ref: &dyn Fn(&Item) -> Option<String>,
    date_unix: i64,
) -> String {
    let fields = selected_field_defs(list, selected_keys);
    let mut out = String::new();
    if markdown {
        out.push_str(&format!("# {}\n\n", list.name));
    } else {
        out.push_str(&format!(
            "{}\n{}\n\n",
            list.name,
            "=".repeat(list.name.chars().count())
        ));
    }
    out.push_str(&format!("Category: {}\n", list.category.name()));
    out.push_str(&format!("Items: {}\n", list.items.len()));
    out.push_str(&format!("Exported: {}\n", iso_date(date_unix)));
    out.push('\n');
    for (i, item) in list.items.iter().enumerate() {
        if list.ranked {
            if markdown {
                out.push_str(&format!("## {}. {}\n\n", i + 1, item.title));
            } else {
                out.push_str(&format!("{}. {}\n", i + 1, item.title));
            }
        } else if markdown {
            out.push_str(&format!("## {}\n\n", item.title));
        } else {
            out.push_str(&format!("{}\n", item.title));
        }
        let lines = item_lines(item, &fields, markdown, cover_ref(item).as_deref());
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// Build a .txt export string (used directly by tests and the GUI writer).
pub fn build_txt(list: &FavList, selected_keys: &[String]) -> String {
    build_document(list, selected_keys, false, &|_| None, now_unix())
}

/// Build a Markdown export string; `cover_ref` resolves asset paths.
pub fn build_md(
    list: &FavList,
    selected_keys: &[String],
    cover_ref: &dyn Fn(&Item) -> Option<String>,
) -> String {
    build_document(list, selected_keys, true, cover_ref, now_unix())
}

/// Write a .txt export to `path`.
pub fn write_txt(list: &FavList, selected_keys: &[String], path: &Path) -> io::Result<()> {
    fs::write(path, build_txt(list, selected_keys))
}

/// Write a .md export to `path`, copying covers into `<stem>_assets/`
/// beside it. `covers_dir` is the data folder's cover cache. Returns the
/// asset paths written (relative refs as used inside the document).
pub fn write_md(
    list: &FavList,
    selected_keys: &[String],
    path: &Path,
    covers_dir: &Path,
) -> io::Result<Vec<PathBuf>> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("favelists")
        .to_string();
    let assets_name = format!("{stem}_assets");
    let assets_dir = path.parent().unwrap_or(Path::new(".")).join(&assets_name);
    let mut written = Vec::new();
    // First pass: copy assets so the document can reference what exists.
    let mut refs: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
    for item in &list.items {
        if let Some(cover) = &item.cover {
            let src = covers_dir.join(cover);
            if src.exists() {
                fs::create_dir_all(&assets_dir)?;
                let dest = assets_dir.join(cover);
                if fs::copy(&src, &dest).is_ok() {
                    refs.insert(item.id, format!("{assets_name}/{cover}"));
                    written.push(dest);
                }
            }
        }
    }
    let text = build_md(list, selected_keys, &|item: &Item| {
        refs.get(&item.id).cloned()
    });
    fs::write(path, text)?;
    Ok(written)
}

/// Filesystem-safe file stem from a list name ("My Films!" -> "My_Films").
pub fn safe_file_stem(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "favelists".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Item};

    fn sample_list(ranked: bool) -> FavList {
        let mut list = FavList::new(1, "Best Sci-Fi Films", Category::Movies, ranked);
        let mut a = Item::new(1, "Alien");
        a.fields.insert("year".into(), "1979".into());
        a.fields
            .insert("genre".into(), "Horror, Science Fiction".into());
        a.fields.insert("director".into(), "Ridley Scott".into());
        a.fields.insert("runtime".into(), "117 min".into());
        a.fields.insert("cast".into(), "Sigourney Weaver".into());
        a.cover = Some("cover-1.jpg".into());
        let mut b = Item::new(2, "Arrival");
        b.fields.insert("year".into(), "2016".into());
        b.fields
            .insert("director".into(), "Denis Villeneuve".into());
        list.add_item(a);
        list.add_item(b);
        list
    }

    fn all_keys(list: &FavList) -> Vec<String> {
        list.category
            .fields()
            .iter()
            .map(|f| f.key.to_string())
            .collect()
    }

    #[test]
    fn txt_header_and_ranked_numbering() {
        let list = sample_list(true);
        let text = build_txt(&list, &all_keys(&list));
        assert!(text.starts_with("Best Sci-Fi Films\n"));
        assert!(text.contains("Category: Movies"));
        assert!(text.contains("Items: 2"));
        assert!(text.contains("Exported: "));
        assert!(text.contains("1. Alien"));
        assert!(text.contains("Director: Ridley Scott"));
        assert!(text.contains("2. Arrival"));
    }

    #[test]
    fn txt_respects_field_selection() {
        let list = sample_list(false);
        let text = build_txt(&list, &["director".to_string()]);
        assert!(text.contains("Director: Ridley Scott"));
        assert!(!text.contains("Runtime:"));
        assert!(!text.contains("1. Alien")); // unranked: no numbering
        assert!(text.contains("Alien"));
    }

    #[test]
    fn md_includes_relative_cover_refs_and_assets() {
        let list = sample_list(true);
        let dir = std::env::temp_dir().join(format!("favelists-md-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let covers = dir.join("covers");
        fs::create_dir_all(&covers).unwrap();
        fs::write(covers.join("cover-1.jpg"), b"jpeg-bytes").unwrap();
        let md_path = dir.join("best.md");
        let written = write_md(&list, &all_keys(&list), &md_path, &covers).unwrap();
        assert_eq!(written.len(), 1);
        let text = fs::read_to_string(&md_path).unwrap();
        assert!(text.contains("# Best Sci-Fi Films"));
        assert!(text.contains("## 1. Alien"));
        assert!(text.contains("![cover](best_assets/cover-1.jpg)"));
        assert!(dir.join("best_assets/cover-1.jpg").exists());
        // Arrival has no cover -> no image line for it.
        assert_eq!(text.matches("![cover]").count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rating_and_note_render_only_when_selected_and_present() {
        let mut list = sample_list(true);
        list.items[0].rating = Some(8);
        list.items[0].note = "  Saw it opening night. ".into();
        let mut keys = all_keys(&list);
        // Not selected: no trace of either pseudo-field.
        let text = build_txt(&list, &keys);
        assert!(!text.contains("Rating:"));
        assert!(!text.contains("Note:"));
        keys.push("rating".to_string());
        keys.push("note".to_string());
        let text = build_txt(&list, &keys);
        assert!(text.contains("Rating: 8/10"));
        assert!(text.contains("Note: Saw it opening night.")); // trimmed
                                                               // The unrated, noteless second item renders nothing extra.
        assert_eq!(text.matches("Rating:").count(), 1);
        assert_eq!(text.matches("Note:").count(), 1);
        // Pseudo-fields come after the category fields.
        let pos_director = text.find("Director: Ridley Scott").unwrap();
        let pos_rating = text.find("Rating: 8/10").unwrap();
        assert!(pos_director < pos_rating);
        // Markdown renders them too.
        let md = build_md(&list, &keys, &|_| None);
        assert!(md.contains("Rating: 8/10"));
        assert!(md.contains("Note: Saw it opening night."));
    }

    #[test]
    fn safe_stems() {
        assert_eq!(safe_file_stem("My Films!"), "My_Films");
        assert_eq!(safe_file_stem("  "), "favelists");
        assert_eq!(safe_file_stem("Top 10: Games"), "Top_10_Games");
    }
}
