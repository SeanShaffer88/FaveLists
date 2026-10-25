//! Core data model: the 11 fixed categories, their exact field sets (spec §2),
//! items, lists, the library, and user settings. Pure data + logic; no I/O
//! beyond (de)serialisation, so everything here is unit-testable off-Windows.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Default drinks-database update source: the `drinks.json` asset on the
/// latest FaveLists GitHub release.
pub const DEFAULT_DRINKS_UPDATE_URL: &str =
    "https://github.com/SeanShaffer88/FaveLists/releases/latest/download/drinks.json";
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A definition of one stored detail field for a category, e.g. ("director",
/// "Director"). The key is stable (used in files/exports); the label is what
/// the UI and text exports show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldDef {
    pub key: &'static str,
    pub label: &'static str,
}

/// The 11 fixed v1 categories (spec §2). No anime, no software, no
/// user-created categories.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
pub enum Category {
    #[default]
    Movies,
    TvShows,
    Books,
    Artists,
    Albums,
    Songs,
    VideoGames,
    Podcasts,
    BoardGames,
    EnergyDrinks,
    Sodas,
}

impl Category {
    pub const ALL: [Category; 11] = [
        Category::Movies,
        Category::TvShows,
        Category::Books,
        Category::Artists,
        Category::Albums,
        Category::Songs,
        Category::VideoGames,
        Category::Podcasts,
        Category::BoardGames,
        Category::EnergyDrinks,
        Category::Sodas,
    ];

    /// Display name, also used in export headers and backup scope labels.
    pub fn name(self) -> &'static str {
        match self {
            Category::Movies => "Movies",
            Category::TvShows => "TV Shows",
            Category::Books => "Books",
            Category::Artists => "Music Artists/Bands",
            Category::Albums => "Albums",
            Category::Songs => "Songs",
            Category::VideoGames => "Video Games",
            Category::Podcasts => "Podcasts",
            Category::BoardGames => "Board Games",
            Category::EnergyDrinks => "Energy Drinks",
            Category::Sodas => "Sodas",
        }
    }

    /// Stable machine id used in files.
    pub fn id(self) -> &'static str {
        match self {
            Category::Movies => "movies",
            Category::TvShows => "tv_shows",
            Category::Books => "books",
            Category::Artists => "artists",
            Category::Albums => "albums",
            Category::Songs => "songs",
            Category::VideoGames => "video_games",
            Category::Podcasts => "podcasts",
            Category::BoardGames => "board_games",
            Category::EnergyDrinks => "energy_drinks",
            Category::Sodas => "sodas",
        }
    }

    pub fn from_id(id: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.id() == id)
    }

    /// The category whose items are looked up in the bundled offline drinks
    /// database; `Some(kind)` for the two drinks categories only.
    pub fn drinks_kind(self) -> Option<crate::drinks::DrinkKind> {
        match self {
            Category::EnergyDrinks => Some(crate::drinks::DrinkKind::EnergyDrink),
            Category::Sodas => Some(crate::drinks::DrinkKind::Soda),
            _ => None,
        }
    }

    /// Exact field set for this category (spec §2). Order is display order.
    pub fn fields(self) -> &'static [FieldDef] {
        macro_rules! f {
            ($($k:literal => $l:literal),+ $(,)?) => {
                &[$(FieldDef { key: $k, label: $l }),+]
            };
        }
        match self {
            Category::Movies => f!(
                "year" => "Year", "genre" => "Genre", "director" => "Director",
                "runtime" => "Runtime", "cast" => "Main cast",
            ),
            Category::TvShows => f!(
                "years" => "Years", "genre" => "Genre", "creator" => "Creator",
                "seasons" => "Seasons", "cast" => "Main cast",
            ),
            Category::Books => f!(
                "year" => "Year", "genre" => "Genre", "author" => "Author",
                "pages" => "Pages",
            ),
            Category::Artists => f!("genre" => "Genre", "country" => "Country"),
            Category::Albums => f!(
                "year" => "Year", "genre" => "Genre", "artist" => "Artist",
            ),
            Category::Songs => f!(
                "year" => "Year", "genre" => "Genre", "artist" => "Artist",
                "album" => "Album",
            ),
            Category::VideoGames => f!(
                "year" => "Year", "genre" => "Genre", "developer" => "Developer",
                "platforms" => "Platforms",
            ),
            Category::Podcasts => f!("host" => "Host", "category" => "Category"),
            Category::BoardGames => f!("year" => "Year", "publisher" => "Publisher"),
            Category::EnergyDrinks | Category::Sodas => {
                f!("brand" => "Brand", "flavor" => "Flavor")
            }
        }
    }

    /// The field used as the "year" for sorting/exports, when the category has
    /// one.
    pub fn year_key(self) -> Option<&'static str> {
        match self {
            Category::Movies
            | Category::Books
            | Category::Albums
            | Category::Songs
            | Category::VideoGames
            | Category::BoardGames => Some("year"),
            Category::TvShows => Some("years"),
            _ => None,
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One entry in a list: a single work (one entry per work, no editions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Unique id inside the library (monotonic counter based).
    pub id: u64,
    pub title: String,
    /// Category-specific basic fields, keyed by [`FieldDef::key`].
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// Id of this work at the provider it came from (e.g. TMDB id), or `None`
    /// for manual entries. Part of the duplicate key.
    #[serde(default)]
    pub provider_id: Option<String>,
    /// Which provider supplied this item ("tmdb", "openlibrary", …) or
    /// "manual" / "drinks-db".
    #[serde(default)]
    pub source: String,
    /// File name (relative to the data folder's `covers/`) of the cached cover
    /// image, if any.
    #[serde(default)]
    pub cover: Option<String>,
    /// Personal rating, 1–10 (v0.2 program #5 — SEAN's direction reversed
    /// the v1 exclusion of ratings/notes). `None` = unrated. Not a category
    /// field: it belongs to the entry, not the work.
    #[serde(default)]
    pub rating: Option<u8>,
    /// Personal free-text note (v0.2 program #5). Empty = no note.
    #[serde(default)]
    pub note: String,
    /// Unix seconds when the item was added.
    pub added_at: i64,
}

impl Item {
    pub fn new(id: u64, title: impl Into<String>) -> Item {
        Item {
            id,
            title: title.into(),
            fields: BTreeMap::new(),
            provider_id: None,
            source: "manual".to_string(),
            cover: None,
            rating: None,
            note: String::new(),
            added_at: now_unix(),
        }
    }

    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .get(key)
            .map(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
    }

    /// Identity of the *work* for duplicate detection inside one list:
    /// provider id when known, otherwise the normalised title. Two entries
    /// are the same work when their keys match.
    pub fn work_key(&self) -> String {
        match &self.provider_id {
            Some(pid) if !pid.trim().is_empty() => {
                format!("{}:{}", self.source, pid.trim().to_lowercase())
            }
            _ => format!("title:{}", normalise_title(&self.title)),
        }
    }
}

/// Lower-case, punctuation-collapsed title used for duplicate detection and
/// title sorting.
pub fn normalise_title(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut last_space = true;
    for ch in t.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_string()
}

/// One favourites list. Single-type: `category` never changes after creation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FavList {
    pub id: u64,
    pub name: String,
    pub category: Category,
    /// Ranked lists show numbers and export numbered. Switchable at any time;
    /// switching never reorders items.
    pub ranked: bool,
    pub items: Vec<Item>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    Added,
    /// The same work is already in this list.
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKind {
    TitleAZ,
    Year,
    DateAdded,
    /// Highest personal rating first; unrated items last (v0.2 #5).
    Rating,
}

impl FavList {
    pub fn new(id: u64, name: impl Into<String>, category: Category, ranked: bool) -> FavList {
        FavList {
            id,
            name: name.into(),
            category,
            ranked,
            items: Vec::new(),
            created_at: now_unix(),
        }
    }

    pub fn contains_work(&self, item: &Item) -> bool {
        let key = item.work_key();
        self.items.iter().any(|i| i.work_key() == key)
    }

    /// Append an item unless the same work is already in this list.
    pub fn add_item(&mut self, item: Item) -> AddOutcome {
        if self.contains_work(&item) {
            return AddOutcome::Duplicate;
        }
        self.items.push(item);
        AddOutcome::Added
    }

    pub fn remove_item(&mut self, item_id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != item_id);
        self.items.len() != before
    }

    /// Move the item at `from` to position `to` (drag reorder). Out-of-range
    /// indices are ignored.
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
    }

    /// Swap the items at two positions (image-export preview drag-to-swap and
    /// grid drag both reduce to this in the UI layer).
    pub fn swap_items(&mut self, a: usize, b: usize) {
        if a < self.items.len() && b < self.items.len() {
            self.items.swap(a, b);
        }
    }

    pub fn set_ranked(&mut self, ranked: bool) {
        self.ranked = ranked;
    }

    /// Apply a one-shot sort. Stable, so equal keys keep their current order.
    pub fn sort(&mut self, kind: SortKind) {
        match kind {
            SortKind::TitleAZ => {
                self.items.sort_by_key(|i| normalise_title(&i.title));
            }
            SortKind::Year => {
                let key = self.category.year_key();
                self.items.sort_by(|a, b| {
                    let ya = key.and_then(|k| a.field(k)).unwrap_or("");
                    let yb = key.and_then(|k| b.field(k)).unwrap_or("");
                    // Numeric prefix compare ("1999" < "2001"); empty years last.
                    let na = parse_year(ya);
                    let nb = parse_year(yb);
                    match (na, nb) {
                        (Some(x), Some(y)) => x.cmp(&y),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    }
                });
            }
            SortKind::DateAdded => {
                self.items.sort_by_key(|i| i.added_at);
            }
            SortKind::Rating => {
                // Highest first; unrated last. `sort_by` is stable, so
                // items with equal ratings (and the unrated tail) keep
                // their current relative order.
                self.items.sort_by(|a, b| match (a.rating, b.rating) {
                    (Some(x), Some(y)) => y.cmp(&x),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                });
            }
        }
    }

    pub fn item(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }
}

/// First 4-digit year found in a string like "1999" or "1999–2004".
pub fn parse_year(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        let chunk = &s[i..i + 4];
        if chunk.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(y) = chunk.parse::<i64>() {
                if (1000..=2999).contains(&y) {
                    return Some(y);
                }
            }
        }
    }
    None
}

/// The whole library: all lists plus id counters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub lists: Vec<FavList>,
    #[serde(default = "default_next_id")]
    pub next_list_id: u64,
    #[serde(default = "default_next_id")]
    pub next_item_id: u64,
}

fn default_next_id() -> u64 {
    1
}

impl Default for Library {
    fn default() -> Self {
        Library {
            lists: Vec::new(),
            next_list_id: 1,
            next_item_id: 1,
        }
    }
}

impl Library {
    pub fn new_list(&mut self, name: impl Into<String>, category: Category, ranked: bool) -> u64 {
        let id = self.next_list_id;
        self.next_list_id += 1;
        self.lists.push(FavList::new(id, name, category, ranked));
        id
    }

    pub fn alloc_item_id(&mut self) -> u64 {
        let id = self.next_item_id;
        self.next_item_id += 1;
        id
    }

    pub fn list(&self, id: u64) -> Option<&FavList> {
        self.lists.iter().find(|l| l.id == id)
    }

    pub fn list_mut(&mut self, id: u64) -> Option<&mut FavList> {
        self.lists.iter_mut().find(|l| l.id == id)
    }

    pub fn remove_list(&mut self, id: u64) -> bool {
        let before = self.lists.len();
        self.lists.retain(|l| l.id != id);
        self.lists.len() != before
    }

    pub fn lists_in_category(&self, category: Category) -> Vec<&FavList> {
        self.lists
            .iter()
            .filter(|l| l.category == category)
            .collect()
    }

    pub fn total_items(&self) -> usize {
        self.lists.iter().map(|l| l.items.len()).sum()
    }
}

/// User settings, stored in the data folder (settings.json). API keys live
/// here and ONLY here — never in code, never in the repo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub tmdb_api_key: String,
    #[serde(default)]
    pub igdb_client_id: String,
    #[serde(default)]
    pub igdb_client_secret: String,
    /// Where refreshed drinks-database packs come from. Defaults to the
    /// `drinks.json` asset attached to every FaveLists GitHub release
    /// (`releases/latest/download/drinks.json` always resolves to the newest
    /// published pack). Users can point it elsewhere or blank it to disable.
    #[serde(default)]
    pub drinks_update_url: String,
    /// Silent auto-update of the drinks database (spec §6): on by default.
    #[serde(default = "default_true")]
    pub drinks_auto_update: bool,
    /// Unix seconds of the last successful drinks DB update check.
    #[serde(default)]
    pub drinks_last_check: i64,
    /// Remembered detail-field choices per category for text/Markdown export
    /// (spec §7): category id -> field keys the user last exported with.
    #[serde(default)]
    pub export_fields: BTreeMap<String, Vec<String>>,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            tmdb_api_key: String::new(),
            igdb_client_id: String::new(),
            igdb_client_secret: String::new(),
            drinks_update_url: DEFAULT_DRINKS_UPDATE_URL.to_string(),
            drinks_auto_update: true,
            drinks_last_check: 0,
            export_fields: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn has_tmdb(&self) -> bool {
        !self.tmdb_api_key.trim().is_empty()
    }

    pub fn has_igdb(&self) -> bool {
        !self.igdb_client_id.trim().is_empty() && !self.igdb_client_secret.trim().is_empty()
    }

    /// Fields to pre-tick in the export dialog for a category: the remembered
    /// set if one exists, otherwise all of the category's fields.
    pub fn remembered_export_fields(&self, category: Category) -> Vec<String> {
        if let Some(saved) = self.export_fields.get(category.id()) {
            // The v0.2 #5 pseudo-fields ("rating"/"note") are not category
            // fields but are legitimate remembered export choices.
            let valid: Vec<String> = saved
                .iter()
                .filter(|k| {
                    k.as_str() == "rating"
                        || k.as_str() == "note"
                        || category.fields().iter().any(|f| f.key == k.as_str())
                })
                .cloned()
                .collect();
            if !valid.is_empty() {
                return valid;
            }
        }
        category
            .fields()
            .iter()
            .map(|f| f.key.to_string())
            .collect()
    }
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Format unix seconds as an ISO `YYYY-MM-DD` date (UTC). Implemented by hand
/// (civil-from-days) so exports need no date crate.
pub fn iso_date(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    // Howard Hinnant's civil_from_days algorithm.
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u64, title: &str) -> Item {
        Item::new(id, title)
    }

    #[test]
    fn category_field_sets_match_spec() {
        let keys = |c: Category| c.fields().iter().map(|f| f.key).collect::<Vec<_>>();
        assert_eq!(
            keys(Category::Movies),
            ["year", "genre", "director", "runtime", "cast"]
        );
        assert_eq!(
            keys(Category::TvShows),
            ["years", "genre", "creator", "seasons", "cast"]
        );
        assert_eq!(keys(Category::Books), ["year", "genre", "author", "pages"]);
        assert_eq!(keys(Category::Artists), ["genre", "country"]);
        assert_eq!(keys(Category::Albums), ["year", "genre", "artist"]);
        assert_eq!(keys(Category::Songs), ["year", "genre", "artist", "album"]);
        assert_eq!(
            keys(Category::VideoGames),
            ["year", "genre", "developer", "platforms"]
        );
        assert_eq!(keys(Category::Podcasts), ["host", "category"]);
        assert_eq!(keys(Category::BoardGames), ["year", "publisher"]);
        assert_eq!(keys(Category::EnergyDrinks), ["brand", "flavor"]);
        assert_eq!(keys(Category::Sodas), ["brand", "flavor"]);
        assert_eq!(Category::ALL.len(), 11);
    }

    #[test]
    fn duplicate_work_blocked_in_same_list_only() {
        let mut lib = Library::default();
        let a = lib.new_list("Best films", Category::Movies, true);
        let b = lib.new_list("Comfort films", Category::Movies, false);
        let mut i1 = item(lib.alloc_item_id(), "Alien");
        i1.provider_id = Some("348".into());
        i1.source = "tmdb".into();
        let mut i2 = item(lib.alloc_item_id(), "Alien (1979)");
        i2.provider_id = Some("348".into());
        i2.source = "tmdb".into();
        assert_eq!(
            lib.list_mut(a).unwrap().add_item(i1.clone()),
            AddOutcome::Added
        );
        // Same provider id, different display title: still the same work.
        assert_eq!(lib.list_mut(a).unwrap().add_item(i2), AddOutcome::Duplicate);
        // …but the same work is fine in a different list.
        assert_eq!(lib.list_mut(b).unwrap().add_item(i1), AddOutcome::Added);
    }

    #[test]
    fn duplicate_by_normalised_title_for_manual_items() {
        let mut list = FavList::new(1, "Sodas", Category::Sodas, false);
        assert_eq!(list.add_item(item(1, "Coca-Cola")), AddOutcome::Added);
        // "Coca-Cola" and " coca  cola " normalise to the same key.
        assert_eq!(
            list.add_item(item(2, " coca  cola ")),
            AddOutcome::Duplicate
        );
    }

    #[test]
    fn ranked_switch_preserves_order() {
        let mut list = FavList::new(1, "Films", Category::Movies, true);
        list.add_item(item(1, "B"));
        list.add_item(item(2, "A"));
        list.set_ranked(false);
        assert!(!list.ranked);
        assert_eq!(list.items[0].title, "B");
        list.set_ranked(true);
        assert_eq!(list.items[1].title, "A");
    }

    #[test]
    fn move_and_swap_reorder() {
        let mut list = FavList::new(1, "Films", Category::Movies, true);
        for (i, t) in ["a", "b", "c", "d"].iter().enumerate() {
            list.add_item(item(i as u64 + 1, t));
        }
        list.move_item(0, 2);
        assert_eq!(
            list.items
                .iter()
                .map(|i| i.title.as_str())
                .collect::<Vec<_>>(),
            ["b", "c", "a", "d"]
        );
        list.swap_items(0, 3);
        assert_eq!(list.items[0].title, "d");
        list.move_item(99, 0); // no-op, no panic
        assert_eq!(list.items.len(), 4);
    }

    #[test]
    fn sorts_title_year_added() {
        let mut list = FavList::new(1, "Films", Category::Movies, false);
        let mut a = item(1, "Zulu");
        a.fields.insert("year".into(), "1964".into());
        a.added_at = 30;
        let mut b = item(2, "Alien");
        b.fields.insert("year".into(), "1979".into());
        b.added_at = 10;
        let c = item(3, "No Year");
        list.add_item(a);
        list.add_item(b);
        list.add_item(c);
        list.sort(SortKind::TitleAZ);
        assert_eq!(list.items[0].title, "Alien");
        list.sort(SortKind::Year);
        assert_eq!(list.items[0].title, "Zulu");
        assert_eq!(list.items[2].title, "No Year"); // missing year last
        list.sort(SortKind::DateAdded);
        assert_eq!(list.items[0].title, "Alien");
    }

    #[test]
    fn sort_by_rating_unrated_last_and_stable() {
        let mut list = FavList::new(1, "Films", Category::Movies, false);
        let mut a = item(1, "Mid A");
        a.rating = Some(5);
        let b = item(2, "Unrated B");
        let mut c = item(3, "Top");
        c.rating = Some(10);
        let mut d = item(4, "Mid B");
        d.rating = Some(5);
        let e = item(5, "Unrated A");
        for i in [a, b, c, d, e] {
            list.add_item(i);
        }
        list.sort(SortKind::Rating);
        let titles: Vec<&str> = list.items.iter().map(|i| i.title.as_str()).collect();
        // 10 first, the two 5s in their original relative order, then the
        // two unrated in theirs.
        assert_eq!(titles, ["Top", "Mid A", "Mid B", "Unrated B", "Unrated A"]);
    }

    #[test]
    fn remembered_export_fields_default_all_and_filter_invalid() {
        let mut s = Settings::default();
        assert_eq!(s.remembered_export_fields(Category::Books).len(), 4);
        s.export_fields
            .insert("books".into(), vec!["author".into(), "bogus".into()]);
        assert_eq!(s.remembered_export_fields(Category::Books), vec!["author"]);
    }

    #[test]
    fn iso_date_known_values() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(1_761_350_400), "2025-10-25");
        assert_eq!(iso_date(951_782_400), "2000-02-29");
    }

    #[test]
    fn parse_year_variants() {
        assert_eq!(parse_year("1979"), Some(1979));
        assert_eq!(parse_year("1999–2004"), Some(1999));
        assert_eq!(parse_year("unknown"), None);
        assert_eq!(parse_year("12"), None);
    }
}
