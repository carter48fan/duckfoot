//! The handful of controls egui's stock widgets can't be styled into.
//!
//! Stock egui paints a slider's handle with the same `bg_fill` as its rail, gives buttons one
//! fill for every state, and has no notion of a card that lights up when it is the one being
//! edited. Each of those is a few lines of painting here instead.

use std::ops::RangeInclusive;

use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame,
    InnerResponse, Margin, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    TextFormat, Ui, Vec2,
};

use crate::theme::{self, *};

// ── Touch ─────────────────────────────────────────────────────────────────────────────────
//
// A finger needs bigger targets than a pointer, and a slider that jumps to wherever it is
// pressed turns every attempt to scroll past it into an edit. The layout decides whether
// input is touch (`PhotoApp::update`) and every control below reads it from here, so the
// desktop layout keeps its sizes.

fn flag(ctx: &egui::Context, key: &'static str) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new(key)))
        .unwrap_or(false)
}

fn set_flag(ctx: &egui::Context, key: &'static str, on: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(key), on));
}

/// Size controls for fingers, and stop sliders jumping on press.
pub fn set_touch(ctx: &egui::Context, on: bool) {
    set_flag(ctx, "duckfoot.touch", on);
}

pub fn is_touch(ctx: &egui::Context) -> bool {
    flag(ctx, "duckfoot.touch")
}

/// Sliders move ten times finer, as holding Shift does. Touch screens have no Shift.
pub fn set_fine(ctx: &egui::Context, on: bool) {
    set_flag(ctx, "duckfoot.fine", on);
}

const FOCUSED_RAIL: &str = "duckfoot.focused_rail";

/// A control asked, this frame, to show the clipping overlay while it is held.
pub fn hold_clip_overlay(ctx: &egui::Context) {
    set_flag(ctx, "duckfoot.clip_hold", true);
}

/// Whether one did since the last call, which clears it.
pub fn take_clip_hold(ctx: &egui::Context) -> bool {
    let held = flag(ctx, "duckfoot.clip_hold");
    set_flag(ctx, "duckfoot.clip_hold", false);
    held
}

/// A slider has keyboard focus, so ← and → are its, not the photo stepper's.
pub fn rail_has_focus(ctx: &egui::Context) -> bool {
    let rail = ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::new(FOCUSED_RAIL)));
    rail.is_some_and(|id| ctx.memory(|m| m.has_focus(id)))
}

pub fn is_fine(ctx: &egui::Context) -> bool {
    flag(ctx, "duckfoot.fine")
}

/// The 3D views' how-to line, in the words of whichever input is in use.
pub fn orbit_hint(ctx: &egui::Context) -> &'static str {
    if is_touch(ctx) {
        "Drag to rotate 3D view • Pinch to zoom • Double-tap to reset"
    } else {
        "Drag to rotate 3D view • Ctrl+scroll to zoom • Double-click to reset"
    }
}

/// `desktop`, or `touch` when controls are sized for fingers.
pub fn sized(ui: &Ui, desktop: f32, touch: f32) -> f32 {
    if is_touch(ui.ctx()) {
        touch
    } else {
        desktop
    }
}

/// The smallest a control's hit area may be for a finger, in points: Apple's 44 pt and
/// Android's 48 dp, rounded to the smaller.
pub const MIN_TOUCH_TARGET: f32 = 44.0;

/// Allocate `size` for a control's paint, and give it a hit area of at least
/// [`MIN_TOUCH_TARGET`] on touch, centred on it. Controls keep their compact look — the
/// phone sheets are low on purpose — without being hard to hit.
pub fn allocate_control(ui: &mut Ui, size: Vec2, sense: Sense) -> (Rect, Response) {
    let (rect, painted) = ui.allocate_exact_size(size, Sense::hover());
    if !is_touch(ui.ctx()) {
        return (rect, ui.interact(rect, painted.id.with("hit"), sense));
    }
    let grow = vec2(
        ((MIN_TOUCH_TARGET - rect.width()) / 2.0).max(0.0),
        ((MIN_TOUCH_TARGET - rect.height()) / 2.0).max(0.0),
    );
    (
        rect,
        ui.interact(rect.expand2(grow), painted.id.with("hit"), sense),
    )
}

/// A toggle chip: small bold monospace caps. The barrel switcher, scope modes, the stage
/// toolbar's CROP and A / B.
/// How wide a row's trailing controls were last frame, so the next can tell whether they
/// fit at the right end of the row beside what it already holds. `ui.with_layout(right to
/// left)` alone does not check: on a narrow window it draws them over the row's left side.
pub fn trailing_width(ctx: &egui::Context, key: egui::Id) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(key))
        .unwrap_or(f32::INFINITY)
}

/// Record how wide `ui`'s contents came out, for [`trailing_width`].
pub fn remember_trailing_width(ui: &Ui, key: egui::Id) {
    let w = ui.min_rect().width();
    ui.ctx().data_mut(|d| d.insert_temp(key, w));
}

/// What a painter-drawn control is, for screen readers (AccessKit), and a ring round it
/// while it has keyboard focus, so Tab shows where it is. The ring is outside `rect`, so
/// the control's own painting, done after, never covers it.
pub fn describe(ui: &Ui, response: &Response, rect: Rect, info: impl Fn() -> egui::WidgetInfo) {
    response.widget_info(info);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            4,
            Stroke::new(1.5, ACCENT_HI),
            StrokeKind::Outside,
        );
    }
}

pub fn chip(ui: &mut Ui, selected: bool, text: &str) -> Response {
    chip_enabled(ui, true, selected, text)
}

/// [`chip`] with its own enabled flag, for rows that must wrap: an `add_enabled_ui` scope
/// around each chip would stop `horizontal_wrapped` from wrapping them.
pub fn chip_enabled(ui: &mut Ui, enabled: bool, selected: bool, text: &str) -> Response {
    chip_painted(ui, enabled, selected, text, false)
}

/// A [`chip`] with a hairline round it even while it is off: for a panel, where chips sit
/// under labels and sliders. An unframed chip's text starts inside its box, so beside a
/// label it reads as indented; framed, its edge lines up with the column.
pub fn outlined_chip(ui: &mut Ui, enabled: bool, selected: bool, text: &str) -> Response {
    chip_painted(ui, enabled, selected, text, true)
}

fn chip_painted(
    ui: &mut Ui,
    enabled: bool,
    selected: bool,
    text: &str,
    outlined: bool,
) -> Response {
    let enabled = enabled && ui.is_enabled();
    let galley = ui.painter().layout_no_wrap(
        text.to_uppercase(),
        FontId::new(sized(ui, 11.0, 12.5), theme::mono_bold()),
        Color32::PLACEHOLDER,
    );
    // On touch, big enough to see what a finger is aiming at: a hit area grown round a
    // small chip overlaps its neighbours', and the wrong one answers.
    let size = vec2(
        galley.size().x + sized(ui, 18.0, 22.0),
        sized(ui, 26.0, 34.0),
    );
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = allocate_control(ui, size, sense);
    describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, enabled, selected, text)
    });

    let hovered = enabled && response.hovered();
    let idle = if outlined {
        Color32::from_gray(0x2a)
    } else {
        Color32::TRANSPARENT
    };
    let (fill, stroke, colour) = if selected {
        (Color32::TRANSPARENT, ACCENT, ACCENT)
    } else if hovered {
        (
            Color32::TRANSPARENT,
            if outlined { BORDER_HI } else { idle },
            TEXT_HI,
        )
    } else if enabled {
        (Color32::TRANSPARENT, idle, Color32::from_gray(0xcc))
    } else {
        (Color32::TRANSPARENT, idle, TEXT_FAINT)
    };
    let painter = ui.painter();
    painter.rect(rect, 2, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    painter.galley(rect.center() - galley.size() / 2.0, galley, colour);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An amber outline that fills amber under the pointer. One per screen: the action
    /// the panel exists for.
    Primary,
    /// Black with a hairline.
    Secondary,
    /// A dimmer amber hairline: a batch action worth noticing, but not *the* action.
    Accent,
}

/// A painted button. `min_width` of 0 sizes it to its label.
pub fn button(ui: &mut Ui, kind: Kind, text: &str, min_width: f32) -> Response {
    button_enabled(ui, true, kind, text, min_width)
}

/// [`button`] with its own enabled flag, for rows that must wrap: an `add_enabled_ui` scope
/// around each button would stop `horizontal_wrapped` from wrapping them.
pub fn button_enabled(
    ui: &mut Ui,
    enabled: bool,
    kind: Kind,
    text: &str,
    min_width: f32,
) -> Response {
    let height = sized(ui, 30.0, 40.0);
    button_sized(ui, enabled, kind, text, min_width, height)
}

/// [`button_enabled`] at `height`: the phone's one big action on a sheet.
pub fn button_sized(
    ui: &mut Ui,
    enabled: bool,
    kind: Kind,
    text: &str,
    min_width: f32,
    height: f32,
) -> Response {
    let enabled = enabled && ui.is_enabled();
    let family = match kind {
        Kind::Primary | Kind::Accent => theme::mono_bold(),
        Kind::Secondary => FontFamily::Monospace,
    };
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::new(sized(ui, 11.5, 13.0), family),
        Color32::PLACEHOLDER,
    );
    let size = vec2(
        (galley.size().x + sized(ui, 26.0, 28.0)).max(min_width),
        height,
    );
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = allocate_control(ui, size, sense);
    describe(ui, &response, rect, || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text)
    });

    let hovered = enabled && response.hovered();
    let (fill, stroke, colour) = match (kind, enabled, hovered) {
        (_, false, _) => (BG_950, BORDER, TEXT_FAINT),
        (Kind::Primary, true, false) => (BG_950, ACCENT, ACCENT),
        (Kind::Primary, true, true) => (ACCENT, ACCENT, Color32::BLACK),
        (Kind::Secondary, true, false) => (BG_950, Color32::from_gray(0x2a), TEXT),
        (Kind::Secondary, true, true) => (BG_950, ACCENT, ACCENT),
        (Kind::Accent, true, false) => (BG_950, ACCENT.linear_multiply(0.6), ACCENT),
        (Kind::Accent, true, true) => (BG_950, ACCENT, ACCENT),
    };
    let painter = ui.painter();
    painter.rect(rect, 2, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    painter.galley(rect.center() - galley.size() / 2.0, galley, colour);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// A tool on the rail beside the inspector: a line-drawn icon, amber with a hairline at the
/// rail's inner edge while it is the tool in use. Drawn, not a glyph, so no font can lack it.
pub(crate) fn tool_button(ui: &mut Ui, tool: crate::Tool, active: bool) -> Response {
    let (rect, response) = allocate_control(ui, Vec2::splat(36.0), Sense::click());
    let tip = tool.tip();
    describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, tool.label())
    });
    let hovered = response.hovered();
    let colour = if active || hovered { ACCENT } else { TEXT_DIM };
    let painter = ui.painter();
    if active {
        painter.vline(
            rect.left() - 5.0,
            (rect.top() + 9.0)..=(rect.bottom() - 9.0),
            Stroke::new(1.0, ACCENT),
        );
    }
    paint_tool_icon(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
        tool,
        colour,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(tip)
}

/// The tools' icons, in 1.5pt lines within `r` (18pt square).
fn paint_tool_icon(painter: &Painter, r: Rect, tool: crate::Tool, colour: Color32) {
    use crate::Tool;
    let line = Stroke::new(1.5, colour);
    let at = |x: f32, y: f32| r.left_top() + vec2(x, y);
    match tool {
        // Three sliders, their knobs at different places.
        Tool::Edit => {
            for (y, knob) in [(4.0, 12.0), (9.0, 5.0), (14.0, 10.0)] {
                painter.hline(r.left() + 1.0..=r.right() - 1.0, r.top() + y, line);
                painter.circle(at(knob, y), 2.2, BG_950, line);
            }
        }
        // Two corners crossing, as a crop frame's handles.
        Tool::Crop => {
            painter.add(Shape::line(
                vec![at(4.0, 0.5), at(4.0, 14.0), at(17.5, 14.0)],
                line,
            ));
            painter.add(Shape::line(
                vec![at(0.5, 4.0), at(14.0, 4.0), at(14.0, 17.5)],
                line,
            ));
        }
        // A spot, and the healing brush's ring around it.
        Tool::Heal => {
            painter.circle_stroke(r.center(), 8.0, line);
            painter.circle_filled(r.center(), 2.5, colour);
        }
        // A circle, half of it painted in: a mask.
        Tool::Masks => {
            painter.circle_stroke(r.center(), 8.0, line);
            let half: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::FRAC_PI_2 + i as f32 / 16.0 * std::f32::consts::PI;
                    r.center() + vec2(a.cos(), a.sin()) * 8.0
                })
                .collect();
            painter.add(Shape::convex_polygon(half, colour, Stroke::NONE));
        }
        // Scissors: two finger rings, the blades crossing above them.
        Tool::Cutout => {
            painter.circle_stroke(at(4.5, 14.0), 3.0, line);
            painter.circle_stroke(at(13.5, 14.0), 3.0, line);
            painter.line_segment([at(6.5, 11.8), at(15.0, 0.5)], line);
            painter.line_segment([at(11.5, 11.8), at(3.0, 0.5)], line);
        }
        // Two overlapping rounds: one look laid over the photo.
        Tool::Looks => {
            painter.circle_stroke(at(6.5, 9.0), 6.0, line);
            painter.circle_stroke(at(11.5, 9.0), 6.0, line);
        }
    }
}

/// A square icon button, ghost-styled. `glyph` must exist in egui's bundled fallback fonts.
pub fn icon(ui: &mut Ui, glyph: &str, active: bool, tooltip: &str) -> Response {
    let enabled = ui.is_enabled();
    let (rect, response) = allocate_control(ui, Vec2::splat(sized(ui, 30.0, 40.0)), Sense::click());
    // The tooltip says what it does; the glyph alone would be read as "question mark".
    describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, tooltip)
    });
    let hovered = enabled && response.hovered();
    let colour = match (enabled, active, hovered) {
        (false, ..) => TEXT_FAINT,
        (true, true, _) => ACCENT,
        (true, false, true) => ACCENT,
        (true, false, false) => Color32::from_gray(0xcc),
    };
    let painter = ui.painter();
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(sized(ui, 15.0, 20.0)),
        colour,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(tooltip)
}

/// [`icon`] with a drawn glyph instead of a font's: `paint` gets the button's painter, its
/// rect and the colour for its state. A drawn glyph sits on the button's exact centre; a
/// font's sits wherever its metrics put it, which is what made ⚙ and + look off-centre.
pub fn icon_drawn(
    ui: &mut Ui,
    active: bool,
    tooltip: &str,
    paint: impl FnOnce(&Painter, Rect, Color32),
) -> Response {
    let enabled = ui.is_enabled();
    let (rect, response) = allocate_control(ui, Vec2::splat(sized(ui, 30.0, 40.0)), Sense::click());
    describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, tooltip)
    });
    let hovered = enabled && response.hovered();
    let colour = match (enabled, active, hovered) {
        (false, ..) => TEXT_FAINT,
        (true, true, _) | (true, false, true) => ACCENT,
        (true, false, false) => Color32::from_gray(0xcc),
    };
    paint(ui.painter(), rect, colour);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(tooltip)
}

/// A + centred in `rect`, `arm` points from the middle to each end.
pub fn paint_plus(painter: &Painter, rect: Rect, colour: Color32, arm: f32) {
    let c = rect.center();
    let stroke = Stroke::new(1.5, colour);
    painter.hline(c.x - arm..=c.x + arm, c.y, stroke);
    painter.vline(c.x, c.y - arm..=c.y + arm, stroke);
}

/// A × centred in `rect`, `arm` points from the middle along each diagonal.
pub fn paint_cross(painter: &Painter, rect: Rect, colour: Color32, arm: f32) {
    let c = rect.center();
    let stroke = Stroke::new(1.5, colour);
    painter.line_segment([c + vec2(-arm, -arm), c + vec2(arm, arm)], stroke);
    painter.line_segment([c + vec2(-arm, arm), c + vec2(arm, -arm)], stroke);
}

/// A gear centred in `rect`: a ring with eight teeth.
pub fn paint_gear(painter: &Painter, rect: Rect, colour: Color32) {
    let c = rect.center();
    painter.circle_stroke(c, 4.0, Stroke::new(1.5, colour));
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        let dir = vec2(a.cos(), a.sin());
        painter.line_segment([c + dir * 4.5, c + dir * 6.5], Stroke::new(2.0, colour));
    }
}

/// The menu under `anchor`, `width` wide. A popup lays its rows out to the room it is given,
/// so rows that stretch to the available width (see `menu_item`) made every menu as wide as
/// the window; the width is set here, once, for all of them.
pub fn menu_popup<R>(
    anchor: &Response,
    width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<InnerResponse<R>> {
    egui::Popup::menu(anchor).width(width).show(|ui| {
        ui.set_width(width);
        add(ui)
    })
}

/// A 14px check box: amber with a black tick when on, black with a hairline when off.
pub fn paint_checkbox(painter: &Painter, rect: Rect, on: bool, hovered: bool) {
    if on {
        let fill = if hovered { ACCENT_HI } else { ACCENT };
        painter.rect_filled(rect, 2, fill);
        let tick = [
            rect.left_top() + vec2(3.2, 7.2),
            rect.left_top() + vec2(5.9, 9.9),
            rect.left_top() + vec2(10.9, 4.2),
        ];
        painter.add(Shape::line(tick.to_vec(), Stroke::new(1.8, Color32::BLACK)));
    } else {
        let stroke = if hovered { ACCENT } else { BORDER_HI };
        painter.rect(
            rect,
            2,
            BG_950,
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
    }
}

pub fn checkbox(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = allocate_control(ui, Vec2::splat(14.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let checked = *on;
    describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, checked, "module on")
    });
    paint_checkbox(ui.painter(), rect, *on, response.hovered());
    response
}

/// An on / off switch sized for a thumb: a 38×22 track in a 48×34 target.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = allocate_control(ui, vec2(48.0, 34.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let checked = *on;
    describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, checked, "on")
    });
    let track = Rect::from_center_size(rect.center(), vec2(38.0, 22.0));
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    paint_switch(ui.painter(), track, t, response.hovered());
    response
}

/// A switch in the panel's flat style: a hairline pill with a dot in it. Off, the dot is
/// an open ring on the left and the pill is grey; on, it is solid amber on the right with a
/// hairline trailing back to where it started — a two-position slider, which is what it
/// is. `t` runs 0 (off) to 1 (on) so the dot can travel between them.
pub fn paint_switch(painter: &Painter, track: Rect, t: f32, hovered: bool) {
    let h = track.height();
    let dot_r = h * 0.22;
    let inset = h / 2.0;
    let lerp = |a: Color32, b: Color32| {
        Color32::from_rgb(
            egui::lerp(f32::from(a.r())..=f32::from(b.r()), t) as u8,
            egui::lerp(f32::from(a.g())..=f32::from(b.g()), t) as u8,
            egui::lerp(f32::from(a.b())..=f32::from(b.b()), t) as u8,
        )
    };
    let off_edge = if hovered { TEXT_FAINT } else { BORDER_HI };
    painter.rect_stroke(
        track,
        h / 2.0,
        Stroke::new(1.0, lerp(off_edge, ACCENT)),
        StrokeKind::Inside,
    );
    let start = track.left() + inset;
    let x = egui::lerp(start..=track.right() - inset, t);
    let cy = track.center().y;
    if x - dot_r > start {
        painter.hline(start..=x - dot_r, cy, Stroke::new(1.0, ACCENT));
    }
    let off_dot = if hovered { TEXT } else { TEXT_DIM };
    if t > 0.5 {
        painter.circle_filled(pos2(x, cy), dot_r, ACCENT);
    } else {
        painter.circle_stroke(pos2(x, cy), dot_r - 0.75, Stroke::new(1.5, off_dot));
    }
}

/// A hairline rail, a 2px amber run from the origin to the value, and a ring for a thumb
/// (11pt across, 16 on touch) that fills amber once the value has left its default.
///
/// Bipolar ranges fill from zero rather than from the left end, so "+0.4 EV" and "−0.4 EV"
/// read as the same distance from neutral.
///
/// Pressing the rail away from the thumb jumps there (not on touch); dragging moves the
/// value by how far the pointer travels, not to where it is, so grabbing the thumb never
/// makes it leap. Shift while dragging, or [`set_fine`], is ten times finer. Double-click
/// returns to `default`.
pub fn rail(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    width: f32,
    default: f32,
) -> Response {
    let (lo, hi) = (*range.start(), *range.end());
    let origin = if lo < 0.0 && hi > 0.0 { 0.0 } else { lo };
    let spec = RailSpec {
        default,
        origin,
        gradient: None,
        label: "",
        readout: f64::from(*value),
        bubble: None,
    };
    rail_painted(ui, value, range, width, spec)
}

/// Everything about a rail but where it is and what it holds.
struct RailSpec<'a> {
    /// Where the reset tick sits, and where a double-click returns to.
    default: f32,
    /// Where the amber fill starts.
    origin: f32,
    gradient: Option<[Color32; 2]>,
    /// What a screen reader calls it, and the value it reads out (which on a scaled
    /// slider is not the rail position).
    label: &'a str,
    readout: f64,
    /// The value as the thumb's tag shows it while dragged, if it has one.
    bubble: Option<String>,
}

/// `rail`, optionally with a two-colour gradient for a track instead of the amber fill —
/// for a control whose ends are colours, where the track itself says which way is which.
fn rail_painted(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    width: f32,
    spec: RailSpec,
) -> Response {
    let RailSpec {
        default,
        origin,
        gradient,
        label,
        readout,
        bubble,
    } = spec;
    let touch = is_touch(ui.ctx());
    let row_h = sized(ui, 20.0, 32.0);
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, row_h), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    // The thumb is a ring; this is its box at rest, which also insets the track so the ring
    // never hangs off either end.
    let thumb = if touch {
        vec2(16.0, 16.0)
    } else {
        vec2(11.0, 11.0)
    };
    let track = Rect::from_min_max(
        pos2(rect.left() + thumb.x / 2.0, rect.center().y - 1.0),
        pos2(rect.right() - thumb.x / 2.0, rect.center().y + 1.0),
    );
    let x_of = |v: f32| track.left() + ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * track.width();

    let before = *value;
    let enabled = ui.is_enabled();
    describe(ui, &response, rect, || {
        egui::WidgetInfo::slider(enabled, readout, label)
    });
    if response.has_focus() {
        // Reached with Tab, ← and → move it (Shift finer) instead of moving the focus on
        // or stepping to another photo.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    ..Default::default()
                },
            )
        });
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new(FOCUSED_RAIL), response.id));
        let (left, right, shift) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.modifiers.shift,
            )
        });
        let step = (hi - lo) * if shift { 0.002 } else { 0.02 };
        if left {
            *value = (*value - step).max(lo);
        }
        if right {
            *value = (*value + step).min(hi);
        }
    }
    // A screen reader's increment and decrement, a step at a time.
    let (up, down) = ui.input(|i| {
        use egui::accesskit::Action;
        (
            i.num_accesskit_action_requests(response.id, Action::Increment),
            i.num_accesskit_action_requests(response.id, Action::Decrement),
        )
    });
    let step = (hi - lo) * 0.02;
    *value = (*value + step * (up as f32 - down as f32)).clamp(lo, hi);
    if response.double_clicked() {
        *value = default;
    } else if touch {
        let fine = is_fine(ui.ctx());
        // A tap does nothing: under a thumb, a tap is as often a scroll being stopped as
        // a value being chosen, and jumping the slider for it edits the photo by accident.
        // Double-tap resets; a drag moves the value from where it is.
        if response.dragged() {
            let gesture_id = response.id.with("touch_gesture");
            if response.drag_started() {
                let delta = response.drag_delta();
                // Vertical dominant drag -> user is scrolling the drawer vertically.
                // Otherwise user is adjusting the slider horizontally.
                let is_sliding = delta.x.abs() >= delta.y.abs() * 0.8;
                ui.ctx().data_mut(|d| d.insert_temp(gesture_id, is_sliding));
            }
            let is_sliding = ui
                .ctx()
                .data(|d| d.get_temp::<bool>(gesture_id))
                .unwrap_or(true);
            if is_sliding {
                // Relative, like a phone's own sliders: the thumb need not be under the
                // finger, and the value never jumps to wherever the drag began.
                let scale = if fine { 0.1 } else { 1.0 };
                let dx = response.drag_delta().x;
                *value = (*value + dx / track.width() * (hi - lo) * scale).clamp(lo, hi);
            } else {
                let dy = response.drag_delta().y;
                if dy.abs() > 0.0 {
                    ui.scroll_with_delta(vec2(0.0, dy));
                }
            }
        }
        if response.drag_stopped() {
            ui.ctx()
                .data_mut(|d| d.remove_temp::<bool>(response.id.with("touch_gesture")));
        }
    } else {
        // Desktop mouse pointer.
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        if pressed && response.is_pointer_button_down_on() {
            if let Some(p) = response.interact_pointer_pos() {
                if (p.x - x_of(*value)).abs() > thumb.x {
                    let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
                    *value = lo + t * (hi - lo);
                }
            }
        } else if response.dragged() {
            let fine = if ui.input(|i| i.modifiers.shift) || is_fine(ui.ctx()) {
                0.1
            } else {
                1.0
            };
            let dx = response.drag_delta().x;
            *value = (*value + dx / track.width() * (hi - lo) * fine).clamp(lo, hi);
        }
    }
    if *value != before {
        response.mark_changed();
    }

    let (x0, x1) = (x_of(origin), x_of(*value));
    let cy = rect.center().y;

    let hot = enabled && (response.hovered() || response.dragged());
    let moved = (*value - default).abs() > (hi - lo) * 1e-4;

    let painter = ui.painter();
    if let Some([left, right]) = gradient {
        // A colour track is itself the scale, so it is drawn instead of the amber run.
        let band = Rect::from_x_y_ranges(track.x_range(), (cy - 1.0)..=(cy + 1.0));
        let (left, right) = if enabled {
            (left, right)
        } else {
            (left.gamma_multiply(0.4), right.gamma_multiply(0.4))
        };
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(band.left_top(), left);
        mesh.colored_vertex(band.right_top(), right);
        mesh.colored_vertex(band.right_bottom(), right);
        mesh.colored_vertex(band.left_bottom(), left);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(Shape::mesh(mesh));
    } else {
        // One hairline for the whole range, and a 2px amber run from the origin to the
        // value: how far from neutral it is, at a glance.
        painter.hline(track.x_range(), cy, Stroke::new(1.0, BORDER_HI));
        if enabled && x0 != x1 {
            painter.rect_filled(
                Rect::from_x_y_ranges(x0.min(x1)..=x0.max(x1), (cy - 1.0)..=(cy + 1.0)),
                0,
                ACCENT,
            );
        }
    }
    // A tick where the default sits, so "back to neutral" is visible before you do it.
    painter.vline(
        x_of(default),
        (cy - 4.0)..=(cy + 4.0),
        Stroke::new(1.0, BORDER_HI),
    );

    // The thumb: an open ring at the default, filled amber once moved. It grows a little
    // under the pointer and more while held. Eased, so it reads as a response, not a jump.
    let grow = if response.dragged() {
        1.35
    } else if hot {
        1.15
    } else {
        1.0
    };
    let grow = ui
        .ctx()
        .animate_value_with_time(response.id.with("thumb"), grow, 0.12);
    let r = thumb.x / 2.0 * grow;
    let centre = pos2(x1, cy);
    if !enabled {
        painter.circle(centre, r, BG_950, Stroke::new(1.5, TEXT_FAINT));
    } else if moved {
        painter.circle_filled(centre, r, ACCENT);
    } else {
        let ring = if hot { TEXT } else { TEXT_DIM };
        painter.circle(centre, r - 0.75, BG_950, Stroke::new(1.5, ring));
    }
    if response.has_focus() {
        painter.circle_stroke(centre, r + 3.0, Stroke::new(1.0, TEXT));
    }

    // While held, the value rides above the thumb, so the eye need not leave it for the
    // number box. On touch the finger would cover it, and the sheet shows the value anyway.
    if let (true, false, Some(text)) = (response.dragged(), touch, bubble) {
        let galley = painter.layout_no_wrap(text, FontId::monospace(11.0), ACCENT);
        let size = galley.size() + vec2(10.0, 4.0);
        let tag = Rect::from_center_size(pos2(x1, cy - r - 6.0 - size.y / 2.0), size);
        let over = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            response.id.with("bubble"),
        ));
        over.rect(tag, 2, BG_950, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
        over.galley(tag.center() - galley.size() / 2.0, galley, ACCENT);
    }

    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    response.on_hover_text("Drag · Shift for fine · double-click to reset")
}

/// One parameter: `label  ───────■──────  value`. The value box is a `DragValue`, so it can
/// also be dragged or double-clicked and typed into. Double-clicking the label or the rail
/// resets to `default`.
pub fn param_slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    decimals: usize,
    default: f32,
) -> Response {
    let (lo, hi) = (*range.start(), *range.end());
    let span = hi - lo;
    let scale = SliderScale {
        to_rail: &|v| (v - lo) / span,
        from_rail: &|t| lo + t * span,
        speed: span / 400.0,
        gradient: None,
    };
    scaled_slider(ui, label, value, range, decimals, default, scale)
}

/// How a `scaled_slider` lays its value out along the rail.
pub struct SliderScale<'a> {
    /// Value → position along the rail, 0..=1. Must be monotonic.
    pub to_rail: &'a dyn Fn(f32) -> f32,
    /// The inverse of `to_rail`.
    pub from_rail: &'a dyn Fn(f32) -> f32,
    /// Value units per pixel of drag on the number box.
    pub speed: f32,
    /// A coloured track, left to right, in place of the amber fill.
    pub gradient: Option<[Color32; 2]>,
}

/// `param_slider` on a non-linear rail: for a quantity like colour temperature, where equal
/// steps in kelvin are nowhere near equal steps in what you see. The number box still reads
/// and types the value itself.
///
/// On the desktop the label and value share a line and the rail runs the panel's width
/// under them; on a phone the three sit on one row, as the sheets are short.
pub fn scaled_slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    decimals: usize,
    default: f32,
    scale: SliderScale,
) -> Response {
    if is_touch(ui.ctx()) {
        inline_slider(ui, label, value, range, decimals, default, scale)
    } else {
        stacked_slider(ui, label, value, range, decimals, default, scale)
    }
}

/// `label ··········· value` over a full-width rail. Every rail in the panel is one length
/// and a long label never squeezes it.
fn stacked_slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    decimals: usize,
    default: f32,
    scale: SliderScale,
) -> Response {
    let modified = (*value - default).abs() > 1e-6;
    let (lo, hi) = (*range.start(), *range.end());
    let origin = if lo < 0.0 && hi > 0.0 { 0.0 } else { lo };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let width = ui.available_width();
        let line = ui.allocate_ui_with_layout(
            vec2(width, 16.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let text = egui::RichText::new(label).size(12.5).color(if modified {
                    TEXT_HI
                } else {
                    TEXT
                });
                let label_response = ui.add(
                    egui::Label::new(text)
                        .sense(Sense::click())
                        .selectable(false),
                );
                let typed = ui
                    .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        value_box(ui, value, range.clone(), decimals, scale.speed, modified)
                    })
                    .inner;
                (label_response, typed)
            },
        );
        let (mut label_response, typed) = line.inner;
        if label_response.double_clicked() && modified {
            *value = default;
            label_response.mark_changed();
        }
        let mut t = (scale.to_rail)(*value);
        let spec = RailSpec {
            default: (scale.to_rail)(default),
            origin: (scale.to_rail)(origin),
            gradient: scale.gradient,
            label,
            readout: f64::from(*value),
            bubble: Some(format!("{:.decimals$}", *value)),
        };
        let slid = rail_painted(ui, &mut t, 0.0..=1.0, width, spec);
        if slid.changed() {
            *value = (scale.from_rail)(t).clamp(lo, hi);
        }
        label_response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
        let typed = typed.labelled_by(label_response.id);
        label_response | slid | typed
    })
    .inner
}

/// The number beside a slider: no box until the pointer is on it, amber once the value has
/// moved. A `DragValue`, so it can be dragged, or double-clicked and typed into.
fn value_box(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    decimals: usize,
    speed: f32,
    modified: bool,
) -> Response {
    ui.scope(|ui| {
        // As tall as the label beside it, so the two share a baseline.
        ui.spacing_mut().interact_size.y = 16.0;
        // No minimum width: egui centres a short number ("0.00") in its 40 pt default and
        // lets a long one ("0.0000") run to the edge, so a column's values end at
        // different places. Hugging the text, every value ends at the panel's edge.
        ui.spacing_mut().interact_size.x = 0.0;
        ui.spacing_mut().button_padding = vec2(3.0, 0.0);
        let v = ui.visuals_mut();
        v.override_text_color = Some(if modified { ACCENT } else { TEXT });
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
        ] {
            w.weak_bg_fill = Color32::TRANSPARENT;
            w.bg_fill = Color32::TRANSPARENT;
        }
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_HI);
        ui.add(
            egui::DragValue::new(value)
                .range(range)
                .fixed_decimals(decimals)
                .speed(speed)
                // A typed value lands once, on Enter or leaving the field: one undo step,
                // not one per keystroke (and no half-typed values on the image).
                .update_while_editing(false),
        )
    })
    .inner
}

/// The phone's row: `label  ───────●──────  value`.
fn inline_slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    decimals: usize,
    default: f32,
    scale: SliderScale,
) -> Response {
    ui.horizontal(|ui| {
        // Room for the longest label in the stack ("smoothness") at either size, so a
        // module's rails start in one column.
        let label_col = sized(ui, 84.0, 82.0);
        // The value box is sized from its widest possible text, not a guess. In a side panel,
        // a row that overflows by even a few pixels is read as content asking for a wider
        // panel — and it asks again every frame.
        // At least seven characters ("-0.1000", "-15.00"), so every row's rail is one length
        // whatever its range and decimals.
        let widest = [*range.start(), *range.end()]
            .map(|v| format!("{v:.decimals$}").len())
            .into_iter()
            .max()
            .unwrap_or(4)
            .max(7);
        let digit_w = ui
            .painter()
            .layout_no_wrap(
                "0".repeat(widest),
                FontId::monospace(12.0),
                Color32::PLACEHOLDER,
            )
            .size()
            .x;
        // A common floor keeps the rails of a module's rows the same length.
        let value_w =
            (digit_w + 2.0 * ui.spacing().button_padding.x + 4.0).max(sized(ui, 50.0, 56.0));
        let row_h = sized(ui, 24.0, 32.0);
        let modified = (*value - default).abs() > 1e-6;
        let text = ui.painter().layout_no_wrap(
            label.to_string(),
            FontId::proportional(sized(ui, 12.0, 13.0)),
            if modified { TEXT } else { TEXT_DIM },
        );
        // Wider than the common column only for a label that needs it: painted text is no
        // widget, so nothing else would stop it running under the rail.
        let label_w = label_col.max(text.size().x + 6.0);
        let touch = is_touch(ui.ctx());
        // On a phone the whole row is the control (G-03): a sideways drag on the label
        // moves the value as one on the rail does, from where it is.
        let sense = if touch {
            Sense::click_and_drag()
        } else {
            Sense::click()
        };
        let (label_rect, mut label_response) = ui.allocate_exact_size(vec2(label_w, row_h), sense);
        ui.painter().galley(
            label_rect.left_center() - vec2(0.0, text.size().y * 0.5),
            text,
            TEXT,
        );
        if label_response.double_clicked() && modified {
            *value = default;
            label_response.mark_changed();
        }
        let gap = ui.spacing().item_spacing.x;
        let rail_w = (ui.available_width() - value_w - gap).max(40.0);
        let (lo, hi) = (*range.start(), *range.end());
        let origin = if lo < 0.0 && hi > 0.0 { 0.0 } else { lo };
        let mut t = (scale.to_rail)(*value);
        let spec = RailSpec {
            default: (scale.to_rail)(default),
            origin: (scale.to_rail)(origin),
            gradient: scale.gradient,
            label,
            readout: f64::from(*value),
            bubble: None,
        };
        let slid = rail_painted(ui, &mut t, 0.0..=1.0, rail_w, spec);
        if slid.changed() {
            *value = (scale.from_rail)(t).clamp(*range.start(), *range.end());
        }
        if touch && label_response.dragged() {
            let sideways_id = label_response.id.with("sideways");
            if label_response.drag_started() {
                let d = label_response.drag_delta();
                ui.ctx()
                    .data_mut(|m| m.insert_temp(sideways_id, d.x.abs() >= d.y.abs() * 0.8));
            }
            let sideways = ui
                .ctx()
                .data(|m| m.get_temp::<bool>(sideways_id))
                .unwrap_or(true);
            let d = label_response.drag_delta();
            if sideways {
                let fine = if is_fine(ui.ctx()) { 0.1 } else { 1.0 };
                let t = ((scale.to_rail)(*value) + d.x / rail_w * fine).clamp(0.0, 1.0);
                let v = (scale.from_rail)(t).clamp(*range.start(), *range.end());
                if v != *value {
                    *value = v;
                    label_response.mark_changed();
                }
            } else if d.y != 0.0 {
                ui.scroll_with_delta(vec2(0.0, d.y));
            }
        }
        label_response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
        let typed = ui
            .add_sized(
                [value_w, sized(ui, 18.0, 26.0)],
                egui::DragValue::new(value)
                    .range(range)
                    .fixed_decimals(decimals)
                    .speed(scale.speed)
                    // A typed value lands once, on Enter or leaving the field: one undo step,
                    // not one per keystroke (and no half-typed values on the image).
                    .update_while_editing(false),
            )
            .labelled_by(label_response.id);
        label_response | slid | typed
    })
    .inner
}

/// One bar's line (G-01): the full `height` allocated up front and every item centred on
/// it, left to right. A row that only grows as its items arrive centres the early ones on
/// the height it had so far, and a bar's chips, buttons and icons each come at their own
/// height; on one line they share a midline.
pub fn bar_line<R>(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    // In a sideways scroll the width is unbounded: the line grows as its items arrive.
    let width = Some(ui.available_width())
        .filter(|w| w.is_finite())
        .unwrap_or(0.0);
    let row = Rect::from_min_size(ui.cursor().min, vec2(width, height));
    let shown = ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        add,
    );
    ui.advance_cursor_after_rect(row.union(shown.response.rect));
    shown.inner
}

/// The height of a panel's header band.
pub const BAND_H: f32 = 40.0;

/// A full-bleed header band: `BG_925`, a hairline underneath, contents laid out left to right
/// on one midline. Every band is the same height, so the left and right panels' headers sit
/// level: a band that grew with its tallest item would centre the items added before it on
/// a shorter line (see [`bar_line`]).
pub fn band<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let inner = Frame::new()
        .fill(BG_925)
        .inner_margin(Margin::symmetric(12, 0))
        .show(ui, |ui| bar_line(ui, BAND_H, add));
    let r = inner.response.rect;
    ui.painter()
        .hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, BORDER));
    inner
}

/// A bordered module card. The border goes amber while `focused` — the one module the user
/// is working in — and brightens under the pointer otherwise.
pub fn card<R>(ui: &mut Ui, focused: bool, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    // The stroke must be set before `begin`: the frame reserves its width up front, and
    // widening it afterwards would make the card 2px wider than the space it was given —
    // which a side panel reads as content asking for more room, every frame.
    let mut prepared = Frame::new()
        .fill(BG_925)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::symmetric(12, 8))
        .begin(ui);
    let inner = {
        let ui = &mut prepared.content_ui;
        ui.set_min_width(ui.available_width());
        add(ui)
    };
    let outer = prepared.content_ui.min_rect().expand2(vec2(13.0, 9.0));
    let hovered = ui.rect_contains_pointer(outer);
    prepared.frame.stroke.color = if focused {
        ACCENT
    } else if hovered {
        BORDER_HI
    } else {
        BORDER
    };
    let response = prepared.end(ui);
    InnerResponse::new(inner, response)
}

/// A card's heading band inside a panel card: `TITLE ········ right`.
pub fn card_title(ui: &mut Ui, title: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(theme::head(title).size(10.5));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

pub struct SectionHeader {
    /// The row (not the switch) was clicked: fold or unfold.
    pub clicked: bool,
    /// The switch was clicked.
    pub toggled: bool,
}

/// An inspector section's heading: `▸ Title •` with a switch at the end. The arrow turns as
/// the section opens; the dot is amber while anything in the section has been changed.
/// `on` is `None` for a section with nothing to switch off.
pub fn section_header(
    ui: &mut Ui,
    title: &str,
    open: bool,
    edited: bool,
    on: Option<bool>,
) -> SectionHeader {
    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    describe(ui, &response, row, || {
        egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, true, title)
    });
    let hot = response.hovered();
    let cy = row.center().y;

    let mut toggled = false;
    if let Some(on) = on {
        // Set in by its hit area's margin, so that stays inside the row.
        let track = Rect::from_min_size(pos2(row.right() - 35.0, cy - 9.0), vec2(32.0, 18.0));
        let switch_response =
            ui.interact(track.expand(3.0), response.id.with("on"), Sense::click());
        toggled = switch_response.clicked();
        describe(ui, &switch_response, track, || {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, title)
        });
        let t = ui.ctx().animate_bool_responsive(switch_response.id, on);
        paint_switch(ui.painter(), track, t, switch_response.hovered());
    }

    let painter = ui.painter();
    // The arrow points right when shut and down when open, turning between.
    let turn = ui
        .ctx()
        .animate_bool_responsive(response.id.with("turn"), open);
    let centre = pos2(row.left() + 5.0, cy);
    let angle = turn * std::f32::consts::FRAC_PI_2;
    let (sin, cos) = angle.sin_cos();
    let rot = |v: Vec2| centre + vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
    painter.add(Shape::line(
        vec![
            rot(vec2(-2.0, -4.0)),
            rot(vec2(2.0, 0.0)),
            rot(vec2(-2.0, 4.0)),
        ],
        Stroke::new(1.5, if hot { TEXT } else { TEXT_FAINT }),
    ));

    let galley = painter.layout_no_wrap(
        title.to_owned(),
        FontId::new(14.0, theme::sans_strong()),
        if hot { TEXT_HI } else { TEXT },
    );
    let title_pos = pos2(row.left() + 18.0, cy - galley.size().y / 2.0);
    let title_right = title_pos.x + galley.size().x;
    painter.galley(title_pos, galley, TEXT);
    if edited {
        painter.circle_filled(pos2(title_right + 8.0, cy), 2.5, ACCENT);
    }
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    SectionHeader {
        clicked: response.clicked(),
        toggled,
    }
}

pub struct ModuleHeader {
    /// The row (not the check box) was clicked: collapse or expand.
    pub clicked: bool,
    /// The check box was clicked.
    pub toggled: bool,
}

/// A module row: `[✓] title ············ summary`. The whole row toggles collapse; the check
/// box, drawn on top of it, takes its own clicks.
pub fn module_header(
    ui: &mut Ui,
    title: &str,
    enabled: Option<&mut bool>,
    summary: &str,
    summary_colour: Color32,
    focused: bool,
) -> ModuleHeader {
    let (row, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), sized(ui, 32.0, 24.0)),
        Sense::click(),
    );
    describe(ui, &response, row, || {
        egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, true, title)
    });

    let mut toggled = false;
    let mut active = true;
    let mut x = row.left();
    // Where the summary must stop: the row's end, or the switch's left edge.
    let mut right = row.right();
    if let Some(on) = enabled {
        if is_touch(ui.ctx()) {
            // Its hit area is 3 px larger all round; set in by that much so it stays inside
            // the row rather than reaching over the card's edge.
            let box_rect =
                Rect::from_center_size(pos2(row.left() + 10.0, row.center().y), Vec2::splat(14.0));
            let box_response =
                ui.interact(box_rect.expand(3.0), response.id.with("on"), Sense::click());
            if box_response.clicked() {
                *on = !*on;
                toggled = true;
            }
            paint_checkbox(ui.painter(), box_rect, *on, box_response.hovered());
            x = box_rect.right() + 9.0;
        } else {
            // On the desktop a switch at the row's end, as the panel's sections have, set
            // in by its hit area's margin so that stays inside the row.
            let track = Rect::from_min_size(
                pos2(row.right() - 35.0, row.center().y - 9.0),
                vec2(32.0, 18.0),
            );
            let switch_response =
                ui.interact(track.expand(3.0), response.id.with("on"), Sense::click());
            if switch_response.clicked() {
                *on = !*on;
                toggled = true;
            }
            let state = *on;
            describe(ui, &switch_response, track, || {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, state, title)
            });
            let t = ui.ctx().animate_bool_responsive(switch_response.id, *on);
            paint_switch(ui.painter(), track, t, switch_response.hovered());
            right = track.left() - 10.0;
        }
        active = *on;
    }

    let title_colour = if focused {
        ACCENT
    } else if active {
        TEXT_HI
    } else {
        TEXT_DIM
    };
    let painter = ui.painter();
    let title_galley = painter.layout_no_wrap(
        title.to_owned(),
        FontId::new(13.5, theme::sans_strong()),
        title_colour,
    );
    let title_w = title_galley.size().x;
    painter.galley(
        pos2(x, row.center().y - title_galley.size().y / 2.0),
        title_galley,
        title_colour,
    );

    // The summary gets whatever the title leaves, and is truncated rather than overlapping.
    let room = right - (x + title_w + 10.0);
    if room > 24.0 && !summary.is_empty() {
        let mut job = LayoutJob::single_section(
            summary.to_owned(),
            TextFormat::simple(FontId::monospace(11.5), summary_colour),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let galley = painter.layout_job(job);
        painter.galley(
            pos2(
                right - galley.size().x,
                row.center().y - galley.size().y / 2.0,
            ),
            galley,
            summary_colour,
        );
    }

    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    ModuleHeader {
        clicked: response.clicked(),
        toggled,
    }
}

/// An underlined tab, sized to share the row equally with `count` siblings.
pub fn tab(ui: &mut Ui, selected: bool, text: &str, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, text)
    });
    let colour = if selected {
        ACCENT
    } else if response.hovered() {
        TEXT_HI
    } else {
        Color32::from_gray(0xcc)
    };
    let painter = ui.painter();
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::new(12.5, theme::sans_strong()),
        colour,
    );
    if selected {
        painter.rect_filled(
            Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.right_bottom()),
            0,
            ACCENT,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// A dashed hairline rectangle, for drop targets.
pub fn dashed_rect(painter: &Painter, rect: Rect, colour: Color32) {
    let r = rect.shrink(0.5);
    let path = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    painter.extend(Shape::dashed_line(
        &path,
        Stroke::new(1.0, colour),
        4.0,
        3.0,
    ));
}

/// A small status dot. Lime = persisted, amber = pending, faint = nothing to report.
pub fn dot(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(7.0, 12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.0, colour);
}

/// The Duckfoot mark from `brand/duckfoot-mark.svg`: five barrels splayed from one breech.
/// Same geometry as the SVG, minus its background tile, scaled so the strokes fill `size`.
pub fn brand_mark(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_brand_mark(ui.painter(), rect);
}

/// [`brand_mark`] centred in `rect`, scaled to its height.
pub fn paint_brand_mark(painter: &Painter, rect: Rect) {
    let size = rect.height();
    // The drawn strokes span x 135..377, y 75..431 of the SVG's 512-unit viewBox.
    let scale = size / 356.0;
    let at = |x: f32, y: f32| rect.center() + vec2(x - 256.0, y - 253.0) * scale;
    let width = 34.0 * scale;
    let breech = at(256.0, 292.0);
    for (x, y) in [
        (256.0, 92.0),
        (152.0, 156.0),
        (360.0, 156.0),
        (152.0, 414.0),
        (360.0, 414.0),
    ] {
        let tip = at(x, y);
        painter.line_segment([breech, tip], Stroke::new(width, ACCENT));
        // SVG round line caps.
        painter.circle_filled(tip, width / 2.0, ACCENT);
    }
    painter.circle_filled(at(256.0, 288.0), 36.0 * scale, ACCENT);
}

/// Interactive 1-5 star rating widget.
pub fn star_rating(ui: &mut Ui, rating: &mut u8) -> Response {
    let desired_size = vec2(75.0, 16.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());
    let painter = ui.painter();
    let star_width = rect.width() / 5.0;

    let hover_pos = response.hover_pos();
    let hovered_star = hover_pos.map(|p| {
        let x = (p.x - rect.left()).clamp(0.0, rect.width() - 0.1);
        (x / star_width).floor() as u8 + 1
    });

    if response.clicked() {
        if let Some(star) = hovered_star {
            if *rating == star {
                *rating = 0; // Click same star to clear
            } else {
                *rating = star;
            }
        }
    }

    for i in 1..=5 {
        let star_rect = Rect::from_min_size(
            pos2(rect.left() + (i as f32 - 1.0) * star_width, rect.top()),
            vec2(star_width, rect.height()),
        );
        let is_filled = if let Some(h) = hovered_star {
            i <= h
        } else {
            i <= *rating
        };

        let col = if is_filled { ACCENT } else { BORDER_HI };

        let text = if is_filled { "★" } else { "☆" };
        painter.text(
            star_rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(13.0),
            col,
        );
    }

    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    response
}

/// Pick/Reject flag badge: cycles Unflagged -> Pick -> Reject -> Unflagged on click.
pub fn flag_badge(ui: &mut Ui, flag: &mut photo_engine::Flag) -> Response {
    let (text, col, fill) = match flag {
        photo_engine::Flag::Pick => ("PICK", LIME, BG_850),
        photo_engine::Flag::Reject => ("REJ", DANGER, BG_850),
        photo_engine::Flag::Unflagged => ("--", TEXT_FAINT, Color32::TRANSPARENT),
    };
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_string(), FontId::new(9.5, theme::mono_bold()), col);
    let size = vec2(galley.size().x + 10.0, 16.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *flag = match flag {
            photo_engine::Flag::Unflagged => photo_engine::Flag::Pick,
            photo_engine::Flag::Pick => photo_engine::Flag::Reject,
            photo_engine::Flag::Reject => photo_engine::Flag::Unflagged,
        };
    }
    let painter = ui.painter();
    painter.rect(
        rect,
        2.0,
        fill,
        Stroke::new(1.0, col.linear_multiply(0.6)),
        StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, col);
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// Visualizes the exposure response curve knee live in real-time.
/// Shows input EV on x-axis (-3..+3) and output intensity on y-axis (0..1+).
pub fn mini_knee_curve(ui: &mut Ui, ev: f32, rolloff: f32) {
    let size = vec2(ui.available_width(), 48.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();

    // Background & border
    painter.rect(
        rect,
        3.0,
        BG_900,
        Stroke::new(1.0, BORDER),
        StrokeKind::Inside,
    );

    // Grid lines: 100% clip line
    let clip_y = rect.top() + rect.height() * 0.25;
    painter.line_segment(
        [pos2(rect.left(), clip_y), pos2(rect.right(), clip_y)],
        Stroke::new(0.5, BORDER),
    );

    // Compute curve points
    let points_count = 48;
    let mut points = Vec::with_capacity(points_count);

    let gain = 2.0f32.powf(ev);

    for i in 0..points_count {
        let t = i as f32 / (points_count - 1) as f32; // 0..1
        let in_val = t * 2.0; // 0..2 linear sensor signal

        let scaled = in_val * gain;
        let out_val = if rolloff <= 0.001 {
            scaled
        } else {
            let tau = 1.0 - rolloff;
            if scaled <= tau {
                scaled
            } else {
                let d = scaled - tau;
                tau + (d * rolloff) / (rolloff + d)
            }
        };

        let px = rect.left() + t * rect.width();
        let py =
            rect.bottom() - 4.0 - (out_val.clamp(0.0, 2.0) / 1.0) * (rect.bottom() - 4.0 - clip_y);
        points.push(pos2(px, py.clamp(rect.top() + 2.0, rect.bottom() - 2.0)));
    }

    // Draw curve
    for w in points.windows(2) {
        painter.line_segment([w[0], w[1]], Stroke::new(1.5, ACCENT));
    }

    // Legend
    let label = if rolloff <= 0.001 {
        "Classic (Linear Gain)"
    } else if (rolloff - 0.25).abs() < 0.05 {
        "Modern (Soft Shoulder)"
    } else if (rolloff - 0.50).abs() < 0.05 {
        "Filmic (Wide Rolloff)"
    } else {
        "Custom Rolloff"
    };
    // Bottom right, under the shoulder: the curve rises from the bottom left, and a
    // legend there would sit on it.
    painter.text(
        pos2(rect.right() - 6.0, rect.bottom() - 4.0),
        Align2::RIGHT_BOTTOM,
        label,
        FontId::new(9.0, FontFamily::Monospace),
        TEXT_FAINT,
    );
}
