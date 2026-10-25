//! Artist image fallback chain (v0.2 program #6).
//!
//! MusicBrainz — the Artists/Bands provider — carries no artist photos
//! (spec §2 calls artist images best-effort), so artist entries used to
//! start coverless and rely on manual upload. This module is the fallback
//! tried when an artist is added and no cover was obtained:
//!
//! 1. **Deezer** (public API, no key): artist search → `picture_xl`.
//! 2. **Wikipedia** (REST API, no key): page summary → `originalimage`.
//!
//! Both response parsers are pure functions over the response text and
//! are unit-tested against recorded key-free fixtures in
//! `tests/fixtures/`, exactly like the other providers. The network layer
//! is the same thin ureq wrapper the parent module uses. Everything here
//! is best-effort: any failure simply returns `None` and the item stays
//! coverless, as before.

use crate::model::normalise_title;
use serde_json::Value;

/// Parse a Deezer artist-search response: the `picture_xl` of the best
/// entry. An entry whose name matches `name` (normalised) wins over a
/// merely first-listed one — search results can rank a similarly named
/// act higher. Entries with an empty `picture_xl` are skipped.
pub fn parse_deezer_artist_image(text: &str, name: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(text).ok()?;
    let data = v.get("data")?.as_array()?;
    let want = normalise_title(name);
    let mut first_picture: Option<String> = None;
    for entry in data {
        let Some(picture) = entry
            .get("picture_xl")
            .and_then(|p| p.as_str())
            .filter(|p| !p.trim().is_empty())
        else {
            continue;
        };
        let entry_name = entry.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if !want.is_empty() && normalise_title(entry_name) == want {
            return Some(picture.to_string());
        }
        if first_picture.is_none() {
            first_picture = Some(picture.to_string());
        }
    }
    first_picture
}

/// Parse a Wikipedia REST page-summary response: the page's original
/// image (`originalimage.source`), when the page has one.
pub fn parse_wikipedia_artist_image(text: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(text).ok()?;
    v.get("originalimage")?
        .get("source")?
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
}

/// First stage of the chain: resolve an artist name to an image URL via
/// Deezer, then Wikipedia. `None` when neither source has an image.
pub fn fetch_artist_image_url(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    // 1. Deezer artist search.
    let url = format!(
        "https://api.deezer.com/search/artist?q={}",
        super::enc(name)
    );
    if let Ok(text) = super::http_get(&url) {
        if let Some(img) = parse_deezer_artist_image(&text, name) {
            return Some(img);
        }
    }
    // 2. Wikipedia page summary (titles use underscores for spaces).
    let title = name.replace(' ', "_");
    let url = format!(
        "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
        super::enc(&title)
    );
    if let Ok(text) = super::http_get(&url) {
        if let Some(img) = parse_wikipedia_artist_image(&text) {
            return Some(img);
        }
    }
    None
}

/// The fallback chain as one call: image bytes for an artist, or `None`
/// (no image found anywhere, or any network step failed). Callers run
/// this on a background thread, like every other cover fetch.
pub fn fetch_artist_image(name: &str) -> Option<Vec<u8>> {
    let url = fetch_artist_image_url(name)?;
    super::download_bytes(&url).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap()
    }

    #[test]
    fn deezer_fixture_yields_picture_xl() {
        // Recorded response for "Radiohead" (first three entries).
        let url = parse_deezer_artist_image(&fixture("deezer_artist.json"), "Radiohead")
            .expect("Radiohead has a Deezer picture");
        assert!(url.contains("cdn-images.dzcdn.net/images/artist/"));
        assert!(url.contains("1000x1000"));
    }

    #[test]
    fn deezer_prefers_exact_name_match_over_first_hit() {
        // Same recorded entry shapes, ordered so a differently named act
        // comes first: the exact-name entry must still win.
        let text = r#"{"data":[
            {"name":"DJ Radiohead","picture_xl":"https://img.example/dj.jpg"},
            {"name":"Radiohead","picture_xl":"https://img.example/radiohead.jpg"}
        ]}"#;
        assert_eq!(
            parse_deezer_artist_image(text, "Radiohead").as_deref(),
            Some("https://img.example/radiohead.jpg")
        );
        // With no exact match, the first entry with a picture is the
        // best-effort answer.
        assert_eq!(
            parse_deezer_artist_image(text, "Radiohead Tribute").as_deref(),
            Some("https://img.example/dj.jpg")
        );
    }

    #[test]
    fn deezer_skips_empty_pictures_and_tolerates_garbage() {
        let text = r#"{"data":[
            {"name":"NoPic","picture_xl":""},
            {"name":"HasPic","picture_xl":"https://img.example/x.jpg"}
        ]}"#;
        assert_eq!(
            parse_deezer_artist_image(text, "HasPic").as_deref(),
            Some("https://img.example/x.jpg")
        );
        assert_eq!(parse_deezer_artist_image("not json", "X"), None);
        assert_eq!(parse_deezer_artist_image("{}", "X"), None);
        assert_eq!(parse_deezer_artist_image(r#"{"data":[]}"#, "X"), None);
    }

    #[test]
    fn wikipedia_fixture_yields_original_image() {
        // Recorded REST summary for "Radiohead" (trimmed to the fields
        // the parser and a human reader care about).
        let url = parse_wikipedia_artist_image(&fixture("wikipedia_artist_summary.json"))
            .expect("the Radiohead page has an original image");
        // The recorded response serves the original via thumb.wikimedia.org
        // (a 5940px-wide source image), not upload.wikimedia.org.
        assert!(url.contains("wikimedia.org"));
        assert!(url.contains("RadioheadO2211125_composite.jpg"));
    }

    #[test]
    fn wikipedia_missing_image_is_none() {
        assert_eq!(parse_wikipedia_artist_image("{}"), None);
        assert_eq!(
            parse_wikipedia_artist_image(r#"{"originalimage":{"source":""}}"#),
            None
        );
        assert_eq!(parse_wikipedia_artist_image("not json"), None);
    }
}
