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
//! The v0.3 Slice 3 (#8) extension widens the matching in two steps:
//!
//! * Grouping uses a **v2 normalisation** ([`normalise_work_title_v2`]):
//!   on top of the v1 rules, number words (one–ten) and roman numerals
//!   (II–X) become digits, and edition tokens ("extended edition",
//!   "director's cut", "remastered", "special edition", "the movie")
//!   are stripped — so "Part II" ≡ "Part 2" and an edition variant
//!   groups with the plain release under the unchanged exact/possible
//!   year rule. Groups formed this way report `match_kind: "title"`.
//! * A **fuzzy pass** then looks at items still ungrouped: same
//!   category, year-compatible copies (either year missing, equal, or
//!   one year apart) whose v2 titles are within a small Levenshtein
//!   distance group as **possible** with `match_kind: "similar"`.
//!   There is deliberately NO token-subset matching: a sequel whose
//!   title merely extends another ("The Matrix" vs "The Matrix
//!   Reloaded") is far beyond the distance bound and never groups.
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

/// How a group's copies were matched (v0.3 #8): `Title` — the copies
/// share one v2-normalised title; `Similar` — the fuzzy pass paired
/// titles that are close but not identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Title,
    Similar,
}

impl MatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchKind::Title => "title",
            MatchKind::Similar => "similar",
        }
    }

    pub fn from_id(s: &str) -> Option<MatchKind> {
        match s {
            "title" => Some(MatchKind::Title),
            "similar" => Some(MatchKind::Similar),
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
    /// Whether the copies share the title outright or were paired by
    /// the fuzzy pass (v0.3 #8).
    pub match_kind: MatchKind,
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
            "match_kind": self.match_kind.as_str(),
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

/// Number words and roman numerals that become digits in the v2
/// normalisation, so "Part II", "Part Two" and "Part 2" are one title.
const NUMBER_TOKENS: [(&str, &str); 19] = [
    ("one", "1"),
    ("two", "2"),
    ("three", "3"),
    ("four", "4"),
    ("five", "5"),
    ("six", "6"),
    ("seven", "7"),
    ("eight", "8"),
    ("nine", "9"),
    ("ten", "10"),
    ("ii", "2"),
    ("iii", "3"),
    ("iv", "4"),
    ("v", "5"),
    ("vi", "6"),
    ("vii", "7"),
    ("viii", "8"),
    ("ix", "9"),
    ("x", "10"),
];

/// Edition phrases stripped by the v2 normalisation, as token
/// sequences. ("Director's Cut" reaches here as `director s cut` —
/// the v1 normalisation turns the apostrophe into a space.)
const EDITION_SEQUENCES: [&[&str]; 6] = [
    &["extended", "edition"],
    &["special", "edition"],
    &["directors", "cut"],
    &["director", "cut"],
    &["director", "s", "cut"],
    &["the", "movie"],
];

/// Whether `tokens[i..]` starts with the token sequence `seq`.
fn seq_at(tokens: &[&str], i: usize, seq: &[&str]) -> bool {
    tokens.len() - i >= seq.len() && tokens[i..i + seq.len()] == *seq
}

/// Normalise a title for duplicate grouping, v2 (v0.3 #8): the v1
/// [`normalise_work_title`] plus token-level cleanups — number words
/// and roman numerals become digits, and edition phrases
/// ([`EDITION_SEQUENCES`] and the single token "remastered") are
/// stripped. Two titles equal under v2 group under the unchanged
/// exact/possible year rule.
pub fn normalise_work_title_v2(title: &str) -> String {
    let base = normalise_work_title(title);
    let tokens: Vec<&str> = base.split(' ').filter(|t| !t.is_empty()).collect();
    let mut kept: Vec<&str> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        if let Some(seq) = EDITION_SEQUENCES.iter().find(|seq| seq_at(&tokens, i, seq)) {
            i += seq.len();
            continue;
        }
        if tokens[i] == "remastered" {
            i += 1;
            continue;
        }
        kept.push(tokens[i]);
        i += 1;
    }
    kept.iter()
        .map(|t| {
            NUMBER_TOKENS
                .iter()
                .find(|(word, _)| word == t)
                .map(|(_, digit)| *digit)
                .unwrap_or(t)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Bounded Levenshtein distance over chars (hand-rolled, no
/// dependency): `Some(d)` when the true distance is ≤ `max`, `None`
/// when it exceeds it. Early-exits on length difference and on a DP
/// row whose minimum already exceeds the bound.
pub fn levenshtein(a: &str, b: &str, max: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            row_min = row_min.min(cur[j]);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let d = prev[b.len()];
    if d <= max {
        Some(d)
    } else {
        None
    }
}

/// The fuzzy distance bound for a pair of v2 titles, keyed on the
/// shorter title's length in chars: short titles get no fuzzy pass
/// at all (a 1-edit change to a 4-char title is a different work —
/// "Dune" vs "Dune 2" territory).
fn fuzzy_bound(a: &str, b: &str) -> Option<usize> {
    let shorter = a.chars().count().min(b.chars().count());
    if shorter >= 12 {
        Some(2)
    } else if shorter >= 6 {
        Some(1)
    } else {
        None
    }
}

/// Year compatibility for the fuzzy pass: either side missing (or
/// unparseable) is compatible; parsed years must be equal or exactly
/// one apart.
fn years_compatible(a: Option<&str>, b: Option<&str>) -> bool {
    let ya = a.and_then(crate::model::parse_year);
    let yb = b.and_then(crate::model::parse_year);
    match (ya, yb) {
        (Some(x), Some(y)) => x == y || (x - y).abs() == 1,
        _ => true,
    }
}

/// The exact/possible rule, shared by title groups: exact when at
/// most one distinct non-empty year is present.
fn confidence_for(copies: &[DupCopy]) -> Confidence {
    let mut years = copies.iter().filter_map(|c| c.year.as_deref());
    match years.next() {
        None => Confidence::Exact,
        Some(first) => {
            if years.all(|y| y == first) {
                Confidence::Exact
            } else {
                Confidence::Possible
            }
        }
    }
}

fn uf_find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// The fuzzy pass (v0.3 #8) over items the title pass left ungrouped:
/// pairs in the same category with compatible years and v2 titles
/// within the length-keyed Levenshtein bound are unioned into groups.
/// Groups are always `Possible` / `MatchKind::Similar`; copies stay
/// in library order and the group's key is its first copy's v2 title.
fn fuzzy_groups(ungrouped: Vec<(Category, String, DupCopy)>) -> Vec<DupGroup> {
    let n = ungrouped.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in (i + 1)..n {
            if ungrouped[i].0 != ungrouped[j].0 {
                continue;
            }
            if !years_compatible(
                ungrouped[i].2.year.as_deref(),
                ungrouped[j].2.year.as_deref(),
            ) {
                continue;
            }
            let (a, b) = (&ungrouped[i].1, &ungrouped[j].1);
            let close = fuzzy_bound(a, b)
                .map(|max| levenshtein(a, b, max).is_some())
                .unwrap_or(false);
            if close {
                let (ri, rj) = (uf_find(&mut parent, i), uf_find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    // Components in first-appearance order (members ascending =
    // library order, since `ungrouped` was built in library order).
    let roots: Vec<usize> = (0..n).map(|i| uf_find(&mut parent, i)).collect();
    let mut components: Vec<(usize, Vec<usize>)> = Vec::new();
    for (i, root) in roots.iter().enumerate() {
        match components.iter().position(|(r, _)| r == root) {
            Some(p) => components[p].1.push(i),
            None => components.push((*root, vec![i])),
        }
    }
    let mut ungrouped = ungrouped;
    components
        .into_iter()
        .map(|(_, members)| members)
        .filter(|c| c.len() >= 2)
        .map(|members| {
            let (category, key) = (ungrouped[members[0]].0, ungrouped[members[0]].1.clone());
            // Placeholder copies are swapped out in member order;
            // indices are unique, so nothing real is displaced.
            let copies = members
                .iter()
                .map(|&i| {
                    std::mem::replace(
                        &mut ungrouped[i].2,
                        DupCopy {
                            list_id: 0,
                            list_name: String::new(),
                            item_id: 0,
                            title: String::new(),
                            year: None,
                        },
                    )
                })
                .collect();
            DupGroup {
                category,
                category_id: category.id().to_string(),
                key,
                confidence: Confidence::Possible,
                match_kind: MatchKind::Similar,
                copies,
            }
        })
        .collect()
}

/// Find every group of ≥2 copies of the same work in the library.
/// Groups come out in a deterministic order (category, then normalised
/// title); copies inside a group are in library order.
pub fn find_duplicates(library: &Library) -> Vec<DupGroup> {
    let mut by_key: BTreeMap<(Category, String), Vec<DupCopy>> = BTreeMap::new();
    for list in &library.lists {
        for item in &list.items {
            let key = normalise_work_title_v2(&item.title);
            if key.is_empty() {
                // A title that is all punctuation (or only an edition
                // phrase) has no identity to group by; grouping them
                // would be noise.
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
    let mut groups: Vec<DupGroup> = Vec::new();
    let mut ungrouped: Vec<(Category, String, DupCopy)> = Vec::new();
    for ((category, key), copies) in by_key {
        if copies.len() >= 2 {
            groups.push(DupGroup {
                category,
                category_id: category.id().to_string(),
                confidence: confidence_for(&copies),
                key,
                match_kind: MatchKind::Title,
                copies,
            });
        } else {
            ungrouped.extend(copies.into_iter().map(|c| (category, key.clone(), c)));
        }
    }
    groups.extend(fuzzy_groups(ungrouped));
    groups.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.key.cmp(&b.key))
            .then_with(|| a.match_kind.as_str().cmp(b.match_kind.as_str()))
    });
    groups
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
        // Results produced before v0.3 #8 have no match_kind; they
        // were all title matches.
        match_kind: g
            .get("match_kind")
            .and_then(Value::as_str)
            .and_then(MatchKind::from_id)
            .unwrap_or(MatchKind::Title),
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
    fn normalise_v2_numbers_romans_and_editions() {
        // Number words and roman numerals become digits.
        assert_eq!(
            normalise_work_title_v2("Galaxy Quest Part II"),
            "galaxy quest part 2"
        );
        assert_eq!(
            normalise_work_title_v2("Galaxy Quest Part Two"),
            "galaxy quest part 2"
        );
        assert_eq!(normalise_work_title_v2("Rocky IV"), "rocky 4");
        assert_eq!(
            normalise_work_title_v2("Final Fantasy X"),
            "final fantasy 10"
        );
        assert_eq!(normalise_work_title_v2("The One"), "1");
        // Edition phrases are stripped wherever they sit.
        assert_eq!(
            normalise_work_title_v2("Blade Runner: Director's Cut"),
            "blade runner"
        );
        assert_eq!(normalise_work_title_v2("Alien (Director's Cut)"), "alien");
        assert_eq!(normalise_work_title_v2("Dune: Extended Edition"), "dune");
        assert_eq!(normalise_work_title_v2("Heat — Remastered"), "heat");
        assert_eq!(normalise_work_title_v2("Pokemon: The Movie"), "pokemon");
        assert_eq!(
            normalise_work_title_v2("Star Wars: Special Edition"),
            "star wars"
        );
        // Untouched titles pass through with the v1 rules only.
        assert_eq!(normalise_work_title_v2("The Matrix"), "matrix");
        assert_eq!(normalise_work_title_v2("The Lego Movie"), "lego movie");
    }

    #[test]
    fn levenshtein_bounded() {
        assert_eq!(levenshtein("kitten", "sitting", 3), Some(3));
        assert_eq!(levenshtein("kitten", "sitting", 2), None);
        assert_eq!(levenshtein("flaw", "lawn", 2), Some(2));
        assert_eq!(levenshtein("same", "same", 0), Some(0));
        assert_eq!(levenshtein("abc", "abd", 0), None);
        assert_eq!(levenshtein("", "abc", 3), Some(3));
        assert_eq!(levenshtein("", "abc", 2), None);
        assert_eq!(levenshtein("abc", "", 3), Some(3));
    }

    #[test]
    fn v2_titles_group_and_fuzzy_pairs_flag_similar() {
        let lib = lib_with(&[
            (
                "Films",
                Category::Movies,
                "Galaxy Quest Part II",
                Some("1999"),
            ),
            (
                "Watchlist",
                Category::Movies,
                "Galaxy Quest Part 2",
                Some("1999"),
            ),
            ("Films", Category::Movies, "Blade Runner", Some("1982")),
            (
                "Watchlist",
                Category::Movies,
                "Blade Runner: Director's Cut",
                Some("1982"),
            ),
            (
                "Films",
                Category::Movies,
                "The Shawshank Redemption",
                Some("1994"),
            ),
            (
                "Watchlist",
                Category::Movies,
                "The Shawshank Redemtion",
                Some("1994"),
            ),
        ]);
        let groups = find_duplicates(&lib);
        assert_eq!(groups.len(), 3);
        let part = groups
            .iter()
            .find(|g| g.key == "galaxy quest part 2")
            .unwrap();
        assert_eq!(part.match_kind, MatchKind::Title);
        assert_eq!(part.confidence, Confidence::Exact);
        let blade = groups.iter().find(|g| g.key == "blade runner").unwrap();
        assert_eq!(blade.match_kind, MatchKind::Title);
        let typo = groups
            .iter()
            .find(|g| g.key == "shawshank redemption")
            .unwrap();
        assert_eq!(typo.match_kind, MatchKind::Similar);
        assert_eq!(typo.confidence, Confidence::Possible);
    }

    #[test]
    fn fuzzy_never_groups_sequels_or_year_clashes() {
        let lib = lib_with(&[
            // A sequel extends the title: far beyond the bound.
            ("Films", Category::Movies, "The Matrix", Some("1999")),
            (
                "Watchlist",
                Category::Movies,
                "The Matrix Reloaded",
                Some("2003"),
            ),
            // Short titles get no fuzzy pass at all.
            ("Films", Category::Movies, "Dune", Some("2021")),
            ("Watchlist", Category::Movies, "Dune 2", Some("2021")),
            // A typo pair whose years are far apart stays ungrouped.
            (
                "Films",
                Category::Movies,
                "Shawshank Redemption",
                Some("1994"),
            ),
            (
                "Watchlist",
                Category::Movies,
                "Shawshank Redemtion",
                Some("2010"),
            ),
        ]);
        assert!(find_duplicates(&lib).is_empty());
    }

    #[test]
    fn fuzzy_year_off_by_one_still_groups() {
        let lib = lib_with(&[
            (
                "Films",
                Category::Movies,
                "Shawshank Redemption",
                Some("1994"),
            ),
            (
                "Watchlist",
                Category::Movies,
                "Shawshank Redemtion",
                Some("1995"),
            ),
        ]);
        let groups = find_duplicates(&lib);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].match_kind, MatchKind::Similar);
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
