//! Online auto-fill providers (spec §4). Every provider's response parsing is
//! a pure function over the response text and is unit-tested against recorded
//! fixtures in `tests/fixtures/`, so the test suite runs fully offline. The
//! network calls are thin ureq wrappers around those parsers.
//!
//! API keys come from the user's settings at runtime. There are NO keys in
//! this code and none may ever be committed.

use crate::model::{normalise_title, Category};
use serde_json::Value;
use std::collections::BTreeMap;

/// One auto-fill suggestion: everything the dropdown shows (thumbnail, title,
/// year, distinguishing line) plus the fields/thumbnail to use when the user
/// picks it.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// Provider-side id of the work ("348" for TMDB, an MBID, an OL key…).
    pub provider_id: String,
    /// Provider name recorded on the item ("tmdb", "openlibrary", …).
    pub source: &'static str,
    pub title: String,
    /// Year display ("1979", "2002–2003") when known.
    pub year: Option<String>,
    /// One distinguishing line: "Ridley Scott" / "Ursula K. Le Guin" /
    /// "Radiohead" / "PC (Microsoft Windows), PlayStation 4".
    pub detail: String,
    /// Small image URL for the dropdown row (may equal `cover_url`).
    pub thumb_url: Option<String>,
    /// Full-size cover/poster URL to cache when picked.
    pub cover_url: Option<String>,
    /// Category fields already known from the search response. Providers
    /// with a separate details call (TMDB, BGG) fill the rest in
    /// [`fetch_details`].
    pub fields: BTreeMap<String, String>,
}

impl Suggestion {
    fn new(source: &'static str, provider_id: impl Into<String>, title: impl Into<String>) -> Self {
        Suggestion {
            provider_id: provider_id.into(),
            source,
            title: title.into(),
            year: None,
            detail: String::new(),
            thumb_url: None,
            cover_url: None,
            fields: BTreeMap::new(),
        }
    }

    fn put(&mut self, key: &str, value: impl Into<String>) {
        let v = value.into();
        if !v.trim().is_empty() {
            self.fields.insert(key.to_string(), v);
        }
    }
}

/// Order suggestions the way the spec asks: titles that BEGIN WITH the query
/// first, then titles that merely CONTAIN it; stable within each group.
pub fn order_results(query: &str, mut results: Vec<Suggestion>) -> Vec<Suggestion> {
    let q = normalise_title(query);
    if q.is_empty() {
        return results;
    }
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    let mut rest = Vec::new();
    for r in results.drain(..) {
        let t = normalise_title(&r.title);
        if t.starts_with(&q) {
            starts.push(r);
        } else if t.contains(&q) {
            contains.push(r);
        } else {
            rest.push(r);
        }
    }
    starts.extend(contains);
    starts.extend(rest);
    starts
}

pub const MAX_RESULTS: usize = 10;

fn cap(mut v: Vec<Suggestion>) -> Vec<Suggestion> {
    v.truncate(MAX_RESULTS);
    v
}

// ---------------------------------------------------------------------------
// TMDB (movies + TV). Needs the user's v3 API key.
// ---------------------------------------------------------------------------

const TMDB_BASE: &str = "https://api.themoviedb.org/3";
const TMDB_IMG: &str = "https://image.tmdb.org/t/p";

/// Parse the TMDB genre list (`{"genres":[{"id":27,"name":"Horror"}]}`).
pub fn parse_tmdb_genres(text: &str) -> BTreeMap<i64, String> {
    let mut out = BTreeMap::new();
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        if let Some(gs) = v.get("genres").and_then(|g| g.as_array()) {
            for g in gs {
                if let (Some(id), Some(name)) = (
                    g.get("id").and_then(|i| i.as_i64()),
                    g.get("name").and_then(|n| n.as_str()),
                ) {
                    out.insert(id, name.to_string());
                }
            }
        }
    }
    out
}

fn tmdb_year(date: Option<&str>) -> Option<String> {
    date.filter(|d| d.len() >= 4).map(|d| d[..4].to_string())
}

pub fn parse_tmdb_movies(text: &str, genres: &BTreeMap<i64, String>) -> Vec<Suggestion> {
    parse_tmdb_search(text, genres, true)
}

pub fn parse_tmdb_tv(text: &str, genres: &BTreeMap<i64, String>) -> Vec<Suggestion> {
    parse_tmdb_search(text, genres, false)
}

fn parse_tmdb_search(text: &str, genres: &BTreeMap<i64, String>, movie: bool) -> Vec<Suggestion> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(results) = v.get("results").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in results {
        let Some(id) = r.get("id").and_then(|i| i.as_i64()) else {
            continue;
        };
        let title = if movie {
            r.get("title").and_then(|t| t.as_str())
        } else {
            r.get("name").and_then(|t| t.as_str())
        };
        let Some(title) = title else { continue };
        let mut s = Suggestion::new("tmdb", id.to_string(), title);
        let date = if movie {
            r.get("release_date").and_then(|d| d.as_str())
        } else {
            r.get("first_air_date").and_then(|d| d.as_str())
        };
        s.year = tmdb_year(date);
        if let Some(y) = &s.year {
            s.put(if movie { "year" } else { "years" }, y.clone());
        }
        if let Some(ids) = r.get("genre_ids").and_then(|g| g.as_array()) {
            let names: Vec<String> = ids
                .iter()
                .filter_map(|i| i.as_i64())
                .filter_map(|i| genres.get(&i).cloned())
                .collect();
            if !names.is_empty() {
                s.put("genre", names.join(", "));
                s.detail = names.join(", ");
            }
        }
        if let Some(p) = r.get("poster_path").and_then(|p| p.as_str()) {
            s.thumb_url = Some(format!("{TMDB_IMG}/w92{p}"));
            s.cover_url = Some(format!("{TMDB_IMG}/w500{p}"));
        }
        out.push(s);
    }
    out
}

/// Extra fields from a TMDB details response (movie: director/runtime/cast;
/// TV: creator/seasons/cast/years range).
pub fn parse_tmdb_details(text: &str, movie: bool) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return out;
    };
    if let Some(gs) = v.get("genres").and_then(|g| g.as_array()) {
        let names: Vec<&str> = gs
            .iter()
            .filter_map(|g| g.get("name").and_then(|n| n.as_str()))
            .collect();
        if !names.is_empty() {
            out.insert("genre".to_string(), names.join(", "));
        }
    }
    let credits = v.get("credits");
    if let Some(cast) = credits
        .and_then(|c| c.get("cast"))
        .and_then(|c| c.as_array())
    {
        let mut names: Vec<&str> = cast
            .iter()
            .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
            .collect();
        names.truncate(5);
        if !names.is_empty() {
            out.insert("cast".to_string(), names.join(", "));
        }
    }
    if movie {
        if let Some(rt) = v.get("runtime").and_then(|r| r.as_i64()) {
            if rt > 0 {
                out.insert("runtime".to_string(), format!("{rt} min"));
            }
        }
        if let Some(crew) = credits
            .and_then(|c| c.get("crew"))
            .and_then(|c| c.as_array())
        {
            if let Some(d) = crew
                .iter()
                .find(|c| c.get("job").and_then(|j| j.as_str()) == Some("Director"))
                .and_then(|c| c.get("name").and_then(|n| n.as_str()))
            {
                out.insert("director".to_string(), d.to_string());
            }
        }
        if let Some(y) = tmdb_year(v.get("release_date").and_then(|d| d.as_str())) {
            out.insert("year".to_string(), y);
        }
    } else {
        if let Some(creators) = v.get("created_by").and_then(|c| c.as_array()) {
            let names: Vec<&str> = creators
                .iter()
                .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
                .collect();
            if !names.is_empty() {
                out.insert("creator".to_string(), names.join(", "));
            }
        }
        if let Some(n) = v.get("number_of_seasons").and_then(|n| n.as_i64()) {
            out.insert("seasons".to_string(), n.to_string());
        }
        let first = tmdb_year(v.get("first_air_date").and_then(|d| d.as_str()));
        let last = tmdb_year(v.get("last_air_date").and_then(|d| d.as_str()));
        let years = match (first, last) {
            (Some(f), Some(l)) if f != l => format!("{f}\u{2013}{l}"),
            (Some(f), _) => f,
            _ => String::new(),
        };
        if !years.is_empty() {
            out.insert("years".to_string(), years);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Open Library (books). No key.
// ---------------------------------------------------------------------------

pub fn parse_openlibrary(text: &str) -> Vec<Suggestion> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(docs) = v.get("docs").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for d in docs {
        let Some(title) = d.get("title").and_then(|t| t.as_str()) else {
            continue;
        };
        let key = d.get("key").and_then(|k| k.as_str()).unwrap_or("");
        let mut s = Suggestion::new("openlibrary", key, title);
        let authors: Vec<&str> = d
            .get("author_name")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        if !authors.is_empty() {
            s.put("author", authors.join(", "));
            s.detail = authors.join(", ");
        }
        if let Some(y) = d.get("first_publish_year").and_then(|y| y.as_i64()) {
            s.year = Some(y.to_string());
            s.put("year", y.to_string());
        }
        if let Some(subjects) = d.get("subject").and_then(|s| s.as_array()) {
            let first = subjects
                .iter()
                .filter_map(|x| x.as_str())
                .take(2)
                .collect::<Vec<_>>();
            if !first.is_empty() {
                s.put("genre", first.join(", "));
            }
        }
        if let Some(pages) = d.get("number_of_pages_median").and_then(|p| p.as_i64()) {
            if pages > 0 {
                s.put("pages", pages.to_string());
            }
        }
        if let Some(cover) = d.get("cover_i").and_then(|c| c.as_i64()) {
            s.thumb_url = Some(format!("https://covers.openlibrary.org/b/id/{cover}-S.jpg"));
            s.cover_url = Some(format!("https://covers.openlibrary.org/b/id/{cover}-L.jpg"));
        }
        out.push(s);
    }
    out
}

// ---------------------------------------------------------------------------
// MusicBrainz + Cover Art Archive (artists, albums, songs). No key.
// ---------------------------------------------------------------------------

const CAA: &str = "https://coverartarchive.org";

fn mb_top_tag(v: &Value) -> Option<String> {
    let tags = v.get("tags").and_then(|t| t.as_array())?;
    let mut best: Option<(&str, i64)> = None;
    for t in tags {
        if let (Some(name), Some(count)) = (
            t.get("name").and_then(|n| n.as_str()),
            t.get("count").and_then(|c| c.as_i64()),
        ) {
            if best.map(|(_, c)| count > c).unwrap_or(true) {
                best = Some((name, count));
            }
        }
    }
    best.map(|(n, _)| n.to_string())
}

fn mb_artist_credit(v: &Value) -> Option<String> {
    v.get("artist-credit")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|a| a.get("name").and_then(|n| n.as_str()))
        .map(|s| s.to_string())
}

fn year_of(date: Option<&str>) -> Option<String> {
    date.filter(|d| d.len() >= 4).map(|d| d[..4].to_string())
}

pub fn parse_musicbrainz_artists(text: &str) -> Vec<Suggestion> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(list) = v.get("artists").and_then(|a| a.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for a in list {
        let (Some(name), Some(id)) = (
            a.get("name").and_then(|n| n.as_str()),
            a.get("id").and_then(|i| i.as_str()),
        ) else {
            continue;
        };
        let mut s = Suggestion::new("musicbrainz", id, name);
        if let Some(c) = a.get("country").and_then(|c| c.as_str()) {
            s.put("country", c);
            s.detail = c.to_string();
        }
        if let Some(tag) = mb_top_tag(a) {
            s.put("genre", tag);
            if s.detail.is_empty() {
                s.detail = s.fields["genre"].clone();
            } else {
                s.detail = format!("{} · {}", s.detail, s.fields["genre"]);
            }
        }
        // Artist photos are best-effort (spec §2): MusicBrainz carries none,
        // so no thumb/cover here — the UI offers upload after adding.
        out.push(s);
    }
    out
}

pub fn parse_musicbrainz_albums(text: &str) -> Vec<Suggestion> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(list) = v.get("release-groups").and_then(|a| a.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for g in list {
        let (Some(title), Some(id)) = (
            g.get("title").and_then(|t| t.as_str()),
            g.get("id").and_then(|i| i.as_str()),
        ) else {
            continue;
        };
        let mut s = Suggestion::new("musicbrainz", id, title);
        if let Some(artist) = mb_artist_credit(g) {
            s.put("artist", artist.clone());
            s.detail = artist;
        }
        if let Some(y) = year_of(g.get("first-release-date").and_then(|d| d.as_str())) {
            s.year = Some(y.clone());
            s.put("year", y);
        }
        if let Some(tag) = mb_top_tag(g) {
            s.put("genre", tag);
        }
        s.thumb_url = Some(format!("{CAA}/release-group/{id}/front-250"));
        s.cover_url = Some(format!("{CAA}/release-group/{id}/front-500"));
        out.push(s);
    }
    out
}

pub fn parse_musicbrainz_songs(text: &str) -> Vec<Suggestion> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(list) = v.get("recordings").and_then(|a| a.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in list {
        let (Some(title), Some(id)) = (
            r.get("title").and_then(|t| t.as_str()),
            r.get("id").and_then(|i| i.as_str()),
        ) else {
            continue;
        };
        let mut s = Suggestion::new("musicbrainz", id, title);
        if let Some(artist) = mb_artist_credit(r) {
            s.put("artist", artist.clone());
            s.detail = artist;
        }
        if let Some(tag) = mb_top_tag(r) {
            s.put("genre", tag);
        }
        if let Some(release) = r
            .get("releases")
            .and_then(|x| x.as_array())
            .and_then(|x| x.first())
        {
            if let Some(album) = release.get("title").and_then(|t| t.as_str()) {
                s.put("album", album);
                if !s.detail.is_empty() {
                    s.detail = format!("{} · {}", s.detail, album);
                }
            }
            if let Some(y) = year_of(release.get("date").and_then(|d| d.as_str())) {
                s.year = Some(y.clone());
                s.put("year", y);
            }
            if let Some(rid) = release.get("id").and_then(|i| i.as_str()) {
                s.thumb_url = Some(format!("{CAA}/release/{rid}/front-250"));
                s.cover_url = Some(format!("{CAA}/release/{rid}/front-500"));
            }
        }
        out.push(s);
    }
    out
}

// ---------------------------------------------------------------------------
// IGDB (games) via Twitch credentials. No key in code: the caller passes the
// user's client id + secret from settings; we exchange them for a token.
// ---------------------------------------------------------------------------

/// Parse the Twitch OAuth token response (`{"access_token": "…", …}`).
pub fn parse_twitch_token(text: &str) -> Option<String> {
    serde_json::from_str::<Value>(text)
        .ok()?
        .get("access_token")?
        .as_str()
        .map(|s| s.to_string())
}

pub fn parse_igdb_games(text: &str) -> Vec<Suggestion> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(list) = v.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for g in list {
        let (Some(name), Some(id)) = (
            g.get("name").and_then(|n| n.as_str()),
            g.get("id").and_then(|i| i.as_i64()),
        ) else {
            continue;
        };
        let mut s = Suggestion::new("igdb", id.to_string(), name);
        if let Some(ts) = g.get("first_release_date").and_then(|t| t.as_i64()) {
            let y = crate::model::iso_date(ts)[..4].to_string();
            s.year = Some(y.clone());
            s.put("year", y);
        }
        if let Some(gs) = g.get("genres").and_then(|x| x.as_array()) {
            let names: Vec<&str> = gs
                .iter()
                .filter_map(|x| x.get("name").and_then(|n| n.as_str()))
                .collect();
            if !names.is_empty() {
                s.put("genre", names.join(", "));
            }
        }
        let mut devs: Vec<&str> = Vec::new();
        let mut pubs: Vec<&str> = Vec::new();
        if let Some(companies) = g.get("involved_companies").and_then(|x| x.as_array()) {
            for c in companies {
                if let Some(n) = c
                    .get("company")
                    .and_then(|co| co.get("name"))
                    .and_then(|n| n.as_str())
                {
                    if c.get("developer").and_then(|d| d.as_bool()) == Some(true) {
                        devs.push(n);
                    } else if c.get("publisher").and_then(|d| d.as_bool()) == Some(true) {
                        pubs.push(n);
                    }
                }
            }
        }
        if devs.is_empty() {
            devs = pubs;
        }
        if !devs.is_empty() {
            s.put("developer", devs.join(", "));
        }
        if let Some(ps) = g.get("platforms").and_then(|x| x.as_array()) {
            let names: Vec<&str> = ps
                .iter()
                .filter_map(|x| x.get("name").and_then(|n| n.as_str()))
                .collect();
            if !names.is_empty() {
                s.put("platforms", names.join(", "));
                s.detail = names.join(", ");
            }
        }
        if s.detail.is_empty() {
            s.detail = s.fields.get("developer").cloned().unwrap_or_default();
        }
        if let Some(img) = g
            .get("cover")
            .and_then(|c| c.get("image_id"))
            .and_then(|i| i.as_str())
        {
            s.thumb_url = Some(format!(
                "https://images.igdb.com/igdb/image/upload/t_thumb/{img}.jpg"
            ));
            s.cover_url = Some(format!(
                "https://images.igdb.com/igdb/image/upload/t_cover_big/{img}.jpg"
            ));
        }
        out.push(s);
    }
    out
}

// ---------------------------------------------------------------------------
// iTunes Search API (podcasts). No key.
// ---------------------------------------------------------------------------

pub fn parse_itunes_podcasts(text: &str) -> Vec<Suggestion> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(list) = v.get("results").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for p in list {
        let (Some(name), Some(id)) = (
            p.get("collectionName").and_then(|n| n.as_str()),
            p.get("collectionId").and_then(|i| i.as_i64()),
        ) else {
            continue;
        };
        let mut s = Suggestion::new("itunes", id.to_string(), name);
        if let Some(host) = p.get("artistName").and_then(|a| a.as_str()) {
            s.put("host", host);
            s.detail = host.to_string();
        }
        if let Some(genre) = p.get("primaryGenreName").and_then(|g| g.as_str()) {
            s.put("category", genre);
            if !s.detail.is_empty() {
                s.detail = format!("{} · {}", s.detail, genre);
            }
        }
        if let Some(art) = p
            .get("artworkUrl600")
            .and_then(|a| a.as_str())
            .or_else(|| p.get("artworkUrl100").and_then(|a| a.as_str()))
        {
            s.thumb_url = Some(art.to_string());
            s.cover_url = Some(art.to_string());
        }
        out.push(s);
    }
    out
}

// ---------------------------------------------------------------------------
// BoardGameGeek XML API (board games). No key. BGG's XML is regular enough
// that targeted scanning beats pulling in an XML crate for two endpoints.
// ---------------------------------------------------------------------------

fn xml_attr(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(xml_unescape(&tag[start..end]))
}

fn xml_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Each `<item …> … </item>` block of a BGG response, as raw text slices.
fn xml_items(xml: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<item ") {
        let after = &rest[start..];
        match after.find("</item>") {
            Some(end) => {
                out.push(&after[..end + 7]);
                rest = &after[end + 7..];
            }
            None => break,
        }
    }
    out
}

fn xml_tag_value(block: &str, tag: &str) -> Option<String> {
    // <tag … value="X" /> or <tag>text</tag>
    let open = format!("<{tag}");
    let idx = block.find(&open)?;
    let tail = &block[idx..];
    let gt = tail.find('>')?;
    if tail[..=gt].ends_with("/>") || !tail[..gt].contains("value=") {
        if let Some(v) = xml_attr(&tail[..gt], "value") {
            return Some(v);
        }
    }
    if tail[..gt].contains("value=") {
        return xml_attr(&tail[..gt], "value");
    }
    let text_start = idx + gt + 1;
    let close = format!("</{tag}>");
    let end = block[text_start..].find(&close)? + text_start;
    Some(xml_unescape(block[text_start..end].trim()))
}

pub fn parse_bgg_search(text: &str) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for block in xml_items(text) {
        let head_end = block.find('>').unwrap_or(block.len());
        let head = &block[..head_end];
        let Some(id) = xml_attr(head, "id") else {
            continue;
        };
        let Some(name) = xml_tag_value(block, "name") else {
            continue;
        };
        let mut s = Suggestion::new("bgg", id, name);
        if let Some(y) = xml_tag_value(block, "yearpublished") {
            s.year = Some(y.clone());
            s.put("year", y);
        }
        out.push(s);
    }
    out
}

/// Details from a BGG `thing` response: year, publisher (first publisher
/// link), image.
pub fn parse_bgg_thing(text: &str) -> (BTreeMap<String, String>, Option<String>) {
    let mut fields = BTreeMap::new();
    let mut image = None;
    // Single-thing responses: only the first item matters.
    if let Some(block) = xml_items(text).into_iter().next() {
        if let Some(y) = xml_tag_value(block, "yearpublished") {
            fields.insert("year".to_string(), y);
        }
        if let Some(img) = xml_tag_value(block, "image") {
            image = Some(img);
        }
        // First boardgamepublisher link wins.
        let mut rest = block;
        while let Some(idx) = rest.find("<link ") {
            let tail = &rest[idx..];
            let gt = tail.find('>').unwrap_or(tail.len());
            let tag = &tail[..gt];
            if xml_attr(tag, "type").as_deref() == Some("boardgamepublisher") {
                if let Some(pub_name) = xml_attr(tag, "value") {
                    fields.insert("publisher".to_string(), pub_name);
                }
                break;
            }
            rest = &tail[gt..];
        }
    }
    (fields, image)
}

// ---------------------------------------------------------------------------
// Network layer (thin wrappers; parsers above do the real work).
// ---------------------------------------------------------------------------

fn http_get(url: &str) -> Result<String, String> {
    let resp = ureq::get(url)
        .set("User-Agent", crate::USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    resp.into_string()
        .map_err(|e| format!("request failed: {e}"))
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url)
        .set("User-Agent", crate::USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let mut buf = Vec::new();
    use std::io::Read;
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| format!("download failed: {e}"))?;
    Ok(buf)
}

/// Download a cover image's bytes (used when an item is added, spec §4).
pub fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    http_get_bytes(url)
}

fn enc(q: &str) -> String {
    // Minimal percent-encoding for query values.
    let mut out = String::with_capacity(q.len());
    for b in q.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Which categories can search right now, and if not, why not (shown in the
/// UI as a hint above the search box).
pub fn availability(category: Category, settings: &crate::model::Settings) -> Result<(), String> {
    match category {
        Category::Movies | Category::TvShows if !settings.has_tmdb() => Err(
            "Add your free TMDB API key in Settings to search movies and TV shows — or use Add manually below."
                .to_string(),
        ),
        Category::VideoGames if !settings.has_igdb() => Err(
            "Add your Twitch Client ID and Client Secret in Settings to search video games — or use Add manually below."
                .to_string(),
        ),
        _ => Ok(()),
    }
}

/// Search the provider for a category. Drinks categories are handled locally
/// by the GUI (bundled DB), never here.
pub fn search(
    category: Category,
    query: &str,
    settings: &crate::model::Settings,
) -> Result<Vec<Suggestion>, String> {
    let results = match category {
        Category::Movies | Category::TvShows => {
            let key = settings.tmdb_api_key.trim();
            let kind = if category == Category::Movies {
                "movie"
            } else {
                "tv"
            };
            let genres_text = http_get(&format!(
                "{TMDB_BASE}/genre/{kind}/list?api_key={key}&language=en-US"
            ))?;
            let genres = parse_tmdb_genres(&genres_text);
            let text = http_get(&format!(
                "{TMDB_BASE}/search/{kind}?api_key={key}&language=en-US&query={}&page=1",
                enc(query)
            ))?;
            if category == Category::Movies {
                parse_tmdb_movies(&text, &genres)
            } else {
                parse_tmdb_tv(&text, &genres)
            }
        }
        Category::Books => {
            let text = http_get(&format!(
                "https://openlibrary.org/search.json?q={}&limit=20&fields=title,author_name,first_publish_year,cover_i,key,subject,number_of_pages_median",
                enc(query)
            ))?;
            parse_openlibrary(&text)
        }
        Category::Artists | Category::Albums | Category::Songs => {
            // (endpoint, Lucene field for the name, parser)
            type MbParser = fn(&str) -> Vec<Suggestion>;
            let (entity, field, parser): (&str, &str, MbParser) = match category {
                Category::Artists => ("artist", "artist", parse_musicbrainz_artists),
                Category::Albums => ("release-group", "releasegroup", parse_musicbrainz_albums),
                _ => ("recording", "recording", parse_musicbrainz_songs),
            };
            let text = http_get(&format!(
                "https://musicbrainz.org/ws/2/{entity}/?query={}&fmt=json&limit=20",
                enc(&format!("{field}:\"{query}\""))
            ))?;
            parser(&text)
        }
        Category::VideoGames => {
            let token = igdb_token(settings)?;
            let body = format!(
                "search \"{}\"; fields name,first_release_date,genres.name,platforms.name,cover.image_id,involved_companies.company.name,involved_companies.developer,involved_companies.publisher; where version_parent = null; limit 20;",
                query.replace('"', "")
            );
            let resp = ureq::post("https://api.igdb.com/v4/games")
                .set("User-Agent", crate::USER_AGENT)
                .set("Client-ID", settings.igdb_client_id.trim())
                .set("Authorization", &format!("Bearer {token}"))
                .timeout(std::time::Duration::from_secs(20))
                .send_string(&body)
                .map_err(|e| format!("IGDB request failed: {e}"))?;
            let text = resp
                .into_string()
                .map_err(|e| format!("IGDB request failed: {e}"))?;
            parse_igdb_games(&text)
        }
        Category::Podcasts => {
            let text = http_get(&format!(
                "https://itunes.apple.com/search?media=podcast&entity=podcast&limit=20&term={}",
                enc(query)
            ))?;
            parse_itunes_podcasts(&text)
        }
        Category::BoardGames => {
            let text = http_get(&format!(
                "https://boardgamegeek.com/xmlapi2/search?type=boardgame&query={}",
                enc(query)
            ))?;
            parse_bgg_search(&text)
        }
        Category::EnergyDrinks | Category::Sodas => {
            return Err("drinks are searched in the bundled offline database".to_string());
        }
    };
    Ok(cap(order_results(query, results)))
}

/// Exchange the user's Twitch credentials for an IGDB access token.
pub fn igdb_token(settings: &crate::model::Settings) -> Result<String, String> {
    let resp = ureq::post("https://id.twitch.tv/oauth2/token")
        .timeout(std::time::Duration::from_secs(20))
        .send_form(&[
            ("client_id", settings.igdb_client_id.trim()),
            ("client_secret", settings.igdb_client_secret.trim()),
            ("grant_type", "client_credentials"),
        ])
        .map_err(|e| format!("Twitch token request failed: {e}"))?;
    let text = resp
        .into_string()
        .map_err(|e| format!("Twitch token request failed: {e}"))?;
    parse_twitch_token(&text).ok_or_else(|| "Twitch did not return a token".to_string())
}

/// Second-stage details for providers whose search response lacks some
/// fields (TMDB director/cast, BGG publisher). Merged over the suggestion's
/// fields by the caller.
pub fn fetch_details(
    category: Category,
    suggestion: &Suggestion,
    settings: &crate::model::Settings,
) -> BTreeMap<String, String> {
    match (category, suggestion.source) {
        (Category::Movies, "tmdb") | (Category::TvShows, "tmdb") => {
            let kind = if category == Category::Movies {
                "movie"
            } else {
                "tv"
            };
            let url = format!(
                "{TMDB_BASE}/{kind}/{}?api_key={}&language=en-US&append_to_response=credits",
                suggestion.provider_id,
                settings.tmdb_api_key.trim()
            );
            match http_get(&url) {
                Ok(text) => parse_tmdb_details(&text, category == Category::Movies),
                Err(_) => BTreeMap::new(),
            }
        }
        (Category::BoardGames, "bgg") => {
            let url = format!(
                "https://boardgamegeek.com/xmlapi2/thing?id={}&type=boardgame",
                suggestion.provider_id
            );
            match http_get(&url) {
                Ok(text) => parse_bgg_thing(&text).0,
                Err(_) => BTreeMap::new(),
            }
        }
        _ => BTreeMap::new(),
    }
}

/// Cover URL refinement after a details fetch (BGG's search has no image;
/// its thing response does).
pub fn details_cover_url(
    category: Category,
    suggestion: &Suggestion,
    _settings: &crate::model::Settings,
) -> Option<String> {
    if category == Category::BoardGames && suggestion.source == "bgg" {
        let url = format!(
            "https://boardgamegeek.com/xmlapi2/thing?id={}&type=boardgame",
            suggestion.provider_id
        );
        if let Ok(text) = http_get(&url) {
            return parse_bgg_thing(&text).1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap()
    }

    #[test]
    fn order_results_starts_with_first() {
        let mk = |t: &str| Suggestion::new("x", t, t);
        let out = order_results(
            "alien",
            vec![
                mk("The Alien Factor"),
                mk("Aliens"),
                mk("Alien"),
                mk("Predator"),
            ],
        );
        assert_eq!(out[0].title, "Aliens");
        assert_eq!(out[1].title, "Alien");
        assert_eq!(out[2].title, "The Alien Factor");
        assert_eq!(out[3].title, "Predator");
    }

    #[test]
    fn tmdb_movie_parse() {
        let genres = parse_tmdb_genres(&fixture("tmdb_genres.json"));
        let out = parse_tmdb_movies(&fixture("tmdb_movie.json"), &genres);
        assert_eq!(out.len(), 3);
        let alien = &out[0];
        assert_eq!(alien.title, "Alien");
        assert_eq!(alien.provider_id, "348");
        assert_eq!(alien.year.as_deref(), Some("1979"));
        assert_eq!(alien.fields["genre"], "Horror, Science Fiction");
        assert!(alien.cover_url.as_ref().unwrap().contains("w500"));
        assert!(out[2].cover_url.is_none()); // null poster_path
    }

    #[test]
    fn tmdb_details_movie_director_runtime_cast() {
        let f = parse_tmdb_details(&fixture("tmdb_movie_details.json"), true);
        assert_eq!(f["director"], "Ridley Scott");
        assert_eq!(f["runtime"], "117 min");
        assert!(f["cast"].starts_with("Sigourney Weaver"));
        assert_eq!(f["cast"].matches(',').count(), 4); // top 5 cast
    }

    #[test]
    fn tmdb_details_tv_creator_seasons_years() {
        let f = parse_tmdb_details(&fixture("tmdb_tv_details.json"), false);
        assert_eq!(f["creator"], "Joss Whedon");
        assert_eq!(f["seasons"], "1");
        assert_eq!(f["years"], "2002\u{2013}2003");
        assert!(f["cast"].contains("Nathan Fillion"));
    }

    #[test]
    fn openlibrary_parse() {
        let out = parse_openlibrary(&fixture("openlibrary.json"));
        assert_eq!(out.len(), 2);
        let b = &out[0];
        assert_eq!(b.fields["author"], "Ursula K. Le Guin");
        assert_eq!(b.fields["year"], "1969");
        assert_eq!(b.fields["pages"], "304");
        assert!(b.cover_url.as_ref().unwrap().contains("9255566-L"));
        assert_eq!(b.detail, "Ursula K. Le Guin");
    }

    #[test]
    fn musicbrainz_parses() {
        let artists = parse_musicbrainz_artists(&fixture("musicbrainz_artists.json"));
        assert_eq!(artists[0].fields["country"], "GB");
        assert_eq!(artists[0].fields["genre"], "alternative rock");
        assert!(artists[0].cover_url.is_none()); // best-effort: none from MB

        let albums = parse_musicbrainz_albums(&fixture("musicbrainz_albums.json"));
        assert_eq!(albums[0].fields["artist"], "Radiohead");
        assert_eq!(albums[0].fields["year"], "1997");
        assert!(albums[0]
            .cover_url
            .as_ref()
            .unwrap()
            .contains("coverartarchive.org"));

        let songs = parse_musicbrainz_songs(&fixture("musicbrainz_songs.json"));
        assert_eq!(songs[0].fields["album"], "OK Computer");
        assert_eq!(songs[0].fields["artist"], "Radiohead");
        assert!(songs[0].cover_url.is_some());
    }

    #[test]
    fn igdb_parse_and_token() {
        let out = parse_igdb_games(&fixture("igdb_games.json"));
        assert_eq!(out.len(), 2);
        let g = &out[0];
        assert_eq!(g.fields["year"], "2015");
        assert_eq!(g.fields["developer"], "CD Projekt RED");
        assert!(g.fields["platforms"].contains("PlayStation 4"));
        assert!(g.cover_url.as_ref().unwrap().contains("t_cover_big"));
        assert!(out[1].cover_url.is_none());
        assert_eq!(
            parse_twitch_token("{\"access_token\":\"abc123\",\"expires_in\":1}").as_deref(),
            Some("abc123")
        );
        assert_eq!(parse_twitch_token("{}"), None);
    }

    #[test]
    fn itunes_parse() {
        let out = parse_itunes_podcasts(&fixture("itunes_podcasts.json"));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].fields["host"], "Goalhanger");
        assert_eq!(out[0].fields["category"], "History");
        assert!(out[0].cover_url.as_ref().unwrap().contains("artwork"));
    }

    #[test]
    fn bgg_parse_search_and_thing() {
        let out = parse_bgg_search(&fixture("bgg_search.xml"));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Gloomhaven");
        assert_eq!(out[0].fields["year"], "2017");
        let (fields, image) = parse_bgg_thing(&fixture("bgg_thing.xml"));
        assert_eq!(fields["publisher"], "Cephalofair Games");
        assert_eq!(fields["year"], "2017");
        assert_eq!(
            image.as_deref(),
            Some("https://cf.geekdo-images.com/full.jpg")
        );
    }

    #[test]
    fn availability_gates() {
        let s = crate::model::Settings::default();
        assert!(availability(Category::Movies, &s).is_err());
        assert!(availability(Category::Books, &s).is_ok());
        assert!(availability(Category::VideoGames, &s).is_err());
        let s2 = crate::model::Settings {
            tmdb_api_key: "k".into(),
            ..Default::default()
        };
        assert!(availability(Category::Movies, &s2).is_ok());
    }

    #[test]
    fn parsers_tolerate_garbage() {
        assert!(parse_tmdb_movies("not json", &BTreeMap::new()).is_empty());
        assert!(parse_openlibrary("{}").is_empty());
        assert!(parse_bgg_search("<html>").is_empty());
        assert!(parse_igdb_games("[]").is_empty());
    }
}
