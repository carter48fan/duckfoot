//! The export settings, as controls: one panel for the desktop Export dialog, the phone's
//! Export sheet and every batch recipe, so what a recipe can do and what the single export
//! can do are the same list.

use crate::*;
use photo_engine::{
    BitDepth, ExportFormat, ExportSettings, MetadataPolicy, Resize, SharpenAmount, SharpenFor,
};

/// Which kind of resize, without its numbers.
#[derive(Clone, Copy, PartialEq)]
enum SizeKind {
    Original,
    Long,
    Short,
    Box,
    Megapixels,
    Percent,
}

impl SizeKind {
    const ALL: [(SizeKind, &'static str); 6] = [
        (SizeKind::Original, "original"),
        (SizeKind::Long, "long edge"),
        (SizeKind::Short, "short edge"),
        (SizeKind::Box, "fit box"),
        (SizeKind::Megapixels, "megapixels"),
        (SizeKind::Percent, "percent"),
    ];

    fn of(r: Resize) -> Self {
        match r {
            Resize::Original => Self::Original,
            Resize::LongEdge(_) => Self::Long,
            Resize::ShortEdge(_) => Self::Short,
            Resize::FitDimensions { .. } => Self::Box,
            Resize::Megapixels(_) => Self::Megapixels,
            Resize::Percentage(_) => Self::Percent,
        }
    }

    /// The resize this kind starts at when chosen.
    fn start(self) -> Resize {
        match self {
            Self::Original => Resize::Original,
            Self::Long => Resize::LongEdge(2048),
            Self::Short => Resize::ShortEdge(1080),
            Self::Box => Resize::FitDimensions {
                width: 1920,
                height: 1080,
            },
            Self::Megapixels => Resize::Megapixels(12.0),
            Self::Percent => Resize::Percentage(50.0),
        }
    }
}

/// How wide the label column is, when there is room for one.
const LABEL_W: f32 = 92.0;

/// One setting: its name, and its controls. Where the panel is wide enough the name sits
/// in a column of its own, on the first line of chips, so every row's choices start at one
/// edge; where it is not (a phone, a recipe's narrow card), the name goes above them.
fn row(ui: &mut egui::Ui, wide: bool, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let caption = theme::tag(label).color(theme::TEXT_DIM);
    if !wide {
        ui.label(caption);
        add(ui);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        // As tall as a chip, so the name is on its line's midline.
        let chip_h = widgets::sized(ui, 26.0, 34.0);
        ui.allocate_ui_with_layout(
            vec2(LABEL_W, chip_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(vec2(LABEL_W, chip_h));
                ui.label(caption);
            },
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 6.0);
            ui.set_width(ui.available_width());
            add(ui);
        });
    });
}

/// A note under a row's controls.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(theme::dim(text).size(11.0));
}

/// A row of chips, one per choice; returns the one clicked.
fn chips<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: T,
    choices: impl IntoIterator<Item = (T, &'static str)>,
) -> Option<T> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (value, label) in choices {
            if widgets::outlined_chip(ui, true, value == current, label).clicked()
                && value != current
            {
                picked = Some(value);
            }
        }
    });
    picked
}

/// The whole panel. `source` is the photo's size after its crop, when known, for the
/// line saying what will be written. Returns whether anything changed.
pub(crate) fn settings_editor(
    ui: &mut egui::Ui,
    s: &mut ExportSettings,
    source: Option<(u32, u32)>,
) -> bool {
    settings_editor_with(ui, s, source, true, true)
}

/// [`settings_editor`], without the background row when `background` is false: the
/// desktop and the phone remove a background in its own tool, where it can be seen; a
/// batch recipe still can. Without `summary`, the line saying what will be written is
/// left to the caller, to put by its button.
pub(crate) fn settings_editor_with(
    ui: &mut egui::Ui,
    s: &mut ExportSettings,
    source: Option<(u32, u32)>,
    background: bool,
    summary_line: bool,
) -> bool {
    let before = *s;
    ui.spacing_mut().item_spacing.y = 10.0;
    let wide = ui.available_width() >= 360.0;

    row(ui, wide, "format", |ui| {
        if let Some(f) = chips(ui, s.format, ExportFormat::ALL.map(|f| (f, f.label()))) {
            s.format = f;
        }
        if s.format.is_lossy() {
            let mut q = f32::from(s.quality);
            let tip = if s.format.lossless_at_100() {
                "quality (100 = lossless)"
            } else {
                "quality"
            };
            if widgets::param_slider(ui, tip, &mut q, 1.0..=100.0, 0, 92.0).changed() {
                s.quality = q.round().clamp(1.0, 100.0) as u8;
            }
        }
    });
    if s.format.has_16_bit() {
        row(ui, wide, "bit depth", |ui| {
            if let Some(d) = chips(
                ui,
                s.depth,
                [(BitDepth::Eight, "8-bit"), (BitDepth::Sixteen, "16-bit")],
            ) {
                s.depth = d;
            }
        });
    }

    use photo_engine::gamut::OutputSpace;
    row(ui, wide, "colour", |ui| {
        let format = s.format;
        let spaces = OutputSpace::ALL
            .into_iter()
            .filter(|c| format.holds(*c))
            .map(|c| (c, c.label()));
        if let Some(c) = chips(ui, s.effective_space(), spaces) {
            s.color_space = c;
        }
        if s.format == ExportFormat::Avif {
            note(
                ui,
                "AVIF is written in sRGB: its encoder can tag no other space.",
            );
        } else if s.format == ExportFormat::Jxl && s.color_space == OutputSpace::AdobeRgb {
            note(
                ui,
                "Written lossless: JPEG XL's lossy mode cannot describe Adobe RGB.",
            );
        } else if s.color_space != OutputSpace::Srgb {
            note(
                ui,
                "For screens and print that manage colour; the web and most apps assume sRGB.",
            );
        }
    });

    row(ui, wide, "size", |ui| {
        let kind = SizeKind::of(s.sizing);
        if let Some(k) = chips(ui, kind, SizeKind::ALL) {
            s.sizing = k.start();
        }
        let drag_u32 = |ui: &mut egui::Ui, v: &mut u32, suffix: &str| {
            ui.add(
                egui::DragValue::new(v)
                    .range(16..=65535)
                    .speed(8.0)
                    .suffix(suffix)
                    .update_while_editing(false),
            )
        };
        if s.sizing != Resize::Original {
            ui.horizontal(|ui| match &mut s.sizing {
                Resize::Original => {}
                Resize::LongEdge(v) | Resize::ShortEdge(v) => {
                    drag_u32(ui, v, " px");
                }
                Resize::FitDimensions { width, height } => {
                    drag_u32(ui, width, " px");
                    ui.label(theme::dim("×"));
                    drag_u32(ui, height, " px");
                }
                Resize::Megapixels(v) => {
                    ui.add(
                        egui::DragValue::new(v)
                            .range(0.1..=400.0)
                            .speed(0.1)
                            .fixed_decimals(1)
                            .suffix(" MP")
                            .update_while_editing(false),
                    );
                }
                Resize::Percentage(v) => {
                    ui.add(
                        egui::DragValue::new(v)
                            .range(1.0..=100.0)
                            .speed(0.5)
                            .fixed_decimals(0)
                            .suffix(" %")
                            .update_while_editing(false),
                    );
                }
            });
        }
    });

    row(ui, wide, "sharpen for", |ui| {
        if let Some(f) = chips(ui, s.sharpen, SharpenFor::ALL.map(|f| (f, f.label()))) {
            s.sharpen = f;
        }
        if s.sharpen != SharpenFor::Off {
            if let Some(a) = chips(
                ui,
                s.sharpen_amount,
                SharpenAmount::ALL.map(|a| (a, a.label())),
            ) {
                s.sharpen_amount = a;
            }
        }
    });

    row(ui, wide, "metadata", |ui| {
        if let Some(m) = chips(ui, s.metadata, MetadataPolicy::ALL.map(|m| (m, m.label()))) {
            s.metadata = m;
        }
        if s.metadata == MetadataPolicy::All {
            let mut keep = !s.strip_location;
            if widgets::outlined_chip(ui, true, keep, "include location")
                .on_hover_text(if keep {
                    "GPS goes out with the file"
                } else {
                    "GPS is left out"
                })
                .clicked()
            {
                keep = !keep;
                s.strip_location = !keep;
            }
        }
    });

    if background {
        row(ui, wide, "background", |ui| {
            if widgets::outlined_chip(ui, true, s.remove_background, "remove background").clicked()
            {
                s.remove_background = !s.remove_background;
            }
            if s.remove_background {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    let is_white = s.matte_color == Some([255, 255, 255]);
                    let is_trans = s.matte_color.is_none();
                    if widgets::outlined_chip(ui, true, is_trans, "transparent").clicked() {
                        s.matte_color = None;
                    }
                    if widgets::outlined_chip(ui, true, is_white, "on white").clicked() {
                        s.matte_color = Some([255, 255, 255]);
                    }
                    if widgets::outlined_chip(
                        ui,
                        true,
                        s.cutout_params.defringe_white,
                        "defringe white",
                    )
                    .clicked()
                    {
                        s.cutout_params.defringe_white = !s.cutout_params.defringe_white;
                    }
                });
                widgets::param_slider(
                    ui,
                    "threshold",
                    &mut s.cutout_params.threshold,
                    0.05..=0.95,
                    2,
                    0.50,
                );
                let mut shift_f = s.cutout_params.edge_shift as f32;
                if widgets::param_slider(ui, "edge shift", &mut shift_f, -10.0..=10.0, 0, 0.0)
                    .changed()
                {
                    s.cutout_params.edge_shift = shift_f.round() as i32;
                }
            }
        });
    }

    if s.format == ExportFormat::Jpeg {
        row(ui, wide, "hdr", |ui| {
            let hover = if s.remove_background {
                "Not with the background removed: the map would brighten the matte"
            } else {
                "A gain map after the JPEG (ISO 21496-1, Ultra HDR): an HDR screen shows \
                 what the export takes to white up to two stops brighter; anything else \
                 shows the JPEG as it is"
            };
            let on = s.wants_gain_map();
            if widgets::outlined_chip(ui, !s.remove_background, on, "gain map")
                .on_hover_text(hover)
                .clicked()
            {
                s.hdr_gain_map = !s.hdr_gain_map;
            }
        });
    }

    if summary_line {
        summary_label(ui, s, source);
    }
    *s != before
}

/// What will be written, in one line: under the settings, or beside the button that
/// writes it.
pub(crate) fn summary_label(ui: &mut egui::Ui, s: &ExportSettings, source: Option<(u32, u32)>) {
    ui.label(
        RichText::new(summary(s, source))
            .family(theme::mono_bold())
            .size(11.0)
            .color(theme::ACCENT),
    );
}

/// "2048 × 1365 · JPEG q85 · 8-bit sRGB · screen sharpening · all metadata, no location".
pub(crate) fn summary(s: &ExportSettings, source: Option<(u32, u32)>) -> String {
    let mut parts = Vec::new();
    match source {
        Some((w, h)) => {
            let (tw, th) = s.sizing.target(w, h);
            parts.push(format!("{tw} × {th}"));
        }
        None if s.sizing != Resize::Original => parts.push("resized".into()),
        None => parts.push("full size".into()),
    }
    parts.push(if s.format.is_lossy() {
        format!("{} q{}", s.format.label(), s.quality)
    } else {
        s.format.label().to_string()
    });
    let bits = match s.effective_depth() {
        BitDepth::Eight => "8-bit",
        BitDepth::Sixteen => "16-bit",
    };
    parts.push(format!("{bits} {}", s.effective_space().label()));
    if s.sharpen != SharpenFor::Off {
        parts.push(format!(
            "{} sharpening",
            s.sharpen.label().to_lowercase().replace(" paper", "")
        ));
    }
    if s.remove_background {
        if let Some([r, g, b]) = s.matte_color {
            parts.push(format!("rembg matte #{:02x}{:02x}{:02x}", r, g, b));
        } else {
            parts.push("rembg cutout".into());
        }
    }
    if s.wants_gain_map() {
        parts.push("HDR gain map".into());
    }
    parts.push(match s.metadata {
        MetadataPolicy::All if s.strip_location => "metadata, no location".into(),
        MetadataPolicy::All => "all metadata".into(),
        MetadataPolicy::CopyrightOnly => "copyright only".into(),
        MetadataPolicy::None => "no metadata".into(),
    });
    parts.join(" · ")
}

impl PhotoApp {
    /// The open photo's size as it will export before any resize: after turns and crop.
    pub(crate) fn export_source_size(&self) -> Option<(u32, u32)> {
        let image = self.ready_image()?;
        let (w, h) = self
            .stack
            .geometry
            .output_size(image.sensor_width, image.sensor_height);
        let (_, _, cw, ch) = self.stack.geometry.crop.sanitised().to_pixels(w, h);
        Some((cw, ch))
    }

    /// The desktop Export dialog: the settings, then where to write.
    pub(crate) fn export_dialog(&mut self, ctx: &egui::Context) {
        if !self.export_dialog_open {
            return;
        }
        let mut open = true;
        let mut go = false;
        let source = self.export_source_size();
        egui::Window::new("Export")
            .open(&mut open)
            .fade_in(false)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                let mut settings = self.export_settings;
                // The background comes off in its own tool, where it can be seen.
                settings_editor_with(ui, &mut settings, source, false, false);
                self.export_settings = settings;
                ui.add_space(4.0);
                ui.separator();
                summary_label(ui, &self.export_settings, source);
                ui.add_space(2.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let ready = self.ready_image().is_some()
                        && !matches!(self.export, Export::Writing { .. });
                    if widgets::button_enabled(ui, ready, Kind::Primary, "Export…", 120.0)
                        .on_hover_text("Choose where to write it")
                        .clicked()
                    {
                        go = true;
                    }
                    if widgets::button(ui, Kind::Secondary, "Cancel", 0.0).clicked() {
                        self.export_dialog_open = false;
                    }
                });
            });
        if !open {
            self.export_dialog_open = false;
        }
        if go {
            self.export_dialog_open = false;
            self.pick_and_export(ctx);
        }
    }
}
