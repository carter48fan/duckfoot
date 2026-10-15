//! One gesture recogniser for the whole phone layout (G-02). It is fed the pointer each
//! frame and says what the finger is doing: a slide along an axis it has locked to, a
//! flick when it lets go, a tap, a double tap, a hold, or two fingers at once.
//!
//! - The axis locks after [`LOCK`] points, and stays locked: a swipe is never misread as a
//!   scroll, nor a wobble as a turn.
//! - A flick is judged by speed as well as distance, so a short fast swipe counts.
//! - A touch that starts within [`EDGE`] of a screen edge is the system's (Back), not ours.
//!
//! Pure logic, driven by time and positions: the tests feed it synthetic touches.

use egui::{Pos2, Rect, Vec2};

/// Travel before a touch commits to an axis.
pub const LOCK: f32 = 10.0;
/// Of each side edge, left to the system's Back gesture.
pub const EDGE: f32 = 16.0;
/// A release this far along its axis is a flick whatever its speed…
pub const FLICK_DISTANCE: f32 = 56.0;
/// …and one at least this far is, if it was moving fast enough when it let go.
pub const FLICK_MIN: f32 = 20.0;
/// Points per second.
pub const FLICK_SPEED: f32 = 500.0;
/// Seconds held still before a touch is a hold.
pub const HOLD: f64 = 0.45;
/// Seconds between two taps that make a double tap, and how near they must land.
pub const DOUBLE_TAP: f64 = 0.3;
pub const DOUBLE_TAP_REACH: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    pub fn of(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.x,
            Axis::Y => v.y,
        }
    }

    /// The axis across this one.
    pub fn other(self) -> Axis {
        match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        }
    }
}

/// What the pointer is doing this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Seconds, from any clock that only goes forward.
    pub time: f64,
    /// Where the (first) finger is, while one is down.
    pub pos: Option<Pos2>,
    /// Fingers on the screen: two or more is a pinch or a two-finger swipe.
    pub fingers: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A finger came down (not at an edge).
    Down(Pos2),
    /// One finger moving along its locked axis: this frame's travel, and the whole of it.
    /// `speed` is points per second along the axis, over the last tenth of a second.
    /// `across` is this frame's travel across the axis, which the lock otherwise ignores.
    Slide {
        axis: Axis,
        delta: f32,
        across: f32,
        total: f32,
        speed: f32,
    },
    /// Two fingers moving together: this frame's travel of the first.
    TwoFingers(Vec2),
    /// A finger let go after a swipe: `sign` is +1 toward the end of the axis (right,
    /// down), -1 toward its start. `fingers` is the most that were down during it.
    Flick {
        axis: Axis,
        sign: i32,
        fingers: usize,
    },
    /// A finger let go after sliding, fast or not.
    Released,
    Tap(Pos2),
    DoubleTap(Pos2),
    /// A finger held still; `HoldEnd` when it lets go.
    Hold(Pos2),
    HoldEnd,
}

#[derive(Debug, Clone)]
struct Touch {
    start: Pos2,
    start_time: f64,
    last: Pos2,
    axis: Option<Axis>,
    fingers: usize,
    held: bool,
    /// Recent (time, position), for the speed at release.
    trail: Vec<(f64, Pos2)>,
}

#[derive(Debug, Clone, Default)]
pub struct Recogniser {
    touch: Option<Touch>,
    /// A touch that began at an edge: ignored until it lets go.
    ignoring: bool,
    last_tap: Option<(f64, Pos2)>,
}

impl Recogniser {
    /// Whether a touch is under way and has committed to an axis or to two fingers.
    pub fn is_gesturing(&self) -> bool {
        self.touch
            .as_ref()
            .is_some_and(|t| t.axis.is_some() || t.fingers > 1 || t.held)
    }

    /// The axis the touch under way has locked to.
    pub fn axis(&self) -> Option<Axis> {
        self.touch.as_ref().and_then(|t| t.axis)
    }

    /// Where the touch under way began.
    pub fn start(&self) -> Option<Pos2> {
        self.touch.as_ref().map(|t| t.start)
    }

    /// Feed one frame. `screen` is where the edges are.
    pub fn update(&mut self, s: Sample, screen: Rect) -> Vec<Event> {
        let mut out = Vec::new();
        match (s.pos, self.touch.as_mut()) {
            (Some(p), None) => {
                if self.ignoring {
                    return out;
                }
                if p.x < screen.left() + EDGE || p.x > screen.right() - EDGE {
                    self.ignoring = true;
                    return out;
                }
                self.touch = Some(Touch {
                    start: p,
                    start_time: s.time,
                    last: p,
                    axis: None,
                    fingers: s.fingers.max(1),
                    held: false,
                    trail: vec![(s.time, p)],
                });
                out.push(Event::Down(p));
            }
            (Some(p), Some(t)) => {
                let delta = p - t.last;
                t.last = p;
                t.fingers = t.fingers.max(s.fingers);
                t.trail.push((s.time, p));
                t.trail.retain(|(time, _)| s.time - time <= 0.1);
                let travel = p - t.start;
                if t.fingers > 1 {
                    out.push(Event::TwoFingers(delta));
                    // Two fingers lock the axis for the flick they may end in.
                    if t.axis.is_none() && travel.length() > LOCK {
                        t.axis = Some(if travel.x.abs() >= travel.y.abs() {
                            Axis::X
                        } else {
                            Axis::Y
                        });
                    }
                    return out;
                }
                if t.axis.is_none() && !t.held {
                    if travel.length() > LOCK {
                        t.axis = Some(if travel.x.abs() >= travel.y.abs() {
                            Axis::X
                        } else {
                            Axis::Y
                        });
                        // The travel that decided it counts as the first slide.
                        let axis = t.axis.unwrap_or(Axis::X);
                        out.push(Event::Slide {
                            axis,
                            delta: axis.of(travel),
                            across: axis.other().of(travel),
                            total: axis.of(travel),
                            speed: speed(&t.trail, axis),
                        });
                    } else if s.time - t.start_time >= HOLD {
                        t.held = true;
                        out.push(Event::Hold(t.start));
                    }
                } else if let Some(axis) = t.axis {
                    if delta != Vec2::ZERO {
                        out.push(Event::Slide {
                            axis,
                            delta: axis.of(delta),
                            across: axis.other().of(delta),
                            total: axis.of(travel),
                            speed: speed(&t.trail, axis),
                        });
                    }
                }
            }
            (None, Some(_)) => {
                let t = self.touch.take().expect("matched Some");
                if t.held {
                    out.push(Event::HoldEnd);
                } else if let Some(axis) = t.axis {
                    let along = axis.of(t.last - t.start);
                    let fast = speed(&t.trail, axis).abs() >= FLICK_SPEED;
                    if along.abs() >= FLICK_DISTANCE || (along.abs() >= FLICK_MIN && fast) {
                        out.push(Event::Flick {
                            axis,
                            sign: if along > 0.0 { 1 } else { -1 },
                            fingers: t.fingers,
                        });
                    }
                    out.push(Event::Released);
                } else if t.fingers == 1 {
                    let at = t.start;
                    match self.last_tap {
                        Some((when, where_))
                            if s.time - when <= DOUBLE_TAP
                                && (at - where_).length() <= DOUBLE_TAP_REACH =>
                        {
                            self.last_tap = None;
                            out.push(Event::DoubleTap(at));
                        }
                        _ => {
                            self.last_tap = Some((s.time, at));
                            out.push(Event::Tap(at));
                        }
                    }
                }
            }
            (None, None) => self.ignoring = false,
        }
        out
    }
}

/// Speed along `axis` over the trail, points per second.
fn speed(trail: &[(f64, Pos2)], axis: Axis) -> f32 {
    match (trail.first(), trail.last()) {
        (Some((t0, p0)), Some((t1, p1))) if t1 > t0 => axis.of(*p1 - *p0) / (t1 - t0) as f32,
        _ => 0.0,
    }
}

/// The egui input as a [`Sample`].
pub fn sample(ctx: &egui::Context) -> Sample {
    // Outside `input`: the context's lock is not taken twice.
    let fingers = ctx.multi_touch().map_or(1, |m| m.num_touches);
    ctx.input(|i| Sample {
        time: i.time,
        pos: i.pointer.latest_pos().filter(|_| i.pointer.any_down()),
        fingers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    const SCREEN: Rect = Rect {
        min: pos2(0.0, 0.0),
        max: pos2(400.0, 800.0),
    };

    /// A finger from `from` to `to` over `seconds`, in 60 frames a second, then let go.
    fn stroke(r: &mut Recogniser, t0: f64, from: Pos2, to: Pos2, seconds: f64) -> Vec<Event> {
        let frames = ((seconds * 60.0).round() as usize).max(1);
        let mut out = Vec::new();
        for i in 0..=frames {
            let f = i as f32 / frames as f32;
            let p = from + (to - from) * f;
            out.extend(r.update(
                Sample {
                    time: t0 + seconds * i as f64 / frames as f64,
                    pos: Some(p),
                    fingers: 1,
                },
                SCREEN,
            ));
        }
        out.extend(r.update(
            Sample {
                time: t0 + seconds + 1.0 / 60.0,
                pos: None,
                fingers: 0,
            },
            SCREEN,
        ));
        out
    }

    fn flicks(events: &[Event]) -> Vec<(Axis, i32)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Flick { axis, sign, .. } => Some((*axis, *sign)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_short_fast_swipe_is_a_flick_and_a_slow_one_is_not() {
        let mut r = Recogniser::default();
        let fast = stroke(&mut r, 0.0, pos2(200.0, 400.0), pos2(170.0, 402.0), 0.05);
        assert_eq!(flicks(&fast), [(Axis::X, -1)]);
        let slow = stroke(&mut r, 5.0, pos2(200.0, 400.0), pos2(170.0, 402.0), 0.6);
        assert!(flicks(&slow).is_empty(), "{slow:?}");
        let long = stroke(&mut r, 9.0, pos2(100.0, 400.0), pos2(300.0, 410.0), 1.0);
        assert_eq!(flicks(&long), [(Axis::X, 1)]);
    }

    #[test]
    fn the_axis_locks_once_and_a_wobble_does_not_turn_it() {
        let mut r = Recogniser::default();
        let mut slides = Vec::new();
        // Right 14 points, then mostly down: still a horizontal slide.
        for (i, p) in [
            (200.0, 400.0),
            (214.0, 402.0),
            (216.0, 440.0),
            (230.0, 480.0),
        ]
        .into_iter()
        .enumerate()
        {
            for e in r.update(
                Sample {
                    time: i as f64 / 60.0,
                    pos: Some(pos2(p.0, p.1)),
                    fingers: 1,
                },
                SCREEN,
            ) {
                if let Event::Slide { axis, .. } = e {
                    slides.push(axis);
                }
            }
        }
        assert!(!slides.is_empty());
        assert!(slides.iter().all(|a| *a == Axis::X), "{slides:?}");
        // Under the lock distance nothing slides.
        let mut r = Recogniser::default();
        let small = stroke(&mut r, 0.0, pos2(200.0, 400.0), pos2(206.0, 405.0), 0.2);
        assert!(!small.iter().any(|e| matches!(e, Event::Slide { .. })));
    }

    #[test]
    fn taps_double_taps_and_holds() {
        let mut r = Recogniser::default();
        let at = pos2(200.0, 400.0);
        let one = stroke(&mut r, 0.0, at, at, 0.05);
        assert!(one.contains(&Event::Tap(at)));
        let two = stroke(&mut r, 0.2, at + egui::vec2(4.0, 0.0), at, 0.05);
        assert!(
            two.iter().any(|e| matches!(e, Event::DoubleTap(_))),
            "{two:?}"
        );
        // Taps too far apart in time are two taps.
        let a = stroke(&mut r, 3.0, at, at, 0.05);
        let b = stroke(&mut r, 4.0, at, at, 0.05);
        assert!(a
            .iter()
            .chain(&b)
            .all(|e| !matches!(e, Event::DoubleTap(_))));
        // Held still: a hold, then its end, and no tap.
        let held = stroke(&mut r, 8.0, at, at, 0.8);
        assert!(held.contains(&Event::Hold(at)));
        assert!(held.contains(&Event::HoldEnd));
        assert!(!held.iter().any(|e| matches!(e, Event::Tap(_))));
    }

    #[test]
    fn a_touch_at_the_edge_is_left_to_the_system() {
        let mut r = Recogniser::default();
        let edge = stroke(&mut r, 0.0, pos2(4.0, 400.0), pos2(200.0, 400.0), 0.2);
        assert!(edge.is_empty(), "{edge:?}");
        let right = stroke(&mut r, 1.0, pos2(396.0, 400.0), pos2(200.0, 400.0), 0.2);
        assert!(right.is_empty());
        // The next touch, away from the edge, is ours again.
        let next = stroke(&mut r, 2.0, pos2(100.0, 400.0), pos2(300.0, 400.0), 0.2);
        assert_eq!(flicks(&next), [(Axis::X, 1)]);
    }

    #[test]
    fn two_fingers_never_slide_but_can_flick() {
        let mut r = Recogniser::default();
        let mut events = Vec::new();
        for i in 0..=10 {
            events.extend(r.update(
                Sample {
                    time: i as f64 / 60.0,
                    pos: Some(pos2(100.0 + 20.0 * i as f32, 400.0)),
                    fingers: 2,
                },
                SCREEN,
            ));
        }
        events.extend(r.update(
            Sample {
                time: 0.5,
                pos: None,
                fingers: 0,
            },
            SCREEN,
        ));
        assert!(!events.iter().any(|e| matches!(e, Event::Slide { .. })));
        assert!(events.iter().any(|e| matches!(e, Event::TwoFingers(_))));
        assert!(events.contains(&Event::Flick {
            axis: Axis::X,
            sign: 1,
            fingers: 2
        }));
    }
}
