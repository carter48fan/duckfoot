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

fn row_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .monospace()
            .size(10.0)
            .extra_letter_spacing(0.6)
            .color(theme::TEXT_FAINT),
    );
}

/// A row of chips, one per choice; returns the one clicked.
fn chips<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    current: T,
    choices: impl IntoIterator<Item = (T, &'static str)>,
) -> Option<T> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for (value, label) in choices {
            if widgets::chip(ui, value == current, label).clicked() && value != current {
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
    let before = *s;
    ui.spacing_mut().item_spacing.y = 6.0;

    row_label(ui, "Format");
    if let Some(f) = chips(ui, s.format, ExportFormat::ALL.map(|f| (f, f.label()))) {
        s.format = f;
    }
    if s.format.is_lossy() {
        let mut q = f32::from(s.quality);
        let tip = if s.format == ExportFormat::Webp {
            "quality (100 = lossless)"
        } else {
            "quality"
        };
        if widgets::param_slider(ui, tip, &mut q, 1.0..=100.0, 0, 92.0).changed() {
            s.quality = q.round().clamp(1.0, 100.0) as u8;
        }
    }
    if s.format.has_16_bit() {
        row_label(ui, "Bits per channel");
        if let Some(d) = chips(
            ui,
            s.depth,
            [(BitDepth::Eight, "8-bit"), (BitDepth::Sixteen, "16-bit")],
        ) {
            s.depth = d;
        }
    }

    row_label(ui, "Colour space");
    use photo_engine::gamut::OutputSpace;
    if let Some(c) = chips(ui, s.color_space, OutputSpace::ALL.map(|c| (c, c.label()))) {
        s.color_space = c;
    }
    if s.color_space != OutputSpace::Srgb {
        ui.label(
            theme::tag(
                "for screens and print that manage colour; the web and most apps assume sRGB",
            )
            .color(theme::TEXT_FAINT),
        );
    }

    row_label(ui, "Size");
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

    row_label(ui, "Output sharpening");
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

    row_label(ui, "Metadata");
    if let Some(m) = chips(ui, s.metadata, MetadataPolicy::ALL.map(|m| (m, m.label()))) {
        s.metadata = m;
    }
    if s.metadata == MetadataPolicy::All {
        ui.horizontal_wrapped(|ui| {
            let mut keep = !s.strip_location;
            if widgets::chip(ui, keep, "include location").clicked() {
                keep = !keep;
                s.strip_location = !keep;
            }
            ui.label(
                theme::dim(if keep {
                    "GPS goes out with the file"
                } else {
                    "GPS is left out"
                })
                .size(11.0),
            );
        });
    }

    ui.label(
        RichText::new(summary(s, source))
            .family(theme::mono_bold())
            .size(11.0)
            .color(theme::ACCENT),
    );
    *s != before
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
    parts.push(format!("{bits} {}", s.color_space.label()));
    if s.sharpen != SharpenFor::Off {
        parts.push(format!(
            "{} sharpening",
            s.sharpen.label().to_lowercase().replace(" paper", "")
        ));
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
            .default_width(380.0)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let mut settings = self.export_settings;
                settings_editor(ui, &mut settings, source);
                self.export_settings = settings;
                ui.add_space(6.0);
                ui.horizontal(|ui| {
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
