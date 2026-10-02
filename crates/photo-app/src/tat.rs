//! Targeted Adjustment Tool (TAT) / On-Image Dragging.
//!
//! Allows clicking and dragging directly on the photo to adjust the specific color band
//! (Hue, Saturation, Luminance) or tonal zone under the cursor.

use crate::ColorEqTab;
use egui::Pos2;

/// Which parameter family the TAT is currently adjusting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TatMode {
    ColorEq(ColorEqTab),
}

/// Active drag state for targeted adjustment.
#[derive(Debug, Clone)]
pub struct TatDrag {
    pub band_name: &'static str,
    pub initial_val: f32,
    pub start_pos: Pos2,
    pub current_delta: f32,
}

/// Convert linear RGB to HSL (Hue 0..360, Saturation 0..1, Lightness 0..1).
pub fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let l = (max + min) * 0.5;
    if delta < 1e-4 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let mut h = if (max - r).abs() < 1e-4 {
        (g - b) / delta + (if g < b { 6.0 } else { 0.0 })
    } else if (max - g).abs() < 1e-4 {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    h *= 60.0;
    (h, s, l)
}

/// Map a hue angle in degrees (0..360) to the corresponding 8-band ColorEq band name.
pub fn hue_to_band(hue: f32) -> &'static str {
    let h = (hue % 360.0 + 360.0) % 360.0;
    if h < 15.0 || h >= 340.0 {
        "Red"
    } else if h < 45.0 {
        "Orange"
    } else if h < 90.0 {
        "Yellow"
    } else if h < 150.0 {
        "Green"
    } else if h < 210.0 {
        "Aqua"
    } else if h < 260.0 {
        "Blue"
    } else if h < 300.0 {
        "Purple"
    } else {
        "Magenta"
    }
}
