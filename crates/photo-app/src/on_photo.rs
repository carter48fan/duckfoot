//! The photo as the control (G-02, G-03), on a touch screen: one recogniser reads every
//! touch that starts on the stage.
//!
//! In Edit, with a module whose controls are values along a line (see [`crate::knobs`]):
//!
//! - swipe **up or down** to choose which value: a list floats over the photo;
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
    pub(crate) stepped: u32,
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
            && crate::categories::Category::of(self.compact_module) == self.compact_category
            && at_fit
            && !self.cropping
            && !self.split_active
            && self.selected_mask.is_none()
            && !self.healing
            && !self.picking_wb
            && self.ready_image().is_some()
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
                ..
            } if editing => {
                let t = &mut self.photo_touch;
                t.choose_travel += delta;
                let module = self.knob_module();
                let n = knobs(module).len();
                let (mut i, _) = self.photo_knob().expect("editing has knobs");
                while self.photo_touch.choose_travel.abs() >= CHOOSE_STEP {
                    let down = self.photo_touch.choose_travel > 0.0;
                    self.photo_touch.choose_travel -=
                        CHOOSE_STEP.copysign(self.photo_touch.choose_travel);
                    i = if down {
                        (i + 1).min(n - 1)
                    } else {
                        i.saturating_sub(1)
                    };
                }
                if self.photo_knob().map(|(j, _)| j) != Some(i) {
                    self.photo_touch.log.chose += 1;
                }
                self.photo_touch.knob = Some((module, i));
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
                    self.step_module(step);
                } else {
                    self.photo_touch.log.stepped += 1;
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
                let list = knobs(self.knob_module());
                let Some((chosen, _)) = self.photo_knob() else {
                    return;
                };
                let row = 34.0;
                let h = row * list.len() as f32 + 12.0;
                let panel = Rect::from_center_size(
                    rect.center(),
                    vec2(220.0_f32.min(rect.width() - 32.0), h),
                );
                painter.rect_filled(panel, 12.0, Color32::from_black_alpha(200));
                for (i, k) in list.iter().enumerate() {
                    let y = panel.top() + 6.0 + row * (i as f32 + 0.5);
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
                    painter.text(
                        pos2(panel.left() + 18.0, y),
                        Align2::LEFT_CENTER,
                        k.name,
                        FontId::new(15.0, theme::sans_strong()),
                        if on { theme::ACCENT } else { theme::TEXT },
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
