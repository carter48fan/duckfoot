//! Local adjustments (W-01, DEC-5): up to eight masks, each a linear gradient, a radial
//! (an ellipse) or a brush (painted dabs), with one fixed block of adjustments. No layers, blend modes or mask
//! arithmetic: the set is fixed, so it evaluates analytically in the fused adjust pass,
//! and the pipeline's order stays the pipeline's.
//!
//! Masks live in sensor coordinates (0..1 of the sensor's width and height), so they stay
//! on what they were drawn over when the photo is turned, flipped or cropped. Radial radii
//! are fractions of the sensor's long side, so a circle stays a circle.
//!
//! A brush mask is its dabs, in the order painted. The engine rasterises them into a small
//! texture (see [`BRUSH_SIDE`]) that preview and export both sample, so they agree; the CPU
//! evaluates the same dabs the same way for hit tests.

use serde::{Deserialize, Serialize};

/// The most masks a photo has: the shader's fixed array.
pub const MAX_MASKS: usize = 8;
/// Dabs one brush mask keeps: a few long strokes. A sidecar stays small.
pub const MAX_DABS: usize = 6000;
/// The long side, in texels, of the texture a brush mask is rasterised into. Brush edges are
/// soft, so ~6 photosites a texel on a 24 MP sensor shows no steps, and eight layers cost
/// under 6 MB.
pub const BRUSH_SIDE: u32 = 1024;

/// One touch of the brush: a soft disc.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dab {
    /// Sensor position, 0..1 of width and height.
    pub at: [f32; 2],
    /// A fraction of the sensor's long side.
    pub radius: f32,
    /// Where the edge starts to fade, as a fraction of the radius: 0 is all feather.
    pub hardness: f32,
    /// Take the mask away under it instead of adding.
    #[serde(default)]
    pub erase: bool,
}

impl Dab {
    /// How much of this dab covers a point `d` of its radius from its centre.
    pub fn coverage(&self, d: f32) -> f32 {
        1.0 - smoothstep(self.hardness.clamp(0.0, 0.99), 1.0, d)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Painted: where the dabs cover, in the order they were laid down.
    Brush { dabs: Vec<Dab> },
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
    /// Noise reduction, 0..=100: where the mask applies, at least this much (W-01).
    #[serde(default)]
    pub denoise: f32,
}

/// Restrict a local mask to pixels in a specific luminance or color range.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum RangeMask {
    #[default]
    None,
    /// Restrict by pixel luminance (0.0..=1.0).
    Luminance {
        /// Lower and upper bounds of luminance.
        range: [f32; 2],
        /// Feather softness of the bounds transition.
        feather: f32,
    },
    /// Restrict by pixel hue (0.0..=360.0 degrees).
    Color {
        /// Center hue in degrees.
        hue: f32,
        /// Angular spread around target hue in degrees.
        spread: f32,
        /// Angular feather softness in degrees.
        feather: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub range: RangeMask,
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
            range: RangeMask::None,
        }
    }

    pub fn linear(from: [f32; 2], to: [f32; 2]) -> Self {
        Self {
            shape: Shape::Linear { from, to },
            invert: false,
            on: true,
            adjust: Adjust::default(),
            range: RangeMask::None,
        }
    }

    pub fn brush() -> Self {
        Self {
            shape: Shape::Brush { dabs: Vec::new() },
            invert: false,
            on: true,
            adjust: Adjust::default(),
            range: RangeMask::None,
        }
    }

    /// "radial", "gradient" or "brush", for the mask list.
    pub fn kind(&self) -> &'static str {
        match self.shape {
            Shape::Linear { .. } => "gradient",
            Shape::Radial { .. } => "radial",
            Shape::Brush { .. } => "brush",
        }
    }

    /// How much of this mask's adjustment applies at sensor position `p` (0..1 of the
    /// sensor), for a `sensor`-sized image. The shader's `mask_weight`.
    pub fn weight(&self, p: [f32; 2], sensor: (u32, u32)) -> f32 {
        let (w, h) = (sensor.0 as f32, sensor.1 as f32);
        let px = [p[0] * w, p[1] * h];
        let inside = match &self.shape {
            Shape::Brush { dabs } => {
                let long = w.max(h);
                dabs.iter().fold(0.0, |v, dab| {
                    let d = (px[0] - dab.at[0] * w).hypot(px[1] - dab.at[1] * h)
                        / (dab.radius * long).max(0.5);
                    composite(v, dab, d)
                })
            }
            &Shape::Linear { from, to } => {
                let (a, b) = ([from[0] * w, from[1] * h], [to[0] * w, to[1] * h]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let len2 = (d[0] * d[0] + d[1] * d[1]).max(1e-6);
                let t = (((px[0] - a[0]) * d[0] + (px[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0);
                1.0 - smoothstep(0.0, 1.0, t)
            }
            &Shape::Radial {
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
            Shape::Brush { dabs } => Shape::Brush {
                dabs: dabs
                    .into_iter()
                    .filter(|d| {
                        d.at.iter()
                            .chain([&d.radius, &d.hardness])
                            .all(|v| v.is_finite())
                    })
                    .take(MAX_DABS)
                    .map(|d| Dab {
                        at: d.at.map(|v| v.clamp(-1.0, 2.0)),
                        radius: d.radius.clamp(0.001, 1.0),
                        hardness: d.hardness.clamp(0.0, 0.99),
                        erase: d.erase,
                    })
                    .collect(),
            },
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
        a.denoise = f(a.denoise, 0.0, 100.0, 0.0);
        self.range = match self.range {
            RangeMask::None => RangeMask::None,
            RangeMask::Luminance { range, feather } => RangeMask::Luminance {
                range: [f(range[0], 0.0, 1.0, 0.0), f(range[1], 0.0, 1.0, 1.0)],
                feather: f(feather, 0.001, 1.0, 0.1),
            },
            RangeMask::Color { hue, spread, feather } => RangeMask::Color {
                hue: f(hue, 0.0, 360.0, 0.0),
                spread: f(spread, 1.0, 180.0, 30.0),
                feather: f(feather, 0.001, 180.0, 10.0),
            },
        };
        self
    }

    /// Calculate range mask attenuation for given RGB sample [0.0..1.0].
    pub fn range_factor(&self, rgb: [f32; 3]) -> f32 {
        match self.range {
            RangeMask::None => 1.0,
            RangeMask::Luminance { range, feather } => {
                let lum = (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]).clamp(0.0, 1.0);
                let f = feather.max(0.001);
                let lower = smoothstep(range[0] - f, range[0], lum);
                let upper = 1.0 - smoothstep(range[1], range[1] + f, lum);
                lower * upper
            }
            RangeMask::Color { hue, spread, feather } => {
                let max = rgb[0].max(rgb[1]).max(rgb[2]);
                let min = rgb[0].min(rgb[1]).min(rgb[2]);
                let delta = max - min;
                if delta <= 1e-4 {
                    return 0.0;
                }
                let h = if (max - rgb[0]).abs() < 1e-5 {
                    60.0 * (((rgb[1] - rgb[2]) / delta) % 6.0)
                } else if (max - rgb[1]).abs() < 1e-5 {
                    60.0 * (((rgb[2] - rgb[0]) / delta) + 2.0)
                } else {
                    60.0 * (((rgb[0] - rgb[1]) / delta) + 4.0)
                };
                let h = (h + 360.0) % 360.0;
                let diff = ((h - hue + 540.0) % 360.0 - 180.0).abs();
                let f = feather.max(0.001);
                1.0 - smoothstep(spread, spread + f, diff)
            }
        }
    }
}

/// One dab laid over coverage `v`, `d` of its radius from its centre.
fn composite(v: f32, dab: &Dab, d: f32) -> f32 {
    let c = dab.coverage(d);
    if dab.erase {
        v * (1.0 - c)
    } else {
        v.max(c)
    }
}

/// A brush mask's texture: `w × h` coverage bytes for a sensor of the given aspect, into
/// which dabs are laid one at a time. Returns the texel rectangle that changed
/// (x, y, width, height), so only that has to be uploaded.
pub fn rasterise(buf: &mut [u8], (w, h): (u32, u32), dabs: &[Dab]) -> Option<(u32, u32, u32, u32)> {
    let long = w.max(h) as f32;
    let mut dirty: Option<(u32, u32, u32, u32)> = None;
    for dab in dabs {
        let (cx, cy) = (dab.at[0] * w as f32, dab.at[1] * h as f32);
        let r = (dab.radius * long).max(0.5);
        let x0 = ((cx - r).floor().max(0.0) as u32).min(w);
        let y0 = ((cy - r).floor().max(0.0) as u32).min(h);
        let x1 = ((cx + r).ceil().max(0.0) as u32).min(w);
        let y1 = ((cy + r).ceil().max(0.0) as u32).min(h);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let d = ((x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy)) / r;
                if d >= 1.0 {
                    continue;
                }
                let i = (y * w + x) as usize;
                let v = composite(buf[i] as f32 / 255.0, dab, d);
                buf[i] = (v * 255.0).round() as u8;
            }
        }
        dirty = Some(match dirty {
            None => (x0, y0, x1, y1),
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
        });
    }
    dirty.map(|(a, b, c, d)| (a, b, c - a, d - b))
}

/// The texture size for a brush mask on a `sensor`-sized image: [`BRUSH_SIDE`] on the long
/// side, the sensor's aspect.
pub fn brush_size(sensor: (u32, u32)) -> (u32, u32) {
    let (w, h) = (sensor.0.max(1) as f32, sensor.1.max(1) as f32);
    let s = BRUSH_SIDE as f32 / w.max(h);
    (
        ((w * s).round() as u32).max(1),
        ((h * s).round() as u32).max(1),
    )
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
                .cloned()
                .map(Mask::sanitised)
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
    fn a_brush_covers_where_it_was_painted_and_erasing_takes_it_back() {
        let dab = |x: f32, erase: bool| Dab {
            at: [x, 0.5],
            radius: 0.05,
            hardness: 0.5,
            erase,
        };
        let mut m = Mask::brush();
        m.shape = Shape::Brush {
            dabs: vec![dab(0.3, false), dab(0.35, false)],
        };
        let s = (1000, 500);
        assert_eq!(m.weight([0.3, 0.5], s), 1.0);
        assert_eq!(m.weight([0.8, 0.5], s), 0.0);
        let Shape::Brush { dabs } = &mut m.shape else {
            unreachable!()
        };
        dabs.push(dab(0.3, true));
        assert_eq!(m.weight([0.3, 0.5], s), 0.0);
        // Only half erased where the eraser's edge fades.
        let edge = m.weight([0.37, 0.5], s);
        assert!(edge > 0.9, "{edge}");

        // The texture holds the same coverage.
        let size = brush_size(s);
        assert_eq!(size, (1024, 512));
        let mut buf = vec![0u8; (size.0 * size.1) as usize];
        let Shape::Brush { dabs } = &m.shape else {
            unreachable!()
        };
        let dirty = rasterise(&mut buf, size, &dabs[..2]).unwrap();
        let at = |buf: &[u8], x: f32, y: f32| {
            buf[((y * size.1 as f32) as u32 * size.0 + (x * size.0 as f32) as u32) as usize]
        };
        assert_eq!(at(&buf, 0.3, 0.5), 255);
        assert_eq!(at(&buf, 0.8, 0.5), 0);
        assert!(dirty.0 < 307 && dirty.0 + dirty.2 > 358, "{dirty:?}");
        rasterise(&mut buf, size, &dabs[2..]);
        assert_eq!(at(&buf, 0.3, 0.5), 0);
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

    #[test]
    fn range_mask_restricts_by_luminance_and_color() {
        let mut m = Mask::radial([0.5, 0.5], 0.5);
        // By default, no range mask -> factor is 1.0 everywhere
        assert_eq!(m.range_factor([0.1, 0.1, 0.1]), 1.0);
        assert_eq!(m.range_factor([0.9, 0.9, 0.9]), 1.0);

        // Luminance range mask: highlight only (0.7..1.0, feather 0.05)
        m.range = RangeMask::Luminance {
            range: [0.7, 1.0],
            feather: 0.05,
        };
        assert_eq!(m.range_factor([0.1, 0.1, 0.1]), 0.0);
        assert_eq!(m.range_factor([0.9, 0.9, 0.9]), 1.0);

        // Color range mask: target green (hue ~120°, spread 30°, feather 5°)
        m.range = RangeMask::Color {
            hue: 120.0,
            spread: 30.0,
            feather: 5.0,
        };
        // Pure green: hue 120°
        assert_eq!(m.range_factor([0.0, 1.0, 0.0]), 1.0);
        // Pure red: hue 0° -> completely out of range
        assert_eq!(m.range_factor([1.0, 0.0, 0.0]), 0.0);
    }
}
