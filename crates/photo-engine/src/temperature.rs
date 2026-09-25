//! Colour temperature and tint: white balance the way a photographer reads it.
//!
//! The stack stores white balance as multipliers on the camera's as-shot neutral
//! (`WhiteBalance`), because that is what the shader applies. People set it in kelvin and
//! tint, so this is the two-way mapping between the two for one camera, through the
//! camera's own colour matrix. A temperature here names a real light: 3200 K is tungsten
//! on every camera, and the as-shot white reads as whatever the camera actually metered.
//!
//! Both directions must be exact inverses of each other. The inspector turns the stored
//! multipliers back into kelvin every frame, and any mismatch is fed back into the slider
//! on the next drag tick — the previous mapping lost ~58% of every temperature change that
//! way, so the slider slid back toward 5500 K as fast as it could be dragged.
//!
//! Definitions follow the DNG SDK (`dng_temperature`), which is what Lightroom and Camera
//! Raw show: Robertson's isotemperature lines in CIE 1960 uv, and tint as −3000 × the
//! distance from the Planckian locus. Setting a higher temperature warms the picture;
//! a positive tint makes it more magenta.

use crate::decode::{invert3, mul3v, XYZ_TO_SRGB};
use crate::params::WhiteBalance;

/// Tint units per unit of uv distance off the locus. Negative: DNG's tint grows toward
/// magenta, which is below the locus.
const TINT_SCALE: f64 = -3000.0;

/// Robertson's table: mired, u, v, and the slope of the isotemperature line.
#[rustfmt::skip]
const ROBERTSON: [[f64; 4]; 31] = [
    [  0.0, 0.18006, 0.26352,   -0.24341],
    [ 10.0, 0.18066, 0.26589,   -0.25479],
    [ 20.0, 0.18133, 0.26846,   -0.26876],
    [ 30.0, 0.18208, 0.27119,   -0.28539],
    [ 40.0, 0.18293, 0.27407,   -0.30470],
    [ 50.0, 0.18388, 0.27709,   -0.32675],
    [ 60.0, 0.18494, 0.28021,   -0.35156],
    [ 70.0, 0.18611, 0.28342,   -0.37915],
    [ 80.0, 0.18740, 0.28668,   -0.40955],
    [ 90.0, 0.18880, 0.28997,   -0.44278],
    [100.0, 0.19032, 0.29326,   -0.47888],
    [125.0, 0.19462, 0.30141,   -0.58204],
    [150.0, 0.19962, 0.30921,   -0.70471],
    [175.0, 0.20525, 0.31647,   -0.84901],
    [200.0, 0.21142, 0.32312,   -1.0182],
    [225.0, 0.21807, 0.32909,   -1.2168],
    [250.0, 0.22511, 0.33439,   -1.4512],
    [275.0, 0.23247, 0.33904,   -1.7298],
    [300.0, 0.24010, 0.34308,   -2.0637],
    [325.0, 0.24792, 0.34655,   -2.4681],
    [350.0, 0.25591, 0.34951,   -2.9641],
    [375.0, 0.26400, 0.35200,   -3.5814],
    [400.0, 0.27218, 0.35407,   -4.3633],
    [425.0, 0.28039, 0.35577,   -5.3762],
    [450.0, 0.28863, 0.35714,   -6.7262],
    [475.0, 0.29685, 0.35823,   -8.5955],
    [500.0, 0.30505, 0.35907,  -11.324],
    [525.0, 0.31320, 0.35968,  -15.628],
    [550.0, 0.32129, 0.36011,  -23.325],
    [575.0, 0.32931, 0.36038,  -40.770],
    [600.0, 0.33724, 0.36051, -116.45],
];

/// The table's span, in mired: 0 is infinitely blue, 600 is ~1667 K.
const MIRED_MAX: f64 = 600.0;

/// Unit vector along row `i`'s isotemperature line, pointing below the locus.
fn slope_unit(i: usize) -> (f64, f64) {
    let t = ROBERTSON[i][3];
    let len = (1.0 + t * t).sqrt();
    (1.0 / len, t / len)
}

/// Chromaticity of a light given as temperature and tint (`dng_temperature::Get_xy_coord`).
fn xy_of(mired: f64, tint: f64) -> (f64, f64) {
    let r = mired.clamp(0.0, MIRED_MAX);
    let i = (0..30).find(|&i| r < ROBERTSON[i + 1][0]).unwrap_or(29);
    let (a, b) = (ROBERTSON[i], ROBERTSON[i + 1]);
    // Weight of the lower row.
    let f = (b[0] - r) / (b[0] - a[0]);
    let mut u = a[1] * f + b[1] * (1.0 - f);
    let mut v = a[2] * f + b[2] * (1.0 - f);

    let (u1, v1) = slope_unit(i);
    let (u2, v2) = slope_unit(i + 1);
    let (du, dv) = (u1 * f + u2 * (1.0 - f), v1 * f + v2 * (1.0 - f));
    let len = (du * du + dv * dv).sqrt();
    let offset = tint / TINT_SCALE;
    u += du / len * offset;
    v += dv / len * offset;

    let d = u - 4.0 * v + 2.0;
    (1.5 * u / d, v / d)
}

/// Temperature (in mired) and tint of a chromaticity (`dng_temperature::Set_xy_coord`).
///
/// Close to, but not exactly, the inverse of `xy_of` — the two interpolate between table
/// rows differently — so `temperature_of` finishes the job with a few Newton steps.
fn robertson_estimate(x: f64, y: f64) -> (f64, f64) {
    let d = 1.5 - x + 6.0 * y;
    let (u, v) = (2.0 * x / d, 3.0 * y / d);

    let (mut last_dt, mut last_du, mut last_dv) = (0.0, 0.0, 0.0);
    for i in 1..=30 {
        let (du, dv) = slope_unit(i);
        let (uu, vv) = (u - ROBERTSON[i][1], v - ROBERTSON[i][2]);
        // Signed distance from this row's isotemperature line.
        let dt = -uu * dv + vv * du;
        if dt <= 0.0 || i == 30 {
            let dt = -dt.min(0.0);
            let f = if i == 1 {
                0.0
            } else {
                last_dt / (last_dt + dt)
            };
            let mired = ROBERTSON[i - 1][0] * f + ROBERTSON[i][0] * (1.0 - f);
            let uu = u - (ROBERTSON[i - 1][1] * f + ROBERTSON[i][1] * (1.0 - f));
            let vv = v - (ROBERTSON[i - 1][2] * f + ROBERTSON[i][2] * (1.0 - f));
            let (du, dv) = (du * (1.0 - f) + last_du * f, dv * (1.0 - f) + last_dv * f);
            let len = (du * du + dv * dv).sqrt();
            return (mired, (uu * du / len + vv * dv / len) * TINT_SCALE);
        }
        (last_dt, last_du, last_dv) = (dt, du, dv);
    }
    unreachable!("the last row always answers")
}

/// The exact inverse of `xy_of`: temperature in mired, and tint.
fn temperature_of(x: f64, y: f64) -> (f64, f64) {
    let (mut mired, mut tint) = robertson_estimate(x, y);
    const H: f64 = 1e-3;
    for _ in 0..6 {
        let (fx, fy) = xy_of(mired, tint);
        let (ex, ey) = (x - fx, y - fy);
        if ex.abs() < 1e-12 && ey.abs() < 1e-12 {
            break;
        }
        let (mx, my) = xy_of(mired + H, tint);
        let (tx, ty) = xy_of(mired, tint + H);
        let (a, c) = ((mx - fx) / H, (my - fy) / H);
        let (b, d) = ((tx - fx) / H, (ty - fy) / H);
        let det = a * d - b * c;
        if det.abs() < 1e-18 {
            break;
        }
        mired = (mired + (d * ex - b * ey) / det).clamp(0.0, MIRED_MAX);
        tint += (a * ey - c * ex) / det;
    }
    (mired, tint)
}

fn to_kelvin(mired: f64) -> f64 {
    1e6 / mired.max(1e-6)
}

/// Kelvin/tint ↔ `WhiteBalance` for one camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WhiteScale {
    /// As-shot neutral multipliers, green = 1: what `WhiteBalance { 1.0, 1.0 }` means.
    as_shot: [f32; 3],
    /// Multipliers that neutralise D65 — the one light whose camera response the
    /// normalised `cam_to_srgb` no longer records on its own.
    daylight: [f32; 3],
    /// White-balanced camera RGB → linear sRGB.
    cam_to_srgb: [[f32; 3]; 3],
}

impl WhiteScale {
    pub fn new(as_shot: [f32; 3], daylight: [f32; 3], cam_to_srgb: [[f32; 3]; 3]) -> Self {
        Self {
            as_shot,
            daylight,
            cam_to_srgb,
        }
    }

    /// An image already in sRGB, balanced for D65: a JPEG, or anything with no camera data.
    pub fn srgb() -> Self {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        Self::new([1.0; 3], [1.0; 3], identity)
    }

    /// Temperature in kelvin and tint that this white balance sets.
    pub fn temperature(&self, wb: &WhiteBalance) -> (f32, f32) {
        let mult = [
            self.as_shot[0] * wb.tint_r,
            self.as_shot[1],
            self.as_shot[2] * wb.tint_b,
        ];
        let Some((x, y)) = self.white_of(mult) else {
            return (6500.0, 0.0);
        };
        let (mired, tint) = temperature_of(x, y);
        (to_kelvin(mired) as f32, tint as f32)
    }

    /// The white balance that makes a light of this temperature and tint neutral.
    /// `None` when the camera's matrix cannot reach that light with positive multipliers.
    pub fn white_balance(&self, kelvin: f32, tint: f32) -> Option<WhiteBalance> {
        let (x, y) = xy_of(1e6 / kelvin.max(1.0) as f64, tint as f64);
        let mult = self.multipliers_for(x, y)?;
        let rel = |m: f64, shot: f32| (m / shot as f64) as f32;
        let (tint_r, tint_b) = (rel(mult[0], self.as_shot[0]), rel(mult[2], self.as_shot[2]));
        // Past this the light is one the camera barely saw at all, and the gain is noise.
        let ok = |v: f32| v.is_finite() && (1.0 / 64.0..=64.0).contains(&v);
        (ok(tint_r) && ok(tint_b)).then_some(WhiteBalance { tint_r, tint_b })
    }

    /// Camera multipliers, green = 1, that neutralise a light of chromaticity `xy`.
    fn multipliers_for(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        if y <= 0.0 {
            return None;
        }
        let xyz = [x / y, 1.0, (1.0 - x - y) / y];
        let srgb = mul3v(&XYZ_TO_SRGB, xyz);
        let srgb_to_cam = invert3(&self.cam_to_srgb)?;
        // The normalised matrix maps D65 to (1, 1, 1); the daylight multipliers put back
        // the camera's own, unequal, response to it.
        let cam = mul3v(&srgb_to_cam, srgb);
        let raw: Vec<f64> = (0..3).map(|i| cam[i] / self.daylight[i] as f64).collect();
        if raw.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return None;
        }
        Some([raw[1] / raw[0], 1.0, raw[1] / raw[2]])
    }

    /// Chromaticity of the light these camera multipliers neutralise.
    fn white_of(&self, mult: [f32; 3]) -> Option<(f64, f64)> {
        if mult.iter().any(|m| !m.is_finite() || *m <= 0.0) {
            return None;
        }
        let cam = [0, 1, 2].map(|i| (self.daylight[i] / mult[i]) as f64);
        let srgb = mul3v(&self.cam_to_srgb, cam);
        let xyz = mul3v(&invert3(&XYZ_TO_SRGB)?, srgb);
        let sum = xyz[0] + xyz[1] + xyz[2];
        (sum.is_finite() && sum > 0.0 && xyz[1] > 0.0).then(|| (xyz[0] / sum, xyz[1] / sum))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canon EOS 7D, as `decode::cam_to_srgb` builds it from dcraw's adobe_coeff.
    fn canon() -> WhiteScale {
        WhiteScale::new(
            [2.0, 1.0, 1.5],
            [2.07, 1.0, 1.36],
            [
                [1.9205, -1.0414, 0.1209],
                [-0.2470, 1.7594, -0.5124],
                [-0.0070, -0.4896, 1.4965],
            ],
        )
    }

    #[test]
    fn known_lights_have_their_known_temperatures() {
        // D65 is 6504 K, a little green of the locus.
        let (m, tint) = temperature_of(0.3127, 0.3290);
        assert!((to_kelvin(m) - 6504.0).abs() < 25.0, "{}", to_kelvin(m));
        assert!(tint > 0.0 && tint < 15.0, "{tint}");
        // Illuminant A, tungsten, sits on the locus at 2856 K.
        let (m, tint) = temperature_of(0.44757, 0.40745);
        assert!((to_kelvin(m) - 2856.0).abs() < 15.0, "{}", to_kelvin(m));
        assert!(tint.abs() < 3.0, "{tint}");
    }

    #[test]
    fn chromaticity_round_trips_exactly() {
        for kelvin in [
            2000.0, 2856.0, 3200.0, 4100.0, 5500.0, 6500.0, 9000.0, 20000.0,
        ] {
            for tint in [-150.0, -30.0, 0.0, 12.0, 150.0] {
                let (x, y) = xy_of(1e6 / kelvin, tint);
                let (m, t) = temperature_of(x, y);
                assert!(
                    (to_kelvin(m) - kelvin).abs() < 0.05,
                    "{kelvin} {tint} -> {m}"
                );
                assert!((t - tint).abs() < 1e-3, "{kelvin} {tint} -> {t}");
            }
        }
    }

    #[test]
    fn white_balance_round_trips_through_kelvin() {
        // The slider reads kelvin back from the stored multipliers every frame, so any
        // loss here is a slider that drifts under the pointer.
        // sRGB's primaries cannot reach as far into the warm lights as a sensor can.
        for (scale, warmest) in [(canon(), 2000.0), (WhiteScale::srgb(), 3200.0)] {
            for kelvin in [warmest, 5000.0, 7500.0, 12000.0, 50000.0] {
                for tint in [-60.0f32, 0.0, 40.0] {
                    let wb = scale.white_balance(kelvin, tint).unwrap();
                    let (k, t) = scale.temperature(&wb);
                    assert!((k - kelvin).abs() < 0.5, "{kelvin} -> {k}");
                    assert!((t - tint).abs() < 0.05, "{tint} -> {t}");
                }
            }
        }
    }

    #[test]
    fn as_shot_is_the_identity_and_daylight_is_d65() {
        let scale = canon();
        let (k, t) = scale.temperature(&WhiteBalance {
            tint_r: 1.0,
            tint_b: 1.0,
        });
        let back = scale.white_balance(k, t).unwrap();
        assert!((back.tint_r - 1.0).abs() < 1e-4 && (back.tint_b - 1.0).abs() < 1e-4);

        // Multipliers equal to the daylight ones mean the light was D65.
        let wb = WhiteBalance {
            tint_r: 2.07 / 2.0,
            tint_b: 1.36 / 1.5,
        };
        let (k, _) = scale.temperature(&wb);
        assert!((k - 6504.0).abs() < 25.0, "{k}");
    }

    #[test]
    fn warmer_setting_warms_and_positive_tint_goes_magenta() {
        let scale = WhiteScale::srgb();
        let cool = scale.white_balance(4000.0, 0.0).unwrap();
        let warm = scale.white_balance(8000.0, 0.0).unwrap();
        // A higher temperature says the light was bluer, so red is boosted, blue cut.
        assert!(warm.tint_r > cool.tint_r && warm.tint_b < cool.tint_b);

        // Multipliers are relative to green: a magenta tint raises both red and blue.
        let green = scale.white_balance(6500.0, -40.0).unwrap();
        let magenta = scale.white_balance(6500.0, 40.0).unwrap();
        assert!(magenta.tint_r > green.tint_r && magenta.tint_b > green.tint_b);
    }
}
