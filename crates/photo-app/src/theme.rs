//! DESIGN.md's look and feel, as egui visuals.
//!
//! Gunmetal neutrals, one warm brass accent, no gradients or shadows anywhere near the
//! media. The brief bans exactly the things immediate-mode UI is worst at faking, so this
//! is a short file.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const BG_950: Color32 = Color32::from_rgb(0x0b, 0x0b, 0x0c);
pub const BG_900: Color32 = Color32::from_rgb(0x13, 0x14, 0x15);
pub const BG_850: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x1e);
pub const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x30);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xe8);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x8d, 0x91);

/// Warm brass, after the duckfoot pistol — brass and walnut. Not indigo.
pub const ACCENT: Color32 = Color32::from_rgb(0xc9, 0x99, 0x2e);
pub const DANGER: Color32 = Color32::from_rgb(0xc0, 0x46, 0x3c);

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();

    v.panel_fill = BG_950;
    v.window_fill = BG_900;
    v.extreme_bg_color = BG_950;
    v.faint_bg_color = BG_900;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.error_fg_color = DANGER;
    v.warn_fg_color = ACCENT;

    // No shadows. They lie about what you're editing.
    v.window_shadow = Default::default();
    v.popup_shadow = Default::default();

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(2);
        w.bg_stroke = Stroke::new(1.0, BORDER);
    }
    v.widgets.noninteractive.bg_fill = BG_900;
    v.widgets.inactive.bg_fill = BG_850;
    v.widgets.hovered.bg_fill = BG_850;
    v.widgets.active.bg_fill = BG_850;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.linear_multiply(0.6));
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);

    ctx.set_visuals(v);

    ctx.all_styles_mut(|style| {
        style.spacing.slider_width = 150.0;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    });
}

/// Monospace for anything read as a value rather than as prose.
pub fn num(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).monospace().color(TEXT)
}

pub fn dim(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(TEXT_DIM)
}
