//! The photo as the control (G-02, G-03), on a touch screen: one recogniser reads every
//! touch that starts on the stage.
//!
//! In Edit, with a module whose controls are values along a line (see [`crate::knobs`]):
//!
//! - swipe **up or down** to choose which value (up for the next, on a phone): a list
//!   floats over the photo; on a phone in Tune it has Light, Colour and Detail as tabs
//!   along its top, and the same finger drifting left or right moves between them;
//! - swipe **left or right** to change it, slowly for fine steps; a readout says what it is;
//! - **double-tap** puts it back; **hold** shows the original.
//!
//! Elsewhere, and in Edit for modules with nothing to turn, a sideways flick steps to the
//! next photo (or module, in Edit). Zoomed in, one finger pans and two fingers flick.

use std::time::{Duration, Instant};

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Stroke};

use crate::gesture::{self, Axis, Event};
use crate::knobs::{knobs, Knob};
use crate::{theme, InspectorSectionId, PhotoApp};

/// Travel, in points, that moves the choice on by one value.
const CHOOSE_STEP: f32 = 40.0;
/// Sideways travel, while choosing, that moves to the next tab of the list.
const TAB_STEP: f32 = 56.0;
/// The height of the list's tabs, over its rows.
const TABS_H: f32 = 32.0;
/// Slower than this (points per second) a slide is fine: a quarter of the change.
const FINE_SPEED: f32 = 150.0;
/// How long the readout stays after the finger lets go.
const READOUT: Duration = Duration::from_millis(900);

/// How many of each gesture the photo has taken, for the tour to see one done (G-04).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GestureLog {
    pub(crate) chose: u32,
    pub(crate) adjusted: u32,
    pub(crate) reset: u32,
    pub(crate) held: u32,
}

/// What a touch on the photo is doing, kept between frames.
#[derive(Default)]
pub(crate) struct PhotoTouch {
    pub(crate) recogniser: gesture::Recogniser,
    /// The desktop's right-button or Alt drag, read the same way.
    desk: gesture::Recogniser,
    /// The module and which of its knobs is chosen.
    pub(crate) knob: Option<(InspectorSectionId, usize)>,
    /// Vertical travel not yet turned into a step of the choice.
    choose_travel: f32,
    /// Sideways travel, while choosing, not yet turned into a step of the tab.
    tab_travel: f32,
    /// The value is being changed: its undo step is open.
    adjusting: bool,
    /// When the readout or the list was last shown, and which.
    pub(crate) shown: Option<(Instant, Shown)>,
    /// Held: the original shows until the finger lets go.
    pub(crate) holding: bool,
    /// Did the touch under way start on the stage.
    ours: bool,
    pub(crate) log: GestureLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shown {
    /// The list of values, while choosing.
    Choosing,
    /// The chosen value's readout.
    Value,
    /// "Original" while held.
    Original,
}

impl PhotoApp {
    /// Whether the photo is the Edit sheet's control right now: a phone, the Edit sheet open
    /// on a module with values to turn (under the ruler, or its tool open), and nothing else
    /// claiming the photo's touches.
    pub(crate) fn edits_on_photo(&self, at_fit: bool) -> bool {
        self.layout.is_compact()
            && self.compact_tab == Some(crate::compact::Tab::Edit)
            && !self.customize_open
            && !self.cutout_studio.open
            && !knobs(self.compact_module).is_empty()
            && self.photo_turns_a_value()
            && at_fit
            && !self.cropping
            && !self.split_active
            && self.selected_mask.is_none()
            && !self.healing
            && !self.picking_wb
            && self.ready_image().is_some()
    }

    /// On a phone, whether the module the Edit sheet is on has a value the open group lists
    /// (under the ruler), or its tool is open: a tool in the Tools grid that is not open
    /// leaves the photo alone.
    fn photo_turns_a_value(&self) -> bool {
        let module = self.compact_module;
        (self.compact_controls_open
            && crate::categories::Category::of(module) == self.compact_category)
            || self
                .compact_category
                .chips()
                .iter()
                .any(|c| matches!(c, crate::categories::Chip::Knob(id, _) if *id == module))
    }

    /// The values a swipe up or down walks, and which of them is chosen. On a phone, under
    /// the ruler, it is the group's list (in Tune, the chosen value's section: light,
    /// colour or detail), across modules; with a tool open, or on the desktop, the
    /// module's own.
    pub(crate) fn swipe_list(&self) -> (Vec<(InspectorSectionId, usize)>, usize) {
        let module = self.knob_module();
        let at = self.photo_knob().map_or(0, |(i, _)| i);
        if self.layout.is_compact() && !self.compact_controls_open {
            let cat = self.compact_category;
            let chip = crate::categories::Chip::Knob(module, at);
            if let Some(i) = cat.chips().iter().position(|c| *c == chip) {
                let list = cat.knob_list(i);
                if let Some(pos) = list.iter().position(|k| *k == (module, at)) {
                    return (list, pos);
                }
            }
        }
        let list = (0..knobs(module).len()).map(|i| (module, i)).collect();
        (list, at)
    }

    /// The tabs over the list of values, and which is open: on a phone, under the ruler,
    /// in a group whose sections are each a list of their own (Tune's Light, Colour and
    /// Detail).
    pub(crate) fn palette_tabs(&self) -> Option<(&'static [(&'static str, usize)], usize)> {
        let cat = self.compact_category;
        if !self.layout.is_compact() || self.compact_controls_open || !cat.swipes_by_section() {
            return None;
        }
        let (i, _) = self.photo_knob()?;
        let chip = crate::categories::Chip::Knob(self.knob_module(), i);
        let at = cat.chips().iter().position(|c| *c == chip)?;
        Some((cat.sections(), cat.section_of(at)))
    }

    /// Move the choice `steps` along [`Self::swipe_list`], stopping at its ends.
    pub(crate) fn step_choice(&mut self, steps: i32) {
        if steps == 0 {
            return;
        }
        let (list, at) = self.swipe_list();
        if list.is_empty() {
            return;
        }
        let to = (at as i32 + steps).clamp(0, list.len() as i32 - 1) as usize;
        if to != at {
            self.photo_touch.log.chose += 1;
            let (id, i) = list[to];
            if self.layout.is_compact() && !self.compact_controls_open {
                // Under the ruler: the ruler, its readout and the group's dots follow.
                self.choose_chip(crate::categories::Chip::Knob(id, i));
            } else {
                self.photo_touch.knob = Some((id, i));
            }
        }
        self.photo_touch.shown = Some((Instant::now(), Shown::Choosing));
    }

    /// The module the photo's gestures turn: the Edit sheet's on a phone, the card last
    /// worked on on the desktop.
    pub(crate) fn knob_module(&self) -> InspectorSectionId {
        if self.layout.is_compact() {
            self.compact_module
        } else {
            self.focused_section
                .filter(|id| !knobs(*id).is_empty())
                .unwrap_or(InspectorSectionId::Exposure)
        }
    }

    /// The knob the photo turns: the one last chosen for this module, else its first.
    pub(crate) fn photo_knob(&self) -> Option<(usize, &'static Knob)> {
        let module = self.knob_module();
        let list = knobs(module);
        let i = match self.photo_touch.knob {
            Some((id, i)) if id == module => i.min(list.len().saturating_sub(1)),
            _ => 0,
        };
        list.get(i).map(|k| (i, k))
    }

    /// Reads this frame's touches on the stage (`rect`). `free` says nothing else on the
    /// stage (a mask handle, the split, the crop) has the pointer.
    pub(crate) fn stage_gestures(
        &mut self,
        ctx: &egui::Context,
        rect: Rect,
        at_fit: bool,
        free: bool,
    ) {
        if !crate::widgets::is_touch(ctx) {
            return;
        }
        let sample = gesture::sample(ctx);
        let t = &mut self.photo_touch;
        if t.recogniser.start().is_none() {
            // A touch belongs to the stage if it lands there; others are left alone.
            // Not one that lands on something floating over the photo (the tour's card).
            // The tour's layer spans the screen but only its card and dim take touches, so
            // for it the card is what counts.
            let tour = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("duckfoot_tour"));
            let card = self.tour_step.and(self.tour_card).map(|(card, _)| card);
            let covered = |p: egui::Pos2| {
                card.is_some_and(|c| c.contains(p))
                    || ctx.layer_id_at(p).is_some_and(|l| {
                        l != tour
                            && matches!(l.order, egui::Order::Foreground | egui::Order::Tooltip)
                    })
            };
            t.ours = sample.pos.is_some_and(|p| rect.contains(p) && !covered(p)) && free;
        }
        if !t.ours && sample.pos.is_some() {
            return;
        }
        let events = t.recogniser.update(sample, ctx.content_rect());
        let editing = self.edits_on_photo(at_fit);
        for e in events {
            self.photo_event(e, at_fit, editing);
        }
        if self.photo_touch.recogniser.start().is_some() || self.photo_touch.shown.is_some() {
            ctx.request_repaint();
        }
    }

    /// A right-button or Alt drag on the desktop's photo: the same gestures as a finger,
    /// on the card last worked on (G-03).
    pub(crate) fn desktop_photo_drag(&mut self, ctx: &egui::Context, response: &egui::Response) {
        let alt = ctx.input(|i| i.modifiers.alt);
        let dragging = response.dragged_by(egui::PointerButton::Secondary)
            || (alt && response.dragged_by(egui::PointerButton::Primary));
        if !dragging && self.photo_touch.desk.start().is_none() {
            return;
        }
        let sample = gesture::Sample {
            time: ctx.input(|i| i.time),
            pos: if dragging {
                ctx.pointer_latest_pos()
            } else {
                None
            },
            fingers: 1,
        };
        let editing = !knobs(self.knob_module()).is_empty()
            && !self.cropping
            && self.selected_mask.is_none()
            && !self.healing;
        // No edge is the system's on a desktop.
        let screen = Rect::EVERYTHING;
        for e in self.photo_touch.desk.update(sample, screen) {
            // A tap or a hold is the ordinary click's, not this drag's.
            if matches!(e, Event::Slide { .. } | Event::Released) {
                self.photo_event(e, true, editing);
            }
        }
        ctx.request_repaint();
    }

    /// Alt+↑/↓ chooses the value the photo's gestures turn, Alt+←/→ turns it a
    /// hundredth of its range (a thousandth with fine on).
    pub(crate) fn knob_keys(&mut self, ctx: &egui::Context) {
        if crate::widgets::rail_has_focus(ctx) || ctx.egui_wants_keyboard_input() {
            return;
        }
        let (up, down, left, right) = ctx.input(|i| {
            let alt = i.modifiers.alt;
            (
                alt && i.key_pressed(egui::Key::ArrowUp),
                alt && i.key_pressed(egui::Key::ArrowDown),
                alt && i.key_pressed(egui::Key::ArrowLeft),
                alt && i.key_pressed(egui::Key::ArrowRight),
            )
        });
        let module = self.knob_module();
        let n = knobs(module).len();
        let Some((i, knob)) = self.photo_knob() else {
            return;
        };
        if up || down {
            let j = if down {
                (i + 1).min(n - 1)
            } else {
                i.saturating_sub(1)
            };
            self.photo_touch.knob = Some((module, j));
            self.photo_touch.shown = Some((Instant::now(), Shown::Choosing));
        }
        if (left || right) && self.ready_image().is_some() {
            let white = self.white_scale();
            let (lo, hi) = knob.range(&self.stack);
            let step = (hi - lo) / if self.fine_adjust { 1000.0 } else { 100.0 };
            let v = knob.get(&self.stack, &white) + if right { step } else { -step };
            self.history.checkpoint(&self.stack);
            knob.set(module, &mut self.stack, &white, v);
            self.needs_render = true;
            self.photo_touch.shown = Some((Instant::now(), Shown::Value));
        }
    }

    fn photo_event(&mut self, e: Event, at_fit: bool, editing: bool) {
        let white = self.white_scale();
        match e {
            Event::Slide {
                axis: Axis::Y,
                delta,
                across,
                ..
            } if editing => {
                // Drifting sideways while choosing moves between the list's tabs: left for
                // the next, as a row of pages is swiped.
                if self.palette_tabs().is_some() {
                    let t = &mut self.photo_touch;
                    t.tab_travel += across;
                    if t.tab_travel.abs() >= TAB_STEP {
                        let step = if t.tab_travel < 0.0 { 1 } else { -1 };
                        t.tab_travel = 0.0;
                        t.choose_travel = 0.0;
                        self.step_section(step);
                    }
                }
                // On a phone, up for the next value, as a list is scrolled; the desktop's
                // drag keeps down for the next, as its Alt+↓ does.
                let delta = if self.layout.is_compact() {
                    -delta
                } else {
                    delta
                };
                let t = &mut self.photo_touch;
                t.choose_travel += delta;
                let mut steps = 0;
                while t.choose_travel.abs() >= CHOOSE_STEP {
                    let down = t.choose_travel > 0.0;
                    t.choose_travel -= CHOOSE_STEP.copysign(t.choose_travel);
                    steps += if down { 1 } else { -1 };
                }
                self.step_choice(steps);
                self.photo_touch.shown = Some((Instant::now(), Shown::Choosing));
            }
            Event::Slide {
                axis: Axis::X,
                delta,
                speed,
                ..
            } if editing => {
                let Some((_, knob)) = self.photo_knob() else {
                    return;
                };
                if delta == 0.0 {
                    // Only wobbling across: nothing to turn, and no undo step to open.
                    return;
                }
                if !self.photo_touch.adjusting {
                    self.photo_touch.adjusting = true;
                    self.photo_touch.log.adjusted += 1;
                    self.history.checkpoint(&self.stack);
                }
                // A slide across the photo's width covers most of the range; slowly, a
                // quarter of that, and a tenth again with fine on.
                let width = self.last_panel.x.max(1.0);
                let (lo, hi) = knob.range(&self.stack);
                let mut per_point = (hi - lo) / (width * 1.2);
                if speed.abs() < FINE_SPEED {
                    per_point *= 0.25;
                }
                if self.fine_adjust {
                    per_point *= 0.1;
                }
                let v = knob.get(&self.stack, &white) + delta * per_point;
                knob.set(self.knob_module(), &mut self.stack, &white, v);
                self.needs_render = true;
                self.photo_touch.shown = Some((Instant::now(), Shown::Value));
            }
            Event::DoubleTap(_) if editing => {
                let Some((_, knob)) = self.photo_knob() else {
                    return;
                };
                self.photo_touch.log.reset += 1;
                let home = knob.get(&self.baseline, &white);
                if (knob.get(&self.stack, &white) - home).abs() > f32::EPSILON {
                    self.history.checkpoint(&self.stack);
                    knob.set(self.knob_module(), &mut self.stack, &white, home);
                    self.needs_render = true;
                }
                self.photo_touch.shown = Some((Instant::now(), Shown::Value));
            }
            Event::Hold(_) if self.ready_image().is_some() => {
                self.photo_touch.holding = true;
                self.photo_touch.shown = Some((Instant::now(), Shown::Original));
                self.needs_render = true;
            }
            Event::HoldEnd => {
                if self.photo_touch.holding {
                    self.photo_touch.log.held += 1;
                    self.photo_touch.holding = false;
                    self.photo_touch.shown = None;
                    self.needs_render = true;
                }
            }
            Event::Released => {
                self.photo_touch.adjusting = false;
                self.photo_touch.choose_travel = 0.0;
                self.photo_touch.tab_travel = 0.0;
            }
            // Not editing on the photo: a sideways flick steps, one finger at fit and two
            // zoomed in (where one pans).
            Event::Flick {
                axis: Axis::X,
                sign,
                fingers,
            } if !editing && ((at_fit && fingers == 1) || (!at_fit && fingers >= 2)) => {
                let step = -sign;
                if self.layout.is_compact() && self.compact_tab == Some(crate::compact::Tab::Edit) {
                    // In a tool, the next tool; in the Tools grid, nothing to step.
                    if self.compact_controls_open || !self.compact_category.is_grid() {
                        self.step_module(step);
                    }
                } else if !self.single_photo() {
                    self.pending_step = Some(step as isize);
                }
            }
            _ => {}
        }
    }

    /// The readout, the list of values, or "original", over the photo like a toast.
    pub(crate) fn paint_photo_overlay(&mut self, ui: &egui::Ui, rect: Rect) {
        let Some((when, what)) = self.photo_touch.shown else {
            return;
        };
        let touching = self.photo_touch.recogniser.start().is_some()
            || self.photo_touch.desk.start().is_some();
        if !touching && what != Shown::Original && when.elapsed() > READOUT {
            self.photo_touch.shown = None;
            return;
        }
        let painter = ui.painter().with_clip_rect(rect);
        let white = self.white_scale();
        let pill = |text: String, size: f32, at: egui::Pos2, colour: Color32| {
            let galley =
                painter.layout_no_wrap(text, FontId::new(size, theme::sans_strong()), colour);
            let r = Rect::from_center_size(at, galley.size() + vec2(28.0, 14.0));
            painter.rect_filled(r, r.height() / 2.0, Color32::from_black_alpha(190));
            painter.galley(r.center() - galley.size() / 2.0, galley, colour);
            r
        };
        match what {
            Shown::Original => {
                pill(
                    "original".into(),
                    16.0,
                    pos2(rect.center().x, rect.top() + 36.0),
                    theme::TEXT_HI,
                );
            }
            Shown::Value => {
                if let Some((_, knob)) = self.photo_knob() {
                    let v = knob.get(&self.stack, &white);
                    let text = format!("{}  {}", knob.name, knob.format(v));
                    let r = pill(
                        text,
                        20.0,
                        pos2(rect.center().x, rect.top() + 44.0),
                        theme::TEXT_HI,
                    );
                    // Where the value sits in its range, under the readout.
                    let (lo, hi) = knob.range(&self.stack);
                    let f = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
                    let bar = Rect::from_min_size(
                        pos2(r.left() + 14.0, r.bottom() + 6.0),
                        vec2(r.width() - 28.0, 3.0),
                    );
                    painter.rect_filled(bar, 1.5, Color32::from_white_alpha(60));
                    painter.rect_filled(
                        Rect::from_min_size(bar.min, vec2(bar.width() * f, bar.height())),
                        1.5,
                        theme::ACCENT,
                    );
                }
            }
            Shown::Choosing => {
                let (list, chosen) = self.swipe_list();
                if list.is_empty() {
                    return;
                }
                // As many rows as fit, the chosen one kept in view: a long list (all of
                // Tune) scrolls under the finger rather than running off the photo.
                let tabs = self.palette_tabs();
                let tabs_h = if tabs.is_some() { TABS_H } else { 0.0 };
                let row = 34.0;
                let fit = (((rect.height() - 40.0 - tabs_h) / row).floor() as usize).clamp(3, 9);
                let shown = list.len().min(fit);
                let first = chosen.saturating_sub(shown / 2).min(list.len() - shown);
                let h = row * shown as f32 + 12.0 + tabs_h;
                let panel = Rect::from_center_size(
                    rect.center(),
                    vec2(240.0_f32.min(rect.width() - 32.0), h),
                );
                painter.rect_filled(panel, 12.0, Color32::from_black_alpha(200));
                if let Some((sections, open)) = tabs {
                    // The tabs, side by side over the rows: the open one lit and underlined.
                    let w = panel.width() / sections.len().max(1) as f32;
                    for (s, (name, _)) in sections.iter().enumerate() {
                        let x = panel.left() + w * (s as f32 + 0.5);
                        let y = panel.top() + TABS_H / 2.0 + 2.0;
                        let on = s == open;
                        let ink = if on { theme::TEXT_HI } else { theme::TEXT_DIM };
                        let r = painter.text(
                            pos2(x, y),
                            Align2::CENTER_CENTER,
                            name.to_uppercase(),
                            FontId::new(11.5, theme::mono_bold()),
                            ink,
                        );
                        if on {
                            painter.hline(
                                r.x_range(),
                                r.bottom() + 4.0,
                                Stroke::new(2.0, theme::ACCENT),
                            );
                        }
                    }
                    painter.hline(
                        (panel.left() + 10.0)..=(panel.right() - 10.0),
                        panel.top() + TABS_H,
                        Stroke::new(1.0, Color32::from_white_alpha(28)),
                    );
                }
                let compact = self.layout.is_compact();
                for (row_i, i) in (first..first + shown).enumerate() {
                    let (id, ki) = list[i];
                    let Some(k) = knobs(id).get(ki) else {
                        continue;
                    };
                    let name = if compact {
                        crate::categories::Chip::Knob(id, ki).label()
                    } else {
                        k.name
                    };
                    let y = panel.top() + tabs_h + 6.0 + row * (row_i as f32 + 0.5);
                    let on = i == chosen;
                    if on {
                        let band = Rect::from_center_size(
                            pos2(panel.center().x, y),
                            vec2(panel.width() - 12.0, row - 4.0),
                        );
                        painter.rect_filled(band, 8.0, theme::ACCENT.gamma_multiply(0.25));
                        painter.rect_stroke(
                            band,
                            8.0,
                            Stroke::new(1.0, theme::ACCENT),
                            egui::StrokeKind::Inside,
                        );
                    }
                    // Faded towards a cut-off end, so it reads as a list that goes on.
                    let edge = (row_i == 0 && first > 0)
                        || (row_i + 1 == shown && first + shown < list.len());
                    let ink = if on {
                        theme::ACCENT
                    } else if edge {
                        theme::TEXT_FAINT
                    } else {
                        theme::TEXT
                    };
                    painter.text(
                        pos2(panel.left() + 18.0, y),
                        Align2::LEFT_CENTER,
                        name,
                        FontId::new(15.0, theme::sans_strong()),
                        ink,
                    );
                    painter.text(
                        pos2(panel.right() - 18.0, y),
                        Align2::RIGHT_CENTER,
                        k.format(k.get(&self.stack, &white)),
                        FontId::monospace(13.0),
                        theme::TEXT_DIM,
                    );
                }
            }
        }
    }
}

impl PhotoApp {
    /// The name of the value a swipe across the photo changes, while the photo is the
    /// control.
    pub fn photo_value(&self) -> Option<&'static str> {
        self.photo_knob().map(|(_, k)| k.name)
    }

    /// Held on a phone: the original is showing.
    pub fn showing_original(&self) -> bool {
        self.photo_touch.holding
    }
}

impl PhotoApp {
    /// Shows the value readout, or the list of values, as a touch would (for the tour and
    /// screenshots).
    pub fn show_photo_readout(&mut self, choosing: bool) {
        let what = if choosing {
            Shown::Choosing
        } else {
            Shown::Value
        };
        self.photo_touch.shown = Some((Instant::now(), what));
    }
}
