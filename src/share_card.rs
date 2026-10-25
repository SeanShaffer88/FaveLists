//! Share-card export (v0.2 program #8): ONE designed PNG per list —
//! 1200×630, family styling (near-black background, red accent) — with
//! the list name as a header and up to 25 entries as rows (rank number,
//! cover thumbnail, title, year). Unlike the cover-art image export
//! (which draws no text, ever), this card is meant to be read, so it
//! rasterises text with `fontdue`.
//!
//! Fonts are loaded AT RUNTIME from OS font locations (Segoe UI, then
//! Arial on Windows; DejaVu Sans / Liberation Sans on Linux) and are
//! never bundled into the repo or the binary. When no system font can be
//! found the export fails with a clean error saying a system font is
//! required — it never panics and never silently produces a textless
//! card.
//!
//! All layout math (row geometry, column split, pixel-width truncation)
//! is pure and unit-tested; measuring sits behind the [`TextMeasure`]
//! trait so the truncation tests need no font at all.

use crate::render;
use fontdue::{Font, FontSettings};
use image::{DynamicImage, Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The card is always exactly this size (the standard social-card
/// canvas).
pub const CARD_W: u32 = 1200;
pub const CARD_H: u32 = 630;
/// Most entries a card will render; the command clamps `count` to this.
pub const MAX_ENTRIES: usize = 25;

const MARGIN: f32 = 48.0;
const HEADER_SIZE: f32 = 40.0;
const HEADER_BASELINE: f32 = 84.0;
const RULE_Y: f32 = 108.0;
const ROWS_TOP: f32 = 124.0;
const ROWS_BOTTOM: f32 = 582.0; // CARD_H - 48
const FOOTER_SIZE: f32 = 15.0;
const FOOTER_BASELINE: f32 = 610.0; // CARD_H - 20
const FOOTER_TEXT: &str = "Made with FaveLists";
/// Entries per card before the rows split into two columns.
const TWO_COLUMN_THRESHOLD: usize = 10;

const COL_TEXT: Rgba<u8> = Rgba([0xF5, 0xF5, 0xF5, 255]);
const COL_ACCENT: Rgba<u8> = Rgba([0xE5, 0x48, 0x4D, 255]);
const COL_MUTED: Rgba<u8> = Rgba([0xA8, 0xA8, 0xA8, 255]);
const COL_HAIRLINE: Rgba<u8> = Rgba([0x24, 0x24, 0x24, 255]);
const COL_ROW_BG: Rgba<u8> = Rgba([0x14, 0x14, 0x14, 255]);

// ---------------------------------------------------------------------------
// Pure layout geometry
// ---------------------------------------------------------------------------

/// Geometry of one entry row. All values are canvas pixels (f32 because
/// row heights divide the zone evenly); drawing rounds at blit time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowGeom {
    /// 1-based rank shown at the row's left.
    pub rank: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Cover thumbnail square (crop-to-fill) or placeholder tile.
    pub thumb_x: f32,
    pub thumb_y: f32,
    pub thumb_size: f32,
    /// Right edge of the rank number (rank text is right-aligned to it).
    pub rank_right: f32,
    /// Left edge of the title/year text.
    pub text_x: f32,
    /// Width available to title + year together.
    pub text_w: f32,
    /// Baseline for the title/year/rank text.
    pub baseline: f32,
    pub title_size: f32,
    pub rank_size: f32,
}

/// The whole card's geometry for `count` entries.
#[derive(Debug, Clone, PartialEq)]
pub struct CardLayout {
    pub header_x: f32,
    pub header_baseline: f32,
    pub header_size: f32,
    pub header_max_w: f32,
    pub rule_y: f32,
    pub footer_baseline: f32,
    pub footer_size: f32,
    pub columns: usize,
    pub rows: Vec<RowGeom>,
}

/// Compute the card layout for `count` entries (clamped to 1–25).
/// Up to 10 entries get one full-width column; beyond that the rows
/// split into two columns so the text stays legible.
pub fn card_layout(count: usize) -> CardLayout {
    let count = count.clamp(1, MAX_ENTRIES);
    let columns = if count > TWO_COLUMN_THRESHOLD { 2 } else { 1 };
    let per_col = count.div_ceil(columns);
    let inner_w = CARD_W as f32 - 2.0 * MARGIN;
    let gutter = 36.0;
    let col_w = (inner_w - gutter * (columns - 1) as f32) / columns as f32;
    let zone_h = ROWS_BOTTOM - ROWS_TOP;
    let row_h = zone_h / per_col as f32;
    let thumb_size = (row_h - 12.0).clamp(18.0, 84.0);
    let title_size = (row_h * 0.44).clamp(14.0, 23.0);
    let mut rows = Vec::with_capacity(count);
    for i in 0..count {
        let col = i / per_col;
        let in_col = i % per_col;
        let x = MARGIN + col as f32 * (col_w + gutter);
        let y = ROWS_TOP + in_col as f32 * row_h;
        let thumb_x = x + 56.0;
        rows.push(RowGeom {
            rank: i + 1,
            x,
            y,
            w: col_w,
            h: row_h,
            thumb_x,
            thumb_y: y + (row_h - thumb_size) / 2.0,
            thumb_size,
            rank_right: x + 44.0,
            text_x: thumb_x + thumb_size + 14.0,
            text_w: (x + col_w) - (thumb_x + thumb_size + 14.0) - 6.0,
            baseline: y + row_h / 2.0 + title_size * 0.35,
            title_size,
            rank_size: title_size,
        });
    }
    CardLayout {
        header_x: MARGIN,
        header_baseline: HEADER_BASELINE,
        header_size: HEADER_SIZE,
        header_max_w: inner_w,
        rule_y: RULE_Y,
        footer_baseline: FOOTER_BASELINE,
        footer_size: FOOTER_SIZE,
        columns,
        rows,
    }
}

// ---------------------------------------------------------------------------
// Text measuring + truncation (font-agnostic via the trait)
// ---------------------------------------------------------------------------

/// Pixel-width measuring for one font/size combination. Production code
/// uses [`FontMeasure`]; tests substitute a fixed-width fake so the
/// truncation logic is proven without any font file.
pub trait TextMeasure {
    fn width(&self, text: &str, px: f32) -> f32;
}

/// [`TextMeasure`] backed by a real font (advance widths summed; the
/// renderer advances the pen by exactly these amounts, so measuring and
/// drawing agree by construction).
pub struct FontMeasure<'a> {
    pub font: &'a Font,
}

impl TextMeasure for FontMeasure<'_> {
    fn width(&self, text: &str, px: f32) -> f32 {
        text.chars()
            .map(|c| self.font.metrics(c, px).advance_width)
            .sum()
    }
}

/// Truncate `text` to fit `max_width` pixels at size `px`, appending an
/// ellipsis when anything was cut. Returns the text unchanged when it
/// fits, and an empty string when even the ellipsis cannot fit.
pub fn truncate_to_width(text: &str, px: f32, max_width: f32, measure: &dyn TextMeasure) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if measure.width(text, px) <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    let ew = measure.width(ellipsis, px);
    if ew > max_width {
        return String::new();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let mut cand = out.clone();
        cand.push(ch);
        if measure.width(&cand, px) + ew > max_width {
            break;
        }
        out = cand;
    }
    out.push_str(ellipsis);
    out
}

// ---------------------------------------------------------------------------
// System font loading (runtime only — fonts are never bundled)
// ---------------------------------------------------------------------------

/// The fonts a card renders with: a regular weight, plus a bold weight
/// for the header when the OS offers one.
pub struct CardFonts {
    pub regular: Font,
    pub bold: Option<Font>,
}

const NO_FONT_MESSAGE: &str = "share card export requires a system font \
    (looked for Segoe UI / Arial on Windows, DejaVu Sans / Liberation Sans \
    on Linux); none was found on this system";

static FONT_CACHE: Mutex<Option<Arc<CardFonts>>> = Mutex::new(None);

/// The process-wide cached card fonts, loaded on first use.
pub fn fonts() -> Result<Arc<CardFonts>, String> {
    let mut guard = FONT_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(cached) = &*guard {
        return Ok(cached.clone());
    }
    let loaded = load_fonts().ok_or_else(|| NO_FONT_MESSAGE.to_string())?;
    let arc = Arc::new(loaded);
    *guard = Some(arc.clone());
    Ok(arc)
}

/// Whether a usable system font exists (the render test uses this to
/// skip cleanly on fontless machines instead of failing).
pub fn system_font_available() -> bool {
    fonts().is_ok()
}

fn load_fonts() -> Option<CardFonts> {
    let regular = load_first(&font_candidates(false))?;
    let bold = load_first(&font_candidates(true));
    Some(CardFonts { regular, bold })
}

fn load_first(paths: &[PathBuf]) -> Option<Font> {
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                return Some(font);
            }
        }
    }
    None
}

fn font_candidates(bold: bool) -> Vec<PathBuf> {
    let names: &[&str] = if bold {
        &[
            r"C:\Windows\Fonts\segoeuib.ttf",
            r"C:\Windows\Fonts\arialbd.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
        ]
    } else {
        &[
            r"C:\Windows\Fonts\segoeui.ttf",
            r"C:\Windows\Fonts\arial.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ]
    };
    let mut out: Vec<PathBuf> = names.iter().map(PathBuf::from).collect();
    // Bounded fallback: search the system font tree for the DejaVu file
    // (distro layouts vary).
    let target = if bold {
        "DejaVuSans-Bold.ttf"
    } else {
        "DejaVuSans.ttf"
    };
    if let Some(found) = find_font_file(Path::new("/usr/share/fonts"), target, 4) {
        out.push(found);
    }
    out
}

/// Bounded recursive search for a file name under `dir`.
fn find_font_file(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in &entries {
        if path.is_file() && path.file_name().is_some_and(|n| n == name) {
            return Some(path.clone());
        }
    }
    if depth > 0 {
        for path in &entries {
            if path.is_dir() {
                if let Some(found) = find_font_file(path, name, depth - 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// One card row's content. `cover` is the decoded cover image, or `None`
/// for the placeholder tile.
pub struct CardEntry<'a> {
    pub title: &'a str,
    pub year: Option<&'a str>,
    pub cover: Option<&'a DynamicImage>,
}

/// A rendered card plus the entry indices (into the rendered order) that
/// had no usable cover and got a placeholder tile.
pub struct RenderedCard {
    pub image: RgbaImage,
    pub missing_indices: Vec<usize>,
}

/// Render the share card for the first `MAX_ENTRIES` of `entries`
/// (callers pass the already-clamped slice). Fails only when no system
/// font is available.
pub fn render_card(list_name: &str, entries: &[CardEntry<'_>]) -> Result<RenderedCard, String> {
    let fonts = fonts()?;
    let measure = FontMeasure {
        font: &fonts.regular,
    };
    let layout = card_layout(entries.len().max(1));
    let mut img = RgbaImage::from_pixel(CARD_W, CARD_H, render::BG);

    // Header: list name (bold when the OS offers one), red accent rule.
    let header_font = fonts.bold.as_ref().unwrap_or(&fonts.regular);
    let header_measure = FontMeasure { font: header_font };
    let header = truncate_to_width(
        list_name,
        layout.header_size,
        layout.header_max_w,
        &header_measure,
    );
    draw_text(
        &mut img,
        header_font,
        &header,
        layout.header_x,
        layout.header_baseline,
        layout.header_size,
        COL_TEXT,
    );
    fill_rect(
        &mut img,
        MARGIN as i64,
        layout.rule_y as i64,
        (CARD_W as f32 - 2.0 * MARGIN) as u32,
        1,
        COL_HAIRLINE,
    );
    fill_rect(
        &mut img,
        MARGIN as i64,
        layout.rule_y as i64 - 1,
        72,
        3,
        COL_ACCENT,
    );

    // Rows.
    let mut missing_indices = Vec::new();
    for (i, geom) in layout.rows.iter().enumerate() {
        let Some(entry) = entries.get(i) else { break };
        fill_rect(
            &mut img,
            geom.x as i64 - 10,
            geom.y as i64 + 2,
            geom.w as u32 + 20,
            geom.h as u32 - 4,
            COL_ROW_BG,
        );
        // Rank, right-aligned, in the accent colour.
        let rank_text = geom.rank.to_string();
        let rank_w = measure.width(&rank_text, geom.rank_size);
        draw_text(
            &mut img,
            &fonts.regular,
            &rank_text,
            geom.rank_right - rank_w,
            geom.baseline,
            geom.rank_size,
            COL_ACCENT,
        );
        // Cover or placeholder.
        let tx = geom.thumb_x.round() as u32;
        let ty = geom.thumb_y.round() as u32;
        let ts = geom.thumb_size.round() as u32;
        match entry.cover {
            Some(cover) => draw_cover(&mut img, cover, tx, ty, ts),
            None => {
                missing_indices.push(i);
                draw_placeholder(&mut img, &fonts.regular, entry.title, tx, ty, ts);
            }
        }
        // Title, then the year (muted) when present. The year reserves
        // its width first so the title truncates before it, not over it.
        let year = entry.year.map(str::trim).filter(|y| !y.is_empty());
        let year_w = year
            .map(|y| measure.width(y, geom.title_size) + 12.0)
            .unwrap_or(0.0);
        let title = truncate_to_width(
            entry.title,
            geom.title_size,
            (geom.text_w - year_w).max(0.0),
            &measure,
        );
        let title_w = draw_text(
            &mut img,
            &fonts.regular,
            &title,
            geom.text_x,
            geom.baseline,
            geom.title_size,
            COL_TEXT,
        );
        if let Some(y) = year {
            draw_text(
                &mut img,
                &fonts.regular,
                y,
                geom.text_x + title_w + 12.0,
                geom.baseline,
                geom.title_size,
                COL_MUTED,
            );
        }
    }

    // Footer.
    draw_text(
        &mut img,
        &fonts.regular,
        FOOTER_TEXT,
        MARGIN,
        layout.footer_baseline,
        layout.footer_size,
        COL_MUTED,
    );

    Ok(RenderedCard {
        image: img,
        missing_indices,
    })
}

/// Draw one line of text with its left edge at `x` and its baseline at
/// `baseline`; returns the pen advance (the text's pixel width).
fn draw_text(
    img: &mut RgbaImage,
    font: &Font,
    text: &str,
    x: f32,
    baseline: f32,
    px: f32,
    color: Rgba<u8>,
) -> f32 {
    let mut pen = x;
    for ch in text.chars() {
        let (metrics, coverage) = font.rasterize(ch, px);
        if metrics.width > 0 && metrics.height > 0 {
            let gx = (pen + metrics.xmin as f32).round() as i64;
            let gy = (baseline - metrics.ymin as f32 - metrics.height as f32).round() as i64;
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let a = coverage[row * metrics.width + col];
                    if a > 0 {
                        blend_pixel(
                            img,
                            gx + col as i64,
                            gy + row as i64,
                            color,
                            a as f32 / 255.0,
                        );
                    }
                }
            }
        }
        pen += metrics.advance_width;
    }
    pen - x
}

/// The missing-cover placeholder: a dark tile with a deep-red border and
/// the title's first letter, centred (the art-only export's placeholder,
/// plus the initial this card is allowed to show).
fn draw_placeholder(img: &mut RgbaImage, font: &Font, title: &str, x: u32, y: u32, size: u32) {
    if size == 0 {
        return;
    }
    for yy in 0..size {
        for xx in 0..size {
            let border = xx < 2 || yy < 2 || xx + 2 >= size || yy + 2 >= size;
            let color = if border {
                render::TILE_BORDER
            } else {
                render::TILE_BG
            };
            put_pixel(img, x as i64 + xx as i64, y as i64 + yy as i64, color);
        }
    }
    let letter = title
        .trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_string());
    let px = size as f32 * 0.52;
    let measure = FontMeasure { font };
    let w = measure.width(&letter, px);
    draw_text(
        img,
        font,
        &letter,
        x as f32 + (size as f32 - w) / 2.0,
        y as f32 + size as f32 / 2.0 + px * 0.35,
        px,
        COL_MUTED,
    );
}

/// Draw a cover image crop-to-fill into the square at (x, y, size).
fn draw_cover(img: &mut RgbaImage, cover: &DynamicImage, x: u32, y: u32, size: u32) {
    if size == 0 {
        return;
    }
    let rgba = cover.to_rgba8();
    let (cw, ch) = (rgba.width(), rgba.height());
    if cw == 0 || ch == 0 {
        return;
    }
    let scale = (size as f32 / cw as f32).max(size as f32 / ch as f32);
    let sw = ((cw as f32 * scale).ceil() as u32).max(size);
    let sh = ((ch as f32 * scale).ceil() as u32).max(size);
    let resized = image::imageops::resize(&rgba, sw, sh, image::imageops::FilterType::Triangle);
    let x0 = (resized.width() - size) / 2;
    let y0 = (resized.height() - size) / 2;
    let tile = image::imageops::crop_imm(&resized, x0, y0, size, size).to_image();
    blit(img, &tile, x as i64, y as i64);
}

/// Alpha-blend `src` over `dst` at the offset (same semantics as the
/// cover-art renderer's blit).
fn blit(dst: &mut RgbaImage, src: &RgbaImage, ox: i64, oy: i64) {
    for y in 0..src.height() as i64 {
        for x in 0..src.width() as i64 {
            let (dx, dy) = (ox + x, oy + y);
            if dx < 0 || dy < 0 || dx >= dst.width() as i64 || dy >= dst.height() as i64 {
                continue;
            }
            let s = src.get_pixel(x as u32, y as u32);
            if s[3] == 0 {
                continue;
            }
            if s[3] == 255 {
                dst.put_pixel(dx as u32, dy as u32, *s);
            } else {
                blend_pixel(dst, dx, dy, *s, s[3] as f32 / 255.0);
            }
        }
    }
}

fn blend_pixel(img: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>, alpha: f32) {
    if x < 0 || y < 0 || x >= img.width() as i64 || y >= img.height() as i64 {
        return;
    }
    let d = img.get_pixel(x as u32, y as u32);
    let mut px = [0u8; 4];
    for i in 0..3 {
        px[i] = (color[i] as f32 * alpha + d[i] as f32 * (1.0 - alpha)) as u8;
    }
    px[3] = 255;
    img.put_pixel(x as u32, y as u32, Rgba(px));
}

fn put_pixel(img: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>) {
    if x >= 0 && y >= 0 && x < img.width() as i64 && y < img.height() as i64 {
        img.put_pixel(x as u32, y as u32, color);
    }
}

fn fill_rect(img: &mut RgbaImage, x: i64, y: i64, w: u32, h: u32, color: Rgba<u8>) {
    for yy in 0..h as i64 {
        for xx in 0..w as i64 {
            put_pixel(img, x + xx, y + yy, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed-width fake measurer: every char is `per_char` pixels wide,
    /// so truncation expectations are exact.
    struct FixedMeasure(f32);
    impl TextMeasure for FixedMeasure {
        fn width(&self, text: &str, _px: f32) -> f32 {
            text.chars().count() as f32 * self.0
        }
    }

    #[test]
    fn layout_single_column_up_to_ten() {
        let l = card_layout(10);
        assert_eq!(l.columns, 1);
        assert_eq!(l.rows.len(), 10);
        // Rows stack monotonically and stay inside the rows zone/canvas.
        for (i, r) in l.rows.iter().enumerate() {
            assert_eq!(r.rank, i + 1);
            assert!(r.y >= ROWS_TOP && r.y + r.h <= ROWS_BOTTOM + 0.01);
            assert!(r.x >= MARGIN && r.x + r.w <= CARD_W as f32 - MARGIN + 0.01);
            if i > 0 {
                assert!(r.y > l.rows[i - 1].y);
            }
        }
        // Thumbnails are square and inside their rows.
        for r in &l.rows {
            assert!(r.thumb_size >= 18.0);
            assert!(r.thumb_y >= r.y && r.thumb_y + r.thumb_size <= r.y + r.h + 0.01);
        }
    }

    #[test]
    fn layout_two_columns_beyond_ten() {
        let l = card_layout(25);
        assert_eq!(l.columns, 2);
        assert_eq!(l.rows.len(), 25);
        // Column 1 holds ranks 1..=13, column 2 ranks 14..=25.
        assert!(l.rows[12].x < l.rows[13].x, "rank 14 starts column 2");
        assert_eq!(l.rows[13].rank, 14);
        for r in &l.rows {
            assert!(r.y + r.h <= ROWS_BOTTOM + 0.01);
            assert!(r.x + r.w <= CARD_W as f32 - MARGIN + 0.01);
            // Text starts after the thumbnail and fits the column.
            assert!(r.text_x > r.thumb_x);
            assert!(r.text_x + r.text_w <= r.x + r.w + 0.01);
        }
    }

    #[test]
    fn layout_clamps_count() {
        assert_eq!(card_layout(0).rows.len(), 1);
        assert_eq!(card_layout(99).rows.len(), MAX_ENTRIES);
        let one = card_layout(1);
        assert_eq!(one.columns, 1);
        assert_eq!(one.rows.len(), 1);
        // A single row gets the largest thumbnail the cap allows.
        assert_eq!(one.rows[0].thumb_size, 84.0);
    }

    #[test]
    fn truncate_fits_and_cuts_with_ellipsis() {
        let m = FixedMeasure(10.0);
        assert_eq!(truncate_to_width("short", 20.0, 100.0, &m), "short");
        // 10 chars * 10px = 100 exactly: fits.
        assert_eq!(
            truncate_to_width("0123456789", 20.0, 100.0, &m),
            "0123456789"
        );
        // 11 chars don't fit: 9 chars + ellipsis (10px) = 100.
        assert_eq!(
            truncate_to_width("0123456789A", 20.0, 100.0, &m),
            "012345678…"
        );
        // No room even for the ellipsis: empty.
        assert_eq!(truncate_to_width("abc", 20.0, 5.0, &m), "");
        assert_eq!(truncate_to_width("abc", 20.0, 0.0, &m), "");
        // Unicode is cut on char boundaries, never mid-char.
        let cut = truncate_to_width("αααααα", 20.0, 35.0, &m);
        assert_eq!(cut, "αα…");
    }

    #[test]
    fn render_skips_or_draws_when_font_present() {
        // Wherever a system font exists this draws a real card; where
        // none does, fonts() must fail cleanly (that IS the contract).
        if !system_font_available() {
            return;
        }
        let cover =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(60, 90, Rgba([200, 30, 30, 255])));
        let entries = vec![
            CardEntry {
                title: "Alien",
                year: Some("1979"),
                cover: Some(&cover),
            },
            CardEntry {
                title: "Coverless",
                year: None,
                cover: None,
            },
        ];
        let rendered = render_card("Best Sci-Fi", &entries).unwrap();
        assert_eq!(rendered.image.dimensions(), (CARD_W, CARD_H));
        assert_eq!(rendered.missing_indices, vec![1]);
        // Corner is the family background; the covered row shows red.
        assert_eq!(*rendered.image.get_pixel(2, 2), render::BG);
        let row = &card_layout(2).rows[0];
        let cx = (row.thumb_x + row.thumb_size / 2.0) as u32;
        let cy = (row.thumb_y + row.thumb_size / 2.0) as u32;
        assert_eq!(rendered.image.get_pixel(cx, cy)[0], 200);
    }
}
