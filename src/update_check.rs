//! App update check (v0.3 program #18): on launch the GUI quietly
//! asks the project's releases page whether a newer version exists
//! and, if so, shows "vX available" in the status bar. No download,
//! no nagging: the app is offline-first, so ANY failure (offline,
//! rate limit, unexpected payload) is silent — the check simply
//! finds nothing.
//!
//! Version parsing and comparison are pure and tested; the network
//! call is a thin ureq seam in the providers family's style.

/// The GitHub API endpoint for the newest published release.
pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/SeanShaffer88/FaveLists/releases/latest";
/// Where the status-bar notice links to.
pub const RELEASES_PAGE_URL: &str = "https://github.com/SeanShaffer88/FaveLists/releases";

/// Parse a version into (major, minor, patch): an optional leading
/// `v`, one to three dot-separated numeric parts (missing parts
/// count as 0). Anything else — empty, letters, a fourth part — is
/// junk and returns None.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let t = text.trim();
    let t = t.strip_prefix(['v', 'V']).unwrap_or(t);
    if t.is_empty() {
        return None;
    }
    let mut parts = [0u64; 3];
    for (n, part) in t.split('.').enumerate() {
        if n >= 3 || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        parts[n] = part.parse().ok()?;
    }
    Some((parts[0], parts[1], parts[2]))
}

/// Is `latest` strictly newer than `current`? Unparseable input on
/// either side is never "newer" — a junk tag must not nag.
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// The newest release's version from a latest-release API payload:
/// its `tag_name`, when present and non-empty.
pub fn latest_from_api_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let tag = value.get("tag_name")?.as_str()?.trim();
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_string())
    }
}

/// The network seam: fetch the latest release and return the newer
/// version as a display string ("v0.3.1") when one exists, None
/// when up to date or on ANY failure (offline-first: silent).
pub fn fetch_newer_version() -> Option<String> {
    let resp = ureq::get(LATEST_RELEASE_API_URL)
        .set("User-Agent", crate::USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .ok()?;
    let text = resp.into_string().ok()?;
    let tag = latest_from_api_json(&text)?;
    let latest = parse_version(&tag)?;
    if is_newer(env!("CARGO_PKG_VERSION"), &tag) {
        Some(format!("v{}.{}.{}", latest.0, latest.1, latest.2))
    } else {
        None
    }
}
