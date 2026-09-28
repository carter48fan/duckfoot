//! Manual lens corrections: radial distortion, lateral chromatic aberration and
//! vignetting, as the adjust shader applies them. The sliders are user units; this is
//! where they become the model's coefficients, once, for the shader and for the CPU
//! mirror of its mapping (`sensor_point`) alike.

use crate::params::Lens;

/// `k` at a distortion of ±100: a quarter of the half-diagonal at the corner, past the
/// strongest barrel a rectilinear zoom shows.
const MAX_K: f32 = 0.25;
/// Red or blue scaled by this at ±100: about 7 px at the corner of a 24 MP frame, past the
/// worst fringes a real lens leaves.
const MAX_CA: f32 = 0.002;
/// Stops of brightening at the corner at +100: a fast lens wide open.
const MAX_VIGNETTING_STOPS: f32 = 1.5;

/// The coefficients of one `Lens`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LensModel {
    /// r' = r·(1 + k·r²), r a fraction of the half-diagonal. Negative pulls the frame in
    /// (removes barrel), positive pushes it out (removes pincushion).
    pub k: f32,
    /// Applied before `k`, so the frame's corners land on the sensor's.
    pub zoom: f32,
    pub ca_red: f32,
    pub ca_blue: f32,
    pub vignetting_stops: f32,
}

impl LensModel {
    pub const IDENTITY: Self = Self {
        k: 0.0,
        zoom: 1.0,
        ca_red: 1.0,
        ca_blue: 1.0,
        vignetting_stops: 0.0,
    };

    pub fn new(lens: &Lens) -> Self {
        // Removing barrel is +distortion: the edges are pulled in toward where a straight
        // line would have put them, so the sensor is sampled nearer the middle.
        let k = -finite(lens.distortion).clamp(-100.0, 100.0) / 100.0 * MAX_K;
        Self {
            k,
            zoom: fill_zoom(k),
            ca_red: 1.0 + finite(lens.ca_red).clamp(-100.0, 100.0) / 100.0 * MAX_CA,
            ca_blue: 1.0 + finite(lens.ca_blue).clamp(-100.0, 100.0) / 100.0 * MAX_CA,
            vignetting_stops: finite(lens.vignetting).clamp(-100.0, 100.0) / 100.0
                * MAX_VIGNETTING_STOPS,
        }
    }

    /// Where the lens put the point `p` would have on an ideal sensor, both relative to the
    /// sensor's centre, in pixels. The shader's `lens_distorted`.
    pub fn distorted(&self, p: (f32, f32), sensor: (u32, u32)) -> (f32, f32) {
        let (x, y) = (p.0 * self.zoom, p.1 * self.zoom);
        let half_diag = (sensor.0 as f32).hypot(sensor.1 as f32) * 0.5;
        let r2 = (x * x + y * y) / (half_diag * half_diag);
        let f = 1.0 + self.k * r2;
        (x * f, y * f)
    }
}

/// The scale that puts the frame's corner (r = 1) back on the sensor's corner after a
/// pincushion correction, which would otherwise sample past the sensor there: the root of
/// s·(1 + k·s²) = 1. A barrel correction samples inside the sensor everywhere, so needs none.
/// Either way every edge point lands inside, since it has r < 1.
fn fill_zoom(k: f32) -> f32 {
    if k <= 0.0 {
        return 1.0;
    }
    let mut s = 1.0f32;
    for _ in 0..8 {
        s -= (s + k * s * s * s - 1.0) / (1.0 + 3.0 * k * s * s);
    }
    s
}

fn finite(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

/// A lens correction that comes with the photo rather than from the sliders: a DNG's own
/// `WarpRectilinear` (see `opcodes`), or a lensfun profile (see `lensfun`). In the sensor
/// texture's pixels, it says where each colour plane of an ideal image point was
/// recorded, and how much the lens darkened it there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileWarp {
    /// The optical centre, in texture pixels.
    pub centre: [f32; 2],
    /// The radius the polynomials are in units of.
    pub m: f32,
    /// Per plane (R, G, B): r' = r·f(r), f(r) = Σ cᵢ·rⁱ for i = 0..8.
    pub planes: [[f32; 8]; 3],
    /// Lateral colour, after the distortion: red's and blue's r' = r·(v + c·r + b·r²).
    pub tca: Option<[[f32; 3]; 2]>,
    /// The fall-off the lens recorded, 1 + k1·r² + k2·r⁴ + k3·r⁶, and the radius (in
    /// pixels) its r is in units of.
    pub vignetting: Option<([f32; 3], f32)>,
}

impl FileWarp {
    /// A DNG's warp, for a mosaic kept from `area` of the image the opcode describes.
    /// Radial terms only; the tangential ones, which no phone this was checked against
    /// sets, are left out.
    pub fn new(warp: &crate::opcodes::Warp, area: crate::opcodes::Area) -> Option<Self> {
        let (aw, ah) = (area.size.0 as f64, area.size.1 as f64);
        let (cx, cy) = (warp.centre.0 * aw, warp.centre.1 * ah);
        let m = crate::opcodes::max_corner_distance2(cx, cy, aw, ah).sqrt();
        // kr0 + kr1·r² + kr2·r⁴ + kr3·r⁶, as powers of r.
        let plane = |i: usize| {
            let k = warp.planes.get(i).or(warp.planes.first())?;
            let k = k.map(|v| v as f32);
            Some([k[0], 0.0, k[1], 0.0, k[2], 0.0, k[3], 0.0])
        };
        let planes = [plane(0)?, plane(1)?, plane(2)?];
        // A warp that moves nothing is no warp.
        if planes
            .iter()
            .all(|k| *k == [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
        {
            return None;
        }
        Some(Self {
            centre: [
                (cx - area.offset.0 as f64) as f32,
                (cy - area.offset.1 as f64) as f32,
            ],
            m: m as f32,
            planes,
            tca: None,
            vignetting: None,
        })
    }

    /// Where plane `plane` (0 R, 1 G, 2 B) of the ideal point `p` was recorded, both in
    /// texture pixels. The shader's `file_warp`, then its `tca`.
    pub fn source(&self, plane: usize, p: (f32, f32)) -> (f32, f32) {
        let scale = |d: (f32, f32), f: &dyn Fn(f32) -> f32| {
            let r = d.0.hypot(d.1) / self.m;
            let k = f(r);
            (d.0 * k, d.1 * k)
        };
        let c = self.planes[plane];
        let mut d = (p.0 - self.centre[0], p.1 - self.centre[1]);
        d = scale(d, &|r| {
            c[0] + r
                * (c[1] + r * (c[2] + r * (c[3] + r * (c[4] + r * (c[5] + r * (c[6] + r * c[7]))))))
        });
        if let (Some(tca), true) = (self.tca, plane != 1) {
            let [v, c1, b] = tca[plane / 2];
            d = scale(d, &|r| v + r * (c1 + r * b));
        }
        (self.centre[0] + d.0, self.centre[1] + d.1)
    }

    /// The gain that undoes the recorded fall-off at sensor position `p`.
    pub fn gain(&self, p: (f32, f32)) -> f32 {
        let Some((k, unit)) = self.vignetting else {
            return 1.0;
        };
        let r2 = ((p.0 - self.centre[0]).powi(2) + (p.1 - self.centre[1]).powi(2)) / (unit * unit);
        1.0 / (1.0 + r2 * (k[0] + r2 * (k[1] + r2 * k[2]))).max(0.05)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_lens_is_the_identity() {
        assert_eq!(LensModel::new(&Lens::default()), LensModel::IDENTITY);
        let m = LensModel::IDENTITY;
        assert_eq!(m.distorted((123.0, -45.0), (6000, 4000)), (123.0, -45.0));
    }

    #[test]
    fn corners_stay_on_the_sensor_either_way() {
        let sensor = (6000, 4000);
        let corner = (3000.0, 2000.0);
        for distortion in [-100.0, -40.0, 40.0, 100.0] {
            let m = LensModel::new(&Lens {
                distortion,
                ..Lens::default()
            });
            let (x, y) = m.distorted(corner, sensor);
            assert!(x <= 3000.01 && y <= 2000.01, "{distortion}: ({x}, {y})");
            // And the edges, which have r < 1.
            let (ex, _) = m.distorted((3000.0, 0.0), sensor);
            assert!(ex <= 3000.01, "{distortion}: edge at {ex}");
        }
        // A pincushion correction fills the frame exactly: its corner is the sensor's.
        let m = LensModel::new(&Lens {
            distortion: -100.0,
            ..Lens::default()
        });
        let (x, y) = m.distorted(corner, sensor);
        assert!((x - 3000.0).abs() < 0.05 && (y - 2000.0).abs() < 0.05);
    }

    #[test]
    fn removing_barrel_pulls_the_edges_in() {
        let m = LensModel::new(&Lens {
            distortion: 50.0,
            ..Lens::default()
        });
        let (x, _) = m.distorted((3000.0, 0.0), (6000, 4000));
        assert!(x < 3000.0 * 0.95, "{x}");
        // The centre does not move.
        assert_eq!(m.distorted((0.0, 0.0), (6000, 4000)), (0.0, 0.0));
    }

    #[test]
    fn a_file_warp_is_placed_in_the_kept_mosaic() {
        let warp = crate::opcodes::Warp {
            planes: vec![[1.0, -0.05, 0.0, 0.0, 0.0, 0.0]],
            centre: (0.5, 0.5),
        };
        // Kept from (10, 20) of a 200 × 100 active area.
        let area = crate::opcodes::Area {
            offset: (10, 20),
            size: (200, 100),
        };
        let w = FileWarp::new(&warp, area).unwrap();
        assert_eq!(w.centre, [90.0, 30.0]);
        // The centre does not move; the corner is pulled in by 5%.
        assert_eq!(w.source(1, (90.0, 30.0)), (90.0, 30.0));
        let m = (100.0f32 * 100.0 + 50.0 * 50.0).sqrt();
        let corner = (90.0 + 100.0, 30.0 + 50.0);
        let (x, y) = w.source(0, corner);
        assert!((x - (90.0 + 100.0 * 0.95)).abs() < 1e-3, "{x}");
        assert!((y - (30.0 + 50.0 * 0.95)).abs() < 1e-3, "{y}");
        assert!((w.m - m).abs() < 1e-3);
        // An identity warp is none.
        let flat = crate::opcodes::Warp {
            planes: vec![[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]],
            centre: (0.5, 0.5),
        };
        assert!(FileWarp::new(&flat, area).is_none());
    }

    #[test]
    fn nonsense_is_neutral() {
        let m = LensModel::new(&Lens {
            distortion: f32::NAN,
            ca_red: f32::INFINITY,
            ca_blue: f32::NEG_INFINITY,
            vignetting: f32::NAN,
            profile: true,
        });
        assert_eq!(m, LensModel::IDENTITY);
    }
}
