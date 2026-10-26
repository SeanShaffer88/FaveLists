//! Smart views + "Pick for me" support (v0.3 program #2 + #1).
//!
//! A smart view is a cross-list selection computed from the whole
//! library — Top Rated, Recently Added, Needs a Rating. Views are
//! *not* lists: nothing is stored, the single-type list rule is
//! untouched, and every view is a pure function of the current
//! library, so the registry (`library.view`, `library.pick`), the
//! GUI, and any future surface all see identical entries in identical
//! order.
//!
//! Ordering is fully deterministic: each view's documented keys are
//! applied in order and any remaining tie falls back to library order
//! (list id, then item id), so a view never shuffles between frames
//! or surfaces.
//!
//! Picking uses a tiny hand-rolled splitmix64 PRNG — no new
//! dependency for one index draw. A caller-supplied seed makes a pick
//! fully deterministic (the tests pin it); without one the command
//! layer seeds from the system clock. (The modulo reduction in
//! [`SplitMix64::below`] has a negligible bias for tiny ranges; for
//! choosing a film that is irrelevant.)

use crate::model::{normalise_title, Category, FavList, Item, Library};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// A smart view: a named, cross-list selection over the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartView {
    TopRated,
    RecentlyAdded,
    NeedsRating,
}

impl SmartView {
    pub const ALL: [SmartView; 3] = [
        SmartView::TopRated,
        SmartView::RecentlyAdded,
        SmartView::NeedsRating,
    ];

    /// Stable machine id used in command params.
    pub fn id(self) -> &'static str {
        match self {
            SmartView::TopRated => "top_rated",
            SmartView::RecentlyAdded => "recently_added",
            SmartView::NeedsRating => "needs_rating",
        }
    }

    /// Display name (sidebar label, view header, share-card header).
    pub fn name(self) -> &'static str {
        match self {
            SmartView::TopRated => "Top Rated",
            SmartView::RecentlyAdded => "Recently Added",
            SmartView::NeedsRating => "Needs a Rating",
        }
    }

    pub fn from_id(id: &str) -> Option<SmartView> {
        SmartView::ALL.into_iter().find(|v| v.id() == id)
    }
}

/// One entry of a view (or of a pick scope): an item plus where it
/// lives. Mirrors the `library.search` match shape — the command layer
/// serialises it to `{list_id, list_name, category, item_id, title,
/// item}` and the GUI renders the same fields.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewEntry {
    pub list_id: u64,
    pub list_name: String,
    pub category: Category,
    pub item: Item,
}

impl ViewEntry {
    fn of(list: &FavList, item: &Item) -> ViewEntry {
        ViewEntry {
            list_id: list.id,
            list_name: list.name.clone(),
            category: list.category,
            item: item.clone(),
        }
    }
}

/// Entries of one list, in list order.
pub fn list_entries(list: &FavList) -> Vec<ViewEntry> {
    list.items
        .iter()
        .map(|it| ViewEntry::of(list, it))
        .collect()
}

/// Entries of every list, in library order (lists in stored order,
/// items in list order). The deterministic base order every scope and
/// view starts from.
pub fn all_entries(library: &Library) -> Vec<ViewEntry> {
    library.lists.iter().flat_map(list_entries).collect()
}

/// Entries of every list in one category, in library order.
pub fn category_entries(library: &Library, category: Category) -> Vec<ViewEntry> {
    library
        .lists
        .iter()
        .filter(|l| l.category == category)
        .flat_map(list_entries)
        .collect()
}

/// The entries of a smart view, in the view's documented order:
///
/// * **Top Rated** — rated items only; rating descending, ties by
///   title, then list name.
/// * **Recently Added** — every item; `added_at` descending, ties by
///   title.
/// * **Needs a Rating** — unrated items only; list name, then title.
///
/// Title order is the app's canonical one ([`normalise_title`], the
/// same key the list title sort uses); list-name order is
/// case-insensitive. Any tie the documented keys leave open falls back
/// to (list id, item id), so the result is a total, stable order.
pub fn view_items(library: &Library, view: SmartView) -> Vec<ViewEntry> {
    let mut entries = all_entries(library);
    match view {
        SmartView::TopRated => {
            entries.retain(|e| e.item.rating.is_some());
            entries.sort_by(|a, b| {
                b.item
                    .rating
                    .cmp(&a.item.rating)
                    .then_with(|| title_key(a).cmp(&title_key(b)))
                    .then_with(|| list_name_key(a).cmp(&list_name_key(b)))
                    .then_with(|| entry_key(a).cmp(&entry_key(b)))
            });
        }
        SmartView::RecentlyAdded => {
            entries.sort_by(|a, b| {
                b.item
                    .added_at
                    .cmp(&a.item.added_at)
                    .then_with(|| title_key(a).cmp(&title_key(b)))
                    .then_with(|| entry_key(a).cmp(&entry_key(b)))
            });
        }
        SmartView::NeedsRating => {
            entries.retain(|e| e.item.rating.is_none());
            entries.sort_by(|a, b| {
                list_name_key(a)
                    .cmp(&list_name_key(b))
                    .then_with(|| title_key(a).cmp(&title_key(b)))
                    .then_with(|| entry_key(a).cmp(&entry_key(b)))
            });
        }
    }
    entries
}

/// Every coverless item in the library, in library order — the
/// missing-cover sweep (v0.3 #9). Not a smart view (it is a worklist,
/// not a selection), but it shares the [`ViewEntry`] shape, and both
/// the `library.missing_covers` command and the GUI sweep render
/// exactly this computation.
pub fn missing_cover_entries(library: &Library) -> Vec<ViewEntry> {
    all_entries(library)
        .into_iter()
        .filter(|e| e.item.cover.is_none())
        .collect()
}

fn title_key(e: &ViewEntry) -> String {
    normalise_title(&e.item.title)
}

fn list_name_key(e: &ViewEntry) -> String {
    e.list_name.to_lowercase()
}

/// Final tie-break: library identity order.
fn entry_key(e: &ViewEntry) -> (u64, u64) {
    (e.list_id, e.item.id)
}

/// Parse one entry object of a `library.view` / `library.pick`
/// result back into a [`ViewEntry`] (the GUI's pick flow does this
/// with the command's chosen entry). Malformed input yields `None`.
pub fn entry_from_json(v: &Value) -> Option<ViewEntry> {
    let list_id = v.get("list_id")?.as_u64()?;
    let list_name = v.get("list_name")?.as_str()?.to_string();
    let category = Category::from_id(v.get("category")?.as_str()?)?;
    let item: Item = serde_json::from_value(v.get("item")?.clone()).ok()?;
    Some(ViewEntry {
        list_id,
        list_name,
        category,
        item,
    })
}

// ---------------------------------------------------------------------------
// Picking
// ---------------------------------------------------------------------------

/// splitmix64: a tiny deterministic PRNG with a full 64-bit period —
/// more than enough to choose an index, and dependency-free.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> SplitMix64 {
        SplitMix64 { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n` (`n` must be positive).
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        (self.next_u64() % n as u64) as usize
    }
}

/// The index a pick with `seed` chooses among `len` entries, or
/// `None` for an empty scope. Deterministic for a given seed.
pub fn pick_index(seed: u64, len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some(SplitMix64::new(seed).below(len))
    }
}

/// A clock-derived seed for unseeded picks.
pub fn time_seed() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_nanos() as u64) ^ d.as_secs().wrapping_mul(0x9E37_79B9_7F4A_7C15),
        Err(_) => 0xF00D,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_view_ids_and_names_round_trip() {
        for view in SmartView::ALL {
            assert_eq!(SmartView::from_id(view.id()), Some(view));
        }
        assert_eq!(SmartView::TopRated.id(), "top_rated");
        assert_eq!(SmartView::RecentlyAdded.id(), "recently_added");
        assert_eq!(SmartView::NeedsRating.id(), "needs_rating");
        assert_eq!(SmartView::TopRated.name(), "Top Rated");
        assert_eq!(SmartView::RecentlyAdded.name(), "Recently Added");
        assert_eq!(SmartView::NeedsRating.name(), "Needs a Rating");
        assert_eq!(SmartView::from_id("bogus"), None);
    }

    #[test]
    fn splitmix_is_deterministic_and_pick_index_in_bounds() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        for _ in 0..100 {
            let (x, y) = (a.next_u64(), b.next_u64());
            assert_eq!(x, y, "same seed must give the same stream");
        }
        let mut rng = SplitMix64::new(7);
        for _ in 0..1_000 {
            assert!(rng.below(6) < 6);
        }
        assert_eq!(pick_index(42, 0), None);
        for seed in 0..50u64 {
            let idx = pick_index(seed, 5).unwrap();
            assert!(idx < 5);
            assert_eq!(pick_index(seed, 5), Some(idx), "seeded pick is stable");
        }
        assert_eq!(pick_index(123, 1), Some(0));
    }
}
