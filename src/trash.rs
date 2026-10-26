//! Trash (v0.3 program #10): items removed from a list are kept
//! here for 30 days and can be restored; emptying is explicit.
//!
//! The records live on the [`Library`] (`library.trash`), so the
//! registry's whole-library history snapshots cover them: undoing a
//! remove restores the item to its list AND drops the trash record
//! in the same step. Expiry is a pure function of the record's age —
//! [`purge_expired`] drops records older than [`TRASH_TTL_SECS`]; it
//! runs at library load (the store path) and whenever the trash is
//! listed. Purging drops the RECORD only: a trashed item's cover
//! file is never deleted, because a copied item may share it.
//!
//! All logic here is pure data manipulation — no I/O, no GUI types —
//! so the command handlers in `commands` stay one-line calls.

use crate::model::{Item, Library};
use serde::{Deserialize, Serialize};

/// How long a removed item stays restorable: 30 days.
pub const TRASH_TTL_SECS: i64 = 30 * 86_400;

/// One removed item, kept whole so a restore brings back every
/// field, rating, note, tag and status exactly as they were.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrashedItem {
    /// The item snapshot, id included (ids are never re-used while
    /// a record exists, so a restore can keep the id safely).
    pub item: Item,
    pub from_list_id: u64,
    /// The list's name at removal time (the list may since be
    /// renamed or deleted; the record keeps what the user saw).
    pub from_list_name: String,
    /// Unix seconds when the item was removed.
    pub deleted_at: i64,
    /// Position in the list when removed (informational; restores
    /// append at the end).
    pub original_index: usize,
}

/// One row of `trash.list` / the GUI trash view.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrashEntry {
    pub item_id: u64,
    pub title: String,
    pub from_list_id: u64,
    pub from_list_name: String,
    pub deleted_at: i64,
    /// Whole days of the 30-day window left (0 on the last day).
    pub days_left: i64,
}

/// Whole days of the retention window left for a record deleted at
/// `deleted_at`, seen at `now` (never negative).
pub fn days_left(deleted_at: i64, now: i64) -> i64 {
    let age_days = (now - deleted_at).div_euclid(86_400);
    (TRASH_TTL_SECS / 86_400 - age_days).max(0)
}

/// A record is expired once it is strictly older than the window
/// (a record exactly 30 days old has 0 days left but is still
/// restorable until the day turns).
pub fn is_expired(deleted_at: i64, now: i64) -> bool {
    now - deleted_at > TRASH_TTL_SECS
}

/// Drop every expired record. Returns how many were dropped.
pub fn purge_expired(trash: &mut Vec<TrashedItem>, now: i64) -> usize {
    let before = trash.len();
    trash.retain(|t| !is_expired(t.deleted_at, now));
    before - trash.len()
}

/// The trash as display rows, in the order items were trashed.
pub fn entries(library: &Library, now: i64) -> Vec<TrashEntry> {
    library
        .trash
        .iter()
        .map(|t| TrashEntry {
            item_id: t.item.id,
            title: t.item.title.clone(),
            from_list_id: t.from_list_id,
            from_list_name: t.from_list_name.clone(),
            deleted_at: t.deleted_at,
            days_left: days_left(t.deleted_at, now),
        })
        .collect()
}

/// Move one item from its list into the trash. Returns false when
/// the list or the item does not exist (the caller distinguishes:
/// it checks the list first for its error message).
pub fn move_to_trash(library: &mut Library, list_id: u64, item_id: u64, now: i64) -> bool {
    let record = {
        let Some(list) = library.list_mut(list_id) else {
            return false;
        };
        let Some(index) = list.items.iter().position(|i| i.id == item_id) else {
            return false;
        };
        TrashedItem {
            item: list.items.remove(index),
            from_list_id: list_id,
            from_list_name: list.name.clone(),
            deleted_at: now,
            original_index: index,
        }
    };
    library.trash.push(record);
    true
}

/// Move several items of one list into the trash (the bulk-remove
/// path). The returned ids are the ones that actually moved, in
/// REQUEST order — the established `items.bulk_remove` contract —
/// while the trash records are appended in list order (ascending
/// original index). Unknown ids are ignored; a repeated id moves
/// once. Returns an empty vec for an unknown list too (the caller
/// checks the list first for its error message).
pub fn move_many_to_trash(library: &mut Library, list_id: u64, ids: &[u64], now: i64) -> Vec<u64> {
    let Some(list) = library.list(list_id) else {
        return Vec::new();
    };
    let mut wanted: Vec<u64> = Vec::new();
    for id in ids {
        if !wanted.contains(id) && list.items.iter().any(|i| i.id == *id) {
            wanted.push(*id);
        }
    }
    if wanted.is_empty() {
        return wanted;
    }
    let records = {
        let list = library.list_mut(list_id).expect("list checked above");
        let name = list.name.clone();
        let mut positions: Vec<usize> = list
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| wanted.contains(&i.id))
            .map(|(p, _)| p)
            .collect();
        positions.sort_unstable();
        // Remove from the back so earlier positions stay valid,
        // then flip back into ascending original order.
        let mut taken: Vec<(usize, Item)> = Vec::new();
        for pos in positions.iter().rev() {
            taken.push((*pos, list.items.remove(*pos)));
        }
        taken.reverse();
        taken
            .into_iter()
            .map(|(original_index, item)| TrashedItem {
                item,
                from_list_id: list_id,
                from_list_name: name.clone(),
                deleted_at: now,
                original_index,
            })
            .collect::<Vec<_>>()
    };
    library.trash.extend(records);
    wanted
}

/// Why a restore did not happen. The command handler maps these to
/// the registry's stable error codes via [`restore_error`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// No trash record with this item id.
    NotInTrash(u64),
    /// The explicit target list does not exist.
    TargetNotFound(u64),
    /// No explicit target and the original list is gone.
    NoTargetList,
    /// The target list already holds the same work (the model's
    /// `contains_work` rule). Carries the trashed item's title.
    Conflict(String),
}

/// Map a restore failure to the registry's (code, message) pair:
/// a missing record or target list is `not_found`; a restore with
/// nowhere to go or a same-work conflict is `bad_params`.
pub fn restore_error(error: &RestoreError) -> (&'static str, String) {
    match error {
        RestoreError::NotInTrash(id) => ("not_found", format!("no trashed item with id {id}")),
        RestoreError::TargetNotFound(id) => ("not_found", format!("no list with id {id}")),
        RestoreError::NoTargetList => (
            "bad_params",
            "the item's original list no longer exists; pass list_id to choose where it goes"
                .to_string(),
        ),
        RestoreError::Conflict(title) => {
            ("bad_params", format!("'{title}' is already in that list"))
        }
    }
}

/// Restore a trashed item: it keeps its id and is appended at the
/// end of the target list. The target is the explicit `target`
/// list when given, else the item's original list when it still
/// exists. Every failure is checked BEFORE any mutation, so a
/// failed restore never changes the library. On success returns
/// the target list's id.
pub fn restore(
    library: &mut Library,
    item_id: u64,
    target: Option<u64>,
) -> Result<u64, RestoreError> {
    let Some(pos) = library.trash.iter().position(|t| t.item.id == item_id) else {
        return Err(RestoreError::NotInTrash(item_id));
    };
    let target_id = match target {
        Some(id) => {
            if library.list(id).is_none() {
                return Err(RestoreError::TargetNotFound(id));
            }
            id
        }
        None => {
            let from = library.trash[pos].from_list_id;
            if library.list(from).is_some() {
                from
            } else {
                return Err(RestoreError::NoTargetList);
            }
        }
    };
    if library
        .list(target_id)
        .expect("target list checked above")
        .contains_work(&library.trash[pos].item)
    {
        return Err(RestoreError::Conflict(
            library.trash[pos].item.title.clone(),
        ));
    }
    let record = library.trash.remove(pos);
    library
        .list_mut(target_id)
        .expect("target list checked above")
        .items
        .push(record.item);
    Ok(target_id)
}

/// Delete one trash record forever. Returns false for an unknown
/// item id. (The item's cover FILE is left alone — a copy of the
/// item elsewhere may share it.)
pub fn remove_permanent(library: &mut Library, item_id: u64) -> bool {
    let before = library.trash.len();
    library.trash.retain(|t| t.item.id != item_id);
    library.trash.len() != before
}

/// Delete every trash record forever. Returns how many there were.
pub fn empty_trash(library: &mut Library) -> usize {
    let n = library.trash.len();
    library.trash.clear();
    n
}
