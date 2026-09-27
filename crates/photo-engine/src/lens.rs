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
    fn nonsense_is_neutral() {
        let m = LensModel::new(&Lens {
            distortion: f32::NAN,
            ca_red: f32::INFINITY,
            ca_blue: f32::NEG_INFINITY,
            vignetting: f32::NAN,
        });
        assert_eq!(m, LensModel::IDENTITY);
    }
}
