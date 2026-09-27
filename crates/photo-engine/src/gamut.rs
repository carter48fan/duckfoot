//! Colour spaces by their primaries (W-09): the working space the adjust pass edits in,
//! and the spaces an export can be written in. Every matrix is derived here from the
//! spaces' published chromaticities, rather than typed in, so the set stays consistent
//! and a test can hold it to the published values.

use serde::{Deserialize, Serialize};

pub type Mat3 = [[f32; 3]; 3];

/// Where the adjust pass does its work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WorkingSpace {
    /// Linear sRGB, as every edit before W-09 was made: a saturated colour beyond sRGB
    /// comes out of the camera matrix with a negative channel and is clipped, shifting
    /// its hue. Kept for sidecars without the field, so their look does not change.
    Srgb,
    /// Linear Rec.2020, wide enough for every colour a camera records in practice.
    /// Colours are mapped into the output's gamut only at the end, keeping their hue.
    #[default]
    Rec2020,
}

impl WorkingSpace {
    pub fn legacy() -> Self {
        Self::Srgb
    }

    pub fn primaries(self) -> Primaries {
        match self {
            Self::Srgb => SRGB,
            Self::Rec2020 => REC2020,
        }
    }
}

/// What an export is written in, with the matching ICC profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OutputSpace {
    #[default]
    Srgb,
    /// DCI-P3 primaries, D65 white, the sRGB curve: what phones and recent screens show.
    DisplayP3,
    /// The Adobe RGB (1998) primaries and its 2.2 gamma: the usual space for print.
    AdobeRgb,
}

impl OutputSpace {
    pub const ALL: [OutputSpace; 3] = [Self::Srgb, Self::DisplayP3, Self::AdobeRgb];

    pub fn primaries(self) -> Primaries {
        match self {
            Self::Srgb => SRGB,
            Self::DisplayP3 => DISPLAY_P3,
            Self::AdobeRgb => ADOBE_RGB,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::DisplayP3 => "Display P3",
            Self::AdobeRgb => "Adobe RGB",
        }
    }

    /// Its transfer curve: the sRGB curve, or a pure power.
    pub fn gamma(self) -> Option<f32> {
        match self {
            Self::AdobeRgb => Some(563.0 / 256.0),
            _ => None,
        }
    }

    /// The ICC profile an export in it carries.
    pub fn icc(self) -> &'static [u8] {
        match self {
            Self::Srgb => crate::icc::srgb_profile(),
            Self::DisplayP3 => crate::icc::display_p3_profile(),
            Self::AdobeRgb => crate::icc::adobe_rgb_profile(),
        }
    }
}

/// A space's red, green and blue primaries and white point, as CIE xy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Primaries {
    pub red: [f64; 2],
    pub green: [f64; 2],
    pub blue: [f64; 2],
    pub white: [f64; 2],
}

const D65: [f64; 2] = [0.3127, 0.3290];

pub const SRGB: Primaries = Primaries {
    red: [0.64, 0.33],
    green: [0.30, 0.60],
    blue: [0.15, 0.06],
    white: D65,
};
pub const REC2020: Primaries = Primaries {
    red: [0.708, 0.292],
    green: [0.170, 0.797],
    blue: [0.131, 0.046],
    white: D65,
};
pub const DISPLAY_P3: Primaries = Primaries {
    red: [0.680, 0.320],
    green: [0.265, 0.690],
    blue: [0.150, 0.060],
    white: D65,
};
pub const ADOBE_RGB: Primaries = Primaries {
    red: [0.64, 0.33],
    green: [0.21, 0.71],
    blue: [0.15, 0.06],
    white: D65,
};

type M64 = [[f64; 3]; 3];

fn mul(a: &M64, b: &M64) -> M64 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn apply(m: &M64, v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|k| m[i][k] * v[k]).sum())
}

fn invert(m: &M64) -> M64 {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        [
            (e * i - f * h) / det,
            (c * h - b * i) / det,
            (b * f - c * e) / det,
        ],
        [
            (f * g - d * i) / det,
            (a * i - c * g) / det,
            (c * d - a * f) / det,
        ],
        [
            (d * h - e * g) / det,
            (b * g - a * h) / det,
            (a * e - b * d) / det,
        ],
    ]
}

fn xyz([x, y]: [f64; 2]) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// Linear RGB in `p` to CIE XYZ (Y = 1 at white).
fn to_xyz(p: &Primaries) -> M64 {
    let [r, g, b] = [xyz(p.red), xyz(p.green), xyz(p.blue)];
    let m = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let s = apply(&invert(&m), xyz(p.white));
    std::array::from_fn(|i| std::array::from_fn(|j| m[i][j] * s[j]))
}

/// Linear RGB in `from` to linear RGB in `to` (the same white: every space here is D65).
pub fn convert(from: &Primaries, to: &Primaries) -> Mat3 {
    let m = mul(&invert(&to_xyz(to)), &to_xyz(from));
    m.map(|row| row.map(|v| v as f32))
}

/// The weights that give a space's luminance from its linear RGB: the Y row.
pub fn luminance(p: &Primaries) -> [f32; 3] {
    to_xyz(p)[1].map(|v| v as f32)
}

/// `a × b`.
pub fn compose(a: &Mat3, b: &Mat3) -> Mat3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &Mat3, b: &Mat3, tol: f32) -> bool {
        (0..3).all(|i| (0..3).all(|j| (a[i][j] - b[i][j]).abs() < tol))
    }

    #[test]
    fn the_matrices_are_the_published_ones() {
        // IEC 61966-2-1's sRGB → XYZ and BT.2087's sRGB → Rec.2020.
        let srgb = to_xyz(&SRGB).map(|r| r.map(|v| v as f32));
        let want = [
            [0.4124, 0.3576, 0.1805],
            [0.2126, 0.7152, 0.0722],
            [0.0193, 0.1192, 0.9505],
        ];
        assert!(close(&srgb, &want, 2e-4), "{srgb:?}");
        let to_2020 = convert(&SRGB, &REC2020);
        let want = [
            [0.6274, 0.3293, 0.0433],
            [0.0691, 0.9195, 0.0114],
            [0.0164, 0.0880, 0.8956],
        ];
        assert!(close(&to_2020, &want, 2e-4), "{to_2020:?}");
        assert_eq!(
            luminance(&SRGB).map(|v| (v * 1e4).round()),
            [2126.0, 7152.0, 722.0]
        );
    }

    #[test]
    fn there_and_back_is_nothing_and_white_stays_white() {
        let there = convert(&REC2020, &ADOBE_RGB);
        let back = convert(&ADOBE_RGB, &REC2020);
        let id = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        assert!(close(&compose(&back, &there), &id, 1e-5));
        for space in OutputSpace::ALL {
            let m = convert(&REC2020, &space.primaries());
            for row in m {
                assert!((row.iter().sum::<f32>() - 1.0).abs() < 1e-5);
            }
        }
    }
}
