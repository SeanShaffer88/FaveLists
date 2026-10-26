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
    /// Caffeine content in mg per 100 ml, when the OFF export carried a
    /// caffeine nutriment for this product (v0.2 #7). Absent on older
    /// packs and on products with no caffeine data — `serde(default)`
    /// keeps old packs loading, `skip_serializing_if` keeps new packs
    /// from growing a null on every entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caffeine_mg_per_100ml: Option<f64>,
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

    /// Short caffeine label ("32 mg/100ml", "31.5 mg/100ml") for display
    /// and for the `caffeine` field set when a drink is added (v0.2 #7).
    pub fn caffeine_label(&self) -> Option<String> {
        self.caffeine_mg_per_100ml.map(format_caffeine_label)
    }
}

/// Format a caffeine content (mg per 100 ml) as the label stored in a
/// drink item's `caffeine` field: "32 mg/100ml", "31.5 mg/100ml".
/// Shared by the pack entries above and the barcode lookup (#11), so
/// every caffeine field in the app is written the same way — and
/// [`caffeine_value`] can read them all back.
pub fn format_caffeine_label(mg: f64) -> String {
    if mg.fract() == 0.0 {
        format!("{} mg/100ml", mg as i64)
    } else {
        format!("{mg:.1} mg/100ml")
    }
}

/// The numeric caffeine content (mg per 100 ml) of an item, parsed from
/// the leading number of its `caffeine` field ("32 mg/100ml" → 32.0,
/// "31.5 mg/100ml" → 31.5). `None` when the item carries no caffeine
/// field or the value does not start with a number ("caffeine free" is
/// not a number). Drives the caffeine sort + filter (v0.3 #13).
pub fn caffeine_value(item: &crate::model::Item) -> Option<f64> {
    let text = item.field("caffeine")?.trim();
    let digits: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// "Tried it" progress for one drinks list (v0.3 #12): how many of its
/// items carry the `tried` status, out of the list's total. Per-list by
/// design — drink entries carry no pack key, so there is nothing to
/// count pack-wide against.
pub fn tried_progress(list: &crate::model::FavList) -> (usize, usize) {
    let tried = list
        .items
        .iter()
        .filter(|i| i.status == Some(crate::model::ItemStatus::Tried))
        .count();
    (tried, list.items.len())
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
    /// The OFF export's `caffeine_100g` nutriment column (grams per
    /// 100 g), when the row carries it (v0.2 #7).
    pub caffeine_100g: Option<f64>,
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
    // OFF brands are often a comma list ("Coca-Cola, The Coca-Cola Company");
    // the first is the consumer-facing brand.
    let brand = raw
        .brands
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    // Classification (v0.2 #7): the OFF category tags are preferred, but a
    // soda-tagged row with positive energy evidence in its name/brand is
    // an energy drink — OFF cross-tags a few hundred of them.
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
        if has_energy_evidence(name, &brand) {
            DrinkKind::EnergyDrink
        } else {
            DrinkKind::Soda
        }
    } else {
        return None;
    };
    let flavor = parse_flavor(name, &brand);
    Some(DrinkEntry {
        name: name.to_string(),
        brand,
        flavor,
        kind,
        image_url: raw.image_url.clone().filter(|u| !u.trim().is_empty()),
        caffeine_mg_per_100ml: raw.caffeine_100g.and_then(caffeine_mg_per_100ml),
    })
}

/// Convert the OFF `caffeine_100g` nutriment (grams per 100 g) to mg per
/// 100 ml (v0.2 #7). Drinks are ~1 g/ml, so g/100 g × 1000 ≈ mg/100 ml.
/// Implausible values (data-entry errors do happen) are dropped, and the
/// result is rounded to one decimal.
pub fn caffeine_mg_per_100ml(caffeine_100g: f64) -> Option<f64> {
    let mg = caffeine_100g * 1000.0;
    if !(1.0..=400.0).contains(&mg) {
        return None;
    }
    Some((mg * 10.0).round() / 10.0)
}

/// Documented energy-drink brands (normalised: lower-case, punctuation
/// collapsed — see `normalise_title`), used as positive classification
/// evidence in [`has_energy_evidence`] (v0.2 #7). Derived 2026-10-25 from
/// the bundled pack itself (brands whose entries were ≥90% energy drinks
/// across ≥3 entries), plus the canonical energy brands whose OFF rows
/// sit under longer company strings. Glaceau and Powerade were excluded
/// on judgement: both are primarily vitamin/sports-drink brands. The
/// Python generator mirrors this list exactly.
pub const ENERGY_BRANDS: &[&str] = &[
    "1st phorm",
    "3d energy",
    "5 hour energy",
    "alani",
    "alani nu",
    "applied nutrition",
    "arizona super lxr",
    "bang",
    "bang energy",
    "bucked up energy",
    "bum",
    "c4",
    "celcius",
    "celsius",
    "clean cause",
    "fitaid",
    "flrt",
    "full throttle",
    "gfuel",
    "ghost",
    "gorilla mind",
    "gorgie",
    "jocko",
    "lotus",
    "monster energy",
    "nos",
    "oca",
    "optimum nutrition",
    "proper wild",
    "prox",
    "red bull",
    "reign",
    "rockstar",
    "vivee",
    "zoa",
];

/// Positive evidence that a product is an energy drink (v0.2 #7): the
/// word "energy" among the name's tokens, or a brand on the documented
/// [`ENERGY_BRANDS`] list. Absence of evidence never reclassifies
/// anything (plenty of real energy drinks carry neither marker).
pub fn has_energy_evidence(name: &str, brand: &str) -> bool {
    if word_tokens(name).iter().any(|t| t == "energy") {
        return true;
    }
    ENERGY_BRANDS.contains(&normalise_title(brand).as_str())
}

/// The alphanumeric word tokens of a string, lower-cased.
fn word_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            cur.extend(ch.to_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Descriptor phrases stripped during flavour extraction, longest first
/// wherever one phrase contains another ("energy drink" before "energy",
/// "flavoured" before "flavour"). The Python generator mirrors this list
/// exactly — the two are kept in agreement by shared test vectors.
const FLAVOR_DESCRIPTORS: &[&str] = &[
    "regular strength",
    "extra strength",
    "caffeine free",
    "energy drink",
    "zero calorie",
    "calorie free",
    "energy shot",
    "low calorie",
    "zero sugar",
    "sugar free",
    "soft drink",
    "carbonated",
    "naturally",
    "sparkling",
    "beverage",
    "flavored",
    "flavoured",
    "premium",
    "energy",
    "flavor",
    "flavour",
    "drink",
    "soda",
    "diet",
    "mix",
];

/// Unit words that can follow a quantity in a size token ("12 fl oz",
/// "1 liter", "24 pack").
const SIZE_UNITS: &[&str] = &[
    "fl", "oz", "ml", "cl", "l", "g", "kg", "pack", "pk", "ct", "count", "liter", "liters",
    "litre", "litres",
];

/// Best-effort flavour extraction from a product name (v0.2 #7 — a rewrite
/// of the original brand-strip heuristic, which bailed out whenever the
/// recorded brand was not a substring of the name; OFF's `brands` field
/// is usually the manufacturer, so that was most rows).
///
/// The name is split into comma segments. OFF names are typically
/// "Product, descriptors…, flavour", so the flavour is the last segment
/// after the first that still has content once size tokens ("1.93oz",
/// "12 fl oz") and descriptor phrases ("Zero Sugar", "Energy Drink",
/// "Diet") are stripped. With no usable tail segment, the first segment
/// is mined instead — but only with a brand anchor: the brand as a
/// substring, or leading name tokens that are brand tokens. Nothing is
/// ever invented: when no rule fires, the flavour stays empty.
pub fn parse_flavor(name: &str, brand: &str) -> String {
    let mut rest = name.trim().to_string();
    let mut brand_found = false;
    if !brand.is_empty() {
        if let Some(idx) = find_ci(&rest, brand) {
            rest = format!("{}{}", &rest[..idx], &rest[idx + brand.len()..]);
            brand_found = true;
        }
    }
    let segments: Vec<&str> = rest
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if segments.is_empty() {
        return String::new();
    }
    let tail_clean: Vec<String> = segments[1..]
        .iter()
        .map(|s| clean_flavor_text(s))
        .filter(|s| !s.is_empty())
        .collect();
    let result = if let Some(last) = tail_clean.last() {
        last.clone()
    } else {
        let (seg2, stripped) = strip_leading_brand_tokens(segments[0], brand);
        if !brand_found && !stripped {
            return String::new();
        }
        clean_flavor_text(&seg2)
    };
    if result.is_empty() || normalise_title(&result) == normalise_title(name) {
        return String::new();
    }
    title_case(&result)
}

/// Byte index of the first case-insensitive occurrence of `needle` in
/// `haystack` (both compared per-char lower-cased; the index is a char
/// boundary of the original string).
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() {
        return None;
    }
    let h: Vec<(usize, char)> = haystack.char_indices().collect();
    if h.len() < n.len() {
        return None;
    }
    for w in 0..=(h.len() - n.len()) {
        let hit = (0..n.len()).all(|k| h[w + k].1.to_lowercase().eq(n[k].to_lowercase()));
        if hit {
            return Some(h[w].0);
        }
    }
    None
}

/// Strip size tokens and descriptor phrases from one name segment:
/// "-"/"·"/"|" become spaces, sizes go first, descriptors are removed
/// (two passes — removing one phrase can expose another), sizes again,
/// then whitespace is collapsed and edge separators trimmed.
fn clean_flavor_text(segment: &str) -> String {
    let s: String = segment
        .chars()
        .map(|c| {
            if c == '-' || c == '·' || c == '|' {
                ' '
            } else {
                c
            }
        })
        .collect();
    let s = strip_size_tokens(&s);
    let mut s = s;
    for _ in 0..2 {
        for phrase in FLAVOR_DESCRIPTORS {
            s = remove_phrase_ci(&s, phrase);
        }
    }
    let s = strip_size_tokens(&s);
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c| c == ' ' || c == ',' || c == ';')
        .to_string()
}

/// Remove every word-boundary-delimited, case-insensitive occurrence of
/// `phrase` from `text` (boundaries = non-alphanumeric or string edge).
fn remove_phrase_ci(text: &str, phrase: &str) -> String {
    let hay: Vec<char> = text.chars().collect();
    let needle: Vec<char> = phrase.chars().collect();
    if needle.is_empty() || hay.len() < needle.len() {
        return text.to_string();
    }
    let mut out: Vec<char> = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        let matches = i + needle.len() <= hay.len()
            && (0..needle.len()).all(|k| hay[i + k].to_lowercase().eq(needle[k].to_lowercase()));
        if matches {
            let before_ok = i == 0 || !hay[i - 1].is_alphanumeric();
            let after = i + needle.len();
            let after_ok = after >= hay.len() || !hay[after].is_alphanumeric();
            if before_ok && after_ok {
                i = after;
                continue;
            }
        }
        out.push(hay[i]);
        i += 1;
    }
    out.into_iter().collect()
}

/// Is this token (already lower-cased, edge separators stripped) a fused
/// quantity+unit like "1.93oz", "500ml", "12-pack"?
fn is_fused_size(tok: &str) -> bool {
    // Split into the leading numeric part and the trailing unit part.
    let bytes = tok.as_bytes();
    let mut i = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_digit() {
            seen_digit = true;
            i += 1;
        } else if b == b'.' && !seen_dot {
            seen_dot = true;
            i += 1;
        } else {
            break;
        }
    }
    if !seen_digit || i == 0 {
        return false;
    }
    let unit = &tok[i..];
    if let Some(u) = unit.strip_prefix('-') {
        return matches!(u, "pack" | "pk" | "ct");
    }
    matches!(unit, "floz" | "oz" | "ml" | "cl" | "l" | "g" | "kg")
}

/// Is this token a bare quantity like "12" or "1.93" (`^\d+(\.\d+)?$`,
/// exactly as the Python generator's regex)?
fn is_bare_number(tok: &str) -> bool {
    let mut parts = tok.split('.');
    let first = parts.next().unwrap_or("");
    if first.is_empty() || !first.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    match parts.next() {
        None => true,
        Some(rest) => {
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) && parts.next().is_none()
        }
    }
}

/// Drop size tokens from a whitespace-tokenised segment: fused forms
/// ("1.93oz") and quantity+unit pairs ("12 fl oz", "1 liter", "24 pack").
fn strip_size_tokens(segment: &str) -> String {
    let toks: Vec<&str> = segment.split_whitespace().collect();
    let lows: Vec<String> = toks
        .iter()
        .map(|t| {
            t.to_lowercase()
                .trim_matches(|c| matches!(c, ',' | ';' | '-' | '·' | '|' | '/'))
                .to_string()
        })
        .collect();
    let mut out: Vec<&str> = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        let l = &lows[i];
        if is_fused_size(l) {
            i += 1;
            continue;
        }
        if is_bare_number(l) {
            let next = lows.get(i + 1).map(|s| s.as_str()).unwrap_or("");
            if next == "fl" && lows.get(i + 2).map(|s| s.as_str()) == Some("oz") {
                i += 3;
                continue;
            }
            if SIZE_UNITS.contains(&next) {
                i += 2;
                continue;
            }
        }
        out.push(toks[i]);
        i += 1;
    }
    out.join(" ")
}

/// Remove leading words of `seg` that are brand tokens ("Bang" from brand
/// "bang energy" in "Bang Peach Mango"). Returns the remainder and
/// whether anything was stripped.
fn strip_leading_brand_tokens(seg: &str, brand: &str) -> (String, bool) {
    let brand_tokens = word_tokens(brand);
    if brand_tokens.is_empty() {
        return (seg.to_string(), false);
    }
    // Alphanumeric runs of seg, with byte spans.
    let mut runs: Vec<(usize, usize, String)> = Vec::new();
    let mut start: Option<usize> = None;
    for (idx, ch) in seg.char_indices() {
        if ch.is_alphanumeric() {
            if start.is_none() {
                start = Some(idx);
            }
        } else if let Some(s) = start.take() {
            runs.push((s, idx, seg[s..idx].to_lowercase()));
        }
    }
    if let Some(s) = start {
        runs.push((s, seg.len(), seg[s..].to_lowercase()));
    }
    let mut end = 0;
    let mut count = 0;
    for (_, e, tok) in &runs {
        if brand_tokens.contains(tok) {
            end = *e;
            count += 1;
        } else {
            break;
        }
    }
    if count == 0 {
        (seg.to_string(), false)
    } else {
        (seg[end..].to_string(), true)
    }
}

/// Title-case a flavour phrase: per hyphen-part, first char upper, the
/// rest lower ("sicily blood orange" -> "Sicily Blood Orange").
fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|word| {
            word.split('-')
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => {
                            let mut out: String = first.to_uppercase().collect();
                            out.extend(chars.flat_map(|c| c.to_lowercase()));
                            out
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect::<Vec<_>>()
        .join(" ")
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
                if out[i].caffeine_mg_per_100ml.is_none() && e.caffeine_mg_per_100ml.is_some() {
                    out[i].caffeine_mg_per_100ml = e.caffeine_mg_per_100ml;
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
                    if result[i].caffeine_mg_per_100ml.is_none()
                        && e.caffeine_mg_per_100ml.is_some()
                    {
                        result[i].caffeine_mg_per_100ml = e.caffeine_mg_per_100ml;
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
            caffeine_100g: None,
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
            caffeine_mg_per_100ml: None,
        };
        let b = DrinkEntry {
            brand: "Coca-Cola".into(),
            image_url: Some("https://img/1.jpg".into()),
            caffeine_mg_per_100ml: None,
            ..a.clone()
        };
        let different_brand = DrinkEntry {
            name: "Cola".into(),
            brand: "Acme".into(),
            flavor: String::new(),
            kind: DrinkKind::Soda,
            image_url: None,
            caffeine_mg_per_100ml: None,
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
            caffeine_mg_per_100ml: None,
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
                    caffeine_mg_per_100ml: None,
                },
                DrinkEntry {
                    name: "Pepsi Wild Cherry".into(),
                    brand: "Pepsi".into(),
                    flavor: "Wild Cherry".into(),
                    kind: DrinkKind::Soda,
                    image_url: None,
                    caffeine_mg_per_100ml: None,
                },
                DrinkEntry {
                    name: "Cherry Coke Zero".into(),
                    brand: "Coca-Cola".into(),
                    flavor: "Cherry Zero".into(),
                    kind: DrinkKind::Soda,
                    image_url: None,
                    caffeine_mg_per_100ml: None,
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

    /// The v0.2 #7 flavour vectors — the Python generator's `--selftest`
    /// runs the SAME table; the twins must agree on every row.
    #[test]
    fn flavor_extraction_vectors_match_python_twin() {
        let cases: &[(&str, &str, &str)] = &[
            ("Coca-Cola Classic", "Coca-Cola", "Classic"),
            ("Pepsi", "Pepsi", ""),
            ("Pepsi", "Pepsi-Cola", ""),
            ("Ghost Energy Drink", "Ghost", ""),
            ("Fanta Orange Soda", "Fanta", "Orange"),
            (
                "!Ajua!, Caffeine Free Soda, Mandarin Orange",
                "!Ajua!",
                "Mandarin Orange",
            ),
            ("1 Liter Pepsi", "Pepsi", ""),
            ("3D Citrus Mist Energy Drink", "3D Energy", "Citrus Mist"),
            (
                "5-hour Energy Regular Strength Berry 1.93oz",
                "5-hour Energy",
                "Berry",
            ),
            (
                "5-hour ENERGY Extra Strength Hawaiian Breeze",
                "5-hour ENERGY",
                "Hawaiian Breeze",
            ),
            ("Bang Peach Mango", "bang energy", "Peach Mango"),
            ("Bang sour heads", "Bang", "Sour Heads"),
            ("Amazon Energy", "Sambazon", ""),
            (
                "51 Fifty, Energy Drink, Fruit Punch",
                "51 Fifty Energy",
                "Fruit Punch",
            ),
            ("Amp energy, mixed berry energy drink", "", "Mixed Berry"),
            ("Coca-Cola Cherry", "The Coca-Cola Company", "Cherry"),
            ("ACTIVE Essential Amino Acid Blend", "OPTAVIA", ""),
            (
                "Monster Energy Ultra Sunrise",
                "Monster Energy",
                "Ultra Sunrise",
            ),
            (
                "Monster Energy Ultra Sunrise 16 fl oz",
                "Monster Energy",
                "Ultra Sunrise",
            ),
            ("51 Fifty, Sugar Free Energy Drink", "51 Fifty Energy", ""),
            ("Dr Pepper Zero Sugar", "Dr Pepper", ""),
            ("Fanta Orange 12 fl oz", "", ""),
            ("Fanta, Orange", "", "Orange"),
            ("Ahold, diet root beer", "Ahold", "Root Beer"),
            (
                "A' siciliana, sicily blood orange soda",
                "A' Siciliana",
                "Sicily Blood Orange",
            ),
            ("Diet Coke", "Coca-Cola", ""),
            (
                "Coca-Cola, Cherry Zero Sugar, 12 pack",
                "Coca-Cola",
                "Cherry",
            ),
            (
                "Gatorade Frost Glacier Freeze",
                "Gatorade",
                "Frost Glacier Freeze",
            ),
            ("7up, caffeine free naturally flavored soda", "", ""),
            (
                "2 Gingers, Premium Ginger Ale",
                "Polar Corporation",
                "Ginger Ale",
            ),
        ];
        for (name, brand, expected) in cases {
            assert_eq!(
                &parse_flavor(name, brand),
                expected,
                "flavour of {name:?} (brand {brand:?})"
            );
        }
    }

    #[test]
    fn soda_tagged_row_with_energy_evidence_becomes_energy_drink() {
        // Name-token evidence.
        let by_name = filter_product(&raw("Amazon Energy", "Sambazon", &["en:sodas"])).unwrap();
        assert_eq!(by_name.kind, DrinkKind::EnergyDrink);
        // Brand-list evidence.
        let by_brand =
            filter_product(&raw("Bang Peach Mango", "bang energy", &["en:sodas"])).unwrap();
        assert_eq!(by_brand.kind, DrinkKind::EnergyDrink);
        // No evidence: stays soda even with a big-brand name.
        let plain = filter_product(&raw("Coca-Cola Classic", "Coca-Cola", &["en:sodas"])).unwrap();
        assert_eq!(plain.kind, DrinkKind::Soda);
        // OFF tag wins when it already says energy.
        let tagged =
            filter_product(&raw("Mystery Drink", "Generic", &["en:energy-drinks"])).unwrap();
        assert_eq!(tagged.kind, DrinkKind::EnergyDrink);
    }

    #[test]
    fn caffeine_conversion_and_label() {
        assert_eq!(caffeine_mg_per_100ml(0.032), Some(32.0));
        assert_eq!(caffeine_mg_per_100ml(0.0315), Some(31.5));
        assert_eq!(caffeine_mg_per_100ml(0.0), None);
        assert_eq!(caffeine_mg_per_100ml(2.5), None); // 2500 mg — data error
        let mut r = raw("Energy Drink X", "Brand", &["en:energy-drinks"]);
        r.caffeine_100g = Some(0.032);
        let e = filter_product(&r).unwrap();
        assert_eq!(e.caffeine_mg_per_100ml, Some(32.0));
        assert_eq!(e.caffeine_label().as_deref(), Some("32 mg/100ml"));
    }

    #[test]
    fn caffeine_value_parses_the_stored_label() {
        use crate::model::{Category, FavList, Item, ItemStatus};
        let mut item = Item::new(1, "Zap");
        assert_eq!(caffeine_value(&item), None); // no field at all
        item.fields.insert("caffeine".into(), "32 mg/100ml".into());
        assert_eq!(caffeine_value(&item), Some(32.0));
        item.fields
            .insert("caffeine".into(), "31.5 mg/100ml".into());
        assert_eq!(caffeine_value(&item), Some(31.5));
        item.fields.insert("caffeine".into(), "180".into());
        assert_eq!(caffeine_value(&item), Some(180.0));
        item.fields
            .insert("caffeine".into(), "caffeine free".into());
        assert_eq!(caffeine_value(&item), None);
        item.fields.insert("caffeine".into(), "  ".into());
        assert_eq!(caffeine_value(&item), None); // blank counts as absent

        // tried_progress counts only the `tried` status, over all items.
        let mut list = FavList::new(1, "Energy", Category::EnergyDrinks, false);
        assert_eq!(tried_progress(&list), (0, 0));
        let mut a = Item::new(2, "A");
        a.status = Some(ItemStatus::Tried);
        let mut b = Item::new(3, "B");
        b.status = Some(ItemStatus::Want);
        list.items.push(a);
        list.items.push(b);
        list.items.push(Item::new(4, "C"));
        assert_eq!(tried_progress(&list), (1, 3));
        // The label formatter round-trips through caffeine_value.
        let mut d = Item::new(5, "D");
        d.fields
            .insert("caffeine".into(), format_caffeine_label(31.5));
        assert_eq!(caffeine_value(&d), Some(31.5));
    }

    #[test]
    fn old_pack_entries_without_caffeine_key_still_load() {
        let json = r#"{"version":1,"entries":[
            {"name":"Cola","brand":"B","flavor":"Classic","kind":"Soda","image_url":null}
        ]}"#;
        let db: DrinksDb = serde_json::from_str(json).unwrap();
        assert_eq!(db.entries[0].caffeine_mg_per_100ml, None);
        // And the new field serialises away entirely when absent.
        let out = serde_json::to_string(&db.entries[0]).unwrap();
        assert!(
            !out.contains("caffeine"),
            "absent caffeine stays absent: {out}"
        );
    }
}
