//! CSV / JSON data export (v0.3 program #6): the round-trip-out
//! counterpart to the v0.2 CSV import. A single list exports as CSV
//! with the category's own field columns; the whole library exports
//! as CSV (one row per item across all lists) or as JSON — the
//! library's own serialisation, exactly the shape the store writes,
//! so an exported JSON file can be read straight back.
//!
//! The CSV writer follows RFC 4180, matching the reader in
//! [`crate::csv_import`]: a field is quoted when it contains a comma,
//! a quote, CR or LF; embedded quotes are doubled; records end CRLF.
//! Pure string shaping plus thin file writers — no egui types — so it
//! is unit-testable off-Windows, like the other exports.

use crate::model::{Category, FavList, Item, Library};
use std::path::Path;

/// Escape one CSV field per RFC 4180.
pub fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        for ch in value.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
        out
    } else {
        value.to_string()
    }
}

/// Join one record's cells and terminate it (CRLF).
fn csv_record(cells: &[String]) -> String {
    let mut out = cells
        .iter()
        .map(|c| csv_escape(c))
        .collect::<Vec<_>>()
        .join(",");
    out.push_str("\r\n");
    out
}

/// The columns every item row ends with, after the category field
/// columns: the personal data and provenance the CSV import cannot
/// carry, in a stable order.
const TAIL_COLUMNS: [&str; 7] = [
    "rating",
    "note",
    "tags",
    "status",
    "provider_id",
    "source",
    "added_at",
];

/// The tail cells for one item. Tags join with ", " (the field is
/// quoted as a whole, so the commas stay inside one cell).
fn tail_cells(item: &Item) -> Vec<String> {
    vec![
        item.rating.map(|r| r.to_string()).unwrap_or_default(),
        item.note.clone(),
        item.tags.join(", "),
        item.status.map(|s| s.id().to_string()).unwrap_or_default(),
        item.provider_id.clone().unwrap_or_default(),
        item.source.clone(),
        item.added_at.to_string(),
    ]
}

/// A category field cell: the item's value for `key`, or "" — values
/// are read through `Item::field`, so whitespace-only values count as
/// absent, exactly as elsewhere in the app.
fn field_cell(item: &Item, key: &str) -> String {
    item.field(key).unwrap_or_default().to_string()
}

/// Extra field keys carried by `items` beyond `base` (and beyond the
/// tail columns) — e.g. the `caffeine` field drink items carry, which
/// is not part of the category field set. Appended to the columns so
/// no item field is silently dropped from an export.
fn extra_keys(items: &[Item], base: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        for k in item.fields.keys() {
            if !base.contains(k) && !TAIL_COLUMNS.contains(&k.as_str()) && !out.contains(k) {
                out.push(k.clone());
            }
        }
    }
    out
}

/// One list as CSV text: `title`, the category's field keys in
/// field-set order, then any extra fields items carry (see
/// [`extra_keys`]), then the tail columns; one row per item in list
/// order.
pub fn list_csv(list: &FavList) -> String {
    let mut keys: Vec<String> = list
        .category
        .fields()
        .iter()
        .map(|f| f.key.to_string())
        .collect();
    keys.extend(extra_keys(&list.items, &keys));
    let mut out = String::new();
    let mut header = vec!["title".to_string()];
    header.extend(keys.iter().cloned());
    header.extend(TAIL_COLUMNS.iter().map(|s| s.to_string()));
    out.push_str(&csv_record(&header));
    for item in &list.items {
        let mut cells = vec![item.title.clone()];
        cells.extend(keys.iter().map(|k| field_cell(item, k)));
        cells.extend(tail_cells(item));
        out.push_str(&csv_record(&cells));
    }
    out
}

/// The whole library as CSV text: `list,category` columns first
/// (list name, category id), then the union of every present
/// category's field keys — in `Category::ALL` order, field-set order
/// within a category, each key once — plus any extra fields items
/// carry (see [`extra_keys`]), then the tail columns. Rows
/// follow library order: each list's items in list order. (The
/// Podcasts category has a detail field literally keyed `category`;
/// in this union it appears as a later column of its own — readers
/// addressing columns by position are unaffected.)
pub fn library_csv(library: &Library) -> String {
    let mut keys: Vec<String> = Vec::new();
    for cat in Category::ALL {
        if library.lists.iter().any(|l| l.category == cat) {
            for f in cat.fields() {
                let k = f.key.to_string();
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
    }
    for list in &library.lists {
        keys.extend(extra_keys(&list.items, &keys));
    }
    let mut out = String::new();
    let mut header = vec![
        "list".to_string(),
        "category".to_string(),
        "title".to_string(),
    ];
    header.extend(keys.iter().cloned());
    header.extend(TAIL_COLUMNS.iter().map(|s| s.to_string()));
    out.push_str(&csv_record(&header));
    for list in &library.lists {
        for item in &list.items {
            let mut cells = vec![
                list.name.clone(),
                list.category.id().to_string(),
                item.title.clone(),
            ];
            cells.extend(keys.iter().map(|k| field_cell(item, k)));
            cells.extend(tail_cells(item));
            out.push_str(&csv_record(&cells));
        }
    }
    out
}

/// Write one list's CSV to `path`. Returns the number of item rows.
pub fn write_list_csv(list: &FavList, path: &Path) -> std::io::Result<usize> {
    std::fs::write(path, list_csv(list))?;
    Ok(list.items.len())
}

/// Write the whole library's CSV to `path`. Returns (lists, items).
pub fn write_library_csv(library: &Library, path: &Path) -> std::io::Result<(usize, usize)> {
    std::fs::write(path, library_csv(library))?;
    Ok((library.lists.len(), library.total_items()))
}

/// Write the library as JSON — its own pretty serialisation, the
/// same shape the store writes. Returns (lists, items).
pub fn write_library_json(library: &Library, path: &Path) -> std::io::Result<(usize, usize)> {
    let text = serde_json::to_string_pretty(library)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)?;
    Ok((library.lists.len(), library.total_items()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ItemStatus, Library};

    #[test]
    fn escape_quotes_only_when_needed() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("two\nlines"), "\"two\nlines\"");
        assert_eq!(csv_escape(""), "");
    }

    #[test]
    fn library_csv_union_columns_and_rows() {
        let mut lib = Library::default();
        let films = lib.new_list("Films", Category::Movies, false);
        let shelf = lib.new_list("Shelf", Category::Books, false);
        let mut heat = Item::new(lib.alloc_item_id(), "Heat");
        heat.fields.insert("year".to_string(), "1995".to_string());
        heat.status = Some(ItemStatus::Owned);
        let mut dune = Item::new(lib.alloc_item_id(), "Dune");
        dune.fields
            .insert("author".to_string(), "Frank Herbert".to_string());
        lib.list_mut(films).unwrap().items.push(heat);
        lib.list_mut(shelf).unwrap().items.push(dune);
        let text = library_csv(&lib);
        let mut lines = text.split("\r\n");
        // Union in Category::ALL order: Movies keys first, then the
        // Books keys that are new (author, pages).
        assert_eq!(
            lines.next().unwrap(),
            "list,category,title,year,genre,director,runtime,cast,author,pages,\
             rating,note,tags,status,provider_id,source,added_at"
        );
        let row1 = lines.next().unwrap();
        assert!(row1.starts_with("Films,movies,Heat,1995,"), "{row1}");
        assert!(row1.contains(",owned,"), "{row1}");
        let row2 = lines.next().unwrap();
        assert!(row2.starts_with("Shelf,books,Dune,"), "{row2}");
        assert!(row2.contains("Frank Herbert"), "{row2}");
    }

    #[test]
    fn list_csv_appends_extra_item_fields_like_caffeine() {
        let mut lib = Library::default();
        let id = lib.new_list("Energy", Category::EnergyDrinks, false);
        let item_id = lib.alloc_item_id();
        let mut item = Item::new(item_id, "Zap Cola");
        item.fields.insert("brand".to_string(), "Zap".to_string());
        item.fields
            .insert("caffeine".to_string(), "32 mg/100ml".to_string());
        lib.list_mut(id).unwrap().items.push(item);
        let text = list_csv(lib.list(id).unwrap());
        let header = text.lines().next().unwrap();
        assert!(
            header.starts_with("title,brand,flavor,caffeine,rating,"),
            "{header}"
        );
        assert!(text.contains("32 mg/100ml"), "{text}");
    }

    #[test]
    fn library_json_is_the_store_shape() {
        let mut lib = Library::default();
        let id = lib.new_list("Films", Category::Movies, true);
        let item_id = lib.alloc_item_id();
        lib.list_mut(id)
            .unwrap()
            .items
            .push(Item::new(item_id, "Heat"));
        let dir =
            std::env::temp_dir().join(format!("favelists-export-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.json");
        let (lists, items) = write_library_json(&lib, &path).unwrap();
        assert_eq!((lists, items), (1, 1));
        let back: Library = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back, lib);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
