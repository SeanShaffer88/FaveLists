//! Appearance settings (v0.3 Slice 5: #14 text size, #15 cover
//! scale, #16 high contrast) — the pure, testable half of the app's
//! look. The GUI (`app.rs` / `app_settings.rs`) renders state and
//! applies what is computed here; persistence is `Settings` in
//! [`crate::model`], which stores a [`TextSize`], the cover scale,
//! the rows-view flag and the high-contrast flag.
//!
//! All of this is presentation only: none of it is a registry
//! command, exactly like egui zoom has always been.

use eframe::egui;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Family palette (moved here from app.rs, which re-exports it — the
// values are the pre-Slice-5 look, unchanged)
// ---------------------------------------------------------------------------
pub const COL_BG: egui::Color32 = egui::Color32::from_rgb(0x10, 0x10, 0x10);
pub const COL_PANEL: egui::Color32 = egui::Color32::from_rgb(0x16, 0x16, 0x16);
pub const COL_HAIRLINE: egui::Color32 = egui::Color32::from_rgb(0x24, 0x24, 0x24);
pub const COL_ACCENT: egui::Color32 = egui::Color32::from_rgb(0xE5, 0x48, 0x4D);
pub const COL_DEEP_RED: egui::Color32 = egui::Color32::from_rgb(0x6B, 0x07, 0x00);
pub const COL_TEXT: egui::Color32 = egui::Color32::from_rgb(0xF5, 0xF5, 0xF5);
pub const COL_MUTED: egui::Color32 = egui::Color32::from_rgb(0xA8, 0xA8, 0xA8);

// High-contrast variants: same black/red identity, pushed brighter.
pub const COL_HAIRLINE_HC: egui::Color32 = egui::Color32::from_rgb(0x6E, 0x6E, 0x6E);
pub const COL_ACCENT_HC: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x5A, 0x5E);
pub const COL_TEXT_HC: egui::Color32 = egui::Color32::WHITE;

// ---------------------------------------------------------------------------
// #14 Text size
// ---------------------------------------------------------------------------

/// UI text size, applied as an egui zoom factor. `Large` is the
/// v0.2.1 fixed 1.3× zoom turned into the default choice, so
/// existing users keep exactly the size they already have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextSize {
    Comfortable,
    #[default]
    Large,
    ExtraLarge,
}

impl TextSize {
    pub const ALL: [TextSize; 3] = [TextSize::Comfortable, TextSize::Large, TextSize::ExtraLarge];

    /// The global egui zoom factor for this size.
    pub fn factor(self) -> f32 {
        match self {
            TextSize::Comfortable => 1.0,
            TextSize::Large => 1.3,
            TextSize::ExtraLarge => 1.6,
        }
    }

    /// Stable machine id — also the stored settings.json form.
    pub fn id(self) -> &'static str {
        match self {
            TextSize::Comfortable => "comfortable",
            TextSize::Large => "large",
            TextSize::ExtraLarge => "extra_large",
        }
    }

    pub fn from_id(id: &str) -> Option<TextSize> {
        TextSize::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Settings-dialog wording.
    pub fn label(self) -> &'static str {
        match self {
            TextSize::Comfortable => "Comfortable",
            TextSize::Large => "Large",
            TextSize::ExtraLarge => "Extra large",
        }
    }
}

// ---------------------------------------------------------------------------
// #15 Cover scale
// ---------------------------------------------------------------------------

pub const COVER_SCALE_MIN: f32 = 0.75;
pub const COVER_SCALE_MAX: f32 = 1.5;
pub const COVER_SCALE_DEFAULT: f32 = 1.0;

/// Clamp a stored cover scale into the slider's range. A non-finite
/// stored value (a hand-edited settings file) falls back to 1.0
/// rather than poisoning every tile size.
pub fn clamp_cover_scale(scale: f32) -> f32 {
    if !scale.is_finite() {
        return COVER_SCALE_DEFAULT;
    }
    scale.clamp(COVER_SCALE_MIN, COVER_SCALE_MAX)
}

// ---------------------------------------------------------------------------
// #16 High contrast
// ---------------------------------------------------------------------------

/// The main text colour for the mode.
pub fn main_text_color(high_contrast: bool) -> egui::Color32 {
    if high_contrast {
        COL_TEXT_HC
    } else {
        COL_TEXT
    }
}

/// The accent (links, selection stroke) colour for the mode.
pub fn accent_color(high_contrast: bool) -> egui::Color32 {
    if high_contrast {
        COL_ACCENT_HC
    } else {
        COL_ACCENT
    }
}

/// The widget-border colour for the mode.
pub fn border_color(high_contrast: bool) -> egui::Color32 {
    if high_contrast {
        COL_HAIRLINE_HC
    } else {
        COL_HAIRLINE
    }
}

/// The egui `Visuals` for the mode. `visuals(false)` reproduces the
/// pre-Slice-5 `install_theme` look exactly (same fills, same deep
/// red hover/selection); `visuals(true)` keeps the black/red fills
/// and pushes text, accent and borders brighter and stronger so
/// edges read clearly.
pub fn visuals(high_contrast: bool) -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = COL_BG;
    visuals.window_fill = COL_PANEL;
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x0A, 0x0A, 0x0A);
    visuals.hyperlink_color = accent_color(high_contrast);
    visuals.selection.bg_fill = COL_DEEP_RED;
    if high_contrast {
        visuals.faint_bg_color = egui::Color32::from_rgb(0x2A, 0x2A, 0x2A);
        visuals.override_text_color = Some(COL_TEXT_HC);
        let border = egui::Stroke::new(1.5_f32, COL_HAIRLINE_HC);
        visuals.widgets.noninteractive.bg_stroke = border;
        visuals.widgets.inactive.bg_stroke = border;
        visuals.widgets.hovered.bg_stroke = border;
        visuals.widgets.active.bg_stroke = border;
        visuals.widgets.open.bg_stroke = border;
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x2E, 0x2E, 0x2E);
        visuals.widgets.hovered.bg_fill = COL_DEEP_RED;
        visuals.widgets.active.bg_fill = COL_DEEP_RED;
        visuals.selection.stroke = egui::Stroke::new(2.0_f32, COL_ACCENT_HC);
    } else {
        visuals.faint_bg_color = egui::Color32::from_rgb(0x1B, 0x1B, 0x1B);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, COL_HAIRLINE);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x1E, 0x1E, 0x1E);
        visuals.widgets.hovered.bg_fill = COL_DEEP_RED;
        visuals.widgets.active.bg_fill = COL_DEEP_RED;
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, COL_ACCENT);
    }
    visuals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brightness(c: egui::Color32) -> u32 {
        c.r() as u32 + c.g() as u32 + c.b() as u32
    }

    #[test]
    fn text_size_factors_and_id_round_trip() {
        assert_eq!(TextSize::Comfortable.factor(), 1.0);
        assert_eq!(TextSize::Large.factor(), 1.3);
        assert_eq!(TextSize::ExtraLarge.factor(), 1.6);
        for size in TextSize::ALL {
            assert_eq!(TextSize::from_id(size.id()), Some(size));
        }
        assert_eq!(TextSize::from_id("huge"), None);
        // Large is the default: it preserves the v0.2.1 fixed 1.3x.
        assert_eq!(TextSize::default(), TextSize::Large);
        assert_eq!(TextSize::default().factor(), 1.3);
        // The stored form is the id.
        assert_eq!(
            serde_json::to_string(&TextSize::ExtraLarge).unwrap(),
            "\"extra_large\""
        );
        assert_eq!(
            serde_json::from_str::<TextSize>("\"large\"").unwrap(),
            TextSize::Large
        );
    }

    #[test]
    fn cover_scale_clamps_to_the_slider_range() {
        assert_eq!(clamp_cover_scale(1.0), 1.0);
        assert_eq!(clamp_cover_scale(0.75), 0.75);
        assert_eq!(clamp_cover_scale(1.5), 1.5);
        assert_eq!(clamp_cover_scale(0.1), COVER_SCALE_MIN);
        assert_eq!(clamp_cover_scale(99.0), COVER_SCALE_MAX);
        assert_eq!(clamp_cover_scale(f32::NAN), COVER_SCALE_DEFAULT);
        assert_eq!(clamp_cover_scale(f32::INFINITY), COVER_SCALE_DEFAULT);
    }

    #[test]
    fn high_contrast_colors_are_brighter_than_normal() {
        let normal = visuals(false);
        let hc = visuals(true);
        // Same black/red identity: the fills do not move.
        assert_eq!(hc.panel_fill, normal.panel_fill);
        assert_eq!(hc.window_fill, normal.window_fill);
        assert_eq!(hc.selection.bg_fill, normal.selection.bg_fill);
        // Key colours differ and are brighter in high contrast.
        assert_ne!(hc.hyperlink_color, normal.hyperlink_color);
        assert!(brightness(hc.hyperlink_color) > brightness(normal.hyperlink_color));
        let hc_border = hc.widgets.noninteractive.bg_stroke.color;
        let normal_border = normal.widgets.noninteractive.bg_stroke.color;
        assert_ne!(hc_border, normal_border);
        assert!(brightness(hc_border) > brightness(normal_border));
        assert!(
            hc.widgets.noninteractive.bg_stroke.width
                > normal.widgets.noninteractive.bg_stroke.width
        );
        let hc_text = hc.override_text_color.unwrap();
        let normal_text = normal.override_text_color.unwrap_or(COL_TEXT);
        assert!(brightness(hc_text) > brightness(normal_text));
        assert!(brightness(hc.selection.stroke.color) > brightness(normal.selection.stroke.color));
        assert!(hc.selection.stroke.width > normal.selection.stroke.width);
        // The helpers the GUI/tests share agree with the structs.
        assert_eq!(main_text_color(true), COL_TEXT_HC);
        assert_eq!(main_text_color(false), COL_TEXT);
        assert_eq!(accent_color(true), hc.hyperlink_color);
        assert_eq!(border_color(false), normal_border);
    }
}
