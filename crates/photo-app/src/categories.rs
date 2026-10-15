//! The phone's edit controls, grouped after Snapseed: four groups along the bottom bar.
//!
//! - **Looks**: one-tap looks, snapshots, history and copy & paste.
//! - **Tune**: every quick value, in three lists: Light, Colour and Detail. One ruler turns
//!   whichever is chosen; swiping up or down (on the photo, or on the ruler) walks the
//!   chosen value's list, and tapping a section moves to another.
//! - **Crop**: crop & rotate, then straighten, perspective and lens distortion on the
//!   ruler.
//! - **Tools**: the tools whose controls are not values along a line — the curve, the
//!   colour bands and wheels, masks, healing, removing the background — as a grid.
//!
//! A chip is a value (a [`Knob`](crate::knobs::Knob)) or a tool which opens in place of
//! the ruler. Every module is reachable from exactly the group that lists it.

use egui::{pos2, vec2, Align2, Color32, FontId, Painter, Rect, Sense, Stroke};

use crate::knobs::knobs;
use crate::{theme, widgets, InspectorSectionId};

/// The bottom bar's groups, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    Looks,
    #[default]
    Tune,
    Crop,
    Tools,
}

/// One thing a group offers.
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

const LOOKS: &[Chip] = &[Looks];

/// Tune's values, in order. [`TUNE_SECTIONS`] names its stretches, each its own swipe list.
const TUNE: &[Chip] = &[
    // Light
    Knob(M::Exposure, 0),
    Knob(M::Tonal, 0),
    Knob(M::Tonal, 1),
    Knob(M::Tonal, 2),
    Knob(M::Tonal, 3),
    Knob(M::Dehaze, 0),
    Knob(M::Vignette, 0),
    // Colour
    Knob(M::WhiteBalance, 0),
    Knob(M::WhiteBalance, 1),
    Knob(M::Vibrance, 0),
    Knob(M::Vibrance, 1),
    // Detail
    Knob(M::Clarity, 0),
    Knob(M::Clarity, 1),
    Knob(M::Sharpen, 0),
    Knob(M::Denoise, 0),
    Knob(M::Denoise, 2),
];

/// Tune's stretches: a name, and where in [`TUNE`] it starts.
const TUNE_SECTIONS: &[(&str, usize)] = &[("light", 0), ("colour", 7), ("detail", 11)];

const CROP: &[Chip] = &[
    Tool(M::Geometry),
    Knob(M::Geometry, 0),
    Knob(M::Geometry, 1),
    Knob(M::Geometry, 2),
    Knob(M::Lens, 0),
];

/// Crop's stretches: the crop itself (a tool), then the values that square the photo up.
const CROP_SECTIONS: &[(&str, usize)] = &[
    ("crop", 0),
    ("straighten", 1),
    ("perspective", 2),
    ("lens", 4),
];

const TOOLS: &[Chip] = &[
    Tool(M::ToneCurve),
    Tool(M::ColorEq),
    Tool(M::ColorBalance),
    Tool(M::Local),
    Tool(M::Heal),
    Cutout,
    Tool(M::Look),
    Tool(M::Filmic3D),
    Tool(M::Scope),
];

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Looks,
        Category::Tune,
        Category::Crop,
        Category::Tools,
    ];

    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Category::Looks => "cat-looks",
            Category::Tune => "cat-tune",
            Category::Crop => "cat-crop",
            Category::Tools => "cat-tools",
        })
    }

    /// What it offers, in order.
    pub fn chips(self) -> &'static [Chip] {
        match self {
            Category::Looks => LOOKS,
            Category::Tune => TUNE,
            Category::Crop => CROP,
            Category::Tools => TOOLS,
        }
    }

    /// Its named stretches, as (name, first chip's index): shown in place of a row of
    /// chips when it has them.
    pub fn sections(self) -> &'static [(&'static str, usize)] {
        match self {
            Category::Tune => TUNE_SECTIONS,
            Category::Crop => CROP_SECTIONS,
            _ => &[],
        }
    }

    /// Which of [`Self::sections`] the chip at `i` is in.
    pub(crate) fn section_of(self, i: usize) -> usize {
        self.sections()
            .iter()
            .rposition(|(_, start)| *start <= i)
            .unwrap_or(0)
    }

    /// Where section `s` runs in [`Self::chips`].
    pub(crate) fn section_range(self, s: usize) -> std::ops::Range<usize> {
        let sections = self.sections();
        let start = sections.get(s).map_or(0, |(_, i)| *i);
        let end = sections.get(s + 1).map_or(self.chips().len(), |(_, i)| *i);
        start..end
    }

    /// Whether it shows its tools as a grid, with no ruler.
    pub fn is_grid(self) -> bool {
        self == Category::Tools
    }

    pub(crate) fn index(self) -> usize {
        Category::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    /// The category `step` places along, if there is one.
    pub(crate) fn offset(self, step: i32) -> Option<Category> {
        let i = self.index() as i32 + step;
        Category::ALL.get(usize::try_from(i).ok()?).copied()
    }

    /// The first value it lists: what it shows when nothing in it was chosen before.
    pub(crate) fn first_knob(self) -> Option<Chip> {
        self.chips()
            .iter()
            .copied()
            .find(|c| matches!(c, Chip::Knob(..)))
    }

    /// Whether a swipe up or down stays inside the section it starts in: Tune's Light,
    /// Colour and Detail are each a list of their own. Crop's sections are a value or two
    /// each, so its swipe walks the whole group.
    pub(crate) fn swipes_by_section(self) -> bool {
        self == Category::Tune
    }

    /// The values a swipe up or down walks from the chip at `at`, in order: its section's
    /// when [`Self::swipes_by_section`], else the whole group's.
    pub(crate) fn knob_list(self, at: usize) -> Vec<(InspectorSectionId, usize)> {
        let range = if self.swipes_by_section() {
            self.section_range(self.section_of(at))
        } else {
            0..self.chips().len()
        };
        self.chips()[range]
            .iter()
            .filter_map(|c| match *c {
                Chip::Knob(id, i) => Some((id, i)),
                _ => None,
            })
            .collect()
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
    /// module's card; out on its own in a list it needs the module's.
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
                (M::Look, 0) => "look strength",
                _ => knobs(id).get(i).map_or("", |k| k.name),
            },
            Chip::Tool(id) => id.title(),
            Chip::Looks => crate::i18n::tr("tab-presets"),
            Chip::Cutout => "remove background",
        }
    }

    /// A tool's name and what it does, on its tile in the Tools grid.
    pub(crate) fn tile(self) -> (&'static str, &'static str) {
        match self {
            Chip::Tool(M::ToneCurve) => ("curves", "tone, by channel"),
            Chip::Tool(M::ColorEq) => ("colour mixer", "each hue on its own"),
            Chip::Tool(M::ColorBalance) => ("colour grade", "tint the tones"),
            Chip::Tool(M::Local) => ("brush & masks", "edit one part"),
            Chip::Tool(M::Heal) => ("healing", "take out spots"),
            Chip::Cutout => ("remove bg", "cut the subject out"),
            Chip::Tool(M::Look) => ("LUT", "a .cube look file"),
            Chip::Tool(M::Filmic3D) => ("filmic", "tone mapping"),
            Chip::Tool(M::Scope) => ("scope", "histogram"),
            Chip::Tool(M::Geometry) => ("crop & rotate", "frame, turn, flip"),
            other => (other.label(), ""),
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
    /// Steps along the list a drag down (-) or up (+) asked for.
    pub choose: i32,
    /// A drag up or down is under way: the list of values is worth showing.
    pub choosing: bool,
    /// Steps between the list's tabs a drift right (-) or left (+) during it asked for.
    pub tab: i32,
}

/// Travel before a drag on the ruler is taken as sideways (turning) or up and down
/// (choosing), how far up or down moves the choice by one, and how far sideways while
/// choosing moves to the next tab.
const DECIDE: f32 = 8.0;
const CHOOSE_STEP: f32 = 28.0;
const TAB_STEP: f32 = 56.0;

/// How many widths of ruler the whole range spans: enough that a finger's travel is a
/// fine change, and the whole range is two sweeps away.
const SPAN: f32 = 2.2;

/// A ruler of ticks under a fixed amber needle. Dragging moves the ticks with the finger,
/// so the value under the needle follows: left for more, right for less, as a dial turned
/// by its edge. Dragging up or down instead chooses another value, as on the photo, so the
/// thumb never has to leave the ruler; drifting sideways during that moves between the
/// list's tabs. A double tap puts it back to `home`. `fine` moves
/// it a tenth as far.
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
    // Which way this drag goes, once it has gone far enough to tell, and its travel so far.
    let state_id = response.id.with("axis");
    if response.dragged() {
        let d = response.drag_delta();
        let (axis, mut travel): (Option<bool>, egui::Vec2) = ui
            .data(|m| m.get_temp(state_id))
            .unwrap_or((None, egui::Vec2::ZERO));
        travel += d;
        let axis =
            axis.or_else(|| (travel.length() >= DECIDE).then(|| travel.x.abs() >= travel.y.abs()));
        match axis {
            Some(true) => {
                if travel.x != 0.0 {
                    // The undo step opens with the first change, not the touch.
                    if !ui.data(|m| m.get_temp::<bool>(state_id.with("turned")).unwrap_or(false)) {
                        out.started = true;
                        ui.data_mut(|m| m.insert_temp(state_id.with("turned"), true));
                    }
                    out.value = Some((value - travel.x * per_point).clamp(lo, hi));
                }
                travel.x = 0.0;
            }
            Some(false) => {
                out.choosing = true;
                // Up for the next value, as a list is scrolled: the finger pushes the
                // list up and what was below comes into place.
                while travel.y.abs() >= CHOOSE_STEP {
                    let step = travel.y.signum();
                    out.choose -= step as i32;
                    travel.y -= CHOOSE_STEP * step;
                }
                // Drifting sideways meanwhile: the next tab to the left, as pages are swiped.
                while travel.x.abs() >= TAB_STEP {
                    let step = travel.x.signum();
                    out.tab -= step as i32;
                    travel.x -= TAB_STEP * step;
                }
            }
            None => {}
        }
        ui.data_mut(|m| m.insert_temp(state_id, (axis, travel)));
    }
    if response.drag_stopped() {
        ui.data_mut(|m| {
            m.remove_temp::<(Option<bool>, egui::Vec2)>(state_id);
            m.remove_temp::<bool>(state_id.with("turned"));
        });
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

/// A group's icon, in strokes: overlapping frames, three sliders, crop corners, a grid.
pub(crate) fn paint_icon(painter: &Painter, c: egui::Pos2, cat: Category, colour: Color32) {
    let stroke = Stroke::new(1.9, colour);
    let line = |a: egui::Vec2, b: egui::Vec2| painter.line_segment([c + a, c + b], stroke);
    match cat {
        Category::Looks => {
            // Two frames, one over the other: a look laid over the photo.
            for (o, r) in [(vec2(-3.0, -3.0), 7.5), (vec2(3.0, 3.0), 7.5)] {
                painter.rect_stroke(
                    Rect::from_center_size(c + o, vec2(r * 2.0, r * 2.0)),
                    2.0,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
        }
        Category::Tune => {
            // Three sliders, their knobs at different places.
            for (y, x) in [(-7.0, 4.0), (0.0, -4.0), (7.0, 1.0)] {
                line(vec2(-11.0, y), vec2(11.0, y));
                painter.circle_filled(c + vec2(x, y), 3.0, colour);
            }
        }
        Category::Crop => {
            line(vec2(-6.0, -11.0), vec2(-6.0, 6.0));
            line(vec2(-6.0, 6.0), vec2(11.0, 6.0));
            line(vec2(-11.0, -6.0), vec2(6.0, -6.0));
            line(vec2(6.0, -6.0), vec2(6.0, 11.0));
        }
        Category::Tools => {
            for (x, y) in [(-5.0, -5.0), (5.0, -5.0), (-5.0, 5.0), (5.0, 5.0)] {
                painter.rect_stroke(
                    Rect::from_center_size(c + vec2(x, y), vec2(7.5, 7.5)),
                    1.5,
                    stroke,
                    egui::StrokeKind::Middle,
                );
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

/// A group's sections side by side in place of a row of chips: each its name over a dot
/// per value, the chosen value's dot amber and large, a changed one's amber. The section
/// the chosen value is in is lit. Nothing to scroll: every value of the group is a dot in
/// one row, and a swipe along the row steps through them. Returns the section tapped.
pub(crate) fn section_bar(
    ui: &mut egui::Ui,
    rect: Rect,
    cat: Category,
    chosen: Option<usize>,
    changed: &[bool],
) -> Option<usize> {
    let sections = cat.sections();
    let n = sections.len().max(1) as f32;
    let w = rect.width() / n;
    let mut out = None;
    let lit = chosen.map(|i| cat.section_of(i));
    for (s, (name, _)) in sections.iter().enumerate() {
        let cell = Rect::from_min_size(
            pos2(rect.left() + w * s as f32, rect.top()),
            vec2(w, rect.height()),
        );
        let response = ui.interact(cell, ui.id().with(("section", s)), Sense::click());
        let on = lit == Some(s);
        widgets::describe(ui, &response, cell, || {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, *name)
        });
        if response.clicked() {
            out = Some(s);
        }
        let painter = ui.painter();
        let ink = if on {
            theme::TEXT_HI
        } else if response.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };
        painter.text(
            pos2(cell.center().x, cell.center().y - 7.0),
            Align2::CENTER_CENTER,
            name.to_uppercase(),
            FontId::new(11.5, theme::mono_bold()),
            ink,
        );
        let range = cat.section_range(s);
        let gap = 9.0;
        let span = gap * (range.len().saturating_sub(1)) as f32;
        let y = cell.center().y + 10.0;
        for (k, i) in range.enumerate() {
            let x = cell.center().x - span / 2.0 + gap * k as f32;
            let p = pos2(x, y);
            if chosen == Some(i) {
                painter.circle_filled(p, 3.5, theme::ACCENT);
            } else if changed.get(i).copied().unwrap_or(false) {
                painter.circle_filled(p, 2.2, theme::ACCENT.gamma_multiply(0.8));
            } else {
                painter.circle_filled(p, 2.0, Color32::from_gray(0x55));
            }
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    out
}

/// A tool's tile in the Tools grid: its name over what it does, amber-edged while it is
/// changing the photo.
pub(crate) fn tool_tile(
    ui: &mut egui::Ui,
    chip: Chip,
    size: egui::Vec2,
    changed: bool,
) -> egui::Response {
    let (name, sub) = chip.tile();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name)
    });
    let painter = ui.painter();
    let hot = response.hovered() || response.is_pointer_button_down_on();
    painter.rect(
        rect,
        10.0,
        if hot { theme::BG_850 } else { theme::BG_900 },
        Stroke::new(
            1.0,
            if changed {
                theme::ACCENT.linear_multiply(0.7)
            } else {
                theme::BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    let clip = painter.with_clip_rect(rect.shrink(2.0));
    clip.text(
        pos2(rect.left() + 12.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        name,
        FontId::new(14.0, theme::sans_strong()),
        if changed {
            theme::ACCENT
        } else {
            theme::TEXT_HI
        },
    );
    clip.text(
        pos2(rect.left() + 12.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        sub,
        FontId::proportional(11.5),
        theme::TEXT_DIM,
    );
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
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

    /// Tune is values only, and a swipe walks one section of them; its sections and Crop's
    /// cover their lists in order. No group but Tools needs a row of chips to scroll.
    #[test]
    fn tune_is_values_in_named_stretches_each_swiped_alone() {
        let tune = Category::Tune;
        assert!(tune.chips().iter().all(|c| !c.is_tool()));
        for s in 0..tune.sections().len() {
            let r = tune.section_range(s);
            for i in r.clone() {
                assert_eq!(tune.knob_list(i).len(), r.len(), "Tune chip {i}");
            }
        }
        // Crop's swipe walks every value it has, across its sections.
        let crop = Category::Crop;
        let values = crop.chips().iter().filter(|c| !c.is_tool()).count();
        assert_eq!(crop.knob_list(crop.chips().len() - 1).len(), values);
        for cat in [Category::Tune, Category::Crop] {
            let mut covered = 0;
            for s in 0..cat.sections().len() {
                let r = cat.section_range(s);
                assert_eq!(r.start, covered, "{cat:?} section {s} leaves a gap");
                assert!(!r.is_empty(), "{cat:?} section {s} is empty");
                for i in r.clone() {
                    assert_eq!(cat.section_of(i), s);
                }
                covered = r.end;
            }
            assert_eq!(covered, cat.chips().len());
        }
        // The grid's tiles each say what they are.
        for chip in Category::Tools.chips() {
            assert!(chip.is_tool());
            assert!(
                !chip.tile().1.is_empty(),
                "{chip:?} has no line on its tile"
            );
        }
    }
}
