//! Duplicate-finder support (v0.2 program #3): finding copies of the same
//! work that appear in more than one place in the library, so the user can
//! merge them into one kept copy.
//!
//! Matching design (the frozen semantics for this slice):
//!
//! * The group key is `(category, normalised title)`. The normalised title
//!   is [`crate::model::normalise_title`] (lower-case, punctuation turned
//!   into spaces, whitespace collapsed) with one leading article
//!   ("the" / "a" / "an") stripped — so "The Matrix", "Matrix" and
//!   "MATRIX!" land in one group, while works in different categories
//!   never group together.
//! * A group is **exact** when the copies' years also agree: every copy
//!   either has the same year/years field value (the category's
//!   [`Category::year_key`] field, compared as the trimmed raw string) or
//!   has no year at all. It is **possible** when two copies carry
//!   different non-empty years — same title, maybe different works
//!   (remakes, reboots), so the user decides.
//! * Provider ids deliberately play no part in the key: the same work
//!   added from a provider in one list and typed in manually in another
//!   has different `work_key`s but is exactly what the user wants to find
//!   here. (The in-list duplicate block in `model` keeps using
//!   `work_key`; this finder is the cross-list complement.)
//!
//! This module is pure data-shaping — no egui types — so it is
//! unit-testable off-Windows, mirroring `search.rs`. The registry command
//! `library.find_duplicates` runs [`find_duplicates`] and serialises the
//! groups; the GUI parses them back with [`groups_from_result`].

use crate::model::{normalise_title, Category, Library};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Whether a duplicate group's copies provably describe the same work
/// (`Exact`) or merely share a normalised title with conflicting years
/// (`Possible`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Exact,
    Possible,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::Exact => "exact",
            Confidence::Possible => "possible",
        }
    }

    pub fn from_id(s: &str) -> Option<Confidence> {
        match s {
            "exact" => Some(Confidence::Exact),
            "possible" => Some(Confidence::Possible),
            _ => None,
        }
    }
}

/// One copy of a duplicated work: where it lives and what it looks like.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DupCopy {
    pub list_id: u64,
    pub list_name: String,
    pub item_id: u64,
    pub title: String,
    /// The item's year/years field value, when it has one (display and
    /// confidence only).
    pub year: Option<String>,
}

/// A set of ≥2 copies of (probably) the same work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DupGroup {
    pub category: Category,
    /// Machine category id (`"movies"`, …), as emitted in results.
    pub category_id: String,
    /// The normalised title the copies were grouped by.
    pub key: String,
    pub confidence: Confidence,
    /// Copies in library order (list order, then in-list order).
    pub copies: Vec<DupCopy>,
}

impl DupGroup {
    /// The JSON shape the `library.find_duplicates` command returns for
    /// one group — the GUI parses it back with [`groups_from_result`].
    pub fn to_json(&self) -> Value {
        json!({
            "category": self.category_id,
            "key": self.key,
            "confidence": self.confidence.as_str(),
            "copies": self.copies.iter().map(|c| json!({
                "list_id": c.list_id,
                "list_name": c.list_name,
                "item_id": c.item_id,
                "title": c.title,
                "year": c.year,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Normalise a title for duplicate grouping: the model's
/// punctuation/whitespace normalisation plus stripping one leading
/// article ("the Matrix" → "matrix"). An article is stripped only as a
/// whole leading word, so "Thelma" keeps its "the".
pub fn normalise_work_title(title: &str) -> String {
    let base = normalise_title(title);
    for article in ["the ", "an ", "a "] {
        if let Some(rest) = base.strip_prefix(article) {
            return rest.to_string();
        }
    }
    base
}

/// Find every group of ≥2 copies of the same work in the library.
/// Groups come out in a deterministic order (category, then normalised
/// title); copies inside a group are in library order.
pub fn find_duplicates(library: &Library) -> Vec<DupGroup> {
    let mut by_key: BTreeMap<(Category, String), Vec<DupCopy>> = BTreeMap::new();
    for list in &library.lists {
        for item in &list.items {
            let key = normalise_work_title(&item.title);
            if key.is_empty() {
                // A title that is all punctuation has no identity to
                // group by; grouping them would be noise.
                continue;
            }
            let year = list
                .category
                .year_key()
                .and_then(|k| item.field(k))
                .map(|s| s.trim().to_string());
            by_key
                .entry((list.category, key))
                .or_default()
                .push(DupCopy {
                    list_id: list.id,
                    list_name: list.name.clone(),
                    item_id: item.id,
                    title: item.title.clone(),
                    year,
                });
        }
    }
    by_key
        .into_iter()
        .filter(|(_, copies)| copies.len() >= 2)
        .map(|((category, key), copies)| {
            // Exact when at most one distinct non-empty year is present;
            // two different non-empty years = a possible match only.
            let mut years = copies.iter().filter_map(|c| c.year.as_deref());
            let confidence = match years.next() {
                None => Confidence::Exact,
                Some(first) => {
                    if years.all(|y| y == first) {
                        Confidence::Exact
                    } else {
                        Confidence::Possible
                    }
                }
            };
            DupGroup {
                category,
                category_id: category.id().to_string(),
                key,
                confidence,
                copies,
            }
        })
        .collect()
}

/// Parse the `groups` array of a `library.find_duplicates` result back
/// into groups for the GUI's duplicates view. Malformed entries are
/// skipped rather than failing the whole view.
pub fn groups_from_result(result: &Value) -> Vec<DupGroup> {
    let Some(groups) = result.get("groups").and_then(Value::as_array) else {
        return Vec::new();
    };
    groups.iter().filter_map(group_from_json).collect()
}

fn group_from_json(g: &Value) -> Option<DupGroup> {
    let category_id = g.get("category")?.as_str()?.to_string();
    let category = Category::from_id(&category_id)?;
    let confidence = Confidence::from_id(g.get("confidence")?.as_str()?)?;
    let raw_copies = g.get("copies")?.as_array()?;
    let mut copies = Vec::new();
    for c in raw_copies {
        copies.push(DupCopy {
            list_id: c.get("list_id")?.as_u64()?,
            list_name: c
                .get("list_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            item_id: c.get("item_id")?.as_u64()?,
            title: c.get("title")?.as_str()?.to_string(),
            year: c.get("year").and_then(Value::as_str).map(|s| s.to_string()),
        });
    }
    if copies.len() < 2 {
        return None;
    }
    Some(DupGroup {
        category,
        category_id,
        key: g
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        confidence,
        copies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Item;

    #[test]
    fn normalise_strips_one_leading_article_only() {
        assert_eq!(normalise_work_title("The Matrix"), "matrix");
        assert_eq!(normalise_work_title("Matrix"), "matrix");
        assert_eq!(normalise_work_title("MATRIX!"), "matrix");
        assert_eq!(normalise_work_title("A Few Good Men"), "few good men");
        assert_eq!(normalise_work_title("An American Tail"), "american tail");
        assert_eq!(normalise_work_title("Thelma & Louise"), "thelma louise");
        assert_eq!(normalise_work_title("Spider-Man"), "spider man");
        assert_eq!(normalise_work_title("  the   DARK  knight "), "dark knight");
    }

    fn lib_with(entries: &[(&str, Category, &str, Option<&str>)]) -> Library {
        // (list name, category, title, year) — one list per distinct name.
        let mut lib = Library::default();
        for (list_name, cat, title, year) in entries {
            let list_id = match lib.lists.iter().find(|l| l.name == *list_name) {
                Some(l) => l.id,
                None => lib.new_list(*list_name, *cat, false),
            };
            let mut item = Item::new(lib.alloc_item_id(), *title);
            if let (Some(k), Some(y)) = (cat.year_key(), year) {
                item.fields.insert(k.to_string(), y.to_string());
            }
            // Direct push: fixture copies may deliberately be within-list
            // duplicates, which add_item would block.
            lib.list_mut(list_id).unwrap().items.push(item);
        }
        lib
    }

    #[test]
    fn exact_group_across_lists_and_possible_on_year_conflict() {
        let lib = lib_with(&[
            ("Films", Category::Movies, "The Matrix", Some("1999")),
            ("Watchlist", Category::Movies, "Matrix", Some("1999")),
            ("Films", Category::Movies, "Dune", Some("2021")),
            ("Watchlist", Category::Movies, "Dune", Some("1984")),
            ("Films", Category::Movies, "Arrival", Some("2016")),
        ]);
        let groups = find_duplicates(&lib);
        assert_eq!(groups.len(), 2);
        let matrix = groups.iter().find(|g| g.key == "matrix").unwrap();
        assert_eq!(matrix.confidence, Confidence::Exact);
        assert_eq!(matrix.copies.len(), 2);
        let dune = groups.iter().find(|g| g.key == "dune").unwrap();
        assert_eq!(dune.confidence, Confidence::Possible);
    }

    #[test]
    fn missing_year_on_some_copies_stays_exact() {
        let lib = lib_with(&[
            ("Films", Category::Movies, "Heat", Some("1995")),
            ("Watchlist", Category::Movies, "Heat", None),
        ]);
        let groups = find_duplicates(&lib);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].confidence, Confidence::Exact);
    }

    #[test]
    fn category_separates_groups_and_no_false_positives() {
        let lib = lib_with(&[
            ("Films", Category::Movies, "Dune", Some("2021")),
            ("Bookshelf", Category::Books, "Dune", Some("1965")),
            ("Films", Category::Movies, "Arrival", Some("2016")),
            ("Watchlist", Category::Movies, "Brazil", Some("1985")),
        ]);
        // Same title in different categories never groups; singletons
        // never group.
        assert!(find_duplicates(&lib).is_empty());
    }

    #[test]
    fn within_list_copies_are_reported() {
        let lib = lib_with(&[
            ("Films", Category::Movies, "Heat", Some("1995")),
            ("Films", Category::Movies, "heat", Some("1995")),
        ]);
        let groups = find_duplicates(&lib);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].copies.len(), 2);
        assert_eq!(groups[0].copies[0].list_id, groups[0].copies[1].list_id);
    }

    #[test]
    fn result_round_trips_through_json() {
        let lib = lib_with(&[
            ("Films", Category::Movies, "The Matrix", Some("1999")),
            ("Watchlist", Category::Movies, "Matrix", Some("1999")),
        ]);
        let groups = find_duplicates(&lib);
        let result = json!({"groups": groups.iter().map(DupGroup::to_json).collect::<Vec<_>>()});
        assert_eq!(groups_from_result(&result), groups);
        assert_eq!(groups_from_result(&json!({})), Vec::new());
    }
}
