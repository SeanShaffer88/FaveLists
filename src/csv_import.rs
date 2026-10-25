//! CSV import (v0.2 program #10): bring an exported ratings/diary/library
//! CSV from another tracker into FaveLists as new lists, or merge it into
//! an existing list. Imports are MERGE-ONLY, exactly like backup import: a
//! work already in the destination list (the model's `contains_work`
//! rule) is skipped and counted, never overwritten, and the whole import
//! runs as one registry dispatch so it is a single undo step.
//!
//! Three export shapes are recognised **by header signature** (header
//! names matched case-insensitively, trimmed):
//!
//! * **Diary/ratings export** (`Letterboxd` style): `Name`, `Year`,
//!   optional `Rating` (0.5–5 in half steps), optional `Letterboxd URI`.
//!   Category: Movies.
//! * **Library export** (`Goodreads` style): `Title`, `Author`,
//!   `My Rating` (0–5), `Year Published`, `Bookshelves`. Category: Books;
//!   the author lands in the `author` field.
//! * **Ratings export** (`IMDb` style): `Const`, `Title`, `Year`,
//!   `Your Rating` (1–10), `Title Type`, `Genres`, `Directors`. Category
//!   comes from `Title Type` per row (movies vs TV shows), so one file
//!   can feed two lists.
//!
//! Ratings land on the v0.2 #5 personal rating: the 5-point scales are
//! doubled and rounded to the nearest 1–10 integer; a 0 or empty rating
//! means unrated (`None`). Columns a row does not carry are left out —
//! values are never fabricated — and detail fields are only stored under
//! keys the destination category actually defines.

use crate::model::{Category, Item, Library};
use std::collections::{BTreeMap, HashMap};

// ---------------------------------------------------------------------------
// CSV reader (RFC 4180)
// ---------------------------------------------------------------------------

/// Parse CSV text into rows of fields.
///
/// Handles the RFC 4180 essentials: quoted fields may contain commas,
/// line breaks, and `""` as an escaped quote; record separators may be
/// CRLF, LF, or a lone CR; a leading UTF-8 BOM is stripped. Completely
/// empty lines produce no row. An unterminated quoted field is the one
/// hard parse error.
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    // True once the current record has seen any content (a character, a
    // quote, or a comma) — lets blank lines and the final newline pass
    // without producing phantom rows.
    let mut record_open = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                in_quotes = true;
                record_open = true;
            }
            ',' => {
                record_open = true;
                row.push(std::mem::take(&mut field));
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if record_open {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    record_open = false;
                }
            }
            _ => {
                record_open = true;
                field.push(c);
            }
        }
    }
    if in_quotes {
        return Err("unterminated quoted field".to_string());
    }
    if record_open {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Format detection + row mapping
// ---------------------------------------------------------------------------

/// One of the recognised CSV export shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvFormat {
    /// Diary/ratings export: `Name`, `Year`, optional `Rating` (0.5–5),
    /// optional `Letterboxd URI`. Single category: Movies.
    LetterboxdDiary,
    /// Library export: `Title`, `Author`, `My Rating` (0–5),
    /// `Year Published`, `Bookshelves`. Single category: Books.
    GoodreadsLibrary,
    /// Ratings export: `Const`, `Title`, `Year`, `Your Rating` (1–10),
    /// `Title Type`, `Genres`, `Directors`. Category per row.
    ImdbRatings,
}

impl CsvFormat {
    /// Stable machine id used in command results.
    pub fn id(self) -> &'static str {
        match self {
            CsvFormat::LetterboxdDiary => "letterboxd",
            CsvFormat::GoodreadsLibrary => "goodreads",
            CsvFormat::ImdbRatings => "imdb",
        }
    }

    /// Functional label for the GUI's detection readout.
    pub fn label(self) -> &'static str {
        match self {
            CsvFormat::LetterboxdDiary => "Diary / ratings export (Letterboxd-style)",
            CsvFormat::GoodreadsLibrary => "Library export (Goodreads-style)",
            CsvFormat::ImdbRatings => "Ratings export (IMDb-style)",
        }
    }

    /// The destination category for single-category formats; `None` for
    /// the ratings export, whose rows map to a category individually.
    pub fn category(self) -> Option<Category> {
        match self {
            CsvFormat::LetterboxdDiary => Some(Category::Movies),
            CsvFormat::GoodreadsLibrary => Some(Category::Books),
            CsvFormat::ImdbRatings => None,
        }
    }
}

/// Header lookup: normalised (trimmed, lower-cased) name → column index.
pub struct HeaderIndex {
    map: HashMap<String, usize>,
}

impl HeaderIndex {
    fn from(headers: &[String]) -> HeaderIndex {
        let mut map = HashMap::new();
        for (i, h) in headers.iter().enumerate() {
            map.entry(normalise_header(h)).or_insert(i);
        }
        HeaderIndex { map }
    }

    fn has(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// The trimmed cell value for a column, or "" when the column is
    /// absent or the row is short.
    fn get(&self, row: &[String], name: &str) -> String {
        self.map
            .get(name)
            .and_then(|&i| row.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }
}

fn normalise_header(h: &str) -> String {
    h.trim().to_lowercase()
}

/// Recognise the export shape from its header row. The error lists the
/// headers that were found, so a wrong file is self-explanatory.
pub fn detect_format(headers: &[String]) -> Result<CsvFormat, String> {
    let idx = HeaderIndex::from(headers);
    // The ratings export's `Const` + `Title Type` pair is unmistakable.
    if idx.has("const") && idx.has("title type") {
        return Ok(CsvFormat::ImdbRatings);
    }
    if idx.has("title") && idx.has("author") {
        return Ok(CsvFormat::GoodreadsLibrary);
    }
    if idx.has("name") && idx.has("year") {
        return Ok(CsvFormat::LetterboxdDiary);
    }
    let found: Vec<String> = headers
        .iter()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .collect();
    Err(format!(
        "unrecognised CSV export format (headers found: {})",
        found.join(", ")
    ))
}

/// One data row mapped onto FaveLists concepts. `title` may be empty —
/// the importer counts those rows as invalid. `category` is `None` only
/// for a ratings-export row whose `Title Type` maps nowhere.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedRow {
    pub title: String,
    pub category: Option<Category>,
    pub fields: BTreeMap<String, String>,
    pub rating: Option<u8>,
    pub provider_id: Option<String>,
    /// Provider tag stored on imported items ("letterboxd" / "goodreads"
    /// / "imdb"), mirroring how auto-fill items record their origin.
    pub source: &'static str,
}

/// Store `value` under `key` — but only when the destination category
/// actually defines that field and the value is non-empty. Fields a
/// category doesn't have are dropped, never smuggled in under a
/// look-alike key.
fn put_field(fields: &mut BTreeMap<String, String>, category: Category, key: &str, value: &str) {
    let value = value.trim();
    if !value.is_empty() && category.fields().iter().any(|f| f.key == key) {
        fields.insert(key.to_string(), value.to_string());
    }
}

/// Store a year value under the category's own year key ("year", or
/// "years" for TV shows).
fn put_year(fields: &mut BTreeMap<String, String>, category: Category, value: &str) {
    if let Some(key) = category.year_key() {
        put_field(fields, category, key, value);
    }
}

/// 5-point scale → the 1–10 personal rating: doubled, rounded to the
/// nearest integer. 0, empty, or unparseable means unrated.
fn rating_from_5(text: &str) -> Option<u8> {
    let v: f64 = text.trim().parse().ok()?;
    if v <= 0.0 {
        None
    } else {
        Some(((v * 2.0).round() as i64).clamp(1, 10) as u8)
    }
}

/// 1–10 scale, taken as-is. 0, empty, or unparseable means unrated.
fn rating_from_10(text: &str) -> Option<u8> {
    let v: f64 = text.trim().parse().ok()?;
    if v <= 0.0 {
        None
    } else {
        Some((v.round() as i64).clamp(1, 10) as u8)
    }
}

/// Map an IMDb-style `Title Type` value to a category. Types that are
/// not listable works here (video games, podcasts, …) map to `None` and
/// the importer skips the row.
fn imdb_category(title_type: &str) -> Option<Category> {
    match title_type.trim().to_lowercase().as_str() {
        "movie" | "short" | "tvmovie" | "video" => Some(Category::Movies),
        "tvseries" | "tvminiseries" | "tvshort" | "tvspecial" | "tvepisode" => {
            Some(Category::TvShows)
        }
        _ => None,
    }
}

/// Map one data row of a detected format.
pub fn map_row(format: CsvFormat, idx: &HeaderIndex, row: &[String]) -> MappedRow {
    match format {
        CsvFormat::LetterboxdDiary => {
            let category = Category::Movies;
            let mut fields = BTreeMap::new();
            put_year(&mut fields, category, &idx.get(row, "year"));
            MappedRow {
                title: idx.get(row, "name"),
                category: Some(category),
                fields,
                rating: rating_from_5(&idx.get(row, "rating")),
                provider_id: None,
                source: "letterboxd",
            }
        }
        CsvFormat::GoodreadsLibrary => {
            let category = Category::Books;
            let mut fields = BTreeMap::new();
            put_field(&mut fields, category, "author", &idx.get(row, "author"));
            let year = idx.get(row, "year published");
            let year = if year.is_empty() {
                idx.get(row, "original publication year")
            } else {
                year
            };
            put_year(&mut fields, category, &year);
            MappedRow {
                title: idx.get(row, "title"),
                category: Some(category),
                fields,
                rating: rating_from_5(&idx.get(row, "my rating")),
                provider_id: None,
                source: "goodreads",
            }
        }
        CsvFormat::ImdbRatings => {
            let category = imdb_category(&idx.get(row, "title type"));
            let mut fields = BTreeMap::new();
            if let Some(cat) = category {
                put_year(&mut fields, cat, &idx.get(row, "year"));
                put_field(&mut fields, cat, "genre", &idx.get(row, "genres"));
                // The `director` field exists only on Movies; for TV
                // rows the column is ignored rather than guessed into
                // the `creator` field.
                put_field(&mut fields, cat, "director", &idx.get(row, "directors"));
            }
            let const_id = idx.get(row, "const");
            MappedRow {
                title: idx.get(row, "title"),
                category,
                fields,
                rating: rating_from_10(&idx.get(row, "your rating")),
                provider_id: if const_id.is_empty() {
                    None
                } else {
                    Some(const_id)
                },
                source: "imdb",
            }
        }
    }
}

/// Parse CSV text, detect the format, and map every data row. Rows that
/// are entirely blank are dropped entirely (they are not data and are
/// never counted anywhere); rows with content but no title are kept so
/// the importer can count them as invalid.
pub fn parse_and_map(text: &str) -> Result<(CsvFormat, Vec<MappedRow>), String> {
    let rows = parse_csv(text)?;
    let (header, data) = rows
        .split_first()
        .ok_or_else(|| "the file is empty — no header row".to_string())?;
    let format = detect_format(header)?;
    let idx = HeaderIndex::from(header);
    let mapped = data
        .iter()
        .filter(|row| row.iter().any(|f| !f.trim().is_empty()))
        .map(|row| map_row(format, &idx, row))
        .collect();
    Ok((format, mapped))
}

// ---------------------------------------------------------------------------
// Inspection (GUI pre-import readout; no mutation)
// ---------------------------------------------------------------------------

/// What the GUI shows after picking a file: the detected format, how
/// many usable rows it carries, and where they would land.
#[derive(Debug, Clone, PartialEq)]
pub struct CsvInspection {
    pub format: CsvFormat,
    /// Data rows with a title (the importable ones).
    pub data_rows: usize,
    /// (category, titled-row count) in category order.
    pub categories: Vec<(Category, usize)>,
}

pub fn inspect(text: &str) -> Result<CsvInspection, String> {
    let (format, rows) = parse_and_map(text)?;
    let mut data_rows = 0;
    let mut counts: BTreeMap<Category, usize> = BTreeMap::new();
    for row in &rows {
        if row.title.is_empty() {
            continue;
        }
        data_rows += 1;
        if let Some(cat) = row.category {
            *counts.entry(cat).or_insert(0) += 1;
        }
    }
    Ok(CsvInspection {
        format,
        data_rows,
        categories: counts.into_iter().collect(),
    })
}

// ---------------------------------------------------------------------------
// Import engine (merge-only)
// ---------------------------------------------------------------------------

/// Where imported rows should go.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportTarget {
    /// Create new list(s): one list named `name` for a single-category
    /// format (in `category`, which must equal the format's category —
    /// `None` means "the format's category"), or, for the split ratings
    /// format, one list per mapped category named `"<name> — <Category>"`
    /// (`category` is ignored there).
    New {
        name: String,
        category: Option<Category>,
    },
    /// Merge into an existing list. Rows whose mapped category differs
    /// from the list's category are skipped (and counted).
    Existing { list_id: u64 },
}

/// Why an import could not run at all. Row-level problems never fail an
/// import — they are counted in the outcome instead.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportError {
    /// The file could not be parsed or its format not recognised.
    Parse(String),
    /// The target list does not exist.
    ListNotFound(u64),
    /// The target category contradicts the file's format.
    CategoryMismatch(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Parse(m) => write!(f, "{m}"),
            ImportError::ListNotFound(id) => write!(f, "no list with id {id}"),
            ImportError::CategoryMismatch(m) => write!(f, "{m}"),
        }
    }
}

/// One destination list's tally in an import outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportListReport {
    pub list_id: u64,
    pub name: String,
    pub category: Category,
    pub added: usize,
}

/// The full result of an import: per-list tallies plus the skip counts.
/// `skipped` in the command result is `skipped_duplicates +
/// skipped_wrong_category`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportOutcome {
    pub format: Option<CsvFormat>,
    pub added: usize,
    /// Rows skipped because the work is already in the destination list.
    pub skipped_duplicates: usize,
    /// Rows skipped because their mapped category has no destination
    /// (a different category in existing-list mode, or an unmappable
    /// `Title Type`).
    pub skipped_wrong_category: usize,
    /// Rows with content but no title.
    pub invalid: usize,
    pub lists: Vec<ImportListReport>,
}

impl ImportOutcome {
    pub fn skipped(&self) -> usize {
        self.skipped_duplicates + self.skipped_wrong_category
    }
}

/// Import already-mapped rows into `library` per `target`, merge-only.
/// New lists are created lazily, on the first row that actually lands in
/// them, so an import that places nothing creates nothing.
pub fn import_mapped(
    library: &mut Library,
    format: CsvFormat,
    rows: &[MappedRow],
    target: &ImportTarget,
) -> Result<ImportOutcome, ImportError> {
    let mut outcome = ImportOutcome {
        format: Some(format),
        ..Default::default()
    };
    // Destination resolution: `dests` holds indices into outcome.lists.
    // For Existing there is exactly one, created up front (and reported
    // even when nothing lands in it).
    let mut dests: Vec<(Category, usize)> = Vec::new();
    match target {
        ImportTarget::Existing { list_id } => {
            let list = library
                .list(*list_id)
                .ok_or(ImportError::ListNotFound(*list_id))?;
            if let Some(fc) = format.category() {
                if list.category != fc {
                    return Err(ImportError::CategoryMismatch(format!(
                        "this file is a {} export, but the chosen list is {}",
                        fc.name(),
                        list.category.name()
                    )));
                }
            }
            outcome.lists.push(ImportListReport {
                list_id: list.id,
                name: list.name.clone(),
                category: list.category,
                added: 0,
            });
            dests.push((list.category, 0));
        }
        ImportTarget::New { category, .. } => {
            if let Some(fc) = format.category() {
                if let Some(c) = category {
                    if *c != fc {
                        return Err(ImportError::CategoryMismatch(format!(
                            "this file is a {} export; it cannot be imported as {}",
                            fc.name(),
                            c.name()
                        )));
                    }
                }
            }
        }
    }

    for row in rows {
        if row.title.is_empty() {
            outcome.invalid += 1;
            continue;
        }
        let Some(cat) = row.category else {
            outcome.skipped_wrong_category += 1;
            continue;
        };
        // Resolve (creating, for New targets) the destination list.
        let report_idx = match target {
            ImportTarget::Existing { .. } => {
                if dests[0].0 != cat {
                    outcome.skipped_wrong_category += 1;
                    continue;
                }
                0
            }
            ImportTarget::New { name, .. } => {
                if let Some(&(_, idx)) = dests.iter().find(|(c, _)| *c == cat) {
                    idx
                } else {
                    let list_name = match format.category() {
                        Some(_) => name.clone(),
                        None => format!("{name} — {}", cat.name()),
                    };
                    let ranked = false;
                    let id = library.new_list(list_name.clone(), cat, ranked);
                    outcome.lists.push(ImportListReport {
                        list_id: id,
                        name: list_name,
                        category: cat,
                        added: 0,
                    });
                    dests.push((cat, outcome.lists.len() - 1));
                    outcome.lists.len() - 1
                }
            }
        };
        let list_id = outcome.lists[report_idx].list_id;
        let item_id = library.alloc_item_id();
        let mut item = Item::new(item_id, row.title.clone());
        item.fields = row.fields.clone();
        item.rating = row.rating;
        item.provider_id = row.provider_id.clone();
        item.source = row.source.to_string();
        let list = library
            .list_mut(list_id)
            .expect("destination list was just resolved");
        match list.add_item(item) {
            crate::model::AddOutcome::Added => {
                outcome.added += 1;
                outcome.lists[report_idx].added += 1;
            }
            crate::model::AddOutcome::Duplicate => {
                outcome.skipped_duplicates += 1;
            }
        }
    }
    Ok(outcome)
}

/// Parse + map + import in one step — the `import.csv` command's core.
pub fn import_csv_text(
    library: &mut Library,
    text: &str,
    target: &ImportTarget,
) -> Result<ImportOutcome, ImportError> {
    let (format, rows) = parse_and_map(text).map_err(ImportError::Parse)?;
    import_mapped(library, format, &rows, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- reader ---------------------------------------------------------

    #[test]
    fn reader_handles_quotes_commas_and_escapes() {
        let rows = parse_csv("a,b,c\n\"Alien, The\",1979,\"He said \"\"run\"\"\"\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], ["a", "b", "c"]);
        assert_eq!(rows[1], ["Alien, The", "1979", "He said \"run\""]);
    }

    #[test]
    fn reader_handles_crlf_lf_cr_and_quoted_newlines() {
        let rows = parse_csv("a,b\r\n1,2\n3,4\r5,\"x\ny\"\n").unwrap();
        assert_eq!(
            rows,
            [
                vec!["a", "b"],
                vec!["1", "2"],
                vec!["3", "4"],
                vec!["5", "x\ny"],
            ]
        );
    }

    #[test]
    fn reader_ignores_trailing_empty_lines_and_bom() {
        let rows = parse_csv("\u{feff}a,b\n1,2\n\n\n").unwrap();
        assert_eq!(rows, [vec!["a", "b"], vec!["1", "2"]]);
        let rows = parse_csv("a,b\n1,2").unwrap(); // no trailing newline
        assert_eq!(rows.len(), 2);
        assert_eq!(parse_csv("").unwrap().len(), 0);
    }

    #[test]
    fn reader_rejects_unterminated_quote() {
        assert!(parse_csv("a,\"unterminated\n").is_err());
    }

    // -- detection --------------------------------------------------------

    fn headers(text: &str) -> Vec<String> {
        parse_csv(text).unwrap().into_iter().next().unwrap()
    }

    #[test]
    fn detection_by_header_signature() {
        assert_eq!(
            detect_format(&headers(
                "Date,Name,Year,Letterboxd URI,Rating\n2020-01-01,X,2000,https://boxd.it/x,4\n"
            ))
            .unwrap(),
            CsvFormat::LetterboxdDiary
        );
        assert_eq!(
            detect_format(&headers(
                "Book Id,Title,Author,My Rating,Year Published,Bookshelves\n"
            ))
            .unwrap(),
            CsvFormat::GoodreadsLibrary
        );
        assert_eq!(
            detect_format(&headers(
                "Const,Your Rating,Title,Year,Title Type,Genres,Directors\n"
            ))
            .unwrap(),
            CsvFormat::ImdbRatings
        );
        // Case-insensitive + padded headers still detect.
        assert_eq!(
            detect_format(&headers(" name , YEAR \n")).unwrap(),
            CsvFormat::LetterboxdDiary
        );
    }

    #[test]
    fn detection_unknown_header_lists_what_was_found() {
        let err = detect_format(&headers("Foo,Bar\n1,2\n")).unwrap_err();
        assert!(err.contains("unrecognised"), "{err}");
        assert!(err.contains("Foo, Bar"), "{err}");
    }

    // -- mapping ----------------------------------------------------------

    #[test]
    fn letterboxd_mapping_and_rating_steps() {
        let text = "Date,Name,Year,Letterboxd URI,Rating\n\
                    2021-03-01,Dune,2021,https://boxd.it/abc,4.5\n\
                    2021-03-02,Alien,1979,https://boxd.it/def,5\n\
                    2021-03-03,Arrival,2016,https://boxd.it/ghi,0.5\n\
                    2021-03-04,Unrated One,2000,https://boxd.it/jkl,\n";
        let (format, rows) = parse_and_map(text).unwrap();
        assert_eq!(format, CsvFormat::LetterboxdDiary);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].title, "Dune");
        assert_eq!(rows[0].category, Some(Category::Movies));
        assert_eq!(rows[0].fields["year"], "2021");
        assert_eq!(rows[0].rating, Some(9)); // 4.5 * 2
        assert_eq!(rows[1].rating, Some(10)); // 5 * 2
        assert_eq!(rows[2].rating, Some(1)); // 0.5 * 2
        assert_eq!(rows[3].rating, None); // empty = unrated
    }

    #[test]
    fn goodreads_mapping_author_year_rating() {
        let text =
            "Book Id,Title,Author,My Rating,Year Published,Original Publication Year,Bookshelves\n\
                    1,Dune,Frank Herbert,5,1990,1965,owned\n\
                    2,The Left Hand of Darkness,Ursula K. Le Guin,4,,1969,owned\n\
                    3,Shelf Sitter,Some Author,0,2001,2001,to-read\n";
        let (format, rows) = parse_and_map(text).unwrap();
        assert_eq!(format, CsvFormat::GoodreadsLibrary);
        assert_eq!(rows[0].category, Some(Category::Books));
        assert_eq!(rows[0].fields["author"], "Frank Herbert");
        assert_eq!(rows[0].fields["year"], "1990");
        assert_eq!(rows[0].rating, Some(10));
        // Empty Year Published falls back to Original Publication Year.
        assert_eq!(rows[1].fields["year"], "1969");
        assert_eq!(rows[1].rating, Some(8));
        assert_eq!(rows[2].rating, None); // 0 = unrated on that scale
    }

    #[test]
    fn imdb_mapping_categories_fields_and_rating() {
        let text = "Const,Your Rating,Date,Title,URL,Title Type,Year,Genres,Directors\n\
                    tt0111161,10,2020-01-01,The Shawshank Redemption,x,movie,1994,\"Drama\",Frank Darabont\n\
                    tt0903747,9,2020-01-02,Breaking Bad,x,tvSeries,2008,\"Crime, Drama\",Vince Gilligan\n\
                    tt0000001,0,2020-01-03,Unrated Short,x,short,1900,Short,Someone\n";
        let (format, rows) = parse_and_map(text).unwrap();
        assert_eq!(format, CsvFormat::ImdbRatings);
        let movie = &rows[0];
        assert_eq!(movie.category, Some(Category::Movies));
        assert_eq!(movie.fields["year"], "1994");
        assert_eq!(movie.fields["genre"], "Drama");
        assert_eq!(movie.fields["director"], "Frank Darabont");
        assert_eq!(movie.rating, Some(10));
        assert_eq!(movie.provider_id.as_deref(), Some("tt0111161"));
        let show = &rows[1];
        assert_eq!(show.category, Some(Category::TvShows));
        // TV rows file the year under "years", and Directors is NOT
        // guessed into "creator".
        assert_eq!(show.fields["years"], "2008");
        assert_eq!(show.fields["genre"], "Crime, Drama");
        assert!(!show.fields.contains_key("director"));
        assert!(!show.fields.contains_key("creator"));
        assert_eq!(show.rating, Some(9));
        let short = &rows[2];
        assert_eq!(short.category, Some(Category::Movies));
        assert_eq!(short.rating, None); // Your Rating 0 = unrated
    }

    #[test]
    fn rating_scale_edges() {
        assert_eq!(rating_from_5("2.5"), Some(5));
        assert_eq!(rating_from_5("0"), None);
        assert_eq!(rating_from_5(""), None);
        assert_eq!(rating_from_5("junk"), None);
        assert_eq!(rating_from_10("7"), Some(7));
        assert_eq!(rating_from_10("0"), None);
        assert_eq!(rating_from_10(""), None);
    }
}
