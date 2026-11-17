//! A photo larger than the GPU can hold, shrunk on the CPU until it fits.
//!
//! A texture side is capped by the device: 8192 px on many phones and older integrated
//! GPUs, 16384 on most desktops. A 100 MP medium-format RAW or a stitched panorama is
//! wider than that, and used to be refused outright. It now opens as a *proxy*: an integer
//! fraction of its size, edited and exported at that size, with a notice saying so.
//!
//! Tiling the full-resolution sensor instead would need a second render path (every
//! module that reads a neighbourhood redone per tile), which invariant 2 rules out.
//!
//! A mosaic is shrunk by demosaicing it the crudest way, averaging each colour inside a
//! block the pattern repeats whole in (2×2 for Bayer, 3×3 for X-Trans), straight into the
//! linear RGB a linear DNG carries; it is then drawn through that path, with its white
//! balance, matrices and orientation untouched. It loses the demosaic's highlight
//! reconstruction, which the linear path does not do.

use crate::decode::{CfaImage, RasterImage};
use crate::lens::FileWarp;

/// The smallest whole factor that brings `width × height` within `limit` on both sides.
pub fn factor_for(width: u32, height: u32, limit: u32) -> u32 {
    let limit = limit.max(1);
    width.max(height).div_ceil(limit).max(1)
}

/// `cfa`, shrunk by a whole factor (returned) so neither side exceeds `limit`; as it was
/// (factor 1) when it already fits. A shrunk mosaic comes back linear (see the module
/// docs); a linear one is box-averaged.
pub fn fit_cfa(cfa: CfaImage, limit: u32) -> (CfaImage, u32) {
    let want = factor_for(cfa.width, cfa.height, limit);
    if want <= 1 {
        return (cfa, 1);
    }
    let (rgb, w, h, k) = match cfa.linear.as_deref() {
        Some(rgb) => {
            let (out, w, h) = box_rgb(rgb, cfa.width, cfa.height, want);
            (out, w, h, want)
        }
        None => {
            // The block the pattern repeats in with every colour in it.
            let block = if cfa.cfa.is_bayer() { 2 } else { 3 };
            let (binned, bw, bh) = bin_mosaic(&cfa, block);
            // Then by whatever whole factor is still needed.
            let rest = factor_for(bw, bh, limit);
            let (out, w, h) = box_rgb(&binned, bw, bh, rest);
            (out, w, h, block * rest)
        }
    };
    let scale = 1.0 / k as f32;
    (
        CfaImage {
            data: Vec::new(),
            width: w,
            height: h,
            linear: Some(rgb),
            warp: cfa.warp.map(|warp| scaled(warp, scale)),
            ..cfa
        },
        k,
    )
}

/// `raster`, box-averaged by a whole factor (returned) so neither side exceeds `limit`.
pub fn fit_raster(raster: RasterImage, limit: u32) -> (RasterImage, u32) {
    let k = factor_for(raster.width, raster.height, limit);
    if k <= 1 {
        return (raster, 1);
    }
    let (w, h) = (raster.width / k, raster.height / k);
    let src = |x: u32, y: u32, c: usize| {
        let i = ((y * raster.width + x) * 4) as usize + c;
        half::f16::from_le_bytes([raster.data[i * 2], raster.data[i * 2 + 1]]).to_f32()
    };
    let mut data = Vec::with_capacity((w * h * 8) as usize);
    let n = (k * k) as f32;
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let mut sum = 0.0;
                for dy in 0..k {
                    for dx in 0..k {
                        sum += src(x * k + dx, y * k + dy, c);
                    }
                }
                data.extend_from_slice(&half::f16::from_f32(sum / n).to_le_bytes());
            }
        }
    }
    (
        RasterImage {
            width: w,
            height: h,
            data,
            ..raster
        },
        k,
    )
}

/// A lens warp in pixels of an image scaled by `s`.
pub fn scaled(mut warp: FileWarp, s: f32) -> FileWarp {
    warp.centre = [warp.centre[0] * s, warp.centre[1] * s];
    warp.m *= s;
    if let Some((k, unit)) = warp.vignetting {
        warp.vignetting = Some((k, unit * s));
    }
    warp
}

/// Each `block × block` of the mosaic as one RGB pixel: each colour the mean of its
/// photosites there, black taken off and scaled so white is 1.0 (the linear contract).
fn bin_mosaic(cfa: &CfaImage, block: u32) -> (Vec<f32>, u32, u32) {
    let (w, h) = (cfa.width / block, cfa.height / block);
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for by in 0..h {
        for bx in 0..w {
            let mut sum = [0.0f32; 3];
            let mut n = [0u32; 3];
            for dy in 0..block {
                for dx in 0..block {
                    let (row, col) = (by * block + dy, bx * block + dx);
                    let c = cfa.cfa.colour_at(row as usize, col as usize) as usize;
                    let v = f32::from(cfa.data[(row * cfa.width + col) as usize]);
                    let range = (cfa.white[c] - cfa.black[c]).max(1.0);
                    sum[c] += (v - cfa.black[c]).max(0.0) / range;
                    n[c] += 1;
                }
            }
            for c in 0..3 {
                out.push(if n[c] > 0 { sum[c] / n[c] as f32 } else { 0.0 });
            }
        }
    }
    (out, w, h)
}

/// RGB box-averaged by `k` each way (the edge's remainder dropped).
fn box_rgb(rgb: &[f32], width: u32, height: u32, k: u32) -> (Vec<f32>, u32, u32) {
    if k <= 1 {
        return (rgb.to_vec(), width, height);
    }
    let (w, h) = (width / k, height / k);
    let n = (k * k) as f32;
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut sum = 0.0;
                for dy in 0..k {
                    for dx in 0..k {
                        sum += rgb[(((y * k + dy) * width + x * k + dx) * 3) as usize + c];
                    }
                }
                out.push(sum / n);
            }
        }
    }
    (out, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::CfaPattern;

    #[test]
    fn the_factor_is_the_least_that_fits() {
        assert_eq!(factor_for(8000, 6000, 16384), 1);
        assert_eq!(factor_for(11648, 8736, 8192), 2);
        assert_eq!(factor_for(30000, 4000, 8192), 4);
    }

    #[test]
    fn a_flat_bayer_field_bins_to_its_level() {
        let mut cfa = crate::testkit::flat(40, 20, 0.5);
        cfa.cfa = CfaPattern::bayer([0, 1, 1, 2]);
        let (out, k) = fit_cfa(cfa, 10);
        assert_eq!(k, 4);
        assert_eq!((out.width, out.height), (10, 5));
        let rgb = out.linear.expect("linear now");
        assert!(out.data.is_empty());
        assert!(rgb.iter().all(|v| (v - rgb[0]).abs() < 1e-3), "not flat");
        assert!(rgb[0] > 0.0);
    }

    #[test]
    fn an_xtrans_mosaic_bins_by_three_with_every_colour() {
        let pattern = CfaPattern::xtrans("GGRGGBGGBGGRBRGRBGGGBGGRGGRGGBRBGBRG").unwrap();
        let mut cfa = crate::testkit::flat(36, 36, 0.3);
        cfa.cfa = pattern;
        let (out, k) = fit_cfa(cfa, 12);
        assert_eq!(k, 3);
        assert_eq!((out.width, out.height), (12, 12));
        let rgb = out.linear.unwrap();
        assert!(
            rgb.iter().all(|v| *v > 0.0),
            "a colour missing from a block"
        );
    }

    #[test]
    fn a_warp_follows_the_shrink() {
        let w = FileWarp {
            centre: [100.0, 50.0],
            m: 80.0,
            planes: [[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]; 3],
            tca: None,
            vignetting: Some(([0.1, 0.0, 0.0], 40.0)),
        };
        let s = scaled(w, 0.5);
        assert_eq!(s.centre, [50.0, 25.0]);
        assert_eq!(s.m, 40.0);
        assert_eq!(s.vignetting.unwrap().1, 20.0);
    }
}
