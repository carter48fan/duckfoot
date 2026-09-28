//! The pitch-black look, as egui visuals.
//!
//! An AMOLED-black canvas. Surfaces step up by a few units of luminance and are separated
//! by 1px hairlines; that is the only elevation there is. One amber accent means "active",
//! one lime means "saved / locked". No gradients, shadows or glows anywhere — they lie
//! about what you're editing.

use egui::epaint::text::VariationCoords;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Stroke,
    TextStyle, Visuals,
};

/// Canvas, image well, graph backgrounds. True black, so an OLED pixel is off.
pub const BG_950: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
/// Header bands and module cards.
pub const BG_925: Color32 = Color32::from_rgb(0x05, 0x05, 0x05);
/// Buttons and tool pods.
pub const BG_900: Color32 = Color32::from_rgb(0x0a, 0x0a, 0x0a);
/// Hover, and the fill behind a selected chip.
pub const BG_850: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);

/// Structural hairlines between panels and around cards.
pub const BORDER: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
/// A border under the pointer.
pub const BORDER_HI: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
/// Slider rails and scrollbar thumbs.
pub const RAIL: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);

pub const TEXT_HI: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const TEXT: Color32 = Color32::from_rgb(0xe5, 0xe5, 0xe5);
/// Secondary readouts and labels.
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x99, 0x99, 0x99);
/// Disabled, or not yet applied (redo steps). 4.6:1 on black: faint, but still legible
/// text (WCAG AA), where #555 at 2.8:1 was not.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x76, 0x76, 0x76);

/// Calibrated amber: selection, the active module, the one primary action.
pub const ACCENT: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);

// White balance tracks: which way each slider pushes the picture.
pub const WB_COOL: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);
pub const WB_WARM: Color32 = Color32::from_rgb(0xf5, 0xc5, 0x42);
pub const WB_GREEN: Color32 = Color32::from_rgb(0x4a, 0xc2, 0x6b);
pub const WB_MAGENTA: Color32 = Color32::from_rgb(0xd9, 0x46, 0xef);
pub const ACCENT_HI: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
/// Lime: persisted, synced, locked. Never decorative.
pub const LIME: Color32 = Color32::from_rgb(0x10, 0xb9, 0x81);
pub const DANGER: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);

pub const RADIUS: u8 = 4;

// Three OFL typefaces, each one variable file instanced at the weights we use. egui has no
// notion of bold — `RichText::strong` only changes colour — so every weight is its own
// named family. The files are borrowed, not copied, per instance.
const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Variable.ttf");
const SPACE_GROTESK: &[u8] = include_bytes!("../assets/fonts/SpaceGrotesk-Variable.ttf");
const HANKEN_GROTESK: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Variable.ttf");

/// JetBrains Mono Bold: file names, tags, anything read as a value and meant to stand out.
pub fn mono_bold() -> FontFamily {
    FontFamily::Name("mono-bold".into())
}

/// Space Grotesk Bold: panel and section headings, always uppercase.
pub fn display() -> FontFamily {
    FontFamily::Name("display".into())
}

/// Hanken Grotesk SemiBold: module titles and button labels.
pub fn sans_strong() -> FontFamily {
    FontFamily::Name("sans-strong".into())
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let instance = |bytes: &'static [u8], weight: f32| {
        FontData::from_static(bytes).tweak(FontTweak {
            coords: VariationCoords::new([(b"wght", weight)]),
            ..Default::default()
        })
    };
    for (name, bytes, weight) in [
        ("jbm-400", JETBRAINS_MONO, 400.0),
        ("jbm-700", JETBRAINS_MONO, 700.0),
        ("space-700", SPACE_GROTESK, 700.0),
        ("hanken-450", HANKEN_GROTESK, 450.0),
        ("hanken-600", HANKEN_GROTESK, 600.0),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), instance(bytes, weight).into());
    }

    // egui's own fonts stay behind ours as fallbacks: they carry the arrows, emoji and
    // geometric symbols the three text faces don't.
    let fallbacks: Vec<String> = [
        "Hack",
        "Ubuntu-Light",
        "NotoEmoji-Regular",
        "emoji-icon-font",
    ]
    .into_iter()
    .filter(|f| fonts.font_data.contains_key(*f))
    .map(str::to_owned)
    .collect();
    let family = |primary: &str| {
        std::iter::once(primary.to_owned())
            .chain(fallbacks.iter().cloned())
            .collect::<Vec<_>>()
    };
    fonts
        .families
        .insert(FontFamily::Proportional, family("hanken-450"));
    fonts
        .families
        .insert(FontFamily::Monospace, family("jbm-400"));
    fonts.families.insert(mono_bold(), family("jbm-700"));
    fonts.families.insert(display(), family("space-700"));
    fonts.families.insert(sans_strong(), family("hanken-600"));

    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut v = Visuals::dark();

    v.panel_fill = BG_950;
    v.window_fill = BG_900;
    v.window_stroke = Stroke::new(1.0, BORDER_HI);
    v.extreme_bg_color = BG_950;
    v.faint_bg_color = BG_925;
    v.code_bg_color = BG_900;
    v.override_text_color = None;
    v.hyperlink_color = ACCENT;
    // Selection is a wash, not solid amber: it also backs selected text, which has to stay
    // readable on top of it.
    v.selection.bg_fill = ACCENT.linear_multiply(0.30);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.error_fg_color = DANGER;
    v.warn_fg_color = ACCENT;
    v.text_cursor.stroke = Stroke::new(1.5, ACCENT);
    v.slider_trailing_fill = true;

    v.window_shadow = Default::default();
    v.popup_shadow = Default::default();
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(3);
        w.expansion = 0.0;
    }
    // `noninteractive.bg_stroke` is also what egui draws between panels.
    v.widgets.noninteractive.bg_fill = BG_925;
    v.widgets.noninteractive.weak_bg_fill = BG_925;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

    // Buttons paint `weak_bg_fill`; rails, check boxes and scrollbar thumbs paint `bg_fill`.
    v.widgets.inactive.weak_bg_fill = BG_900;
    v.widgets.inactive.bg_fill = Color32::from_gray(0x33);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x2a, 0x2a, 0x2a));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);

    v.widgets.hovered.weak_bg_fill = BG_850;
    v.widgets.hovered.bg_fill = Color32::from_gray(0x55);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x44, 0x44, 0x44));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_HI);

    v.widgets.active.weak_bg_fill = BG_850;
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_HI);

    v.widgets.open = v.widgets.active;

    ctx.set_visuals(v);

    ctx.all_styles_mut(|style| {
        use egui::FontFamily::{Monospace, Proportional};
        style.text_styles = [
            (TextStyle::Small, FontId::new(10.0, Proportional)),
            (TextStyle::Body, FontId::new(13.0, Proportional)),
            (TextStyle::Button, FontId::new(12.5, Proportional)),
            (TextStyle::Heading, FontId::new(15.0, display())),
            (TextStyle::Monospace, FontId::new(12.0, Monospace)),
        ]
        .into();
        style.drag_value_text_style = TextStyle::Monospace;
        // A tool, not a document: labels are not text to select.
        style.interaction.selectable_labels = false;

        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(6.0, 5.0);
        s.button_padding = egui::vec2(8.0, 3.0);
        s.interact_size.y = 20.0;
        s.slider_width = 150.0;
        s.slider_rail_height = 3.0;
        s.icon_width = 14.0;
        s.icon_width_inner = 8.0;
        // A real, always-visible scrollbar with its own gutter: the stock "solid" style
        // still hides the handle until hovered, which leaves no way to see or grab it.
        s.scroll = egui::style::ScrollStyle {
            bar_width: 8.0,
            handle_min_length: 28.0,
            bar_inner_margin: 2.0,
            bar_outer_margin: 2.0,
            dormant_handle_opacity: 1.0,
            active_handle_opacity: 1.0,
            interact_handle_opacity: 1.0,
            dormant_background_opacity: 1.0,
            active_background_opacity: 1.0,
            interact_background_opacity: 1.0,
            ..egui::style::ScrollStyle::solid()
        };
    });
}

/// Monospace for anything read as a value rather than as prose.
pub fn num(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).monospace().color(TEXT)
}

pub fn dim(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(TEXT_DIM)
}

/// Panel and section headings: Space Grotesk, uppercase, tracked out.
pub fn head(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into().to_uppercase())
        .family(display())
        .size(11.5)
        .extra_letter_spacing(0.6)
        .color(TEXT_HI)
}

/// Small bold monospace caps: counts, badges, units.
pub fn tag(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into().to_uppercase())
        .family(mono_bold())
        .size(10.5)
        .extra_letter_spacing(0.3)
}
