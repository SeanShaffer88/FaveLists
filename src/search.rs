//! Global search support (v0.2 program #2): turning a `library.search`
//! command result into flat hits and list-grouped hits for the GUI's
//! results view. The search itself lives in the command registry
//! (`commands::cmd_library_search`); this module is pure presentation
//! data-shaping — no egui types — so it is unit-testable off-Windows.
//! The JSON channel and any future surface can shape results the same way.

use crate::model::Category;
use serde_json::Value;

/// One global-search hit, flattened from a `library.search` match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub list_id: u64,
    pub list_name: String,
    pub category: Category,
    /// Machine category id (`"movies"`, …) as it arrived in the result.
    pub category_id: String,
    pub item_id: u64,
    pub title: String,
    /// The item's year/years field value, when it has one (display only).
    pub year: Option<String>,
}

/// Parse the `matches` array of a `library.search` result into hits.
/// Malformed entries are skipped rather than failing the whole view.
pub fn hits_from_result(result: &Value) -> Vec<SearchHit> {
    let Some(matches) = result.get("matches").and_then(Value::as_array) else {
        return Vec::new();
    };
    matches.iter().filter_map(hit_from_match).collect()
}

fn hit_from_match(m: &Value) -> Option<SearchHit> {
    let list_id = m.get("list_id")?.as_u64()?;
    let item_id = m.get("item_id")?.as_u64()?;
    let title = m.get("title")?.as_str()?.to_string();
    let category_id = m
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let category = Category::from_id(&category_id)?;
    let year = list_category_year(m, category);
    Some(SearchHit {
        list_id,
        list_name: m
            .get("list_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        category,
        category_id,
        item_id,
        title,
        year,
    })
}

fn list_category_year(m: &Value, category: Category) -> Option<String> {
    let key = category.year_key()?;
    m.get("item")?
        .get("fields")?
        .get(key)?
        .as_str()
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

/// Group hit indices by list, preserving first-appearance order of the
/// lists (the registry emits matches in library order, so groups come out
/// in sidebar order). Returns `(list_id, hit indices)` pairs.
pub fn grouped(hits: &[SearchHit]) -> Vec<(u64, Vec<usize>)> {
    let mut groups: Vec<(u64, Vec<usize>)> = Vec::new();
    for (i, hit) in hits.iter().enumerate() {
        match groups.iter_mut().find(|(id, _)| *id == hit.list_id) {
            Some((_, idxs)) => idxs.push(i),
            None => groups.push((hit.list_id, vec![i])),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_result() -> Value {
        json!({"matches": [
            {"list_id": 1, "list_name": "Best Sci-Fi", "category": "movies",
             "index": 0, "item_id": 1, "title": "Alien",
             "item": {"id": 1, "title": "Alien",
                      "fields": {"year": "1979", "genre": "Horror"}}},
            {"list_id": 2, "list_name": "Sci-Fi Books", "category": "books",
             "index": 0, "item_id": 2, "title": "Dune",
             "item": {"id": 2, "title": "Dune",
                      "fields": {"year": "1965", "genre": "Sci-Fi"}}},
            {"list_id": 1, "list_name": "Best Sci-Fi", "category": "movies",
             "index": 1, "item_id": 3, "title": "Arrival",
             "item": {"id": 3, "title": "Arrival",
                      "fields": {"year": "2016", "genre": "Sci-Fi"}}},
            {"list_id": 9, "list_name": "Broken", "category": "anime",
             "index": 0, "item_id": 4, "title": "Nope", "item": {"id": 4}},
        ]})
    }

    #[test]
    fn parses_hits_and_skips_malformed() {
        let hits = hits_from_result(&sample_result());
        // The "anime" entry is not one of the 11 categories: skipped.
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].list_name, "Best Sci-Fi");
        assert_eq!(hits[0].category, Category::Movies);
        assert_eq!(hits[0].category_id, "movies");
        assert_eq!(hits[0].year.as_deref(), Some("1979"));
        assert_eq!(hits[1].year.as_deref(), Some("1965"));
        assert_eq!(hits_from_result(&json!({})), Vec::new());
        assert_eq!(hits_from_result(&json!({"matches": "nope"})), Vec::new());
    }

    #[test]
    fn groups_by_list_in_first_appearance_order() {
        let hits = hits_from_result(&sample_result());
        let groups = grouped(&hits);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, 1);
        assert_eq!(groups[0].1, vec![0, 2]); // Alien + Arrival, interleaved in input
        assert_eq!(groups[1].0, 2);
        assert_eq!(groups[1].1, vec![1]);
        assert!(grouped(&[]).is_empty());
    }
}
