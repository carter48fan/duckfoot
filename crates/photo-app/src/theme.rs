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

/// A font with Chinese, Japanese and Korean glyphs, from the system, once it is read.
/// None of the bundled faces has them, so a CJK file or folder name would show as boxes.
/// Bundling one would add 16-20 MB per weight, and a subset cannot know which characters
/// people's file names will use, so the system's own is borrowed instead.
///
/// Read on a thread (the files run to tens of MB) and kept for the life of the process:
/// leaked once, so every egui context (one is rebuilt when the renderer is) borrows the
/// same bytes rather than copying them.
static CJK_FONT: std::sync::OnceLock<Option<&'static [u8]>> = std::sync::OnceLock::new();

/// Where each OS keeps a CJK face, most likely first.
fn cjk_font_paths() -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if cfg!(target_os = "windows") {
        let fonts = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into())
            .join("Fonts");
        for f in [
            "msyh.ttc",
            "YuGothR.ttc",
            "meiryo.ttc",
            "malgun.ttf",
            "msgothic.ttc",
        ] {
            paths.push(fonts.join(f));
        }
    } else if cfg!(target_os = "macos") {
        for f in [
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/PingFang.ttc",
        ] {
            paths.push(f.into());
        }
    } else if cfg!(target_os = "android") {
        paths.push("/system/fonts/NotoSansCJK-Regular.ttc".into());
    } else {
        for f in [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        ] {
            paths.push(f.into());
        }
    }
    paths
}

/// The first readable font file among `paths`, if it looks like one: a broken file must
/// not reach the font parser, which would take the whole UI down with it.
fn read_font(paths: &[std::path::PathBuf]) -> Option<Vec<u8>> {
    const MAX: u64 = 64 << 20;
    paths.iter().find_map(|p| {
        let len = std::fs::metadata(p).ok()?.len();
        if len > MAX {
            return None;
        }
        let bytes = std::fs::read(p).ok()?;
        let magic = bytes.get(..4)?;
        let font = matches!(magic, b"ttcf" | b"OTTO" | b"true" | [0, 1, 0, 0]);
        font.then(|| {
            log::info!("CJK fallback font: {}", p.display());
            bytes
        })
    })
}

/// Starts reading the CJK fallback, once per process; `ctx` is asked to repaint when it
/// is in, so [`refresh_fonts`] can install it.
fn load_cjk_font(ctx: &egui::Context) {
    if CJK_FONT.get().is_some() {
        return;
    }
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("duckfoot-fonts".into())
        .spawn(move || {
            CJK_FONT.get_or_init(|| {
                read_font(&cjk_font_paths()).map(|bytes| &*Box::leak(bytes.into_boxed_slice()))
            });
            ctx.request_repaint();
        });
}

/// Installs the CJK fallback into `ctx` once it has been read. Cheap; call every frame.
pub fn refresh_fonts(ctx: &egui::Context) {
    let id = egui::Id::new("duckfoot-cjk-installed");
    if CJK_FONT.get().copied().flatten().is_none() || ctx.data(|d| d.get_temp::<bool>(id)).is_some()
    {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, true));
    install_fonts(ctx);
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
    // geometric symbols the three text faces don't. The system's CJK face, once read,
    // comes last.
    if let Some(bytes) = CJK_FONT.get().copied().flatten() {
        fonts
            .font_data
            .insert("cjk".to_owned(), FontData::from_static(bytes).into());
    }
    let fallbacks: Vec<String> = [
        "Hack",
        "Ubuntu-Light",
        "NotoEmoji-Regular",
        "emoji-icon-font",
        "cjk",
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
    load_cjk_font(ctx);

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Where this machine has a CJK face, it parses and lays out a CJK name. (Skipped
    /// where it has none.)
    #[test]
    fn a_systems_cjk_face_lays_out_cjk_text() {
        let Some(bytes) = read_font(&cjk_font_paths()) else {
            eprintln!("no CJK font on this machine; skipped");
            return;
        };
        let ctx = egui::Context::default();
        let mut fonts = FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".into(), FontData::from_owned(bytes).into());
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .push("cjk".into());
        ctx.set_fonts(fonts);
        let mut width = 0.0;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let galley = ui.painter().layout_no_wrap(
                "写真_2026.ARW".into(),
                FontId::proportional(14.0),
                Color32::WHITE,
            );
            width = galley.size().x;
        });
        out.textures_delta.clear();
        assert!(width > 0.0);
    }

    #[test]
    fn only_a_font_file_is_taken_as_one() {
        let dir = std::env::temp_dir().join(format!("duckfoot-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("junk.ttc");
        std::fs::write(&junk, b"<html>not a font</html>").unwrap();
        let font = dir.join("font.ttf");
        std::fs::write(&font, HANKEN_GROTESK).unwrap();
        assert!(read_font(&[dir.join("missing.ttc"), junk.clone()]).is_none());
        assert_eq!(
            read_font(&[junk, font]).map(|b| b.len()),
            Some(HANKEN_GROTESK.len())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
