//! RAW file decoding to a cropped CFA mosaic plus the metadata the GPU needs.
//!
//! The previous build read four fields out of the file and guessed the rest. It produced
//! green, uncropped images in the wrong primaries from a hardcoded RGGB assumption. See
//! DESIGN.md — every field handled here is handled because skipping it visibly breaks the
//! image, not for completeness.

use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};

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
        decode_reader(&mut cursor)
    }
}

fn decode_reader(reader: &mut dyn Read) -> Result<CfaImage> {
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

    Ok(CfaImage {
        data,
        width: width as u32,
        height: height as u32,
        cfa2x2,
        black,
        white,
        wb: neutral_multipliers(&raw.wb_coeffs),
        cam_to_srgb: cam_to_srgb(&raw.xyz_to_cam),
        make: raw.make.clone(),
        model: raw.model.clone(),
    })
}

/// As-shot white balance, normalised against green.
///
/// Sensor data without these applied is roughly 2× green — this is the single most
/// visible thing the previous decoder omitted.
fn neutral_multipliers(coeffs: &[f32; 4]) -> [f32; 3] {
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

    #[test]
    fn white_balance_survives_a_garbage_green() {
        assert_eq!(neutral_multipliers(&[1.0, 0.0, 1.0, 0.0]), [1.0, 1.0, 1.0]);
        assert_eq!(
            neutral_multipliers(&[1.0, f32::NAN, 1.0, 0.0]),
            [1.0, 1.0, 1.0]
        );
    }
}
