//! RAW file decoding to a cropped CFA mosaic plus the metadata the GPU needs.
//!
//! The previous build read four fields out of the file and guessed the rest. It produced
//! green, uncropped images in the wrong primaries from a hardcoded RGGB assumption. See
//! DESIGN.md — every field handled here is handled because skipping it visibly breaks the
//! image, not for completeness.

use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};

use crate::params::Geometry;

/// sRGB (D65) primaries — the standard XYZ→linear-sRGB matrix.
const XYZ_TO_SRGB: [[f32; 3]; 3] = [
    [3.2404542, -1.5371385, -0.4985314],
    [-0.969266, 1.8760108, 0.041556],
    [0.0556434, -0.2040259, 1.0572252],
];

/// A decoded sensor mosaic, cropped to the usable area, plus everything needed to turn
/// its samples into colour.
pub struct CfaImage {
    /// One `u16` per photosite, `width * height`, cropped to the usable area.
    pub data: Vec<u16>,
    pub width: u32,
    pub height: u32,
    /// CFA colour index (0=R, 1=G, 2=B) at each position of the 2×2 block, row-major:
    /// `[(0,0), (0,1), (1,0), (1,1)]`. Already phase-corrected for the crop.
    pub cfa2x2: [u32; 4],
    /// Black level per CFA colour index.
    pub black: [f32; 4],
    /// White level per CFA colour index.
    pub white: [f32; 4],
    /// As-shot camera-neutral multipliers, normalised so green is 1.0.
    pub wb: [f32; 3],
    /// Camera-native RGB → linear sRGB, row-normalised so neutral maps to neutral.
    pub cam_to_srgb: [[f32; 3]; 3],
    /// How the camera says the frame is meant to be viewed, as the geometry that does it.
    /// A starting point for a new edit, not a fact about the pixels.
    pub orientation: Geometry,
    pub make: String,
    pub model: String,
}

impl CfaImage {
    pub fn megapixels(&self) -> f32 {
        (self.width as f32 * self.height as f32) / 1_000_000.0
    }
}

/// Behind a trait so `rawler` (~2500 cameras, same LGPL-2.1 licence) is a drop-in later.
/// See DESIGN.md — the licence is a wash, so this is about camera coverage, not legals.
pub trait RawDecoder {
    fn decode_bytes(&self, bytes: &[u8]) -> Result<CfaImage>;

    fn decode_file(&self, path: &Path) -> Result<CfaImage> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("{}: could not be read", path.display()))?;
        self.decode_bytes(&bytes)
            .with_context(|| format!("{}", path.display()))
    }
}

pub struct RawloaderBackend;

impl RawDecoder for RawloaderBackend {
    fn decode_bytes(&self, bytes: &[u8]) -> Result<CfaImage> {
        let mut cursor = Cursor::new(bytes);
        let mut img = decode_reader(&mut cursor)?;
        // rawloader reads DNG white balance only from AsShotNeutral. Plenty of DNGs —
        // Google's Pixel phones among them — record AsShotWhiteXY instead, and without
        // it the image comes out uncorrected: about twice as green as it should be.
        if img.wb_missing {
            if let Some(dng) = dng::colour(bytes) {
                img.cfa.wb = dng.wb;
                img.cfa.cam_to_srgb = cam_to_srgb(&dng.xyz_to_cam);
            }
        }
        Ok(img.cfa)
    }
}

/// A decode, plus whether the file carried no usable as-shot white balance of its own.
struct Decoded {
    cfa: CfaImage,
    wb_missing: bool,
}

fn decode_reader(reader: &mut dyn Read) -> Result<Decoded> {
    let raw = rawloader::decode(reader).map_err(|e| anyhow!("not a supported RAW file: {e:?}"))?;

    // `cpp == 3` means the file is already demosaiced RGB (Foveon, some DNGs). Indexing it
    // as a mosaic is exactly the bug the last decoder shipped, so refuse loudly instead.
    if raw.cpp != 1 {
        bail!(
            "{} {}: {} components per pixel — non-mosaic sensors are not supported yet (@stub)",
            raw.make,
            raw.model,
            raw.cpp
        );
    }

    let samples = match raw.data {
        rawloader::RawImageData::Integer(ref d) => d,
        rawloader::RawImageData::Float(_) => bail!(
            "{} {}: float-encoded RAW is not supported yet (@stub)",
            raw.make,
            raw.model
        ),
    };

    // crops are [top, right, bottom, left]. Skipping this leaves the masked black sensor
    // border in the frame.
    let [ct, cr, cb, cl] = raw.crops;
    let src_w = raw.width;
    let src_h = raw.height;

    let width = src_w
        .checked_sub(cl + cr)
        .filter(|w| *w > 0)
        .ok_or_else(|| anyhow!("crop wider than sensor ({src_w} - {cl} - {cr})"))?;
    let height = src_h
        .checked_sub(ct + cb)
        .filter(|h| *h > 0)
        .ok_or_else(|| anyhow!("crop taller than sensor ({src_h} - {ct} - {cb})"))?;

    if samples.len() < src_w * src_h {
        bail!(
            "truncated sensor data: {} samples for {src_w}×{src_h}",
            samples.len()
        );
    }

    let mut data = Vec::with_capacity(width * height);
    for row in 0..height {
        let start = (row + ct) * src_w + cl;
        data.extend_from_slice(&samples[start..start + width]);
    }

    // Cropping by a non-even offset rotates the mosaic phase. `shift` is rawloader's own
    // correction for exactly this; guessing RGGB here swaps red and blue on any BGGR,
    // GRBG or GBRG sensor.
    let cfa = raw.cfa.shift(cl, ct);
    if !cfa.is_valid() {
        bail!("{} {}: unrecognised CFA pattern", raw.make, raw.model);
    }
    let cfa2x2 = [
        cfa.color_at(0, 0) as u32,
        cfa.color_at(0, 1) as u32,
        cfa.color_at(1, 0) as u32,
        cfa.color_at(1, 1) as u32,
    ];
    if cfa2x2.iter().any(|c| *c > 2) {
        bail!(
            "{} {}: CFA pattern '{}' has a fourth colour — not supported yet (@stub)",
            raw.make,
            raw.model,
            cfa.to_string()
        );
    }

    let mut black = [0.0f32; 4];
    let mut white = [65535.0f32; 4];
    for i in 0..4 {
        black[i] = raw.blacklevels[i] as f32;
        let w = raw.whitelevels[i] as f32;
        // A white level at or below black would make the normalisation divide by zero.
        white[i] = if w > black[i] { w } else { black[i] + 1.0 };
    }
    let white = measured_white(&data, width, &cfa2x2, &black, &white);

    let wb_missing = raw.wb_coeffs[..3]
        .iter()
        .any(|c| !c.is_finite() || *c <= 0.0);
    let mut wb = neutral_multipliers(&raw.wb_coeffs);
    if wb_missing {
        // No as-shot white at all: daylight is a better guess than none, which is 2× green.
        wb = daylight_multipliers(&raw.xyz_to_cam).unwrap_or(wb);
    }

    Ok(Decoded {
        cfa: CfaImage {
            data,
            width: width as u32,
            height: height as u32,
            cfa2x2,
            black,
            white,
            wb,
            cam_to_srgb: cam_to_srgb(&raw.xyz_to_cam),
            orientation: orientation_geometry(raw.orientation),
            make: raw.make.clone(),
            model: raw.model.clone(),
        },
        wb_missing,
    })
}

/// EXIF orientation as the turns and mirrors that display it upright. The shader applies
/// turns first, then mirrors in screen space, so the two diagonal flips are a turn plus a
/// mirror.
fn orientation_geometry(o: rawloader::Orientation) -> Geometry {
    use rawloader::Orientation as O;
    let (quarter_turns, mirror_h, mirror_v) = match o {
        O::Normal | O::Unknown => (0, false, false),
        O::HorizontalFlip => (0, true, false),
        O::Rotate180 => (2, false, false),
        O::VerticalFlip => (0, false, true),
        O::Transpose => (1, true, false),
        O::Rotate90 => (1, false, false),
        O::Transverse => (1, false, true),
        O::Rotate270 => (3, false, false),
    };
    Geometry {
        quarter_turns,
        mirror_h,
        mirror_v,
        ..Geometry::default()
    }
}

/// The level the sensor actually clipped at, where the data shows one.
///
/// A camera database gives one white level per model, but sensors do not clip at one
/// level: Canon bodies saturate lower at their intermediate ISOs (160, 320, 640…) than at
/// full stops. When the real clip sits below the declared white, blown highlights
/// normalise to ~0.87 instead of 1.0, highlight reconstruction never engages, and white
/// balance turns them magenta. When it sits above, highlight range is thrown away.
///
/// A clip is recognisable in the data as a plateau: many photosites at the very top of the
/// range and next to none just below it, because a saturated ADC reports one value while
/// real light is spread out by noise. Where a colour has no plateau — nothing clipped — the
/// declared level stands, unless another colour clipped (the ADC saturates at one level
/// for all three). An unclipped photo is left exactly as before.
fn measured_white(
    data: &[u16],
    width: usize,
    cfa2x2: &[u32; 4],
    black: &[f32; 4],
    declared: &[f32; 4],
) -> [f32; 4] {
    let mut hist = vec![[0u32; 3]; 65536];
    for (row, line) in data.chunks_exact(width).enumerate() {
        let pattern = [cfa2x2[(row & 1) * 2], cfa2x2[(row & 1) * 2 + 1]];
        for (col, &v) in line.iter().enumerate() {
            hist[v as usize][pattern[col & 1] as usize] += 1;
        }
    }

    let mut plateau = [None::<f32>; 3];
    for (c, found) in plateau.iter_mut().enumerate() {
        let count = |v: usize| hist[v][c] as u64;
        // The top of the range, ignoring values only a stray photosite or two reached: a
        // hot pixel above the real clip must not hide the plateau underneath it.
        let Some(max) = (0..65536).rev().find(|&v| count(v) >= 4) else {
            continue;
        };
        let floor = black[c].max(0.0) as usize;
        let range = declared[c] - black[c];
        // A "clip" in the bottom half of the range is a dark frame, not saturation.
        if (max as f32) < black[c] + 0.5 * range {
            continue;
        }
        let tol = (((max - floor) as f32) * 0.001).round().max(2.0) as usize;
        let top: u64 = (max.saturating_sub(tol)..=max).map(count).sum();
        let below: u64 = (max.saturating_sub(8 * tol)..max.saturating_sub(tol))
            .map(count)
            .sum();
        // `below` spans 7 tolerance widths; a plateau is several times denser than that.
        let below_per_band = below as f64 / 7.0;
        if top as f64 > 4.0 * (below_per_band + 1.0) {
            // The lowest value holding a real share of the plateau, so that every clipped
            // photosite normalises to at least 1.0.
            let start = (max.saturating_sub(tol)..=max)
                .find(|&v| count(v) * 4 >= top)
                .unwrap_or(max);
            *found = Some(start as f32);
        }
    }

    let shared = plateau.iter().flatten().copied().reduce(f32::min);
    let mut white = *declared;
    for c in 0..3 {
        if let Some(w) = plateau[c].or(shared) {
            if w > black[c] + 1.0 {
                if (w - declared[c]).abs() > 0.5 {
                    log::info!(
                        "white level, colour {c}: declared {} measured {w}",
                        declared[c]
                    );
                }
                white[c] = w;
            }
        }
    }
    white[3] = white[1];
    white
}

/// As-shot white balance, normalised against green.
///
/// Sensor data without these applied is roughly 2× green — this is the single most
/// visible thing the previous decoder omitted.
fn neutral_multipliers(coeffs: &[f32; 4]) -> [f32; 3] {
    // (the fallback for a file that has none is `daylight_multipliers`)
    let g = coeffs[1];
    if !g.is_finite() || g <= 0.0 {
        return [1.0, 1.0, 1.0];
    }
    let norm = |v: f32| {
        let n = v / g;
        if n.is_finite() && n > 0.0 {
            n.clamp(0.05, 20.0)
        } else {
            1.0
        }
    };
    [norm(coeffs[0]), 1.0, norm(coeffs[2])]
}

/// White balance for a D65 white, from the camera's own colour matrix: the camera's
/// response to daylight is `xyz_to_cam · XYZ(D65)`, and the multipliers undo it.
fn daylight_multipliers(xyz_to_cam: &[[f32; 3]; 4]) -> Option<[f32; 3]> {
    neutral_for_white(&[xyz_to_cam[0], xyz_to_cam[1], xyz_to_cam[2]], D65_XY)
}

/// CIE 1931 chromaticity of D65.
const D65_XY: (f32, f32) = (0.3127, 0.3290);

/// Multipliers that make a white of chromaticity `xy` neutral, given XYZ → camera.
fn neutral_for_white(xyz_to_cam: &[[f32; 3]; 3], (x, y): (f32, f32)) -> Option<[f32; 3]> {
    if y <= 0.0 {
        return None;
    }
    let xyz = [x / y, 1.0, (1.0 - x - y) / y];
    let cam: Vec<f32> = xyz_to_cam
        .iter()
        .map(|row| row.iter().zip(xyz).map(|(m, v)| m * v).sum())
        .collect();
    if cam.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return None;
    }
    let g = cam[1];
    Some([
        (g / cam[0]).clamp(0.05, 20.0),
        1.0,
        (g / cam[2]).clamp(0.05, 20.0),
    ])
}

/// DNG colour tags that rawloader does not read, parsed straight from the TIFF structure.
mod dng {
    /// What the decode needs from a DNG that recorded its white as a chromaticity.
    pub struct Colour {
        pub wb: [f32; 3],
        /// Interpolated for the as-shot white's colour temperature, in rawloader's layout.
        pub xyz_to_cam: [[f32; 3]; 4],
    }

    const AS_SHOT_WHITE_XY: u16 = 0xC629;
    const COLOR_MATRIX_1: u16 = 0xC621;
    const COLOR_MATRIX_2: u16 = 0xC622;
    const CAMERA_CALIBRATION_1: u16 = 0xC623;
    const CAMERA_CALIBRATION_2: u16 = 0xC624;
    const CALIBRATION_ILLUMINANT_1: u16 = 0xC65A;
    const CALIBRATION_ILLUMINANT_2: u16 = 0xC65B;
    const SUB_IFDS: u16 = 0x014A;

    /// As-shot white balance and a matching colour matrix, if the file has
    /// `AsShotWhiteXY` and at least one `ColorMatrix`.
    ///
    /// Follows the DNG spec's model: XYZ → camera is `CameraCalibration · ColorMatrix`,
    /// each interpolated between its two calibration illuminants linearly in inverse
    /// colour temperature. The white is given as a chromaticity, so its temperature is
    /// known directly and no iteration is needed (that is only for `AsShotNeutral`).
    pub fn colour(bytes: &[u8]) -> Option<Colour> {
        let tiff = Tiff::new(bytes)?;
        let white = tiff.find(AS_SHOT_WHITE_XY)?;
        let (x, y) = (*white.first()?, *white.get(1)?);

        let cm1 = tiff.find(COLOR_MATRIX_1).and_then(|v| matrix(&v));
        let cm2 = tiff.find(COLOR_MATRIX_2).and_then(|v| matrix(&v));
        let cc1 = tiff.find(CAMERA_CALIBRATION_1).and_then(|v| matrix(&v));
        let cc2 = tiff.find(CAMERA_CALIBRATION_2).and_then(|v| matrix(&v));
        let t1 = tiff
            .find(CALIBRATION_ILLUMINANT_1)
            .and_then(|v| kelvin(*v.first()?));
        let t2 = tiff
            .find(CALIBRATION_ILLUMINANT_2)
            .and_then(|v| kelvin(*v.first()?));

        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let (cm, cc) = match (cm1, cm2, t1, t2) {
            (Some(m1), Some(m2), Some(t1), Some(t2)) if t1 != t2 => {
                let w = weight(cct(x, y), t1, t2);
                (
                    lerp(&m1, &m2, w),
                    lerp(&cc1.unwrap_or(identity), &cc2.unwrap_or(identity), w),
                )
            }
            (Some(m), _, _, _) => (m, cc1.unwrap_or(identity)),
            (None, Some(m), _, _) => (m, cc2.unwrap_or(identity)),
            (None, None, _, _) => return None,
        };
        let xyz_to_cam = super::mul3(&cc, &cm);
        let wb = super::neutral_for_white(&xyz_to_cam, (x as f32, y as f32))?;
        Some(Colour {
            wb,
            xyz_to_cam: [xyz_to_cam[0], xyz_to_cam[1], xyz_to_cam[2], [0.0; 3]],
        })
    }

    fn matrix(v: &[f64]) -> Option<[[f32; 3]; 3]> {
        if v.len() < 9 {
            return None;
        }
        let mut m = [[0.0f32; 3]; 3];
        for (i, value) in v.iter().take(9).enumerate() {
            m[i / 3][i % 3] = *value as f32;
        }
        Some(m)
    }

    /// Weight toward illuminant 1, linear in inverse temperature, clamped to the pair.
    fn weight(cct: f64, t1: f64, t2: f64) -> f32 {
        let (inv, inv1, inv2) = (1.0 / cct, 1.0 / t1, 1.0 / t2);
        ((inv - inv2) / (inv1 - inv2)).clamp(0.0, 1.0) as f32
    }

    fn lerp(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3], w: f32) -> [[f32; 3]; 3] {
        let mut out = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                out[i][j] = w * a[i][j] + (1.0 - w) * b[i][j];
            }
        }
        out
    }

    /// McCamy's approximation: correlated colour temperature from a chromaticity. Good to
    /// a few kelvin across the range DNG calibration illuminants span.
    fn cct(x: f64, y: f64) -> f64 {
        let n = (x - 0.3320) / (0.1858 - y);
        (449.0 * n.powi(3) + 3525.0 * n.powi(2) + 6823.3 * n + 5520.33).clamp(1000.0, 25000.0)
    }

    /// EXIF LightSource codes, as colour temperatures.
    fn kelvin(code: f64) -> Option<f64> {
        Some(match code as u32 {
            1 | 4 | 9 => 5500.0, // daylight, flash, fine weather
            2 => 4200.0,         // fluorescent
            3 => 2850.0,         // tungsten
            10 => 6500.0,        // cloudy
            11 => 7500.0,        // shade
            12 => 6430.0,        // daylight fluorescent
            13 => 5000.0,        // day white fluorescent
            14 => 4150.0,        // cool white fluorescent
            15 => 3450.0,        // white fluorescent
            17 => 2856.0,        // standard light A
            18 => 4874.0,        // standard light B
            19 => 6774.0,        // standard light C
            20 => 5503.0,        // D55
            21 => 6504.0,        // D65
            22 => 7504.0,        // D75
            23 => 5003.0,        // D50
            24 => 3200.0,        // ISO studio tungsten
            _ => return None,
        })
    }

    /// Just enough TIFF to find a numeric tag anywhere in IFD0, its chain, and SubIFDs.
    struct Tiff<'a> {
        b: &'a [u8],
        le: bool,
    }

    impl<'a> Tiff<'a> {
        fn new(b: &'a [u8]) -> Option<Self> {
            let le = match b.get(..2)? {
                b"II" => true,
                b"MM" => false,
                _ => return None,
            };
            let t = Self { b, le };
            (t.u16(2)? == 42).then_some(t)
        }

        fn u16(&self, at: usize) -> Option<u16> {
            let s: [u8; 2] = self.b.get(at..at + 2)?.try_into().ok()?;
            Some(if self.le {
                u16::from_le_bytes(s)
            } else {
                u16::from_be_bytes(s)
            })
        }

        fn u32(&self, at: usize) -> Option<u32> {
            let s: [u8; 4] = self.b.get(at..at + 4)?.try_into().ok()?;
            Some(if self.le {
                u32::from_le_bytes(s)
            } else {
                u32::from_be_bytes(s)
            })
        }

        fn find(&self, tag: u16) -> Option<Vec<f64>> {
            let mut seen = Vec::new();
            self.search(self.u32(4)? as usize, tag, &mut seen, 0)
        }

        fn search(
            &self,
            ifd: usize,
            tag: u16,
            seen: &mut Vec<usize>,
            depth: u32,
        ) -> Option<Vec<f64>> {
            // Malformed files can loop an IFD chain back on itself.
            if ifd == 0 || depth > 8 || seen.contains(&ifd) {
                return None;
            }
            seen.push(ifd);
            let n = self.u16(ifd)? as usize;
            let mut subs = Vec::new();
            for i in 0..n {
                let e = ifd + 2 + 12 * i;
                let (t, kind, count) = (self.u16(e)?, self.u16(e + 2)?, self.u32(e + 4)? as usize);
                if t == tag {
                    return self.values(e, kind, count);
                }
                if t == SUB_IFDS {
                    subs = self.values(e, kind, count).unwrap_or_default();
                }
            }
            for sub in subs {
                if let Some(v) = self.search(sub as usize, tag, seen, depth + 1) {
                    return Some(v);
                }
            }
            let next = self.u32(ifd + 2 + 12 * n)? as usize;
            self.search(next, tag, seen, depth + 1)
        }

        fn values(&self, entry: usize, kind: u16, count: usize) -> Option<Vec<f64>> {
            let size = match kind {
                3 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 => 8,
                _ => return None,
            };
            let at = if size * count <= 4 {
                entry + 8
            } else {
                self.u32(entry + 8)? as usize
            };
            (0..count.min(64))
                .map(|i| {
                    let p = at + i * size;
                    Some(match kind {
                        3 => self.u16(p)? as f64,
                        4 | 13 => self.u32(p)? as f64,
                        9 => self.u32(p)? as i32 as f64,
                        11 => f32::from_bits(self.u32(p)?) as f64,
                        12 => {
                            let (a, b) = (self.u32(p)? as u64, self.u32(p + 4)? as u64);
                            let bits = if self.le {
                                a | (b << 32)
                            } else {
                                (a << 32) | b
                            };
                            f64::from_bits(bits)
                        }
                        5 => {
                            let (n, d) = (self.u32(p)?, self.u32(p + 4)?);
                            if d == 0 {
                                return None;
                            }
                            n as f64 / d as f64
                        }
                        10 => {
                            let (n, d) = (self.u32(p)? as i32, self.u32(p + 4)? as i32);
                            if d == 0 {
                                return None;
                            }
                            n as f64 / d as f64
                        }
                        _ => return None,
                    })
                })
                .collect()
        }
    }
}

/// Camera-native RGB → linear sRGB.
///
/// `xyz_to_cam` comes from the file. Inverting it gives camera→XYZ; composing with the
/// sRGB matrix gives camera→sRGB. Row-normalising makes a neutral camera signal land on a
/// neutral display signal, which is also why rawloader's dcraw-style ×10000 scaling does
/// not need undoing — a uniform scale cancels in the normalisation.
fn cam_to_srgb(xyz_to_cam: &[[f32; 3]; 4]) -> [[f32; 3]; 3] {
    let m = [xyz_to_cam[0], xyz_to_cam[1], xyz_to_cam[2]];

    let Some(cam_to_xyz) = invert3(&m) else {
        log::warn!("camera has no colour matrix; falling back to identity primaries");
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    };

    let mut out = mul3(&XYZ_TO_SRGB, &cam_to_xyz);

    for row in out.iter_mut() {
        let sum: f32 = row.iter().sum();
        if sum.is_finite() && sum.abs() > 1e-6 {
            for v in row.iter_mut() {
                *v /= sum;
            }
        }
    }

    if out.iter().flatten().any(|v| !v.is_finite()) {
        log::warn!("degenerate colour matrix; falling back to identity primaries");
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    out
}

fn mul3(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

fn invert3(m: &[[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let mut out = [[0.0f32; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            // Transposed cofactor = adjugate.
            let (r0, r1) = ((j + 1) % 3, (j + 2) % 3);
            let (c0, c1) = ((i + 1) % 3, (i + 2) % 3);
            *cell = (m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]) * inv_det;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverts_a_known_matrix() {
        let m = [[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 8.0]];
        let inv = invert3(&m).unwrap();
        assert!((inv[0][0] - 0.5).abs() < 1e-6);
        assert!((inv[1][1] - 0.25).abs() < 1e-6);
        assert!((inv[2][2] - 0.125).abs() < 1e-6);
    }

    #[test]
    fn round_trips_through_multiply() {
        let m = [[0.7, 0.2, 0.1], [0.2, 0.7, 0.1], [0.1, 0.2, 0.7]];
        let id = mul3(&m, &invert3(&m).unwrap());
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((id[i][j] - want).abs() < 1e-4, "{id:?}");
            }
        }
    }

    #[test]
    fn rejects_a_singular_matrix() {
        assert!(invert3(&[[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [1.0, 1.0, 1.0]]).is_none());
    }

    #[test]
    fn falls_back_when_the_camera_has_no_matrix() {
        let out = cam_to_srgb(&[[0.0; 3]; 4]);
        assert_eq!(out, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    }

    #[test]
    fn neutral_camera_signal_maps_to_neutral_display_signal() {
        // Canon 5D Mark III, dcraw-style ×10000 integers.
        let xyz_to_cam = [
            [6722.0, -635.0, -963.0],
            [-4287.0, 12460.0, 2028.0],
            [-908.0, 2162.0, 5668.0],
            [0.0, 0.0, 0.0],
        ];
        for row in cam_to_srgb(&xyz_to_cam) {
            let sum: f32 = row.iter().sum();
            assert!((sum - 1.0).abs() < 1e-4, "row sums to {sum}, not 1.0");
        }
    }

    #[test]
    fn white_balance_normalises_against_green() {
        let wb = neutral_multipliers(&[2048.0, 1024.0, 1536.0, 0.0]);
        assert!((wb[0] - 2.0).abs() < 1e-6);
        assert_eq!(wb[1], 1.0);
        assert!((wb[2] - 1.5).abs() < 1e-6);
    }

    /// A 2×2 RGGB mosaic `w × h`, every photosite `value`.
    fn flat_mosaic(w: usize, h: usize, value: impl Fn(usize) -> u16) -> Vec<u16> {
        (0..w * h).map(value).collect()
    }

    const RGGB: [u32; 4] = [0, 1, 1, 2];

    #[test]
    fn a_clip_below_the_declared_white_is_found() {
        // Canon-style: declared 12277, the sensor actually saturated at 11000.
        let (w, h) = (200, 200);
        let data = flat_mosaic(w, h, |i| {
            if i % 10 == 0 {
                11000
            } else {
                2048 + (i % 7919) as u16
            }
        });
        let white = measured_white(&data, w, &RGGB, &[2048.0; 4], &[12277.0; 4]);
        for (c, w) in white.iter().take(3).enumerate() {
            assert_eq!(*w, 11000.0, "colour {c}");
        }
    }

    #[test]
    fn an_unclipped_image_keeps_the_declared_white() {
        // A smooth ramp that never reaches a single repeated top value.
        let (w, h) = (200, 200);
        let data = flat_mosaic(w, h, |i| 2048 + (i % 9000) as u16);
        let white = measured_white(&data, w, &RGGB, &[2048.0; 4], &[12277.0; 4]);
        assert_eq!(white, [12277.0; 4]);
    }

    #[test]
    fn a_hot_pixel_is_not_a_clip() {
        let (w, h) = (200, 200);
        let data = flat_mosaic(w, h, |i| {
            if i == 555 {
                16383
            } else {
                2048 + (i % 9000) as u16
            }
        });
        let white = measured_white(&data, w, &RGGB, &[2048.0; 4], &[12277.0; 4]);
        assert_eq!(white, [12277.0; 4]);
    }

    #[test]
    fn a_hot_pixel_above_a_real_clip_does_not_hide_it() {
        let (w, h) = (200, 200);
        let data = flat_mosaic(w, h, |i| match i {
            555 => 16383,
            _ if i % 10 == 0 => 11000,
            _ => 2048 + (i % 7919) as u16,
        });
        let white = measured_white(&data, w, &RGGB, &[2048.0; 4], &[12277.0; 4]);
        for (c, w) in white.iter().take(3).enumerate() {
            assert_eq!(*w, 11000.0, "colour {c}");
        }
    }

    #[test]
    fn one_clipped_colour_sets_the_level_for_the_rest() {
        // Green saturates first on a neutral subject; red and blue never reach the clip.
        let (w, h) = (200, 200);
        let data = flat_mosaic(w, h, |i| {
            let (x, y) = (i % w, i / w);
            let green = (x + y) % 2 == 1;
            if green && i % 5 == 0 {
                13583
            } else {
                2048 + (i % 7000) as u16
            }
        });
        let white = measured_white(&data, w, &RGGB, &[2048.0; 4], &[12277.0; 4]);
        assert_eq!(white[0], 13583.0);
        assert_eq!(white[1], 13583.0);
        assert_eq!(white[2], 13583.0);
    }

    #[test]
    fn daylight_white_balance_comes_from_the_matrix() {
        // An identity camera sees D65 as XYZ(D65); the multipliers must undo exactly that.
        let id = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3]];
        let wb = daylight_multipliers(&id).unwrap();
        let (x, y) = D65_XY;
        let xyz = [x / y, 1.0, (1.0 - x - y) / y];
        assert!((wb[0] - 1.0 / xyz[0]).abs() < 1e-4);
        assert!((wb[2] - 1.0 / xyz[2]).abs() < 1e-4);
    }

    #[test]
    fn exif_orientation_maps_to_geometry() {
        use rawloader::Orientation as O;
        assert_eq!(orientation_geometry(O::Rotate90).quarter_turns, 1);
        assert_eq!(orientation_geometry(O::Rotate270).quarter_turns, 3);
        assert!(orientation_geometry(O::Normal).is_identity());
        let t = orientation_geometry(O::Transpose);
        assert!(t.quarter_turns == 1 && t.mirror_h && !t.mirror_v);
    }

    #[test]
    fn white_balance_survives_a_garbage_green() {
        assert_eq!(neutral_multipliers(&[1.0, 0.0, 1.0, 0.0]), [1.0, 1.0, 1.0]);
        assert_eq!(
            neutral_multipliers(&[1.0, f32::NAN, 1.0, 0.0]),
            [1.0, 1.0, 1.0]
        );
    }
}
