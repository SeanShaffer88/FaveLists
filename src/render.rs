//! Image rendering (spec §7): draw cover images into a layout's placements
//! on a near-black canvas. NO text is ever drawn — the output is cover art
//! only. Missing covers become a plain dark tile with a deep-red border (no
//! words, per the no-text rule); the export flow warns about them first via
//! [`missing_covers`].

use crate::layout::{Arrangement, Placement};
use image::{DynamicImage, Rgba, RgbaImage};

/// How a cover fills its placement tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FitMode {
    /// Crop the cover to fill the tile exactly (edges may be cut).
    #[default]
    CropFill,
    /// Show the whole cover, letterboxed inside the tile.
    WholeCover,
}

/// Theme colours from the family palette.
pub const BG: Rgba<u8> = Rgba([16, 16, 16, 255]); // #101010
pub const TILE_BG: Rgba<u8> = Rgba([24, 24, 24, 255]);
pub const TILE_BORDER: Rgba<u8> = Rgba([107, 7, 0, 255]); // #6B0700

/// Items (by index into the export order) that have no usable cover image.
/// The GUI shows this list as the pre-export warning (spec §7).
pub fn missing_covers(covers: &[Option<&DynamicImage>]) -> Vec<usize> {
    covers
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_none())
        .map(|(i, _)| i)
        .collect()
}

/// Rotate an image by degrees around its centre onto a transparent canvas
/// sized to the rotated bounding box. Nearest-neighbour is fine for a
/// collage tilt.
fn rotate(img: &RgbaImage, deg: f32) -> RgbaImage {
    if deg.abs() < 0.01 {
        return img.clone();
    }
    let rad = deg.to_radians();
    let (w, h) = (img.width() as f32, img.height() as f32);
    let (sin, cos) = (rad.sin().abs(), rad.cos().abs());
    let nw = (w * cos + h * sin).ceil() as u32;
    let nh = (w * sin + h * cos).ceil() as u32;
    let mut out = RgbaImage::from_pixel(nw, nh, Rgba([0, 0, 0, 0]));
    let (rc, rs) = (rad.cos(), rad.sin());
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (ncx, ncy) = (nw as f32 / 2.0, nh as f32 / 2.0);
    for y in 0..nh {
        for x in 0..nw {
            // Inverse-rotate the destination point into source space.
            let dx = x as f32 - ncx;
            let dy = y as f32 - ncy;
            let sx = dx * rc + dy * rs + cx;
            let sy = -dx * rs + dy * rc + cy;
            if sx >= 0.0 && sy >= 0.0 && sx < w && sy < h {
                out.put_pixel(x, y, *img.get_pixel(sx as u32, sy as u32));
            }
        }
    }
    out
}

fn blend_over(dst: &mut RgbaImage, src: &RgbaImage, ox: i64, oy: i64) {
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
                let d = dst.get_pixel(dx as u32, dy as u32);
                let a = s[3] as f32 / 255.0;
                let mut px = [0u8; 4];
                for i in 0..3 {
                    px[i] = (s[i] as f32 * a + d[i] as f32 * (1.0 - a)) as u8;
                }
                px[3] = 255;
                dst.put_pixel(dx as u32, dy as u32, Rgba(px));
            }
        }
    }
}

fn draw_tile_placeholder(canvas: &mut RgbaImage, p: &Placement) {
    let (x, y, w, h) = (p.x as i64, p.y as i64, p.w as u32, p.h as u32);
    for yy in 0..h {
        for xx in 0..w {
            let border = xx < 3 || yy < 3 || xx + 3 >= w || yy + 3 >= h;
            let color = if border { TILE_BORDER } else { TILE_BG };
            let (dx, dy) = (x + xx as i64, y + yy as i64);
            if dx >= 0 && dy >= 0 && dx < canvas.width() as i64 && dy < canvas.height() as i64 {
                canvas.put_pixel(dx as u32, dy as u32, color);
            }
        }
    }
}

/// Prepare one cover for its placement: resize + crop/fit, then rotate.
fn prepare_tile(cover: &DynamicImage, p: &Placement, fit: FitMode) -> RgbaImage {
    let (tw, th) = (p.w.max(1.0) as u32, p.h.max(1.0) as u32);
    let rgba = cover.to_rgba8();
    let (cw, ch) = (rgba.width(), rgba.height());
    let tile = match fit {
        FitMode::CropFill => {
            // Scale to cover the tile, centre-crop the overflow.
            let scale = (tw as f32 / cw as f32).max(th as f32 / ch as f32);
            let sw = (cw as f32 * scale).ceil() as u32;
            let sh = (ch as f32 * scale).ceil() as u32;
            let resized = image::imageops::resize(
                &rgba,
                sw.max(tw),
                sh.max(th),
                image::imageops::FilterType::Triangle,
            );
            let x0 = (resized.width() - tw) / 2;
            let y0 = (resized.height() - th) / 2;
            image::imageops::crop_imm(&resized, x0, y0, tw, th).to_image()
        }
        FitMode::WholeCover => {
            let scale = (tw as f32 / cw as f32).min(th as f32 / ch as f32);
            let sw = ((cw as f32 * scale) as u32).max(1);
            let sh = ((ch as f32 * scale) as u32).max(1);
            let resized =
                image::imageops::resize(&rgba, sw, sh, image::imageops::FilterType::Triangle);
            let mut tile = RgbaImage::from_pixel(tw, th, TILE_BG);
            blend_over(
                &mut tile,
                &resized,
                ((tw - sw) / 2) as i64,
                ((th - sh) / 2) as i64,
            );
            tile
        }
    };
    if p.rotation_deg.abs() >= 0.01 {
        rotate(&tile, p.rotation_deg)
    } else {
        tile
    }
}

/// Render the final image. `covers` aligns with `placements` by index;
/// `None` renders a placeholder tile.
pub fn render(
    placements: &[Placement],
    covers: &[Option<&DynamicImage>],
    canvas_w: u32,
    canvas_h: u32,
    arrangement: Arrangement,
    fit: FitMode,
) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(canvas_w, canvas_h, BG);
    if arrangement == Arrangement::Filmstrip {
        // Sprocket-strip bands top and bottom, like a strip of film: two dark
        // bands slightly lighter than the background. Still no text.
        for y in 0..canvas_h {
            if y < 26 || y + 26 >= canvas_h {
                for x in 0..canvas_w {
                    canvas.put_pixel(x, y, Rgba([28, 28, 28, 255]));
                }
            }
        }
    }
    for (i, p) in placements.iter().enumerate() {
        let cover = covers.get(i).copied().flatten();
        match cover {
            Some(img) => {
                let tile = prepare_tile(img, p, fit);
                // Rotated tiles are centred on the placement centre.
                let ox = if p.rotation_deg.abs() >= 0.01 {
                    (p.x + p.w / 2.0 - tile.width() as f32 / 2.0) as i64
                } else {
                    p.x as i64
                };
                let oy = if p.rotation_deg.abs() >= 0.01 {
                    (p.y + p.h / 2.0 - tile.height() as f32 / 2.0) as i64
                } else {
                    p.y as i64
                };
                blend_over(&mut canvas, &tile, ox, oy);
            }
            None => draw_tile_placeholder(&mut canvas, p),
        }
    }
    canvas
}

/// Encode as PNG or JPEG bytes.
pub fn encode(img: &RgbaImage, format: ImageFormat) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    match format {
        ImageFormat::Png => img
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| format!("PNG encode failed: {e}"))?,
        ImageFormat::Jpeg => {
            // JPEG has no alpha; flatten onto the background colour.
            let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            rgb.write_to(&mut cursor, image::ImageFormat::Jpeg)
                .map_err(|e| format!("JPEG encode failed: {e}"))?;
        }
    }
    Ok(buf)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout;

    fn solid(w: u32, h: u32, color: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, Rgba(color)))
    }

    #[test]
    fn renders_grid_with_placeholder_for_missing() {
        let red = solid(100, 150, [200, 30, 30, 255]);
        let covers: Vec<Option<&DynamicImage>> = vec![Some(&red), None, Some(&red), None];
        let aspects = vec![0.67f32; 4];
        let ps = layout::layout(Arrangement::UniformGrid, &aspects, 400.0, 400.0, 10.0);
        let img = render(
            &ps,
            &covers,
            400,
            400,
            Arrangement::UniformGrid,
            FitMode::CropFill,
        );
        assert_eq!(img.dimensions(), (400, 400));
        // A covered tile carries the cover colour at its centre.
        let p0 = ps[0];
        let centre = img.get_pixel((p0.x + p0.w / 2.0) as u32, (p0.y + p0.h / 2.0) as u32);
        assert_eq!(centre[0], 200);
        // The placeholder tile's centre is the tile background, not the cover.
        let p1 = ps[1];
        let c1 = img.get_pixel((p1.x + p1.w / 2.0) as u32, (p1.y + p1.h / 2.0) as u32);
        assert_eq!(*c1, TILE_BG);
    }

    #[test]
    fn missing_covers_reports_indices() {
        let img = solid(10, 10, [0, 0, 0, 255]);
        assert_eq!(
            missing_covers(&[Some(&img), None, None, Some(&img)]),
            vec![1, 2]
        );
    }

    #[test]
    fn encodes_png_and_jpeg() {
        let img = RgbaImage::from_pixel(20, 20, Rgba([10, 20, 30, 255]));
        let png = encode(&img, ImageFormat::Png).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let jpg = encode(&img, ImageFormat::Jpeg).unwrap();
        assert_eq!(&jpg[0..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn whole_cover_letterboxes_without_cropping() {
        // Wide cover in a square tile: whole-cover keeps the colour band in
        // the middle; crop-fill would fill the whole tile.
        let wide = solid(200, 50, [30, 200, 30, 255]);
        let p = Placement {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            rotation_deg: 0.0,
        };
        let img = render(
            &[p],
            &[Some(&wide)],
            100,
            100,
            Arrangement::UniformGrid,
            FitMode::WholeCover,
        );
        assert_eq!(*img.get_pixel(50, 50), Rgba([30, 200, 30, 255]));
        assert_eq!(*img.get_pixel(50, 2), TILE_BG);
        let img2 = render(
            &[p],
            &[Some(&wide)],
            100,
            100,
            Arrangement::UniformGrid,
            FitMode::CropFill,
        );
        assert_eq!(*img2.get_pixel(50, 2), Rgba([30, 200, 30, 255]));
    }

    #[test]
    fn rotation_renders_without_panic() {
        let img = solid(80, 80, [90, 90, 200, 255]);
        let p = Placement {
            x: 50.0,
            y: 50.0,
            w: 80.0,
            h: 80.0,
            rotation_deg: 20.0,
        };
        let out = render(
            &[p],
            &[Some(&img)],
            200,
            200,
            Arrangement::Scattered,
            FitMode::CropFill,
        );
        assert_eq!(out.dimensions(), (200, 200));
    }

    #[test]
    fn aspect_of_loaded_image() {
        let img = solid(300, 200, [1, 2, 3, 255]);
        let (w, h) = image::GenericImageView::dimensions(&img);
        assert!((w as f32 / h as f32 - 1.5).abs() < 0.001);
    }
}
