//! Local adjustments (W-01, DEC-5): up to eight masks, each a linear gradient or a radial
//! (an ellipse), with one fixed block of adjustments. No layers, blend modes or mask
//! arithmetic: the set is fixed, so it evaluates analytically in the fused adjust pass,
//! and the pipeline's order stays the pipeline's.
//!
//! Masks live in sensor coordinates (0..1 of the sensor's width and height), so they stay
//! on what they were drawn over when the photo is turned, flipped or cropped. Radial radii
//! are fractions of the sensor's long side, so a circle stays a circle.

use serde::{Deserialize, Serialize};

/// The most masks a photo has: the shader's fixed array.
pub const MAX_MASKS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// Full effect at `from`, none at `to`, fading between: a graduated filter.
    Linear { from: [f32; 2], to: [f32; 2] },
    /// An ellipse about `centre`, radii a fraction of the long side, turned by `angle`
    /// degrees; full effect inside, fading out over the outer `feather` of the radius.
    Radial {
        centre: [f32; 2],
        radius: [f32; 2],
        angle: f32,
        feather: f32,
    },
}

/// What a mask does where it applies. Sliders are -100..=100 except exposure, in stops.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Adjust {
    pub exposure: f32,
    pub temperature: f32,
    pub tint: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub clarity: f32,
    /// 0..=100.
    pub sharpness: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub shape: Shape,
    /// Apply outside the shape instead of inside it.
    #[serde(default)]
    pub invert: bool,
    /// Off keeps the mask and its settings but stops it applying.
    #[serde(default = "yes")]
    pub on: bool,
    #[serde(default)]
    pub adjust: Adjust,
}

fn yes() -> bool {
    true
}

impl Mask {
    pub fn radial(centre: [f32; 2], radius: f32) -> Self {
        Self {
            shape: Shape::Radial {
                centre,
                radius: [radius, radius],
                angle: 0.0,
                feather: 0.5,
            },
            invert: false,
            on: true,
            adjust: Adjust::default(),
        }
    }

    pub fn linear(from: [f32; 2], to: [f32; 2]) -> Self {
        Self {
            shape: Shape::Linear { from, to },
            invert: false,
            on: true,
            adjust: Adjust::default(),
        }
    }

    /// "radial" or "gradient", for the mask list.
    pub fn kind(&self) -> &'static str {
        match self.shape {
            Shape::Linear { .. } => "gradient",
            Shape::Radial { .. } => "radial",
        }
    }

    /// How much of this mask's adjustment applies at sensor position `p` (0..1 of the
    /// sensor), for a `sensor`-sized image. The shader's `mask_weight`.
    pub fn weight(&self, p: [f32; 2], sensor: (u32, u32)) -> f32 {
        let (w, h) = (sensor.0 as f32, sensor.1 as f32);
        let px = [p[0] * w, p[1] * h];
        let inside = match self.shape {
            Shape::Linear { from, to } => {
                let (a, b) = ([from[0] * w, from[1] * h], [to[0] * w, to[1] * h]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let len2 = (d[0] * d[0] + d[1] * d[1]).max(1e-6);
                let t = (((px[0] - a[0]) * d[0] + (px[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0);
                1.0 - smoothstep(0.0, 1.0, t)
            }
            Shape::Radial {
                centre,
                radius,
                angle,
                feather,
            } => {
                let long = w.max(h);
                let (s, c) = angle.to_radians().sin_cos();
                let q = [px[0] - centre[0] * w, px[1] - centre[1] * h];
                let (x, y) = (q[0] * c + q[1] * s, -q[0] * s + q[1] * c);
                let (rx, ry) = ((radius[0] * long).max(1.0), (radius[1] * long).max(1.0));
                let d = ((x / rx).powi(2) + (y / ry).powi(2)).sqrt();
                let f = feather.clamp(0.01, 1.0);
                1.0 - smoothstep(1.0 - f, 1.0, d)
            }
        };
        if self.invert {
            1.0 - inside
        } else {
            inside
        }
    }

    fn sanitised(mut self) -> Self {
        let f = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.shape = match self.shape {
            Shape::Linear { from, to } => Shape::Linear {
                from: from.map(|v| f(v, -1.0, 2.0, 0.5)),
                to: to.map(|v| f(v, -1.0, 2.0, 0.5)),
            },
            Shape::Radial {
                centre,
                radius,
                angle,
                feather,
            } => Shape::Radial {
                centre: centre.map(|v| f(v, -1.0, 2.0, 0.5)),
                radius: radius.map(|v| f(v, 0.005, 2.0, 0.2)),
                angle: f(angle, -360.0, 360.0, 0.0),
                feather: f(feather, 0.01, 1.0, 0.5),
            },
        };
        let a = &mut self.adjust;
        a.exposure = f(a.exposure, -4.0, 4.0, 0.0);
        for v in [
            &mut a.temperature,
            &mut a.tint,
            &mut a.contrast,
            &mut a.saturation,
            &mut a.clarity,
        ] {
            *v = f(*v, -100.0, 100.0, 0.0);
        }
        a.sharpness = f(a.sharpness, 0.0, 100.0, 0.0);
        self
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The Local module's parameters: its masks, in the order they were added.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Locals {
    #[serde(default)]
    pub masks: Vec<Mask>,
}

impl Locals {
    pub fn sanitised(&self) -> Self {
        Self {
            masks: self
                .masks
                .iter()
                .take(MAX_MASKS)
                .map(|m| m.sanitised())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_radial_is_full_inside_and_nothing_beyond_its_radius() {
        let m = Mask::radial([0.5, 0.5], 0.2);
        let s = (1000, 500);
        assert_eq!(m.weight([0.5, 0.5], s), 1.0);
        // 0.3 of the long side out, past the 0.2 radius.
        assert_eq!(m.weight([0.8, 0.5], s), 0.0);
        // A circle, not stretched to the frame: 0.1 of the long side up is 100 px, 0.2 of
        // the height.
        let up = m.weight([0.5, 0.3], s);
        let right = m.weight([0.6, 0.5], s);
        assert!((up - right).abs() < 1e-5, "{up} vs {right}");
        let mut inv = m;
        inv.invert = true;
        assert_eq!(inv.weight([0.5, 0.5], s), 0.0);
    }

    #[test]
    fn a_gradient_fades_from_its_start_to_its_end() {
        let m = Mask::linear([0.5, 0.0], [0.5, 0.5]);
        let s = (600, 400);
        assert_eq!(m.weight([0.2, 0.0], s), 1.0);
        assert!((m.weight([0.9, 0.25], s) - 0.5).abs() < 1e-5);
        assert_eq!(m.weight([0.5, 0.8], s), 0.0);
    }

    #[test]
    fn nonsense_is_brought_back_and_the_count_capped() {
        let mut bad = Mask::radial([f32::NAN, 9.0], -1.0);
        bad.adjust.exposure = 99.0;
        let l = Locals {
            masks: vec![bad; 12],
        }
        .sanitised();
        assert_eq!(l.masks.len(), MAX_MASKS);
        let Shape::Radial { centre, radius, .. } = l.masks[0].shape else {
            panic!()
        };
        assert_eq!(centre, [0.5, 2.0]);
        assert_eq!(radius, [0.005, 0.005]);
        assert_eq!(l.masks[0].adjust.exposure, 4.0);
    }
}
