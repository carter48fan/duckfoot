//! The handful of controls egui's stock widgets can't be styled into.
//!
//! Stock egui paints a slider's handle with the same `bg_fill` as its rail, gives buttons one
//! fill for every state, and has no notion of a card that lights up when it is the one being
//! edited. Each of those is a few lines of painting here instead.

use std::ops::RangeInclusive;

use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame,
    InnerResponse, Margin, Painter, Rect, Response, Sense, Shape, Stroke, StrokeKind, TextFormat,
    Ui, Vec2,
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
fn sized(ui: &Ui, desktop: f32, touch: f32) -> f32 {
    if is_touch(ui.ctx()) {
        touch
    } else {
        desktop
    }
}

/// A toggle chip: small bold monospace caps. The barrel switcher, scope modes, the stage
/// toolbar's CROP and A / B.
pub fn chip(ui: &mut Ui, selected: bool, text: &str) -> Response {
    let enabled = ui.is_enabled();
    let galley = ui.painter().layout_no_wrap(
        text.to_uppercase(),
        FontId::new(sized(ui, 10.5, 12.0), theme::mono_bold()),
        Color32::PLACEHOLDER,
    );
    let size = vec2(
        galley.size().x + sized(ui, 14.0, 20.0),
        sized(ui, 20.0, 32.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let hovered = enabled && response.hovered();
    let (fill, stroke, colour) = if selected {
        (BG_850, ACCENT.linear_multiply(0.7), ACCENT)
    } else if hovered {
        (BG_850, BORDER_HI, TEXT_HI)
    } else if enabled {
        (
            Color32::TRANSPARENT,
            Color32::TRANSPARENT,
            Color32::from_gray(0xcc),
        )
    } else {
        (Color32::TRANSPARENT, Color32::TRANSPARENT, TEXT_FAINT)
    };
    let painter = ui.painter();
    painter.rect(rect, 3, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    painter.galley(rect.center() - galley.size() / 2.0, galley, colour);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Solid amber. One per screen: the action the panel exists for.
    Primary,
    /// Black with a hairline.
    Secondary,
    /// Black with an amber hairline: a batch action worth noticing, but not *the* action.
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
    let enabled = enabled && ui.is_enabled();
    let family = match kind {
        Kind::Primary | Kind::Accent => theme::mono_bold(),
        Kind::Secondary => FontFamily::Monospace,
    };
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::new(11.5, family),
        Color32::PLACEHOLDER,
    );
    let size = vec2(
        (galley.size().x + sized(ui, 22.0, 28.0)).max(min_width),
        sized(ui, 26.0, 36.0),
    );
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);

    let hovered = enabled && response.hovered();
    let (fill, stroke, colour) = match (kind, enabled, hovered) {
        (_, false, _) => (BG_900, BORDER, TEXT_FAINT),
        (Kind::Primary, true, false) => (ACCENT, ACCENT, Color32::BLACK),
        (Kind::Primary, true, true) => (ACCENT_HI, ACCENT_HI, Color32::BLACK),
        (Kind::Secondary, true, false) => (BG_950, Color32::from_gray(0x2a), TEXT),
        (Kind::Secondary, true, true) => (BG_850, Color32::from_gray(0x55), TEXT_HI),
        (Kind::Accent, true, false) => (BG_850, ACCENT.linear_multiply(0.8), ACCENT),
        (Kind::Accent, true, true) => (ACCENT.linear_multiply(0.18), ACCENT, ACCENT_HI),
    };
    let painter = ui.painter();
    painter.rect(rect, 3, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    painter.galley(rect.center() - galley.size() / 2.0, galley, colour);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// A square icon button, ghost-styled. `glyph` must exist in egui's bundled fallback fonts.
pub fn icon(ui: &mut Ui, glyph: &str, active: bool, tooltip: &str) -> Response {
    let enabled = ui.is_enabled();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::splat(sized(ui, 26.0, 36.0)), Sense::click());
    let hovered = enabled && response.hovered();
    let colour = match (enabled, active, hovered) {
        (false, ..) => TEXT_FAINT,
        (true, true, _) => ACCENT,
        (true, false, true) => TEXT_HI,
        (true, false, false) => Color32::from_gray(0xcc),
    };
    let painter = ui.painter();
    if hovered || active {
        painter.rect_filled(rect, 3, BG_850);
    }
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(sized(ui, 15.0, 18.0)),
        colour,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(tooltip)
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
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    paint_checkbox(ui.painter(), rect, *on, response.hovered());
    response
}

/// A 2px rail with an amber fill and an 8×14 thumb (12×24 on touch).
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
    rail_painted(ui, value, range, width, default, origin, None)
}

/// `rail`, optionally with a two-colour gradient for a track instead of the amber fill —
/// for a control whose ends are colours, where the track itself says which way is which.
fn rail_painted(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    width: f32,
    default: f32,
    origin: f32,
    gradient: Option<[Color32; 2]>,
) -> Response {
    let touch = is_touch(ui.ctx());
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(width, sized(ui, 18.0, 34.0)), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let thumb = if touch {
        vec2(12.0, 24.0)
    } else {
        vec2(8.0, 14.0)
    };
    let track = Rect::from_min_max(
        pos2(rect.left() + thumb.x / 2.0, rect.center().y - 1.0),
        pos2(rect.right() - thumb.x / 2.0, rect.center().y + 1.0),
    );
    let x_of = |v: f32| track.left() + ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * track.width();

    let before = *value;
    if response.double_clicked() {
        *value = default;
    } else {
        // A press jumps the value there with a pointer. A finger lands on a slider on
        // its way to scrolling past it, so on touch only a drag changes the value.
        let pressed = !touch && ui.input(|i| i.pointer.primary_pressed());
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

    let enabled = ui.is_enabled();
    let hot = enabled && (response.hovered() || response.dragged());
    let fill = if enabled { ACCENT } else { TEXT_FAINT };

    let painter = ui.painter();
    if let Some([left, right]) = gradient {
        let band = track.expand2(vec2(0.0, 1.0));
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
        painter.rect_filled(track, 1, RAIL);
        painter.rect_filled(
            Rect::from_x_y_ranges(x0.min(x1)..=x0.max(x1), track.y_range()),
            1,
            fill,
        );
    }
    // A tick where the default sits, so "back to neutral" is visible before you do it.
    let home = x_of(default);
    painter.vline(
        home,
        (track.top() - 3.0)..=(track.bottom() + 3.0),
        Stroke::new(1.0, BORDER_HI),
    );
    let knob = Rect::from_center_size(pos2(x1, rect.center().y), thumb);
    painter.rect_filled(knob, 2, if hot { ACCENT_HI } else { fill });
    painter.vline(
        x1,
        (knob.top() + 4.0)..=(knob.bottom() - 4.0),
        Stroke::new(1.0, Color32::BLACK),
    );
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
pub fn scaled_slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    decimals: usize,
    default: f32,
    scale: SliderScale,
) -> Response {
    ui.horizontal(|ui| {
        const LABEL_W: f32 = 74.0;
        // The value box is sized from its widest possible text, not a guess. In a side panel,
        // a row that overflows by even a few pixels is read as content asking for a wider
        // panel — and it asks again every frame.
        let widest = [*range.start(), *range.end()]
            .map(|v| format!("{v:.decimals$}").len())
            .into_iter()
            .max()
            .unwrap_or(4);
        let digit_w = ui
            .painter()
            .layout_no_wrap(
                "0".repeat(widest),
                FontId::monospace(12.0),
                Color32::PLACEHOLDER,
            )
            .size()
            .x;
        let value_w = (digit_w + 2.0 * ui.spacing().button_padding.x + 4.0).max(50.0);
        let (label_rect, mut label_response) =
            ui.allocate_exact_size(vec2(LABEL_W, 18.0), Sense::click());
        let modified = (*value - default).abs() > 1e-6;
        ui.painter().text(
            label_rect.left_center(),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.0),
            if modified { TEXT } else { TEXT_DIM },
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
        let slid = rail_painted(
            ui,
            &mut t,
            0.0..=1.0,
            rail_w,
            (scale.to_rail)(default),
            (scale.to_rail)(origin),
            scale.gradient,
        );
        if slid.changed() {
            *value = (scale.from_rail)(t).clamp(*range.start(), *range.end());
        }
        let typed = ui.add_sized(
            [value_w, 18.0],
            egui::DragValue::new(value)
                .range(range)
                .fixed_decimals(decimals)
                .speed(scale.speed),
        );
        label_response | slid | typed
    })
    .inner
}

/// A full-bleed header band: `BG_925`, a hairline underneath, contents laid out left to right.
pub fn band<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let inner = Frame::new()
        .fill(BG_925)
        .inner_margin(Margin::symmetric(12, 7))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(add).inner
        });
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
        .inner_margin(Margin::symmetric(10, 5))
        .begin(ui);
    let inner = {
        let ui = &mut prepared.content_ui;
        ui.set_min_width(ui.available_width());
        add(ui)
    };
    let outer = prepared.content_ui.min_rect().expand2(vec2(11.0, 6.0));
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
    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());

    let mut toggled = false;
    let mut active = true;
    let mut x = row.left();
    if let Some(on) = enabled {
        let box_rect =
            Rect::from_center_size(pos2(row.left() + 7.0, row.center().y), Vec2::splat(14.0));
        let box_response =
            ui.interact(box_rect.expand(3.0), response.id.with("on"), Sense::click());
        if box_response.clicked() {
            *on = !*on;
            toggled = true;
        }
        paint_checkbox(ui.painter(), box_rect, *on, box_response.hovered());
        active = *on;
        x = box_rect.right() + 9.0;
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
        FontId::new(13.0, theme::sans_strong()),
        title_colour,
    );
    let title_w = title_galley.size().x;
    painter.galley(
        pos2(x, row.center().y - title_galley.size().y / 2.0),
        title_galley,
        title_colour,
    );

    // The summary gets whatever the title leaves, and is truncated rather than overlapping.
    let room = row.right() - (x + title_w + 10.0);
    if room > 24.0 && !summary.is_empty() {
        let mut job = LayoutJob::single_section(
            summary.to_owned(),
            TextFormat::simple(FontId::monospace(11.5), summary_colour),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let galley = painter.layout_job(job);
        painter.galley(
            pos2(
                row.right() - galley.size().x,
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
    // The drawn strokes span x 135..377, y 75..431 of the SVG's 512-unit viewBox.
    let scale = size / 356.0;
    let at = |x: f32, y: f32| rect.center() + vec2(x - 256.0, y - 253.0) * scale;
    let width = 34.0 * scale;
    let breech = at(256.0, 292.0);
    let painter = ui.painter();
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
    let size = vec2(ui.available_width().min(240.0), 48.0);
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
    painter.text(
        pos2(rect.left() + 6.0, rect.top() + 4.0),
        Align2::LEFT_TOP,
        label,
        FontId::new(9.0, FontFamily::Monospace),
        TEXT_FAINT,
    );
}
