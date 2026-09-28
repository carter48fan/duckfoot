//! The sRGB colour profile every export carries, so a colour-managed viewer shows the
//! file as it looked in Duckfoot instead of guessing (and a wide-gamut screen guessing
//! wrong oversaturates everything).
//!
//! Built here rather than bundled: an ICC v4 display profile for sRGB is a header, a tag
//! table and nine small tags, all of it public constants — the IEC 61966-2-1 primaries
//! adapted to the D50 connection space with Bradford, and the sRGB curve as ICC's
//! parametric curve type 3. Some 550 bytes, no licence to track.

use std::sync::OnceLock;

/// The profile's bytes, built once.
pub fn srgb_profile() -> &'static [u8] {
    static PROFILE: OnceLock<Vec<u8>> = OnceLock::new();
    PROFILE.get_or_init(|| build("sRGB (Duckfoot)", SRGB_D50, None))
}

/// Display P3 (DCI-P3 primaries, D65 white, the sRGB curve), as phones shoot it. The
/// primaries adapted to D50 with Bradford, as Apple's own profile has them.
pub fn display_p3_profile() -> &'static [u8] {
    static PROFILE: OnceLock<Vec<u8>> = OnceLock::new();
    PROFILE.get_or_init(|| {
        build(
            "Display P3 (Duckfoot)",
            [
                [0.515102, 0.241196, -0.001053],
                [0.291965, 0.692245, 0.041885],
                [0.157153, 0.066562, 0.784073],
            ],
            None,
        )
    })
}

/// The Adobe RGB (1998) primaries and white with its 2.2 (563/256) gamma, adapted to D50
/// with Bradford as Adobe's own profile has them. Named as compatible with it: the
/// encoding is Adobe's published specification, the profile is built here.
pub fn adobe_rgb_profile() -> &'static [u8] {
    static PROFILE: OnceLock<Vec<u8>> = OnceLock::new();
    PROFILE.get_or_init(|| {
        build(
            "Compatible with Adobe RGB (1998) (Duckfoot)",
            [
                [0.609741, 0.311111, 0.019470],
                [0.205276, 0.625671, 0.060867],
                [0.149185, 0.063217, 0.744568],
            ],
            Some(563.0 / 256.0),
        )
    })
}

/// sRGB's primaries (red, green, blue XYZ), adapted to D50 with Bradford.
const SRGB_D50: [[f64; 3]; 3] = [
    [0.436066, 0.222488, 0.013916],
    [0.385147, 0.716873, 0.097076],
    [0.143066, 0.060608, 0.714096],
];

/// ICC's s15Fixed16Number.
fn fixed(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut t = b"XYZ \0\0\0\0".to_vec();
    for v in [x, y, z] {
        t.extend_from_slice(&fixed(v));
    }
    t
}

/// multiLocalizedUnicodeType with one en-US record.
fn mluc(text: &str) -> Vec<u8> {
    let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut t = b"mluc\0\0\0\0".to_vec();
    t.extend_from_slice(&1u32.to_be_bytes()); // records
    t.extend_from_slice(&12u32.to_be_bytes()); // record size
    t.extend_from_slice(b"enUS");
    t.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
    t.extend_from_slice(&28u32.to_be_bytes()); // offset of the string in the tag
    t.extend_from_slice(&utf16);
    t
}

/// A matrix/TRC display profile: `colorants` are the red, green and blue primaries' D50
/// XYZ, and every channel takes the sRGB curve, or the pure power `gamma`.
fn build(description: &str, colorants: [[f64; 3]; 3], gamma: Option<f64>) -> Vec<u8> {
    // D50, the profile connection space's white.
    const D50: (f64, f64, f64) = (0.9642, 1.0, 0.8249);
    let mut para = b"para\0\0\0\0".to_vec();
    match gamma {
        // Y = X^g.
        Some(g) => {
            para.extend_from_slice(&0u16.to_be_bytes());
            para.extend_from_slice(&[0, 0]);
            para.extend_from_slice(&fixed(g));
        }
        // The sRGB curve: Y = (aX + b)^g for X >= d, else cX.
        None => {
            para.extend_from_slice(&3u16.to_be_bytes());
            para.extend_from_slice(&[0, 0]);
            for v in [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045] {
                para.extend_from_slice(&fixed(v));
            }
        }
    }
    // Bradford D65 → D50: what the viewer undoes to get back to the display's white.
    let mut chad = b"sf32\0\0\0\0".to_vec();
    for v in [
        1.047882, 0.022918, -0.050217, 0.029586, 0.990478, -0.017075, -0.009247, 0.015075, 0.751678,
    ] {
        chad.extend_from_slice(&fixed(v));
    }
    // (signature, data); the three curves share one block.
    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", mluc(description)),
        (b"cprt", mluc("No copyright, use freely")),
        (b"wtpt", xyz(D50.0, D50.1, D50.2)),
        (b"chad", chad),
        (
            b"rXYZ",
            xyz(colorants[0][0], colorants[0][1], colorants[0][2]),
        ),
        (
            b"gXYZ",
            xyz(colorants[1][0], colorants[1][1], colorants[1][2]),
        ),
        (
            b"bXYZ",
            xyz(colorants[2][0], colorants[2][1], colorants[2][2]),
        ),
        (b"rTRC", para.clone()),
        (b"gTRC", para.clone()),
        (b"bTRC", para),
    ];

    let table_len = 4 + 12 * tags.len();
    let mut data = Vec::new();
    let mut entries = Vec::new();
    let mut curve_at: Option<(u32, u32)> = None;
    for (sig, body) in &tags {
        let is_curve = sig.ends_with(b"TRC");
        let (offset, size) = match (is_curve, curve_at) {
            (true, Some(shared)) => shared,
            _ => {
                let offset = (128 + table_len + data.len()) as u32;
                data.extend_from_slice(body);
                while data.len() % 4 != 0 {
                    data.push(0);
                }
                (offset, body.len() as u32)
            }
        };
        if is_curve {
            curve_at = Some((offset, size));
        }
        entries.push((*sig, offset, size));
    }
    let total = 128 + table_len + data.len();

    let mut p = Vec::with_capacity(total);
    p.extend_from_slice(&(total as u32).to_be_bytes());
    p.extend_from_slice(&[0; 4]); // preferred CMM: none
    p.extend_from_slice(&0x0430_0000u32.to_be_bytes()); // version 4.3
    p.extend_from_slice(b"mntrRGB XYZ ");
    for v in [2026u16, 1, 1, 0, 0, 0] {
        p.extend_from_slice(&v.to_be_bytes());
    }
    p.extend_from_slice(b"acsp");
    p.extend_from_slice(&[0; 4]); // platform
    p.extend_from_slice(&[0; 4]); // flags
    p.extend_from_slice(&[0; 8]); // manufacturer, model
    p.extend_from_slice(&[0; 8]); // attributes
    p.extend_from_slice(&0u32.to_be_bytes()); // perceptual
    for v in [D50.0, D50.1, D50.2] {
        p.extend_from_slice(&fixed(v));
    }
    p.extend_from_slice(&[0; 4]); // creator
    p.extend_from_slice(&[0; 16]); // profile ID: not computed, which v4 allows
    p.extend_from_slice(&[0; 28]);
    debug_assert_eq!(p.len(), 128);
    p.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (sig, offset, size) in entries {
        p.extend_from_slice(sig);
        p.extend_from_slice(&offset.to_be_bytes());
        p.extend_from_slice(&size.to_be_bytes());
    }
    p.extend_from_slice(&data);
    p
}

// ── Reading a profile ───────────────────────────────────────────────────────────────

/// A tone curve from an ICC profile: encoded value (0..1) to linear light.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// `curv` with no entries: the identity.
    Linear,
    Gamma(f32),
    /// `curv` with entries, sampled evenly over 0..1.
    Table(Vec<f32>),
    /// `para`: the function type and its parameters (g, a, b, c, d, e, f).
    Parametric(u16, [f32; 7]),
}

impl Curve {
    pub fn eval(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Linear => x,
            Curve::Gamma(g) => x.powf(*g),
            Curve::Table(t) => {
                let at = x * (t.len() - 1) as f32;
                let i = (at as usize).min(t.len() - 2);
                let f = at - i as f32;
                t[i] + (t[i + 1] - t[i]) * f
            }
            Curve::Parametric(kind, [g, a, b, c, d, e, f]) => match kind {
                0 => x.powf(*g),
                1 => {
                    if x >= -b / a {
                        (a * x + b).powf(*g)
                    } else {
                        0.0
                    }
                }
                2 => {
                    if x >= -b / a {
                        (a * x + b).powf(*g) + c
                    } else {
                        *c
                    }
                }
                3 => {
                    if x >= *d {
                        (a * x + b).powf(*g)
                    } else {
                        c * x
                    }
                }
                _ => {
                    if x >= *d {
                        (a * x + b).powf(*g) + e
                    } else {
                        c * x + f
                    }
                }
            },
        }
    }
}

/// What a matrix/TRC profile — every RGB display and working-space profile a camera or
/// phone embeds (sRGB, Display P3, Adobe RGB, ProPhoto) — says about its pixels: the curve
/// that linearises each channel, and the matrix from those linear values to linear sRGB.
/// Profiles built from lookup tables (printer, CMYK) are not read.
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixTrc {
    pub curves: [Curve; 3],
    /// Linear values in the profile's primaries → linear sRGB, rows summing to one so a
    /// neutral stays neutral (the RAW path's `cam_to_srgb` convention).
    pub to_srgb: [[f32; 3]; 3],
}

impl MatrixTrc {
    /// `None` for anything that is not an RGB or grey matrix/TRC profile.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let be32 = |at: usize| -> Option<u32> {
            Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        };
        if bytes.get(36..40)? != b"acsp" {
            return None;
        }
        let space = bytes.get(16..20)?;
        let count = be32(128)? as usize;
        let tag = |sig: &[u8; 4]| -> Option<&[u8]> {
            (0..count.min(256)).find_map(|i| {
                let at = 132 + 12 * i;
                if bytes.get(at..at + 4)? != sig {
                    return None;
                }
                let (offset, size) = (be32(at + 4)? as usize, be32(at + 8)? as usize);
                bytes.get(offset..offset.checked_add(size)?)
            })
        };
        let fixed = |b: &[u8], at: usize| -> Option<f32> {
            Some(i32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?) as f32 / 65536.0)
        };
        let xyz = |sig: &[u8; 4]| -> Option<[f32; 3]> {
            let t = tag(sig)?;
            (t.get(0..4)? == b"XYZ ").then_some(())?;
            Some([fixed(t, 8)?, fixed(t, 12)?, fixed(t, 16)?])
        };
        match space {
            b"RGB " => {
                let curves = [
                    parse_curve(tag(b"rTRC")?)?,
                    parse_curve(tag(b"gTRC")?)?,
                    parse_curve(tag(b"bTRC")?)?,
                ];
                let (r, g, b) = (xyz(b"rXYZ")?, xyz(b"gXYZ")?, xyz(b"bXYZ")?);
                let to_srgb = colorants_to_srgb([r, g, b])?;
                Some(Self { curves, to_srgb })
            }
            // A grey profile's one curve serves all three copies of the channel.
            b"GRAY" => {
                let k = parse_curve(tag(b"kTRC")?)?;
                Some(Self {
                    curves: [k.clone(), k.clone(), k],
                    to_srgb: IDENTITY,
                })
            }
            _ => None,
        }
    }

    /// Whether this is sRGB, near enough that decoding it as sRGB changes no 8-bit value.
    pub fn is_srgb(&self) -> bool {
        let matrix = self
            .to_srgb
            .iter()
            .flatten()
            .zip(IDENTITY.iter().flatten())
            .all(|(a, b)| (a - b).abs() < 2e-3);
        let srgb = |x: f32| {
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        let curves = self.curves.iter().all(|c| {
            (0..=32).all(|i| {
                let x = i as f32 / 32.0;
                (c.eval(x) - srgb(x)).abs() < 1e-3
            })
        });
        matrix && curves
    }
}

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn parse_curve(t: &[u8]) -> Option<Curve> {
    let be16 = |at: usize| Some(u16::from_be_bytes(t.get(at..at + 2)?.try_into().ok()?));
    let be32 = |at: usize| Some(u32::from_be_bytes(t.get(at..at + 4)?.try_into().ok()?));
    match t.get(0..4)? {
        b"curv" => {
            let n = be32(8)? as usize;
            match n {
                0 => Some(Curve::Linear),
                // u8Fixed8Number.
                1 => Some(Curve::Gamma(be16(12)? as f32 / 256.0)),
                _ => {
                    let table = (0..n.min(65536))
                        .map(|i| Some(be16(12 + 2 * i)? as f32 / 65535.0))
                        .collect::<Option<Vec<_>>>()?;
                    Some(Curve::Table(table))
                }
            }
        }
        b"para" => {
            let kind = be16(8)?;
            let n = [1, 3, 4, 5, 7].get(kind as usize)?;
            let mut p = [0.0f32; 7];
            for (i, v) in p.iter_mut().enumerate().take(*n) {
                *v = be32(12 + 4 * i)? as i32 as f32 / 65536.0;
            }
            Some(Curve::Parametric(kind, p))
        }
        _ => None,
    }
}

/// The matrix from linear values in primaries with these D50 colorants to linear sRGB:
/// to XYZ, Bradford D50 → D65, then sRGB's own inverse matrix. Rows are scaled to sum to
/// one, which only mops up the rounding of a profile's white (the colorants sum to D50).
fn colorants_to_srgb([r, g, b]: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    const D50_TO_D65: [[f32; 3]; 3] = [
        [0.955_576_6, -0.023_039_3, 0.063_163_6],
        [-0.028_289_5, 1.009_941_6, 0.021_007_7],
        [0.012_298_2, -0.020_483, 1.329_909_8],
    ];
    const XYZ_TO_SRGB: [[f32; 3]; 3] = [
        [3.240_454_2, -1.537_138_5, -0.498_531_4],
        [-0.969_266, 1.876_010_8, 0.041_556],
        [0.055_643_4, -0.204_025_9, 1.057_225_2],
    ];
    let mul = |a: [[f32; 3]; 3], b: [[f32; 3]; 3]| {
        let mut m = [[0.0f32; 3]; 3];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        m
    };
    // Columns are the primaries.
    let to_xyz = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let mut m = mul(XYZ_TO_SRGB, mul(D50_TO_D65, to_xyz));
    for row in &mut m {
        let sum: f32 = row.iter().sum();
        if !sum.is_finite() || sum.abs() < 1e-3 {
            return None;
        }
        for v in row.iter_mut() {
            *v /= sum;
        }
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_own_srgb_profile_reads_back_as_srgb() {
        let p = MatrixTrc::parse(srgb_profile()).expect("parse");
        assert!(p.is_srgb(), "{p:?}");
    }

    #[test]
    fn display_p3_reads_as_wider_than_srgb() {
        let p = MatrixTrc::parse(display_p3_profile()).expect("parse");
        assert!(!p.is_srgb());
        // P3's red is outside sRGB: in sRGB terms it needs more than full red, and less
        // than no green or blue.
        let red = [p.to_srgb[0][0], p.to_srgb[1][0], p.to_srgb[2][0]];
        assert!(red[0] > 1.1 && red[1] < 0.0 && red[2] < 0.0, "{red:?}");
        // White stays white.
        for row in p.to_srgb {
            assert!((row.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn adobe_rgb_reads_back_with_its_primaries_and_gamma() {
        let p = MatrixTrc::parse(adobe_rgb_profile()).expect("parse");
        assert!(!p.is_srgb());
        // Its green is wider than sRGB's; its red and blue are sRGB's.
        let want = crate::gamut::convert(&crate::gamut::ADOBE_RGB, &crate::gamut::SRGB);
        for (got, want) in p.to_srgb.iter().flatten().zip(want.iter().flatten()) {
            assert!((got - want).abs() < 2e-3, "{:?}", p.to_srgb);
        }
        let mid = p.curves[1].eval(0.5);
        assert!((mid - 0.5f32.powf(563.0 / 256.0)).abs() < 1e-3, "{mid}");
    }

    #[test]
    fn curves_evaluate_as_icc_defines_them() {
        assert_eq!(Curve::Linear.eval(0.3), 0.3);
        assert!((Curve::Gamma(2.2).eval(0.5) - 0.5f32.powf(2.2)).abs() < 1e-6);
        let t = Curve::Table(vec![0.0, 0.25, 1.0]);
        assert!((t.eval(0.25) - 0.125).abs() < 1e-6);
        // Adobe RGB's gamma, as a one-entry curv.
        let curv = [
            b"curv".as_slice(),
            &[0; 4],
            &1u32.to_be_bytes(),
            &563u16.to_be_bytes(),
        ]
        .concat();
        assert_eq!(parse_curve(&curv), Some(Curve::Gamma(563.0 / 256.0)));
    }

    #[test]
    fn nonsense_is_not_a_profile() {
        assert!(MatrixTrc::parse(b"not a profile").is_none());
        assert!(MatrixTrc::parse(&srgb_profile()[..200]).is_none());
    }

    fn be32(b: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn the_profile_is_well_formed() {
        let p = srgb_profile();
        assert_eq!(be32(p, 0) as usize, p.len(), "size field");
        assert_eq!(&p[36..40], b"acsp");
        assert_eq!(&p[12..24], b"mntrRGB XYZ ");
        let count = be32(p, 128) as usize;
        assert_eq!(count, 10);
        for i in 0..count {
            let at = 132 + 12 * i;
            let (offset, size) = (be32(p, at + 4) as usize, be32(p, at + 8) as usize);
            assert_eq!(offset % 4, 0, "tag {i} is aligned");
            assert!(offset + size <= p.len(), "tag {i} fits");
        }
        // The colorants add up to the D50 white, as a matrix/TRC profile's must.
        let colorant = |sig: &[u8]| {
            let i = (0..count)
                .find(|i| &p[132 + 12 * i..136 + 12 * i] == sig)
                .unwrap();
            let offset = be32(p, 136 + 12 * i) as usize;
            [0, 1, 2].map(|k| be32(p, offset + 8 + 4 * k) as i32 as f64 / 65536.0)
        };
        let sum: Vec<f64> = (0..3)
            .map(|k| colorant(b"rXYZ")[k] + colorant(b"gXYZ")[k] + colorant(b"bXYZ")[k])
            .collect();
        for (got, want) in sum.iter().zip([0.9642, 1.0, 0.8249]) {
            assert!((got - want).abs() < 2e-4, "{sum:?}");
        }
    }
}
