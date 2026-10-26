//! Paste-a-list import (v0.3 program #7): bring a hand-pasted list of
//! titles — optionally enriched by the GUI's provider-match review —
//! into FaveLists as a new list, or merged into an existing one.
//!
//! The semantics deliberately mirror the CSV import engine
//! ([`crate::csv_import`]): the import is MERGE-ONLY (a work already
//! in the destination list, by the model's `contains_work` rule, is
//! skipped and counted, never overwritten), new lists are created
//! lazily so an import that places nothing creates nothing, and the
//! whole batch runs as one registry dispatch, so it is a single undo
//! step. The outcome type and target type are the CSV engine's, so
//! the two commands report in the same shape.
//!
//! Unlike CSV rows, paste rows carry no category of their own, so a
//! new-list target must name its category in the target itself.

use crate::csv_import::{ImportError, ImportListReport, ImportOutcome, ImportTarget};
use crate::model::{Category, Item, Library};
use std::collections::BTreeMap;

/// Maximum rows accepted in one paste import.
pub const MAX_PASTE_ROWS: usize = 500;

/// One row of a paste import: a title plus whatever the caller's
/// review already knows about it. A title-only row is a valid manual
/// add.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PasteRow {
    pub title: String,
    /// Detail fields (the same free key/value bag `item.add` takes);
    /// empty values are dropped on the way in.
    pub fields: BTreeMap<String, String>,
    /// Provider identity of the matched work, when the row was
    /// matched against a provider (or a drinks entry).
    pub provider_id: Option<String>,
    /// Provider tag recorded on the imported item ("tmdb", …);
    /// `None`/empty lands as "manual".
    pub source: Option<String>,
}

/// Import `rows` into `library` per `target`, merge-only. Returns
/// the CSV-shaped outcome: blank-title rows are counted in
/// `invalid`, duplicates (already in the destination, including an
/// earlier row of the same paste) in `skipped_duplicates`.
pub fn import_paste_rows(
    library: &mut Library,
    rows: &[PasteRow],
    target: &ImportTarget,
) -> Result<ImportOutcome, ImportError> {
    let mut outcome = ImportOutcome::default();
    // Destination resolution mirrors the CSV engine: an existing
    // target reports its list even when nothing lands in it; a new
    // target creates its list lazily, on the first row that lands.
    let (category, mut dest): (Category, Option<usize>) = match target {
        ImportTarget::Existing { list_id } => {
            let list = library
                .list(*list_id)
                .ok_or(ImportError::ListNotFound(*list_id))?;
            outcome.lists.push(ImportListReport {
                list_id: list.id,
                name: list.name.clone(),
                category: list.category,
                added: 0,
            });
            (list.category, Some(0))
        }
        ImportTarget::New { category, .. } => {
            // Paste rows carry no category, so the target must.
            let Some(category) = *category else {
                return Err(ImportError::CategoryMismatch(
                    "paste import into a new list needs a target category".to_string(),
                ));
            };
            (category, None)
        }
    };

    for row in rows {
        let title = row.title.trim();
        if title.is_empty() {
            outcome.invalid += 1;
            continue;
        }
        let report_idx = match dest {
            Some(idx) => idx,
            None => {
                let ImportTarget::New { name, .. } = target else {
                    unreachable!("existing targets resolve their list up front")
                };
                let id = library.new_list(name.clone(), category, false);
                outcome.lists.push(ImportListReport {
                    list_id: id,
                    name: name.clone(),
                    category,
                    added: 0,
                });
                dest = Some(0);
                0
            }
        };
        let list_id = outcome.lists[report_idx].list_id;
        let mut item = Item::new(library.alloc_item_id(), title);
        item.fields = row
            .fields
            .iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(k, v)| (k.clone(), v.trim().to_string()))
            .collect();
        if let Some(pid) = row.provider_id.as_deref().map(str::trim) {
            if !pid.is_empty() {
                item.provider_id = Some(pid.to_string());
            }
        }
        if let Some(source) = row.source.as_deref().map(str::trim) {
            if !source.is_empty() {
                item.source = source.to_string();
            }
        }
        let list = library
            .list_mut(list_id)
            .expect("destination list was just resolved");
        match list.add_item(item) {
            crate::model::AddOutcome::Added => {
                outcome.added += 1;
                outcome.lists[report_idx].added += 1;
            }
            crate::model::AddOutcome::Duplicate => {
                outcome.skipped_duplicates += 1;
            }
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Category;

    fn row(title: &str) -> PasteRow {
        PasteRow {
            title: title.to_string(),
            ..PasteRow::default()
        }
    }

    #[test]
    fn new_target_needs_a_category_and_creates_lazily() {
        let mut lib = Library::default();
        // No category: an error, and no list appears.
        let err = import_paste_rows(
            &mut lib,
            &[row("Heat")],
            &ImportTarget::New {
                name: "Films".to_string(),
                category: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, ImportError::CategoryMismatch(_)));
        assert!(lib.lists.is_empty());
        // Blank rows only: still no list (creation is lazy).
        let out = import_paste_rows(
            &mut lib,
            &[row("  ")],
            &ImportTarget::New {
                name: "Films".to_string(),
                category: Some(Category::Movies),
            },
        )
        .unwrap();
        assert_eq!(out.added, 0);
        assert_eq!(out.invalid, 1);
        assert!(lib.lists.is_empty());
    }

    #[test]
    fn duplicates_within_the_paste_count_as_skips() {
        let mut lib = Library::default();
        let out = import_paste_rows(
            &mut lib,
            &[row("Heat"), row("heat"), row("Dune")],
            &ImportTarget::New {
                name: "Films".to_string(),
                category: Some(Category::Movies),
            },
        )
        .unwrap();
        assert_eq!(out.added, 2);
        assert_eq!(out.skipped_duplicates, 1);
        assert_eq!(out.lists.len(), 1);
        assert_eq!(out.lists[0].added, 2);
    }
}
