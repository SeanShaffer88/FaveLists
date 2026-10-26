//! Barcode lookup for drinks (v0.3 program #11): type or scan a
//! UPC/EAN and Open Food Facts — the same source the bundled drinks
//! pack is compiled from — fills in the product.
//!
//! This joins the provider/drinks-search family: it is a network seam
//! in the providers style (a pure parser over the response text, unit-
//! tested offline against fixtures, plus a thin ureq wrapper), NOT a
//! registry command — provider/drinks search has never been routed
//! through the registry either (see PARITY's not-yet-routed seams).
//! Adding the found product goes through the ordinary `item.add`
//! dispatch with source `openfoodfacts` and the barcode as its
//! provider id.
//!
//! No API key is involved, and none may ever be committed.

use crate::drinks;
use serde_json::Value;
use std::collections::BTreeMap;

/// A product resolved from a barcode: everything the add needs.
#[derive(Debug, Clone, PartialEq)]
pub struct BarcodeProduct {
    /// The barcode digits the user entered (also the item's provider id).
    pub code: String,
    /// Product name (`product_name`).
    pub title: String,
    /// First brand of the response's comma-separated `brands`.
    pub brand: String,
    /// Flavour mined from the name by the drinks pipeline's own
    /// extractor; empty when no flavour is parseable.
    pub flavor: String,
    /// Caffeine content in mg per 100 ml, converted and plausibility-
    /// filtered by the drinks conversion; `None` when the response
    /// carries no usable caffeine nutriment.
    pub caffeine_mg_per_100ml: Option<f64>,
    /// The stored-field label for the caffeine value ("32 mg/100ml").
    pub caffeine_label: Option<String>,
}

impl BarcodeProduct {
    /// The `item.add` fields for this product: brand, flavour (when
    /// parseable) and caffeine (when known) — the same keys the drinks
    /// database add path stores.
    pub fn add_fields(&self) -> BTreeMap<String, String> {
        let mut fields = BTreeMap::new();
        if !self.brand.is_empty() {
            fields.insert("brand".to_string(), self.brand.clone());
        }
        if !self.flavor.is_empty() {
            fields.insert("flavor".to_string(), self.flavor.clone());
        }
        if let Some(label) = &self.caffeine_label {
            fields.insert("caffeine".to_string(), label.clone());
        }
        fields
    }
}

/// The outcome of parsing a product response: a product, or a clean
/// "no such product" (the source answers unknown codes with
/// `status: 0`, not an error).
#[derive(Debug, Clone, PartialEq)]
pub enum BarcodeLookup {
    Found(BarcodeProduct),
    NotFound,
}

/// Is this a plausible UPC/EAN code? Digits only, 8–14 of them (UPC-E
/// through EAN-14 / ITF-14). The dialog checks this before any lookup;
/// hardware scanners simply type the digits.
pub fn valid_code(code: &str) -> bool {
    let code = code.trim();
    (8..=14).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_digit())
}

/// Parse an Open Food Facts `api/v2/product/<code>.json` response.
/// Malformed JSON (or a response with no `status`) is a clean `Err`;
/// `status: 0` — and a found shell with no product name — is
/// [`BarcodeLookup::NotFound`].
pub fn parse_product_response(code: &str, text: &str) -> Result<BarcodeLookup, String> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| format!("unexpected barcode response: {e}"))?;
    let status = v
        .get("status")
        .and_then(|s| s.as_i64())
        .ok_or_else(|| "unexpected barcode response: no status field".to_string())?;
    if status == 0 {
        return Ok(BarcodeLookup::NotFound);
    }
    let product = v.get("product");
    let title = product
        .and_then(|p| p.get("product_name"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        return Ok(BarcodeLookup::NotFound);
    }
    // OFF brands are often a comma list; the first is consumer-facing
    // (the same rule the drinks pipeline applies).
    let brand = product
        .and_then(|p| p.get("brands"))
        .and_then(|b| b.as_str())
        .unwrap_or("")
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    // The nutriment arrives as a number in most responses, a string
    // in some; accept both. Conversion + plausibility filtering is
    // the drinks module's, so a barcode add and a pack add agree.
    let caffeine_100g = product
        .and_then(|p| p.get("nutriments"))
        .and_then(|n| n.get("caffeine_100g"))
        .and_then(|c| c.as_f64().or_else(|| c.as_str()?.trim().parse().ok()));
    let caffeine_mg = caffeine_100g.and_then(drinks::caffeine_mg_per_100ml);
    Ok(BarcodeLookup::Found(BarcodeProduct {
        code: code.trim().to_string(),
        flavor: drinks::parse_flavor(&title, &brand),
        title,
        brand,
        caffeine_label: caffeine_mg.map(drinks::format_caffeine_label),
        caffeine_mg_per_100ml: caffeine_mg,
    }))
}

/// Look a barcode up (network). Unknown codes come back
/// [`BarcodeLookup::NotFound`] — the source answers them with an HTTP
/// 404 carrying the `status: 0` body, which is a result, not a
/// failure. Transport/parse failures are a clean `Err`.
pub fn fetch_product(code: &str) -> Result<BarcodeLookup, String> {
    let url = format!(
        "https://world.openfoodfacts.org/api/v2/product/{}.json",
        code.trim()
    );
    let resp = ureq::get(&url)
        .set("User-Agent", crate::USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .call();
    let text = match resp {
        Ok(r) => r
            .into_string()
            .map_err(|e| format!("barcode lookup failed: {e}"))?,
        Err(ureq::Error::Status(404, r)) => r
            .into_string()
            .map_err(|e| format!("barcode lookup failed: {e}"))?,
        Err(e) => return Err(format!("barcode lookup failed: {e}")),
    };
    parse_product_response(code, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> BarcodeProduct {
        match parse_product_response("3012345678901", text).unwrap() {
            BarcodeLookup::Found(p) => p,
            BarcodeLookup::NotFound => panic!("expected a product"),
        }
    }

    #[test]
    fn parses_name_brand_and_caffeine() {
        let p = found(
            r#"{"status":1,"code":"3012345678901","product":{
                "product_name":"ZapCo Wild Berry",
                "brands":"ZapCo, ZapCo Holdings Ltd",
                "nutriments":{"caffeine_100g":0.032}}}"#,
        );
        assert_eq!(p.title, "ZapCo Wild Berry");
        assert_eq!(p.brand, "ZapCo"); // first brand only
        assert_eq!(p.flavor, "Wild Berry");
        assert_eq!(p.caffeine_mg_per_100ml, Some(32.0));
        assert_eq!(p.caffeine_label.as_deref(), Some("32 mg/100ml"));
        let fields = p.add_fields();
        assert_eq!(fields["brand"], "ZapCo");
        assert_eq!(fields["flavor"], "Wild Berry");
        assert_eq!(fields["caffeine"], "32 mg/100ml");
    }

    #[test]
    fn caffeine_string_form_and_implausible_values() {
        let p = found(
            r#"{"status":1,"product":{"product_name":"Zap",
                "nutriments":{"caffeine_100g":"0.0315"}}}"#,
        );
        assert_eq!(p.caffeine_label.as_deref(), Some("31.5 mg/100ml"));
        // 2.5 g/100 g would be 2500 mg/100 ml — a data error, dropped
        // by the same plausibility filter the drinks pipeline uses.
        let p = found(
            r#"{"status":1,"product":{"product_name":"Zap",
                "nutriments":{"caffeine_100g":2.5}}}"#,
        );
        assert_eq!(p.caffeine_mg_per_100ml, None);
        assert!(!p.add_fields().contains_key("caffeine"));
        // No nutriments at all: still a product, just no caffeine.
        let p = found(r#"{"status":1,"product":{"product_name":"Zap"}}"#);
        assert_eq!(p.caffeine_mg_per_100ml, None);
        assert!(p.add_fields().is_empty());
    }

    #[test]
    fn not_found_shapes() {
        assert_eq!(
            parse_product_response("1", r#"{"status":0,"status_verbose":"product not found"}"#)
                .unwrap(),
            BarcodeLookup::NotFound
        );
        // A found shell with no usable name is a not-found, not a
        // nameless product.
        assert_eq!(
            parse_product_response("1", r#"{"status":1,"product":{"brands":"ZapCo"}}"#).unwrap(),
            BarcodeLookup::NotFound
        );
    }

    #[test]
    fn malformed_responses_are_clean_errors() {
        assert!(parse_product_response("1", "not json").is_err());
        assert!(parse_product_response("1", "{}").is_err()); // no status
        assert!(parse_product_response("1", "").is_err());
    }

    #[test]
    fn code_validation() {
        assert!(valid_code("3012345678901")); // EAN-13
        assert!(valid_code("01234565")); // 8-digit floor
        assert!(valid_code(" 3012345678901 ")); // surrounding space ok
        assert!(!valid_code("1234567")); // too short
        assert!(!valid_code("123456789012345")); // too long
        assert!(!valid_code("30123abc78901")); // letters
        assert!(!valid_code(""));
    }
}
