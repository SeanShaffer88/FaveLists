//! The bundled offline drinks database (spec §6): Energy Drinks + Sodas
//! entries compiled from Open Food Facts exports by `tools/drinks-db/` (the
//! Rust twin of that pipeline is `src/bin/drinks_db_build.rs`, sharing the
//! filter/dedupe functions in this module so the logic is unit-tested here).
//!
//! The app searches this database locally — no network — and downloads a
//! drink's image once, when it is added to a list. Refreshed packs can be
//! fetched from a configurable URL (settings); a downloaded pack with a
//! higher version replaces the bundled one.

use crate::model::normalise_title;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DrinkKind {
    EnergyDrink,
    Soda,
}

impl DrinkKind {
    pub fn id(self) -> &'static str {
        match self {
            DrinkKind::EnergyDrink => "energy_drink",
            DrinkKind::Soda => "soda",
        }
    }

    /// Open Food Facts `categories_tags` value that marks this kind.
    pub fn off_category_tag(self) -> &'static str {
        match self {
            DrinkKind::EnergyDrink => "en:energy-drinks",
            DrinkKind::Soda => "en:sodas",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrinkEntry {
    pub name: String,
    #[serde(default)]
    pub brand: String,
    #[serde(default)]
    pub flavor: String,
    pub kind: DrinkKind,
    #[serde(default)]
    pub image_url: Option<String>,
}

impl DrinkEntry {
    /// Dedupe identity: kind + brand + product + flavor (spec §6).
    pub fn dedupe_key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.kind.id(),
            normalise_title(&self.brand),
            normalise_title(&self.name),
            normalise_title(&self.flavor)
        )
    }

    /// One-line detail for dropdowns / exports.
    pub fn detail_line(&self) -> String {
        match (self.brand.is_empty(), self.flavor.is_empty()) {
            (false, false) => format!("{} · {}", self.brand, self.flavor),
            (false, true) => self.brand.clone(),
            (true, false) => self.flavor.clone(),
            (true, true) => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrinksDb {
    /// Pack version; a downloaded pack must be strictly newer to replace
    /// the current one.
    pub version: u32,
    /// Provenance note, e.g. "Open Food Facts export 2026-10-25 (ODbL)".
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub entries: Vec<DrinkEntry>,
}

impl DrinksDb {
    /// The database compiled into the binary at build time.
    pub fn bundled() -> DrinksDb {
        serde_json::from_str(crate::BUNDLED_DRINKS_JSON).unwrap_or_else(|_| DrinksDb {
            version: 0,
            source: String::new(),
            entries: Vec::new(),
        })
    }

    /// Load the effective database: the downloaded pack in the data folder
    /// when it is newer than the bundled one, else the bundled one.
    pub fn load_effective(data_dir: &Path) -> DrinksDb {
        let bundled = DrinksDb::bundled();
        let pack_path = data_dir.join(crate::store::DRINKS_FILE);
        if let Ok(text) = std::fs::read_to_string(&pack_path) {
            if let Ok(pack) = serde_json::from_str::<DrinksDb>(&text) {
                if pack.version > bundled.version && !pack.entries.is_empty() {
                    return pack;
                }
            }
        }
        bundled
    }

    pub fn count(&self, kind: DrinkKind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }

    /// Local search: begins-with matches first, then contains matches,
    /// mirroring the online providers' ordering rule (spec §1).
    pub fn search(&self, kind: DrinkKind, query: &str, limit: usize) -> Vec<DrinkEntry> {
        let q = normalise_title(query);
        if q.is_empty() {
            return Vec::new();
        }
        let mut starts: Vec<DrinkEntry> = Vec::new();
        let mut contains: Vec<DrinkEntry> = Vec::new();
        for e in self.entries.iter().filter(|e| e.kind == kind) {
            let name = normalise_title(&e.name);
            let brand_name = normalise_title(&format!("{} {}", e.brand, e.name));
            if name.starts_with(&q) || brand_name.starts_with(&q) {
                starts.push(e.clone());
            } else if name.contains(&q) || brand_name.contains(&q) {
                contains.push(e.clone());
            }
        }
        starts.sort_by(|a, b| a.name.cmp(&b.name));
        contains.sort_by(|a, b| a.name.cmp(&b.name));
        starts.extend(contains);
        starts.truncate(limit);
        starts
    }
}

use std::path::Path;

/// A raw product row as read from an Open Food Facts export, before
/// filtering. The pipeline (Python tool and Rust builder alike) maps these
/// through [`filter_product`].
#[derive(Debug, Clone, Default)]
pub struct RawProduct {
    pub name: String,
    pub brands: String,
    pub categories_tags: Vec<String>,
    pub countries_tags: Vec<String>,
    pub lang: String,
    pub image_url: Option<String>,
}

/// Decide whether a raw OFF product belongs in the drinks DB, and convert it.
/// Rules (spec §6): US + English only; must carry the soda or energy-drink
/// category tag; must have a name.
pub fn filter_product(raw: &RawProduct) -> Option<DrinkEntry> {
    let name = raw.name.trim();
    if name.is_empty() {
        return None;
    }
    // The en.* export flavour has no lang column (names are English by
    // construction); when a lang is present it must be English.
    if !raw.lang.is_empty() && !raw.lang.eq_ignore_ascii_case("en") {
        return None;
    }
    let is_us = raw
        .countries_tags
        .iter()
        .any(|t| t.eq_ignore_ascii_case("en:united-states"));
    if !is_us {
        return None;
    }
    let kind = if raw
        .categories_tags
        .iter()
        .any(|t| t.eq_ignore_ascii_case(DrinkKind::EnergyDrink.off_category_tag()))
    {
        DrinkKind::EnergyDrink
    } else if raw
        .categories_tags
        .iter()
        .any(|t| t.eq_ignore_ascii_case(DrinkKind::Soda.off_category_tag()))
    {
        DrinkKind::Soda
    } else {
        return None;
    };
    // OFF brands are often a comma list ("Coca-Cola, The Coca-Cola Company");
    // the first is the consumer-facing brand.
    let brand = raw
        .brands
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let flavor = parse_flavor(name, &brand);
    Some(DrinkEntry {
        name: name.to_string(),
        brand,
        flavor,
        kind,
        image_url: raw.image_url.clone().filter(|u| !u.trim().is_empty()),
    })
}

/// Best-effort flavor: the product name with the brand and generic product
/// words removed. "Coca-Cola Classic" (brand "Coca-Cola") -> "Classic";
/// "Monster Energy Ultra Sunrise" -> "Ultra Sunrise" is beyond a heuristic,
/// so there the remainder after the brand is kept whole.
pub fn parse_flavor(name: &str, brand: &str) -> String {
    let mut rest = name.trim().to_string();
    if !brand.is_empty() {
        if let Some(idx) = rest.to_lowercase().find(&brand.to_lowercase()) {
            rest = format!("{}{}", &rest[..idx], &rest[idx + brand.len()..])
                .trim()
                .to_string();
        }
    }
    // A "flavor" that just repeats the whole product name is meaningless
    // (happens when the recorded brand is a variant the strip missed,
    // e.g. name "Pepsi" with brand "Pepsi-Cola").
    if normalise_title(&rest) == normalise_title(name) {
        return String::new();
    }
    // Drop leftover separators and generic words at the edges.
    let generics = ["energy drink", "energy", "soda", "drink", "beverage"];
    let mut changed = true;
    while changed {
        changed = false;
        let lower = rest.to_lowercase();
        for g in generics {
            if lower == g {
                rest.clear();
                changed = true;
                break;
            }
            if let Some(stripped) = lower.strip_suffix(g) {
                if stripped.ends_with(' ') || stripped.ends_with('-') {
                    rest = rest[..stripped.trim_end().len()].trim().to_string();
                    changed = true;
                    break;
                }
            }
        }
        let trimmed = rest
            .trim_matches(|c: char| c == '-' || c == '·' || c == ' ')
            .to_string();
        if trimmed != rest {
            rest = trimmed;
            changed = true;
        }
    }
    rest
}

/// Dedupe entries by [`DrinkEntry::dedupe_key`], keeping the first entry
/// that has an image when duplicates disagree. A second pass collapses
/// rows that are the same product (kind + name + flavor) whose brand
/// strings are compatible — OFF records the same drink under both
/// "Coca-Cola" and "The Coca-Cola Company", and a favorites picker should
/// show it once. When collapsing, the shorter (consumer-facing) brand
/// wins and an image is kept if either row had one.
pub fn dedupe(entries: Vec<DrinkEntry>) -> Vec<DrinkEntry> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out: Vec<DrinkEntry> = Vec::new();
    for e in entries {
        let key = e.dedupe_key();
        match seen.get(&key) {
            Some(&i) => {
                if out[i].image_url.is_none() && e.image_url.is_some() {
                    out[i].image_url = e.image_url;
                }
            }
            None => {
                seen.insert(key, out.len());
                out.push(e);
            }
        }
    }
    // Collapse pass, bucketed by the product key so it stays fast.
    let mut buckets: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    let mut result: Vec<DrinkEntry> = Vec::with_capacity(out.len());
    for e in out {
        let name_key = format!(
            "{}|{}|{}",
            e.kind.id(),
            normalise_title(&e.name),
            normalise_title(&e.flavor)
        );
        let mut merged = false;
        if let Some(indices) = buckets.get(&name_key) {
            for &i in indices {
                if brands_compatible(&result[i].brand, &e.brand) {
                    if result[i].image_url.is_none() {
                        result[i].image_url = e.image_url.clone();
                    }
                    if !e.brand.is_empty() && e.brand.len() < result[i].brand.len() {
                        result[i].brand = e.brand.clone();
                    }
                    merged = true;
                    break;
                }
            }
        }
        if !merged {
            buckets.entry(name_key).or_default().push(result.len());
            result.push(e);
        }
    }
    result
}

/// Are two brand strings plausibly the same brand? Equal after
/// normalisation, either empty, or one contains the other
/// ("coca cola" vs "the coca cola company").
fn brands_compatible(a: &str, b: &str) -> bool {
    let (a, b) = (normalise_title(a), normalise_title(b));
    a.is_empty() || b.is_empty() || a == b || a.contains(&b) || b.contains(&a)
}

/// Validate a downloaded update pack against the current DB: strictly newer
/// version, non-empty, and sane (every entry has a name).
pub fn validate_update(current: &DrinksDb, candidate: &DrinksDb) -> Result<(), String> {
    if candidate.version <= current.version {
        return Err(format!(
            "pack version {} is not newer than current {}",
            candidate.version, current.version
        ));
    }
    if candidate.entries.is_empty() {
        return Err("pack has no entries".to_string());
    }
    if candidate.entries.iter().any(|e| e.name.trim().is_empty()) {
        return Err("pack contains an entry with no name".to_string());
    }
    Ok(())
}

/// Fetch a drinks DB pack from `url`. Returns the parsed pack; the caller
/// validates and saves. Split from validation so tests can exercise the
/// logic offline.
pub fn fetch_update(url: &str) -> Result<DrinksDb, String> {
    let resp = ureq::get(url)
        .set("User-Agent", crate::USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let text = resp
        .into_string()
        .map_err(|e| format!("download failed: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("invalid drinks pack: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(name: &str, brands: &str, cats: &[&str]) -> RawProduct {
        RawProduct {
            name: name.into(),
            brands: brands.into(),
            categories_tags: cats.iter().map(|s| s.to_string()).collect(),
            countries_tags: vec!["en:united-states".into()],
            lang: "en".into(),
            image_url: Some("https://img.example/x.jpg".into()),
        }
    }

    #[test]
    fn filter_accepts_us_en_soda_and_energy() {
        let soda = filter_product(&raw("Coca-Cola Classic", "Coca-Cola", &["en:sodas"])).unwrap();
        assert_eq!(soda.kind, DrinkKind::Soda);
        assert_eq!(soda.brand, "Coca-Cola");
        assert_eq!(soda.flavor, "Classic");
        let energy = filter_product(&raw("Red Bull", "Red Bull", &["en:energy-drinks"])).unwrap();
        assert_eq!(energy.kind, DrinkKind::EnergyDrink);
    }

    #[test]
    fn filter_rejects_non_us_non_en_and_other_categories() {
        let mut r = raw("Fanta", "Fanta", &["en:sodas"]);
        r.countries_tags = vec!["en:france".into()];
        assert!(filter_product(&r).is_none());
        let mut r2 = raw("Fanta", "Fanta", &["en:sodas"]);
        r2.lang = "fr".into();
        assert!(filter_product(&r2).is_none());
        let r3 = raw("Potato Chips", "Lays", &["en:chips"]);
        assert!(filter_product(&r3).is_none());
        let mut r4 = raw("", "X", &["en:sodas"]);
        r4.name = "  ".into();
        assert!(filter_product(&r4).is_none());
    }

    #[test]
    fn flavor_parsing() {
        assert_eq!(parse_flavor("Coca-Cola Classic", "Coca-Cola"), "Classic");
        assert_eq!(parse_flavor("Pepsi", "Pepsi"), "");
        assert_eq!(parse_flavor("Pepsi", "Pepsi-Cola"), "");
        assert_eq!(parse_flavor("Ghost Energy Drink", "Ghost"), "");
        assert_eq!(parse_flavor("Fanta Orange Soda", "Fanta"), "Orange");
    }

    #[test]
    fn dedupe_collapses_brand_variants() {
        let a = DrinkEntry {
            name: "Coca-Cola".into(),
            brand: "The Coca-Cola Company".into(),
            flavor: String::new(),
            kind: DrinkKind::Soda,
            image_url: None,
        };
        let b = DrinkEntry {
            brand: "Coca-Cola".into(),
            image_url: Some("https://img/1.jpg".into()),
            ..a.clone()
        };
        let different_brand = DrinkEntry {
            name: "Cola".into(),
            brand: "Acme".into(),
            flavor: String::new(),
            kind: DrinkKind::Soda,
            image_url: None,
        };
        let generic_other_brand = DrinkEntry {
            brand: "Best Value".into(),
            ..different_brand.clone()
        };
        let out = dedupe(vec![a, b, different_brand, generic_other_brand]);
        // Coca-Cola variants collapse (short brand + image kept); the two
        // generic "Cola" entries from unrelated brands do NOT collapse.
        assert_eq!(out.len(), 3);
        let coke = out.iter().find(|e| e.name == "Coca-Cola").unwrap();
        assert_eq!(coke.brand, "Coca-Cola");
        assert_eq!(coke.image_url.as_deref(), Some("https://img/1.jpg"));
    }

    #[test]
    fn dedupe_merges_and_prefers_image() {
        let a = DrinkEntry {
            name: "Cola".into(),
            brand: "Acme".into(),
            flavor: "Cherry".into(),
            kind: DrinkKind::Soda,
            image_url: None,
        };
        let mut b = a.clone();
        b.image_url = Some("https://img/1.jpg".into());
        let c = DrinkEntry {
            name: "Cola Zero".into(),
            ..a.clone()
        };
        let out = dedupe(vec![a, b, c]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].image_url.as_deref(), Some("https://img/1.jpg"));
    }

    fn sample_db() -> DrinksDb {
        DrinksDb {
            version: 1,
            source: "test".into(),
            entries: vec![
                DrinkEntry {
                    name: "Coca-Cola Classic".into(),
                    brand: "Coca-Cola".into(),
                    flavor: "Classic".into(),
                    kind: DrinkKind::Soda,
                    image_url: None,
                },
                DrinkEntry {
                    name: "Pepsi Wild Cherry".into(),
                    brand: "Pepsi".into(),
                    flavor: "Wild Cherry".into(),
                    kind: DrinkKind::Soda,
                    image_url: None,
                },
                DrinkEntry {
                    name: "Cherry Coke Zero".into(),
                    brand: "Coca-Cola".into(),
                    flavor: "Cherry Zero".into(),
                    kind: DrinkKind::Soda,
                    image_url: None,
                },
            ],
        }
    }

    #[test]
    fn search_starts_with_before_contains() {
        let db = sample_db();
        let hits = db.search(DrinkKind::Soda, "cherry", 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "Cherry Coke Zero"); // name starts with query
        assert_eq!(hits[1].name, "Pepsi Wild Cherry"); // brand/name contains
        assert!(db.search(DrinkKind::EnergyDrink, "cherry", 10).is_empty());
    }

    #[test]
    fn update_validation() {
        let cur = sample_db();
        let mut newer = sample_db();
        newer.version = 2;
        assert!(validate_update(&cur, &newer).is_ok());
        let mut same = sample_db();
        same.version = 1;
        assert!(validate_update(&cur, &same).is_err());
        let mut empty = sample_db();
        empty.version = 3;
        empty.entries.clear();
        assert!(validate_update(&cur, &empty).is_err());
    }

    #[test]
    fn bundled_db_parses_and_has_entries() {
        let db = DrinksDb::bundled();
        assert!(db.version >= 1, "bundled drinks DB must be present");
        assert!(
            db.count(DrinkKind::Soda) > 0 && db.count(DrinkKind::EnergyDrink) > 0,
            "bundled DB should contain real compiled entries of both kinds"
        );
    }
}
