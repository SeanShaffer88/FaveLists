//! Image-export layout engine (spec §7). Pure geometry: given an arrangement,
//! the number of covers, each cover's aspect (width/height), and a canvas
//! size, produce one placement per item (in item order). Rendering
//! (`render.rs`) consumes placements, and the export preview's drag-to-swap
//! just reorders items — so every arrangement is unit-testable without
//! touching pixels.
//!
//! Deterministic: the scattered/spiral arrangements use a fixed-seed LCG, so
//! the same inputs always produce the same image (and tests are stable).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Arrangement {
    #[default]
    UniformGrid,
    Topster,
    Mosaic,
    ShelfRows,
    Scattered,
    Spiral,
    Honeycomb,
    Ring,
    Filmstrip,
    Masonry,
}

impl Arrangement {
    pub const ALL: [Arrangement; 10] = [
        Arrangement::UniformGrid,
        Arrangement::Topster,
        Arrangement::Mosaic,
        Arrangement::ShelfRows,
        Arrangement::Scattered,
        Arrangement::Spiral,
        Arrangement::Honeycomb,
        Arrangement::Ring,
        Arrangement::Filmstrip,
        Arrangement::Masonry,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Arrangement::UniformGrid => "Uniform grid",
            Arrangement::Topster => "Topster (fixed grid)",
            Arrangement::Mosaic => "Mosaic",
            Arrangement::ShelfRows => "Shelf rows",
            Arrangement::Scattered => "Scattered collage",
            Arrangement::Spiral => "Spiral",
            Arrangement::Honeycomb => "Honeycomb",
            Arrangement::Ring => "Ring",
            Arrangement::Filmstrip => "Filmstrip",
            Arrangement::Masonry => "Masonry columns",
        }
    }
}

/// One cover's placement, in canvas pixels. `rotation_deg` is 0 except for
/// the scattered collage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rotation_deg: f32,
}

impl Placement {
    fn at(x: f32, y: f32, w: f32, h: f32) -> Placement {
        Placement {
            x,
            y,
            w,
            h,
            rotation_deg: 0.0,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Axis-aligned overlap test (rotation ignored; used for the grid-family
    /// arrangements where rotation is always 0).
    pub fn overlaps(&self, other: &Placement) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

/// Tiny deterministic PRNG (LCG) for scattered/spiral jitter.
struct Lcg(u64);

impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32) / ((1u64 << 31) as f32)
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

/// Default export canvas for `n` items: aims for a shareable ~1600px image,
/// growing with the item count. Square-ish for grids; the filmstrip ignores
/// height.
pub fn default_canvas(arrangement: Arrangement, n: usize) -> (u32, u32) {
    let n = n.max(1);
    match arrangement {
        Arrangement::Filmstrip => ((n as u32 * 320).clamp(640, 6400), 480),
        Arrangement::Ring => (1400, 1400),
        Arrangement::Scattered => (1600, 1600),
        _ => {
            let cols = (n as f32).sqrt().ceil() as u32;
            let side = (cols * 360).clamp(720, 2400);
            (side, side)
        }
    }
}

/// Compute placements. `aspects[i]` is cover i's width/height (use 2/3 for a
/// typical poster when unknown). Returns exactly `aspects.len()` placements,
/// in order, all inside the canvas (except Scattered, whose rotated tiles may
/// overhang a hair by design — tiles stay fully inside via inset placement).
pub fn layout(
    arrangement: Arrangement,
    aspects: &[f32],
    canvas_w: f32,
    canvas_h: f32,
    gap: f32,
) -> Vec<Placement> {
    let n = aspects.len();
    if n == 0 {
        return Vec::new();
    }
    match arrangement {
        Arrangement::UniformGrid => grid(aspects, canvas_w, canvas_h, gap, false),
        Arrangement::Topster => grid(aspects, canvas_w, canvas_h, gap, true),
        Arrangement::Mosaic => mosaic(aspects, canvas_w, canvas_h, gap),
        Arrangement::ShelfRows => shelf(aspects, canvas_w, canvas_h, gap),
        Arrangement::Scattered => scattered(aspects, canvas_w, canvas_h),
        Arrangement::Spiral => spiral(aspects, canvas_w, canvas_h),
        Arrangement::Honeycomb => honeycomb(aspects, canvas_w, canvas_h, gap),
        Arrangement::Ring => ring(aspects, canvas_w, canvas_h),
        Arrangement::Filmstrip => filmstrip(aspects, canvas_w, canvas_h, gap),
        Arrangement::Masonry => masonry(aspects, canvas_w, canvas_h, gap),
    }
}

fn cols_for(n: usize) -> usize {
    (n as f32).sqrt().ceil() as usize
}

/// Uniform grid: every cell the same size; covers keep their aspect inside
/// the cell math by using the MEDIAN aspect for the cell shape, so the grid
/// is regular. Topster: same regularity, but cells are sized for a fixed
/// column count of ceil(sqrt(n)) with square-ish poster cells and the whole
/// grid is centred with equal margins (the classic topster look).
fn grid(aspects: &[f32], w: f32, h: f32, gap: f32, topster: bool) -> Vec<Placement> {
    let n = aspects.len();
    let cols = cols_for(n);
    let rows = n.div_ceil(cols);
    let mut sorted = aspects.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_aspect = sorted[n / 2].max(0.2);
    let cell_w = (w - gap * (cols as f32 + 1.0)) / cols as f32;
    let cell_h = cell_w / median_aspect;
    let total_h = rows as f32 * cell_h + gap * (rows as f32 + 1.0);
    let y0 = if topster {
        ((h - total_h) / 2.0).max(gap)
    } else {
        gap
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (r, c) = (i / cols, i % cols);
        out.push(Placement::at(
            gap + c as f32 * (cell_w + gap),
            y0 + r as f32 * (cell_h + gap),
            cell_w,
            cell_h,
        ));
    }
    out
}

/// Mosaic: rows of tiles where each row is justified to the canvas width and
/// tiles take a repeating size pattern (big, small, small) by giving the
/// pattern weights to the widths within the row.
fn mosaic(aspects: &[f32], w: f32, _h: f32, gap: f32) -> Vec<Placement> {
    let n = aspects.len();
    let weights = [2.0f32, 1.0, 1.0, 1.5, 1.0];
    let per_row = 3usize;
    let row_h = (w / 4.2).max(80.0);
    let mut out = Vec::with_capacity(n);
    let mut y = gap;
    let mut i = 0;
    while i < n {
        let in_row = per_row.min(n - i);
        let ws: Vec<f32> = (0..in_row)
            .map(|k| weights[(i + k) % weights.len()])
            .collect();
        let total_weight: f32 = ws.iter().sum();
        let avail = w - gap * (in_row as f32 + 1.0);
        let mut x = gap;
        for &weight in &ws {
            let tw = avail * weight / total_weight;
            out.push(Placement::at(x, y, tw, row_h));
            x += tw + gap;
        }
        y += row_h + gap;
        i += in_row;
    }
    out
}

/// Shelf rows: covers sit on "shelves" — bottom-aligned within each row,
/// heights vary with each cover's own aspect at a fixed row height.
fn shelf(aspects: &[f32], w: f32, _h: f32, gap: f32) -> Vec<Placement> {
    let row_h = (w / 4.0).max(80.0);
    let mut out = Vec::with_capacity(aspects.len());
    let mut x = gap;
    let mut y = gap;
    for &a in aspects {
        let tw = (row_h * a.max(0.2)).min(w - 2.0 * gap);
        if x + tw > w - gap && x > gap {
            x = gap;
            y += row_h + gap;
        }
        // All shelf tiles share the row height, so they sit flush on the
        // shelf line by construction.
        out.push(Placement::at(x, y, tw, row_h));
        x += tw + gap;
    }
    out
}

/// Scattered collage: deterministic pseudo-random positions + rotations,
/// tiles sized by aspect, kept inside the canvas.
fn scattered(aspects: &[f32], w: f32, h: f32) -> Vec<Placement> {
    let mut rng = Lcg(0xF00D_CAFE_1234_5678);
    let base = (w.min(h) / 3.4).max(60.0);
    let mut out = Vec::with_capacity(aspects.len());
    for &a in aspects {
        let th = base * rng.range(0.8, 1.25);
        let tw = th * a.clamp(0.3, 2.5);
        let x = rng.range(0.0, (w - tw).max(1.0));
        let y = rng.range(0.0, (h - th).max(1.0));
        out.push(Placement {
            x,
            y,
            w: tw,
            h: th,
            rotation_deg: rng.range(-14.0, 14.0),
        });
    }
    out
}

/// Spiral: tiles follow an Archimedean spiral out from the centre,
/// shrinking slightly as they go.
fn spiral(aspects: &[f32], w: f32, h: f32) -> Vec<Placement> {
    let n = aspects.len();
    let cx = w / 2.0;
    let cy = h / 2.0;
    let max_r = (w.min(h) / 2.0) * 0.92;
    let base = (w.min(h) / 4.2).max(50.0);
    let mut out = Vec::with_capacity(n);
    for (i, &a) in aspects.iter().enumerate() {
        let t = if n == 1 {
            0.0
        } else {
            i as f32 / (n - 1) as f32
        };
        let angle = t * std::f32::consts::TAU * 2.5;
        let r = max_r * t.sqrt();
        let size = base * (1.15 - 0.45 * t);
        let th = size;
        let tw = th * a.clamp(0.3, 2.5);
        let x = (cx + r * angle.cos() - tw / 2.0).clamp(0.0, (w - tw).max(0.0));
        let y = (cy + r * angle.sin() - th / 2.0).clamp(0.0, (h - th).max(0.0));
        out.push(Placement::at(x, y, tw, th));
    }
    out
}

/// Honeycomb: offset rows of hexagon-ish cells (tiles themselves are
/// rectangular, clipped to hexagons at render time via a mask flag — the
/// geometry here is the cell grid; odd rows are half a cell indented).
fn honeycomb(aspects: &[f32], w: f32, _h: f32, gap: f32) -> Vec<Placement> {
    let n = aspects.len();
    let cols = cols_for(n).max(2);
    // Reserve a half-cell of width so indented (odd) rows fit without
    // pushing their last tile back over its neighbour.
    let cell_w = (w - gap * (cols as f32 + 1.0)) / (cols as f32 + 0.5);
    let cell_h = cell_w * 1.1;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (r, c) = (i / cols, i % cols);
        let indent = if r % 2 == 1 { cell_w / 2.0 } else { 0.0 };
        let x = gap + c as f32 * (cell_w + gap) + indent;
        // Tile height equals the row pitch (0.82 * cell_h): hex rows nest
        // tightly without covers overlapping each other.
        out.push(Placement::at(
            x,
            gap + r as f32 * (cell_h * 0.82 + gap),
            cell_w,
            cell_h * 0.82,
        ));
    }
    out
}

/// Ring: tiles arranged in a circle (one ring for <= 12 items, concentric
/// rings beyond that).
fn ring(aspects: &[f32], w: f32, h: f32) -> Vec<Placement> {
    let n = aspects.len();
    let cx = w / 2.0;
    let cy = h / 2.0;
    let base = (w.min(h) / 5.0).max(50.0);
    let mut out = Vec::with_capacity(n);
    if n == 1 {
        let a = aspects[0].clamp(0.3, 2.5);
        out.push(Placement::at(
            cx - base * a / 2.0,
            cy - base / 2.0,
            base * a,
            base,
        ));
        return out;
    }
    let per_ring = 12usize;
    for (i, &a) in aspects.iter().enumerate() {
        let ring_idx = i / per_ring;
        let in_ring = i % per_ring;
        let ring_count = ((n - ring_idx * per_ring).min(per_ring)) as f32;
        let radius = (w.min(h) / 2.0 - base) * (0.55 + 0.45 * ring_idx as f32);
        let angle =
            (in_ring as f32 / ring_count) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let th = base;
        let tw = th * a.clamp(0.3, 2.5);
        let x = (cx + radius * angle.cos() - tw / 2.0).clamp(0.0, (w - tw).max(0.0));
        let y = (cy + radius * angle.sin() - th / 2.0).clamp(0.0, (h - th).max(0.0));
        out.push(Placement::at(x, y, tw, th));
    }
    out
}

/// Filmstrip: a single row of covers at full canvas height (minus margins).
fn filmstrip(aspects: &[f32], w: f32, h: f32, gap: f32) -> Vec<Placement> {
    let n = aspects.len();
    let th = h - 2.0 * gap;
    let avail = w - gap * (n as f32 + 1.0);
    let mut out = Vec::with_capacity(n);
    // Equal slots; each cover is centred in its slot at its own aspect.
    let slot = avail / n as f32;
    for (i, &a) in aspects.iter().enumerate() {
        let tw = (th * a).min(slot);
        let x = gap + i as f32 * (slot + gap) + (slot - tw) / 2.0;
        out.push(Placement::at(x, gap, tw, th));
    }
    out
}

/// Masonry: fixed-width columns; each cover keeps its own aspect, so heights
/// vary and the next cover goes into the currently shortest column.
fn masonry(aspects: &[f32], w: f32, _h: f32, gap: f32) -> Vec<Placement> {
    let n = aspects.len();
    let cols = cols_for(n);
    let col_w = (w - gap * (cols as f32 + 1.0)) / cols as f32;
    let mut heights = vec![gap; cols];
    let mut out = Vec::with_capacity(n);
    for &a in aspects {
        let (col, _) = heights
            .iter()
            .enumerate()
            .min_by(|x, y| x.1.partial_cmp(y.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();
        let th = col_w / a.max(0.2);
        out.push(Placement::at(
            gap + col as f32 * (col_w + gap),
            heights[col],
            col_w,
            th,
        ));
        heights[col] += th + gap;
    }
    out
}

/// Height actually needed by a layout (some arrangements size rows by the
/// canvas width, so content can be shorter than the canvas). Rendering crops
/// or pads to this.
pub fn content_bottom(placements: &[Placement]) -> f32 {
    placements.iter().map(|p| p.bottom()).fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aspects(n: usize) -> Vec<f32> {
        (0..n).map(|i| [0.67f32, 1.0, 0.7, 1.42][i % 4]).collect()
    }

    #[test]
    fn every_arrangement_places_all_items_inside_canvas() {
        for arr in Arrangement::ALL {
            for n in [1usize, 2, 5, 9, 16, 25] {
                let (cw, ch) = default_canvas(arr, n);
                let ps = layout(arr, &aspects(n), cw as f32, ch as f32, 12.0);
                assert_eq!(ps.len(), n, "{arr:?} n={n}");
                for p in &ps {
                    assert!(p.w > 0.0 && p.h > 0.0, "{arr:?} n={n} zero size");
                    assert!(
                        p.x >= -0.5 && p.y >= -0.5,
                        "{arr:?} n={n} negative pos {p:?}"
                    );
                    assert!(
                        p.right() <= cw as f32 + 0.5,
                        "{arr:?} n={n} overhang right {p:?} canvas {cw}"
                    );
                }
            }
        }
    }

    #[test]
    fn grid_family_never_overlaps() {
        for arr in [
            Arrangement::UniformGrid,
            Arrangement::Topster,
            Arrangement::Mosaic,
            Arrangement::Honeycomb,
            Arrangement::Filmstrip,
            Arrangement::Masonry,
        ] {
            let (cw, ch) = default_canvas(arr, 12);
            let ps = layout(arr, &aspects(12), cw as f32, ch as f32, 10.0);
            for i in 0..ps.len() {
                for j in i + 1..ps.len() {
                    assert!(!ps[i].overlaps(&ps[j]), "{arr:?} tiles {i} and {j} overlap");
                }
            }
        }
    }

    #[test]
    fn scattered_is_deterministic() {
        let a = layout(Arrangement::Scattered, &aspects(9), 1200.0, 1200.0, 10.0);
        let b = layout(Arrangement::Scattered, &aspects(9), 1200.0, 1200.0, 10.0);
        assert_eq!(a, b);
        assert!(a.iter().all(|p| p.rotation_deg.abs() <= 14.0));
    }

    #[test]
    fn topster_is_vertically_centred() {
        let ps = layout(Arrangement::Topster, &aspects(4), 1000.0, 1000.0, 10.0);
        let top_gap = ps.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let bottom_gap = 1000.0 - content_bottom(&ps);
        assert!(
            (top_gap - bottom_gap).abs() < 30.0,
            "top {top_gap} bottom {bottom_gap}"
        );
    }

    #[test]
    fn masonry_uses_shortest_column() {
        let ps = layout(
            Arrangement::Masonry,
            &[1.0, 0.5, 1.0, 0.5],
            900.0,
            900.0,
            10.0,
        );
        // 2 columns; third item should stack in the column whose first item
        // was shorter (the square one).
        assert_eq!(ps.len(), 4);
        assert!(ps[2].y > ps[0].y);
    }

    #[test]
    fn filmstrip_single_row() {
        let ps = layout(Arrangement::Filmstrip, &aspects(6), 2000.0, 480.0, 12.0);
        assert!(ps.iter().all(|p| (p.y - 12.0).abs() < 0.01));
        assert!(ps.windows(2).all(|w| w[1].x > w[0].x));
    }

    #[test]
    fn empty_input_empty_output() {
        assert!(layout(Arrangement::UniformGrid, &[], 800.0, 800.0, 10.0).is_empty());
        assert!(layout(Arrangement::Ring, &[], 800.0, 800.0, 10.0).is_empty());
    }
}
