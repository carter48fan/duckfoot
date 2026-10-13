//! HEIF / HEIC stills (iPhone and Mac photos), through the vendored `lc-heif`, behind the
//! `heif` feature: HEVC is patented in some places, so whether a build reads it is a
//! distribution decision. Without the feature a HEIF file is still recognised, and refused
//! with a reason rather than as an unknown format.
//!
//! The decoder hands back 16-bit RGB in the file's own colour space with its ICC profile,
//! already turned and cropped as the container says, so it joins the raster path as a
//! 16-bit image would: linearised through its profile, pixels in their own primaries.

use anyhow::Result;

/// The HEVC brands of ISO/IEC 23008-12. `mif1` / `msf1` alone are only the generic HEIF
/// structure, which AVIF uses too, so they count only beside one of these.
const HEVC_BRANDS: [&[u8; 4]; 6] = [b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx"];

/// Whether `bytes` is a HEIF file of HEVC pictures, by its `ftyp` box.
pub fn is_heif(bytes: &[u8]) -> bool {
    if bytes.get(4..8) != Some(b"ftyp".as_slice()) {
        return false;
    }
    let Some(size) = bytes
        .get(..4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    else {
        return false;
    };
    // Major brand at 8, minor version at 12, compatible brands from 16 to the box's end.
    let end = size.clamp(16, 256).min(bytes.len());
    let major = bytes.get(8..12);
    let compatible = bytes.get(16..end).unwrap_or_default().chunks_exact(4);
    major
        .into_iter()
        .chain(compatible)
        .any(|b| HEVC_BRANDS.iter().any(|h| h.as_slice() == b))
}

/// The primary image: its pixels, and its ICC profile (one made for its `nclx` primaries
/// when it carries none).
#[cfg(feature = "heif")]
pub(crate) fn open(bytes: &[u8]) -> Result<(image::DynamicImage, Option<Vec<u8>>)> {
    use anyhow::{anyhow, bail};
    let info = lc_heif::probe(bytes).map_err(|e| anyhow!("{e}"))?;
    crate::decode::check_pixels(u64::from(info.width), u64::from(info.height))?;
    let options = lc_heif::Options {
        max_pixels: crate::decode::MAX_PIXELS,
        apply_transforms: true,
    };
    let d = lc_heif::decode(bytes, &options).map_err(|e| anyhow!("{e}"))?;
    let icc = match d.icc {
        Some(icc) => Some(icc),
        None => {
            // H.273 code points: transfer 16 is PQ, 18 HLG; primaries 1 BT.709 (sRGB's),
            // 2 unspecified, 12 Display P3, 9 BT.2020.
            if matches!(d.colour.transfer, 16 | 18) {
                bail!("HDR (PQ or HLG) HEIF images can't be opened yet");
            }
            match d.colour.primaries {
                12 => Some(crate::icc::display_p3_profile().to_vec()),
                9 => bail!("BT.2020 HEIF images without a colour profile can't be opened yet"),
                _ => None,
            }
        }
    };
    let (w, h) = (d.width, d.height);
    let img = if d.has_alpha {
        // The sensor texture's alpha is the highlight neutral, not coverage: dropped below.
        image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(w, h, d.data)
            .map(image::DynamicImage::ImageRgba16)
    } else {
        image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(w, h, d.data)
            .map(image::DynamicImage::ImageRgb16)
    }
    .ok_or_else(|| anyhow!("the HEIF decoder returned {w}×{h} with the wrong amount of data"))?;
    Ok((img, icc))
}

#[cfg(not(feature = "heif"))]
pub(crate) fn open(_bytes: &[u8]) -> Result<(image::DynamicImage, Option<Vec<u8>>)> {
    anyhow::bail!("this build of Duckfoot does not open HEIF/HEIC photos (built without `heif`)")
}

/// The file's own thumbnail, if it carries one at least `side` on its long edge, as an 8-bit
/// preview: what the shelf shows without decoding the photo.
pub fn thumbnail(bytes: &[u8], side: u32) -> Option<crate::preview::Preview> {
    #[cfg(feature = "heif")]
    {
        let options = lc_heif::Options {
            max_pixels: crate::decode::MAX_PIXELS,
            apply_transforms: true,
        };
        let d = lc_heif::decode_thumbnail(bytes, side, &options).ok()??;
        Some(to_preview(d))
    }
    #[cfg(not(feature = "heif"))]
    {
        let _ = (bytes, side);
        None
    }
}

/// The photo itself as an 8-bit preview, for a file with no thumbnail big enough: a whole
/// decode, so only when [`thumbnail`] has none.
pub fn preview(bytes: &[u8]) -> Option<crate::preview::Preview> {
    #[cfg(feature = "heif")]
    {
        let options = lc_heif::Options {
            max_pixels: crate::decode::MAX_PIXELS,
            apply_transforms: true,
        };
        lc_heif::decode(bytes, &options).ok().map(to_preview)
    }
    #[cfg(not(feature = "heif"))]
    {
        let _ = bytes;
        None
    }
}

/// Decoded 16-bit samples as an 8-bit preview, alpha dropped, shown as coded (a P3 photo's
/// thumbnail as sRGB, as a RAW's embedded JPEG is).
#[cfg(feature = "heif")]
fn to_preview(d: lc_heif::Decoded) -> crate::preview::Preview {
    {
        let channels = if d.has_alpha { 4 } else { 3 };
        let rgb = d
            .data
            .chunks_exact(channels)
            .flat_map(|p| [p[0], p[1], p[2]].map(|v| (v >> 8) as u8))
            .collect();
        crate::preview::Preview {
            width: d.width,
            height: d.height,
            rgb,
            orientation: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
        let mut b = ((16 + 4 * compatible.len()) as u32).to_be_bytes().to_vec();
        b.extend(b"ftyp");
        b.extend(major);
        b.extend([0; 4]);
        for c in compatible {
            b.extend(*c);
        }
        b
    }

    #[test]
    fn heif_is_told_from_avif_and_everything_else() {
        assert!(is_heif(&ftyp(b"heic", &[b"mif1", b"heic"])));
        assert!(is_heif(&ftyp(b"mif1", &[b"heic", b"miaf"])));
        assert!(!is_heif(&ftyp(b"avif", &[b"mif1", b"miaf"])));
        assert!(!is_heif(&ftyp(b"mif1", &[b"avif"])));
        assert!(!is_heif(b"\xff\xd8\xff\xe0 a JPEG"));
        assert!(!is_heif(&[]));
    }

    #[cfg(feature = "heif")]
    mod decoding {
        use lc_heif::testdata::{build, Prop, Spec};

        fn grey(x: u32, _: u32) -> [u16; 3] {
            [(x * 4 % 220 + 20) as u16, 128, 128]
        }

        /// The first channel of pixel `x` of row 0, as the sensor texture holds it.
        fn red(raster: &crate::RasterImage, x: usize) -> f32 {
            half::f16::from_le_bytes([raster.data[x * 8], raster.data[x * 8 + 1]]).to_f32()
        }

        #[test]
        fn an_srgb_heif_decodes_as_a_raster_does() {
            // Full-range BT.601 luma with neutral chroma: grey, in sRGB's primaries.
            let spec = Spec {
                props: vec![Prop::Nclx(1, 13, 6, true)],
                ..Spec::new(64, 40, &grey)
            };
            let bytes = build(&spec);
            assert!(super::is_heif(&bytes));
            let raster = crate::decode_raster(&bytes).unwrap();
            assert_eq!((raster.width, raster.height), (64, 40));
            assert_eq!(
                raster.cam_to_srgb,
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
            );
            for x in [0, 10, 50] {
                let v = f32::from(grey(x as u32, 0)[0]) / 255.0;
                let want = if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                };
                assert!(
                    (red(&raster, x) - want).abs() < 4e-3,
                    "{x}: {} for {want}",
                    red(&raster, x)
                );
            }
        }

        #[test]
        fn a_p3_heif_keeps_its_primaries_and_hdr_is_refused() {
            let p3 = Spec {
                props: vec![Prop::Nclx(12, 13, 6, true)],
                ..Spec::new(32, 32, &grey)
            };
            let raster = crate::decode_raster(&build(&p3)).unwrap();
            assert_ne!(raster.cam_to_srgb[0][0], 1.0, "P3 was read as sRGB");
            let pq = Spec {
                props: vec![Prop::Nclx(9, 16, 9, true)],
                ..Spec::new(32, 32, &grey)
            };
            let Err(e) = crate::decode_raster(&build(&pq)) else {
                panic!("a PQ HEIF opened");
            };
            assert!(format!("{e:#}").contains("HDR"), "{e:#}");
        }

        #[test]
        fn the_files_own_thumbnail_is_the_shelfs() {
            let spec = Spec {
                thumbnail: Some(&grey),
                ..Spec::new(128, 96, &grey)
            };
            let t = super::thumbnail(&build(&spec), 32).unwrap();
            assert_eq!((t.width, t.height, t.rgb.len()), (64, 48, 64 * 48 * 3));
            assert!(super::thumbnail(&build(&Spec::new(64, 64, &grey)), 32).is_none());
        }
    }

    #[cfg(not(feature = "heif"))]
    #[test]
    fn without_the_feature_heif_is_refused_by_name() {
        let Err(e) = crate::decode_raster(&ftyp(b"heic", &[b"mif1"])) else {
            panic!("HEIF opened without the feature");
        };
        assert!(format!("{e:#}").contains("HEIF"), "{e:#}");
    }
}
