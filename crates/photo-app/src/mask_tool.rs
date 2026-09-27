//! Placing local masks on the stage: where a mask's handles are on screen, which one a
//! pointer is on, and what dragging it does. Geometry only, like `crop_tool`, so it is
//! tested without a window.
//!
//! Masks are stored in sensor coordinates (0..1 of the sensor), the stage shows the turned,
//! straightened, uncropped frame; [`Mapping`] goes between the two.

use egui::{pos2, Pos2, Rect};
use photo_engine::local::{Mask, Shape};
use photo_engine::Geometry;

/// Between sensor coordinates and the screen, for one frame of the stage.
#[derive(Debug, Clone, Copy)]
pub struct Mapping {
    pub geometry: Geometry,
    pub sensor: (u32, u32),
    /// Where the whole turned, uncropped frame sits on screen.
    pub frame: Rect,
}

impl Mapping {
    fn out_size(&self) -> (f32, f32) {
        let (w, h) = self.geometry.output_size(self.sensor.0, self.sensor.1);
        (w as f32, h as f32)
    }

    pub fn to_screen(&self, p: [f32; 2]) -> Pos2 {
        let (sw, sh) = (self.sensor.0 as f32, self.sensor.1 as f32);
        let (ox, oy) =
            photo_engine::output_point(&self.geometry, self.sensor, (p[0] * sw, p[1] * sh));
        let (w, h) = self.out_size();
        pos2(
            self.frame.left() + ox / w * self.frame.width(),
            self.frame.top() + oy / h * self.frame.height(),
        )
    }

    pub fn to_sensor(&self, p: Pos2) -> [f32; 2] {
        let (w, h) = self.out_size();
        let out = (
            (p.x - self.frame.left()) / self.frame.width() * w,
            (p.y - self.frame.top()) / self.frame.height() * h,
        );
        let stack = photo_engine::PhotoStack {
            geometry: self.geometry,
            ..Default::default()
        };
        let (sx, sy) = photo_engine::sensor_point(&stack, None, self.sensor, out);
        [sx / self.sensor.0 as f32, sy / self.sensor.1 as f32]
    }

    fn long(&self) -> f32 {
        self.sensor.0.max(self.sensor.1) as f32
    }

    /// A mask's handles, on screen.
    pub fn handles(&self, mask: &Mask) -> Vec<(Handle, Pos2)> {
        match mask.shape {
            Shape::Linear { from, to } => {
                let mid = [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5];
                vec![
                    (Handle::From, self.to_screen(from)),
                    (Handle::To, self.to_screen(to)),
                    (Handle::Move, self.to_screen(mid)),
                ]
            }
            Shape::Radial {
                centre,
                radius,
                angle,
                ..
            } => {
                let (s, c) = angle.to_radians().sin_cos();
                let (sw, sh) = (self.sensor.0 as f32, self.sensor.1 as f32);
                let at =
                    |dx: f32, dy: f32| self.to_screen([centre[0] + dx / sw, centre[1] + dy / sh]);
                let (rx, ry) = (radius[0] * self.long(), radius[1] * self.long());
                vec![
                    (Handle::Move, self.to_screen(centre)),
                    (Handle::RadiusX, at(rx * c, rx * s)),
                    (Handle::RadiusY, at(-ry * s, ry * c)),
                ]
            }
        }
    }

    /// Where a mask's pin is drawn, to choose it from the photo: its middle.
    pub fn pin(&self, mask: &Mask) -> Pos2 {
        match mask.shape {
            Shape::Linear { from, to } => {
                self.to_screen([(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5])
            }
            Shape::Radial { centre, .. } => self.to_screen(centre),
        }
    }

    /// The outline to draw: a radial's ellipse, or a gradient's three lines (start, middle,
    /// end), each as a polyline on screen.
    pub fn outline(&self, mask: &Mask) -> Vec<Vec<Pos2>> {
        let (sw, sh) = (self.sensor.0 as f32, self.sensor.1 as f32);
        match mask.shape {
            Shape::Radial {
                centre,
                radius,
                angle,
                feather,
            } => {
                let (s, c) = angle.to_radians().sin_cos();
                let ring = |k: f32| -> Vec<Pos2> {
                    (0..=64)
                        .map(|i| {
                            let t = i as f32 / 64.0 * std::f32::consts::TAU;
                            let (x, y) = (
                                t.cos() * radius[0] * self.long() * k,
                                t.sin() * radius[1] * self.long() * k,
                            );
                            self.to_screen([
                                centre[0] + (x * c - y * s) / sw,
                                centre[1] + (x * s + y * c) / sh,
                            ])
                        })
                        .collect()
                };
                vec![ring(1.0), ring(1.0 - feather.clamp(0.01, 1.0))]
            }
            Shape::Linear { from, to } => {
                // Across the gradient's direction, as far as the frame reaches.
                let d = [(to[0] - from[0]) * sw, (to[1] - from[1]) * sh];
                let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-3);
                let across = [-d[1] / len, d[0] / len];
                let reach = sw.hypot(sh);
                let line = |p: [f32; 2]| {
                    vec![
                        self.to_screen([
                            p[0] - across[0] * reach / sw,
                            p[1] - across[1] * reach / sh,
                        ]),
                        self.to_screen([
                            p[0] + across[0] * reach / sw,
                            p[1] + across[1] * reach / sh,
                        ]),
                    ]
                };
                let mid = [(from[0] + to[0]) * 0.5, (from[1] + to[1]) * 0.5];
                vec![line(from), line(mid), line(to)]
            }
        }
    }

    /// The handle under screen point `p`, within `tolerance` pixels; inside a radial or
    /// on a gradient's middle line, the whole mask.
    pub fn hit(&self, mask: &Mask, p: Pos2, tolerance: f32) -> Option<Handle> {
        let near = self
            .handles(mask)
            .into_iter()
            .map(|(h, at)| (h, at.distance(p)))
            .filter(|(_, d)| *d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(h, _)| h);
        if near.is_some() {
            return near;
        }
        let s = self.to_sensor(p);
        match mask.shape {
            Shape::Radial { .. } => {
                let mut solid = *mask;
                solid.invert = false;
                (solid.weight(s, self.sensor) > 0.0).then_some(Handle::Move)
            }
            Shape::Linear { .. } => {
                let mid = &self.outline(mask)[1];
                (distance_to_segment(p, mid[0], mid[1]) <= tolerance).then_some(Handle::Move)
            }
        }
    }

    /// `start` with `handle` dragged from screen point `from` to `to`.
    pub fn dragged(&self, start: &Mask, handle: Handle, from: Pos2, to: Pos2) -> Mask {
        let (a, b) = (self.to_sensor(from), self.to_sensor(to));
        let d = [b[0] - a[0], b[1] - a[1]];
        let mut out = *start;
        let (sw, sh) = (self.sensor.0 as f32, self.sensor.1 as f32);
        match (&mut out.shape, handle) {
            (Shape::Linear { from, to }, Handle::Move) => {
                *from = add(*from, d);
                *to = add(*to, d);
            }
            (Shape::Linear { from, .. }, Handle::From) => *from = add(*from, d),
            (Shape::Linear { to, .. }, Handle::To) => *to = add(*to, d),
            (Shape::Radial { centre, .. }, Handle::Move) => *centre = add(*centre, d),
            (
                Shape::Radial {
                    centre,
                    radius,
                    angle,
                    ..
                },
                Handle::RadiusX | Handle::RadiusY,
            ) => {
                // Where the pointer is from the centre, in sensor pixels.
                let q = [(b[0] - centre[0]) * sw, (b[1] - centre[1]) * sh];
                let dist = q[0].hypot(q[1]) / self.long();
                if handle == Handle::RadiusX {
                    // The long handle also turns the ellipse.
                    radius[0] = dist.max(0.005);
                    *angle = q[1].atan2(q[0]).to_degrees();
                } else {
                    radius[1] = dist.max(0.005);
                }
            }
            _ => {}
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    Move,
    From,
    To,
    RadiusX,
    RadiusY,
}

fn add(p: [f32; 2], d: [f32; 2]) -> [f32; 2] {
    [p[0] + d[0], p[1] + d[1]]
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(turns: u8) -> Mapping {
        let mut geometry = Geometry::default();
        for _ in 0..turns {
            geometry.rotate_right();
        }
        let (w, h) = geometry.output_size(600, 400);
        Mapping {
            geometry,
            sensor: (600, 400),
            frame: Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(w as f32, h as f32) * 0.5),
        }
    }

    #[test]
    fn screen_and_sensor_are_each_others_inverse_turned_or_not() {
        for turns in 0..4 {
            let m = mapping(turns);
            let p = [0.3, 0.7];
            let back = m.to_sensor(m.to_screen(p));
            assert!(
                (back[0] - 0.3).abs() < 1e-4 && (back[1] - 0.7).abs() < 1e-4,
                "{back:?}"
            );
        }
        // Unturned, the sensor's top-left is the frame's.
        let m = mapping(0);
        assert!(m.to_screen([0.0, 0.0]).distance(pos2(10.0, 20.0)) < 1e-3);
    }

    #[test]
    fn a_radial_moves_by_its_middle_and_grows_by_its_handles() {
        let m = mapping(1);
        let mask = Mask::radial([0.5, 0.5], 0.1);
        let centre = m.to_screen([0.5, 0.5]);
        assert_eq!(m.hit(&mask, centre, 8.0), Some(Handle::Move));
        let moved = m.dragged(&mask, Handle::Move, centre, centre + egui::vec2(15.0, 0.0));
        let Shape::Radial { centre: c, .. } = moved.shape else {
            panic!()
        };
        let now = m.to_screen(c);
        assert!((now.x - centre.x - 15.0).abs() < 1e-2 && (now.y - centre.y).abs() < 1e-2);

        let (h, at) = m.handles(&mask)[1];
        assert_eq!(h, Handle::RadiusX);
        assert_eq!(m.hit(&mask, at, 8.0), Some(Handle::RadiusX));
        let out = at + (at - centre); // twice as far out
        let grown = m.dragged(&mask, Handle::RadiusX, at, out);
        let Shape::Radial { radius, .. } = grown.shape else {
            panic!()
        };
        assert!((radius[0] - 0.2).abs() < 1e-3, "{radius:?}");
        // Far outside it, nothing is hit.
        assert_eq!(m.hit(&mask, pos2(-500.0, -500.0), 8.0), None);
    }

    #[test]
    fn a_gradients_ends_move_on_their_own() {
        let m = mapping(0);
        let mask = Mask::linear([0.5, 0.2], [0.5, 0.6]);
        let from = m.to_screen([0.5, 0.2]);
        assert_eq!(m.hit(&mask, from, 6.0), Some(Handle::From));
        let pulled = m.dragged(&mask, Handle::From, from, from + egui::vec2(0.0, -20.0));
        let Shape::Linear { from: f, to } = pulled.shape else {
            panic!()
        };
        assert_eq!(to, [0.5, 0.6]);
        assert!(f[1] < 0.2);
        // Anywhere along the middle line picks up the whole gradient.
        let mid = m.to_screen([0.1, 0.4]);
        assert_eq!(m.hit(&mask, mid, 6.0), Some(Handle::Move));
    }
}
