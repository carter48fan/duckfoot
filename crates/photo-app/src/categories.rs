//! The phone's edit controls, grouped: five categories along the bottom bar, each a
//! row of chips, and one ruler that turns whichever value the chosen chip names. A chip
//! is a value (a [`Knob`](crate::knobs::Knob)) or a tool whose controls are not values
//! along a line — the curve, the colour bands, masks, spots — which opens in place of the
//! ruler. Every module is reachable from exactly the category that lists it.

use egui::{pos2, vec2, Align2, Color32, FontId, Painter, Rect, Sense, Stroke};

use crate::knobs::knobs;
use crate::{theme, widgets, InspectorSectionId};

/// The bottom bar's groups, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    Crop,
    #[default]
    Light,
    Colour,
    Detail,
    Retouch,
}

/// One thing a category's row offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    /// A module's value, by its index in [`knobs`]: the ruler turns it.
    Knob(InspectorSectionId, usize),
    /// A module's own controls, in place of the ruler.
    Tool(InspectorSectionId),
    /// One-tap looks, snapshots and history.
    Looks,
    /// Removing the background.
    Cutout,
}

use Chip::{Cutout, Knob, Looks, Tool};
use InspectorSectionId as M;

const CROP: &[Chip] = &[
    Tool(M::Geometry),
    Knob(M::Geometry, 0),
    Knob(M::Geometry, 1),
    Knob(M::Geometry, 2),
];

const LIGHT: &[Chip] = &[
    Knob(M::Exposure, 0),
    Knob(M::Tonal, 0),
    Knob(M::Tonal, 1),
    Knob(M::Tonal, 2),
    Knob(M::Tonal, 3),
    Knob(M::Dehaze, 0),
    Knob(M::Vignette, 0),
    Knob(M::Vignette, 1),
    Knob(M::Exposure, 1),
    Tool(M::ToneCurve),
    Tool(M::Filmic3D),
    Tool(M::Scope),
];

const COLOUR: &[Chip] = &[
    Knob(M::WhiteBalance, 0),
    Knob(M::WhiteBalance, 1),
    Knob(M::Vibrance, 0),
    Knob(M::Vibrance, 1),
    Looks,
    Tool(M::ColorEq),
    Tool(M::ColorBalance),
    Tool(M::Look),
];

const DETAIL: &[Chip] = &[
    Knob(M::Clarity, 0),
    Knob(M::Clarity, 1),
    Knob(M::Sharpen, 0),
    Knob(M::Denoise, 0),
    Knob(M::Denoise, 2),
    Knob(M::Lens, 0),
];

const RETOUCH: &[Chip] = &[Tool(M::Heal), Tool(M::Local), Cutout];

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Crop,
        Category::Light,
        Category::Colour,
        Category::Detail,
        Category::Retouch,
    ];

    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Category::Crop => "cat-crop",
            Category::Light => "cat-light",
            Category::Colour => "cat-colour",
            Category::Detail => "cat-detail",
            Category::Retouch => "cat-retouch",
        })
    }

    /// What its row offers, in order.
    pub fn chips(self) -> &'static [Chip] {
        match self {
            Category::Crop => CROP,
            Category::Light => LIGHT,
            Category::Colour => COLOUR,
            Category::Detail => DETAIL,
            Category::Retouch => RETOUCH,
        }
    }

    pub(crate) fn index(self) -> usize {
        Category::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    /// The category `step` places along, if there is one.
    pub(crate) fn offset(self, step: i32) -> Option<Category> {
        let i = self.index() as i32 + step;
        Category::ALL.get(usize::try_from(i).ok()?).copied()
    }

    /// The first value in its row: what it shows when nothing in it was chosen before.
    pub(crate) fn first_knob(self) -> Option<Chip> {
        self.chips()
            .iter()
            .copied()
            .find(|c| matches!(c, Chip::Knob(..)))
    }

    /// The category that lists `module`.
    pub fn of(module: InspectorSectionId) -> Category {
        Category::ALL
            .into_iter()
            .find(|c| c.chips().iter().any(|chip| chip.module() == Some(module)))
            .unwrap_or_default()
    }
}

impl Chip {
    /// The module it belongs to, if it is one.
    pub fn module(self) -> Option<InspectorSectionId> {
        match self {
            Chip::Knob(id, _) | Chip::Tool(id) => Some(id),
            Chip::Looks | Chip::Cutout => None,
        }
    }

    /// Its name on the chip and over the ruler. A knob's own name is enough inside its
    /// module's card; out on its own in a row it needs the module's.
    pub fn label(self) -> &'static str {
        match self {
            Chip::Knob(id, i) => match (id, i) {
                (M::Exposure, 0) => "exposure",
                (M::Exposure, 1) => "black point",
                (M::WhiteBalance, 0) => "temperature",
                (M::Dehaze, 0) => "dehaze",
                (M::Vignette, 0) => "vignette",
                (M::Vignette, 1) => "vignette size",
                (M::Sharpen, 0) => "sharpen",
                (M::Denoise, 0) => "noise",
                (M::Denoise, 2) => "colour noise",
                (M::Lens, 0) => "distortion",
                _ => knobs(id).get(i).map_or("", |k| k.name),
            },
            Chip::Tool(id) => id.title(),
            Chip::Looks => crate::i18n::tr("tab-presets"),
            Chip::Cutout => "remove background",
        }
    }

    /// What follows the value in the readout.
    pub(crate) fn unit(self) -> &'static str {
        match self {
            Chip::Knob(M::Exposure, 0) => " EV",
            Chip::Knob(M::WhiteBalance, 0) => " K",
            Chip::Knob(M::Geometry, 0) => "°",
            _ => "",
        }
    }

    /// Whether it opens controls of its own rather than turning the ruler.
    pub fn is_tool(self) -> bool {
        !matches!(self, Chip::Knob(..))
    }
}

/// What a drag along the ruler did this frame.
#[derive(Default)]
pub(crate) struct Turned {
    /// The value it asks for, when it moved.
    pub value: Option<f32>,
    /// A gesture began: one undo step from here.
    pub started: bool,
}

/// How many widths of ruler the whole range spans: enough that a finger's travel is a
/// fine change, and the whole range is two sweeps away.
const SPAN: f32 = 2.2;

/// A ruler of ticks under a fixed amber needle. Dragging moves the ticks with the finger,
/// so the value under the needle follows: left for more, right for less, as a dial turned
/// by its edge. A double tap puts it back to `home`. `fine` moves it a tenth as far.
pub(crate) fn ruler(
    ui: &mut egui::Ui,
    name: &str,
    value: f32,
    range: (f32, f32),
    home: f32,
    fine: bool,
) -> Turned {
    let (lo, hi) = range;
    let size = vec2(ui.available_width(), RULER_H);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::slider(true, value as f64, name)
    });
    let mut out = Turned::default();
    let span = (rect.width() * SPAN).max(1.0);
    let per_point = (hi - lo) / span * if fine { 0.1 } else { 1.0 };
    if response.drag_started() {
        out.started = true;
    }
    if response.dragged() {
        let dx = response.drag_delta().x;
        if dx != 0.0 {
            out.value = Some((value - dx * per_point).clamp(lo, hi));
        }
    }
    if response.double_clicked() {
        out.started = true;
        out.value = Some(home);
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    let painter = ui.painter().with_clip_rect(rect);
    let centre = rect.center().x;
    // Where a value sits, the needle's being `value`.
    let x_of = |v: f32| centre + (v - value) / (hi - lo).max(f32::EPSILON) * span;
    let ticks = 60;
    let base = rect.bottom() - 14.0;
    for i in 0..=ticks {
        let v = lo + (hi - lo) * i as f32 / ticks as f32;
        let x = x_of(v);
        if x < rect.left() - 1.0 || x > rect.right() + 1.0 {
            continue;
        }
        // Fading towards the ends, so the needle reads as the middle of something longer.
        let fade = 1.0
            - ((x - centre).abs() / (rect.width() / 2.0))
                .clamp(0.0, 1.0)
                .powi(2);
        let major = i % 5 == 0;
        let (h, grey) = if major { (18.0, 0x8a) } else { (10.0, 0x4a) };
        let colour = Color32::from_gray(grey).gamma_multiply(fade);
        painter.vline(x, (base - h)..=base, Stroke::new(1.0, colour));
    }
    // Home: a taller white tick, where a double tap returns.
    let hx = x_of(home);
    if (rect.left()..=rect.right()).contains(&hx) {
        let fade = 1.0
            - ((hx - centre).abs() / (rect.width() / 2.0))
                .clamp(0.0, 1.0)
                .powi(2);
        painter.vline(
            hx,
            (base - 26.0)..=base,
            Stroke::new(1.5, theme::TEXT_HI.gamma_multiply(fade)),
        );
    }
    // The needle.
    let needle = Rect::from_center_size(pos2(centre, base - 13.0), vec2(2.5, 38.0));
    painter.rect_filled(needle, 1.25, theme::ACCENT);
    out
}

/// The ruler's height.
pub const RULER_H: f32 = 56.0;
/// A chip's height, and its row's.
pub const CHIP_H: f32 = 36.0;
pub const CHIPS_H: f32 = 46.0;

/// One chip in a category's row: a pill, filled amber while chosen, with a dot when what
/// it names has been changed from how the photo opened.
pub(crate) fn chip(
    ui: &mut egui::Ui,
    label: &str,
    chosen: bool,
    changed: bool,
    tool: bool,
) -> egui::Response {
    let font = FontId::new(14.0, theme::sans_strong());
    let text = if tool {
        format!("{label}  ›")
    } else {
        label.to_string()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text, font, Color32::PLACEHOLDER);
    let dot = if changed { 10.0 } else { 0.0 };
    let w = galley.size().x + 28.0 + dot;
    let (rect, response) = ui.allocate_exact_size(vec2(w, CHIP_H), Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, chosen, label)
    });
    let painter = ui.painter();
    let r = CHIP_H / 2.0;
    let (fill, ink) = if chosen {
        (theme::ACCENT, Color32::BLACK)
    } else if response.hovered() {
        (theme::BG_850, theme::TEXT_HI)
    } else {
        (Color32::TRANSPARENT, theme::TEXT)
    };
    painter.rect_filled(rect, r, fill);
    if !chosen {
        painter.rect_stroke(
            rect,
            r,
            Stroke::new(1.0, theme::BORDER_HI),
            egui::StrokeKind::Inside,
        );
    }
    let text_at = pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0);
    painter.galley(text_at, galley, ink);
    if changed {
        let c = pos2(rect.right() - 14.0, rect.center().y);
        painter.circle_filled(
            c,
            3.0,
            if chosen {
                Color32::BLACK
            } else {
                theme::ACCENT
            },
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A category's icon, in strokes: crop corners, a sun, a drop, a focus frame, a wand.
pub(crate) fn paint_icon(painter: &Painter, c: egui::Pos2, cat: Category, colour: Color32) {
    let stroke = Stroke::new(1.9, colour);
    let line = |a: egui::Vec2, b: egui::Vec2| painter.line_segment([c + a, c + b], stroke);
    match cat {
        Category::Crop => {
            line(vec2(-6.0, -11.0), vec2(-6.0, 6.0));
            line(vec2(-6.0, 6.0), vec2(11.0, 6.0));
            line(vec2(-11.0, -6.0), vec2(6.0, -6.0));
            line(vec2(6.0, -6.0), vec2(6.0, 11.0));
        }
        Category::Light => {
            painter.circle_stroke(c, 4.5, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                line(d * 8.0, d * 11.0);
            }
        }
        Category::Colour => {
            // A drop: a circle's lower part, rising to a point.
            let r = 6.5;
            let centre = c + vec2(0.0, 3.0);
            let tip = c + vec2(0.0, -11.0);
            let steps = 20;
            // The circle's points from where the tangents from the tip touch it.
            let from = -30f32.to_radians();
            let to = 210f32.to_radians();
            let mut points = vec![tip];
            for i in 0..=steps {
                let a = from + (to - from) * i as f32 / steps as f32;
                points.push(centre + vec2(a.cos(), a.sin()) * r);
            }
            points.push(tip);
            painter.add(egui::Shape::line(points, stroke));
        }
        Category::Detail => {
            painter.circle_stroke(c, 3.5, stroke);
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let corner = vec2(10.0 * sx, 10.0 * sy);
                line(corner, corner - vec2(5.0 * sx, 0.0));
                line(corner, corner - vec2(0.0, 5.0 * sy));
            }
        }
        Category::Retouch => {
            line(vec2(-10.0, 10.0), vec2(3.0, -3.0));
            for (a, b) in [
                (vec2(7.0, -11.0), vec2(7.0, -8.0)),
                (vec2(7.0, -2.0), vec2(7.0, 1.0)),
                (vec2(1.0, -6.0), vec2(4.0, -6.0)),
                (vec2(10.0, -6.0), vec2(13.0, -6.0)),
            ] {
                line(a, b);
            }
        }
    }
}

/// A label centred under an icon, for the bar's cells.
pub(crate) fn paint_cell(painter: &Painter, cell: Rect, cat: Category, colour: Color32) {
    paint_icon(
        painter,
        pos2(cell.center().x, cell.center().y - 8.0),
        cat,
        colour,
    );
    painter.text(
        pos2(cell.center().x, cell.center().y + 16.0),
        Align2::CENTER_CENTER,
        cat.label().to_uppercase(),
        FontId::new(11.0, theme::mono_bold()),
        colour,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every module is in exactly one category, and every knob a chip names exists.
    #[test]
    fn every_module_has_one_home_and_every_chip_is_real() {
        for &id in InspectorSectionId::all() {
            let homes = Category::ALL
                .into_iter()
                .filter(|c| c.chips().iter().any(|chip| chip.module() == Some(id)))
                .count();
            assert_eq!(homes, 1, "{id:?} is listed in {homes} categories");
        }
        for cat in Category::ALL {
            for chip in cat.chips() {
                if let Chip::Knob(id, i) = *chip {
                    assert!(i < knobs(id).len(), "{chip:?} names no knob");
                }
                assert!(!chip.label().is_empty(), "{chip:?} has no name");
            }
        }
    }
}
