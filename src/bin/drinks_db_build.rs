//! drinks-db-build — the Rust twin of `tools/drinks-db/build_drinks_db.py`.
//!
//! Reads an Open Food Facts CSV export (tab-separated, the format of
//! `en.openfoodfacts.org.products.csv`) from a file or stdin, filters it
//! down to US/English sodas + energy drinks using
//! `favelists::drinks::filter_product`, dedupes, and writes the compact
//! drinks pack JSON the app bundles.
//!
//! Usage:
//!   drinks-db-build <export.csv> <out.json> [version]
//!   zcat export.csv.gz | drinks-db-build - out.json [version]
//!
//! The Python tool under tools/drinks-db/ does the same job with streaming
//! gzip support and is the documented pipeline; this binary exists so the
//! shared filter/dedupe logic is exercised by the Rust test suite.

use favelists::drinks::{dedupe, filter_product, DrinkKind, DrinksDb, RawProduct};
use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: drinks-db-build <export.csv|-> <out.json> [version]");
        std::process::exit(2);
    }
    let version: u32 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1);
    let text = if args[1] == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .expect("read stdin");
        buf
    } else {
        std::fs::read_to_string(&args[1]).expect("read export file")
    };
    let db = build(&text, version);
    let json = serde_json::to_string(&db).expect("serialise");
    std::fs::write(&args[2], json).expect("write output");
    eprintln!(
        "drinks-db-build: {} entries ({} sodas, {} energy drinks), version {}",
        db.entries.len(),
        db.count(DrinkKind::Soda),
        db.count(DrinkKind::EnergyDrink),
        db.version
    );
}

/// Parse a whole OFF CSV export held in memory (the streaming Python tool is
/// preferred for the multi-GB full export; this suits category extracts).
pub fn build(text: &str, version: u32) -> DrinksDb {
    let mut lines = text.lines();
    let header: Vec<String> = match lines.next() {
        Some(h) => h.split('\t').map(|s| s.trim().to_string()).collect(),
        None => {
            return DrinksDb {
                version,
                source: "Open Food Facts export (ODbL)".to_string(),
                entries: Vec::new(),
            }
        }
    };
    let col = |name: &str| header.iter().position(|h| h == name);
    let (i_name, i_brands, i_cats, i_countries, i_lang, i_img) = (
        col("product_name"),
        col("brands"),
        col("categories_tags"),
        col("countries_tags"),
        col("lang"),
        col("image_front_url").or_else(|| col("image_url")),
    );
    let get = |fields: &[&str], idx: Option<usize>| -> String {
        idx.and_then(|i| fields.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let mut entries = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let raw = RawProduct {
            name: get(&fields, i_name),
            brands: get(&fields, i_brands),
            categories_tags: get(&fields, i_cats)
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            countries_tags: get(&fields, i_countries)
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            lang: get(&fields, i_lang),
            image_url: {
                let u = get(&fields, i_img);
                if u.is_empty() {
                    None
                } else {
                    Some(u)
                }
            },
        };
        if let Some(e) = filter_product(&raw) {
            entries.push(e);
        }
    }
    entries = dedupe(entries);
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    DrinksDb {
        version,
        source: "Open Food Facts export (ODbL); images CC BY-SA".to_string(),
        entries,
    }
}
