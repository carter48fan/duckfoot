//! RAW file decoding to a cropped CFA mosaic plus the metadata the GPU needs.
//!
//! The previous build read four fields out of the file and guessed the rest. It produced
//! green, uncropped images in the wrong primaries from a hardcoded RGGB assumption. See
//! DESIGN.md — every field handled here is handled because skipping it visibly breaks the
//! image, not for completeness.

use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use rawler::decoders::RawDecodeParams;
use rawler::imgop::xyz::Illuminant;
use rawler::rawsource::RawSource;

use crate::params::Geometry;

/// sRGB (D65) primaries — the standard XYZ→linear-sRGB matrix.
pub(crate) const XYZ_TO_SRGB: [[f32; 3]; 3] = [
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
    /// Which colour each photosite measured. Already phase-corrected for the crop.
    pub cfa: CfaPattern,
    /// Black level per CFA colour index.
    pub black: [f32; 4],
    /// White level per CFA colour index.
    pub white: [f32; 4],
    /// As-shot camera-neutral multipliers, normalised so green is 1.0.
    pub wb: [f32; 3],
    /// White-balanced camera RGB → linear sRGB; neutral maps to neutral (see `cam_to_srgb`).
    pub cam_to_srgb: [[f32; 3]; 3],
    /// Multipliers that neutralise a D65 white, green = 1. The normalisation of
    /// `cam_to_srgb` discards the camera's response to daylight, and kelvin cannot be
    /// read off a white balance without it (`temperature::WhiteScale`).
    pub daylight_wb: [f32; 3],
    /// How the camera says the frame is meant to be viewed, as the geometry that does it.
    /// A starting point for a new edit, not a fact about the pixels.
    pub orientation: Geometry,
    pub make: String,
    pub model: String,
    /// The distortion (and colour fringing) correction the file carries for itself, as a
    /// DNG's WarpRectilinear: applied as the photo is drawn, like its colour matrix.
    pub warp: Option<crate::lens::FileWarp>,
}

/// A colour filter array's repeating tile: a 2×2 Bayer tile, or Fuji's 6×6 X-Trans.
/// Colour indices 0 = R, 1 = G, 2 = B, row-major in `colours`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfaPattern {
    pub width: u32,
    pub height: u32,
    pub colours: [u8; 36],
}

impl CfaPattern {
    /// A Bayer tile, `[(0,0), (0,1), (1,0), (1,1)]`.
    pub const fn bayer(tile: [u32; 4]) -> Self {
        let mut colours = [0u8; 36];
        colours[0] = tile[0] as u8;
        colours[1] = tile[1] as u8;
        colours[2] = tile[2] as u8;
        colours[3] = tile[3] as u8;
        Self {
            width: 2,
            height: 2,
            colours,
        }
    }

    /// A 6×6 tile from its rows, as "GGRGGB…" (36 letters).
    pub fn xtrans(pattern: &str) -> Option<Self> {
        let colours: Vec<u8> = pattern
            .chars()
            .map(|c| match c {
                'R' => Some(0),
                'G' => Some(1),
                'B' => Some(2),
                _ => None,
            })
            .collect::<Option<_>>()?;
        Some(Self {
            width: 6,
            height: 6,
            colours: colours.try_into().ok()?,
        })
    }

    pub fn colour_at(&self, row: usize, col: usize) -> u32 {
        let (w, h) = (self.width as usize, self.height as usize);
        self.colours[(row % h) * w + col % w] as u32
    }

    pub fn is_bayer(&self) -> bool {
        (self.width, self.height) == (2, 2)
    }

    /// The top-left 2×2, which for Bayer is the whole pattern.
    pub fn tile2x2(&self) -> [u32; 4] {
        [
            self.colour_at(0, 0),
            self.colour_at(0, 1),
            self.colour_at(1, 0),
            self.colour_at(1, 1),
        ]
    }
}

impl CfaImage {
    pub fn megapixels(&self) -> f32 {
        (self.width as f32 * self.height as f32) / 1_000_000.0
    }
}

/// A decoded raster image (JPEG, PNG, TIFF, WebP) converted to linear camera-RGB half-floats.
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    /// 4 half-floats per pixel (R, G, B, A=0.0) in linear space (8 bytes per pixel), in the
    /// file's own primaries.
    pub data: Vec<u8>,
    /// Those primaries → linear sRGB: the identity for sRGB, which is most files.
    pub cam_to_srgb: [[f32; 3]; 3],
    pub orientation: Geometry,
    pub make: String,
    pub model: String,
}

impl RasterImage {
    pub fn megapixels(&self) -> f32 {
        (self.width as f32 * self.height as f32) / 1_000_000.0
    }
}

/// Decode standard raster image formats into linear camera space.
pub fn decode_raster(bytes: &[u8]) -> Result<RasterImage> {
    use image::ImageDecoder;
    use rayon::prelude::*;

    let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .context("reading raster image format")?
        .into_decoder()
        .context("decoding raster image format")?;
    // A profile that cannot be read is sRGB's fault-tolerant default, as every browser
    // treats it; one that is sRGB takes the fast path below.
    let profile = decoder
        .icc_profile()
        .ok()
        .flatten()
        .and_then(|icc| crate::icc::MatrixTrc::parse(&icc))
        .filter(|p| !p.is_srgb());
    let img = image::DynamicImage::from_decoder(decoder).context("decoding raster image format")?;
    let orientation = raster_orientation(bytes);
    let (width, height) = (img.width(), img.height());
    let srgb_curve = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let linear = |channel: usize, c: f32| match &profile {
        Some(p) => p.curves[channel].eval(c),
        None => srgb_curve(c),
    };
    let f16 = |v: f32| half::f16::from_f32(v).to_le_bytes();

    // 4 half-floats per pixel = 8 bytes per pixel; A = 0.0, the reconstruction neutral.
    // Alpha is not carried: the sensor texture's alpha is that neutral, not coverage.
    let mut data = vec![0u8; width as usize * height as usize * 8];
    let row_out = width as usize * 8;
    // Each channel through a table of every value it can take, already as half-float
    // bits: one lookup per channel instead of a transfer function and a conversion.
    fn by_table<T: Copy + Into<usize> + Sync>(
        data: &mut [u8],
        row_out: usize,
        pixels: &[T],
        luts: &[Vec<[u8; 2]>; 3],
    ) {
        use rayon::prelude::*;
        let row_in = row_out / 8 * 3;
        if row_in == 0 {
            return;
        }
        data.par_chunks_mut(row_out)
            .zip(pixels.par_chunks(row_in))
            .for_each(|(out, src)| {
                for (o, p) in out.chunks_exact_mut(8).zip(src.chunks_exact(3)) {
                    for c in 0..3 {
                        o[c * 2..c * 2 + 2].copy_from_slice(&luts[c][p[c].into()]);
                    }
                }
            });
    }
    let table = |levels: usize| -> [Vec<[u8; 2]>; 3] {
        std::array::from_fn(|c| {
            (0..levels)
                .map(|i| f16(linear(c, i as f32 / (levels - 1) as f32)))
                .collect()
        })
    };
    let deep = matches!(
        img,
        image::DynamicImage::ImageLuma16(_)
            | image::DynamicImage::ImageLumaA16(_)
            | image::DynamicImage::ImageRgb16(_)
            | image::DynamicImage::ImageRgba16(_)
    );
    let float = matches!(
        img,
        image::DynamicImage::ImageRgb32F(_) | image::DynamicImage::ImageRgba32F(_)
    );
    if float {
        // Float TIFFs hold light itself unless a profile says otherwise.
        let rgb = img.into_rgb32f();
        let row_in = width as usize * 3;
        if row_in > 0 {
            data.par_chunks_mut(row_out)
                .zip(rgb.as_raw().par_chunks(row_in))
                .for_each(|(out, src)| {
                    for (o, p) in out.chunks_exact_mut(8).zip(src.chunks_exact(3)) {
                        for c in 0..3 {
                            let v = match &profile {
                                Some(pr) => pr.curves[c].eval(p[c]),
                                None => p[c].max(0.0),
                            };
                            o[c * 2..c * 2 + 2].copy_from_slice(&f16(v));
                        }
                    }
                });
        }
    } else if deep {
        // 16 bits kept as 16: a smooth sky in a 16-bit TIFF is why it was saved as one.
        by_table(&mut data, row_out, img.into_rgb16().as_raw(), &table(65536));
    } else {
        by_table(&mut data, row_out, img.into_rgb8().as_raw(), &table(256));
    }

    Ok(RasterImage {
        width,
        height,
        data,
        // Pixels stay in their own primaries, as a RAW's stay in the camera's, and the
        // adjust pass converts: nothing a wide-gamut file holds goes negative on the way.
        cam_to_srgb: profile.map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], |p| {
            p.to_srgb
        }),
        orientation,
        make: "Raster".to_string(),
        model: "Image".to_string(),
    })
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

pub struct RawlerBackend;

impl RawDecoder for RawlerBackend {
    fn decode_bytes(&self, bytes: &[u8]) -> Result<CfaImage> {
        decode_source_rawler(&RawSource::new_from_slice(bytes))
    }

    /// Memory-mapped, not read: `fs::read` followed by `RawSource::new_from_slice` holds
    /// the whole file in memory twice and copies it twice before decoding starts, which
    /// on a 60 MB RAW is several times the cost of an uncompressed decode itself.
    fn decode_file(&self, path: &Path) -> Result<CfaImage> {
        let source = RawSource::new(path)
            .with_context(|| format!("{}: could not be read", path.display()))?;
        decode_source_rawler(&source).with_context(|| format!("{}", path.display()))
    }
}

fn decode_source_rawler(source: &RawSource) -> Result<CfaImage> {
    let mut img = decode_reader_rawler(source)?;
    with_dng_opcodes(&mut img, source.buf());
    Ok(with_dng_colour(img, source.buf()))
}

/// A DNG's own lens corrections: its gain maps and radial vignette applied to the mosaic
/// now, its warp kept for the adjust pass.
fn with_dng_opcodes(img: &mut Decoded, bytes: &[u8]) {
    let Some(area) = img.area else { return };
    let (list2, list3) = dng::opcodes(bytes);
    if list2.is_empty() && list3.is_empty() {
        return;
    }
    let cfa = &mut img.cfa;
    let (pattern, black) = (cfa.cfa, cfa.black);
    let gains: Vec<_> = list2
        .iter()
        .chain(&list3)
        .filter(|o| !matches!(o, crate::opcodes::Opcode::Warp(_)))
        .cloned()
        .collect();
    crate::opcodes::apply_gains(&gains, &mut cfa.data, cfa.width as usize, area, |r, c| {
        black[pattern.colour_at(r, c) as usize]
    });
    cfa.warp = list3.iter().find_map(|o| match o {
        crate::opcodes::Opcode::Warp(w) => crate::lens::FileWarp::new(w, area),
        _ => None,
    });
}

/// A decode that found no as-shot white balance of its own takes the DNG's colour tags
/// (AsShotWhiteXY and the dual-illuminant matrices), as Pixel-style DNGs need.
fn with_dng_colour(mut img: Decoded, bytes: &[u8]) -> CfaImage {
    if img.wb_missing {
        if let Some(dng) = dng::colour(bytes) {
            img.cfa.wb = dng.wb;
            img.cfa.cam_to_srgb = cam_to_srgb(&dng.xyz_to_cam);
            img.cfa.daylight_wb = daylight_multipliers(&dng.xyz_to_cam).unwrap_or(dng.wb);
        }
    }
    img.cfa
}

/// Default decoder: tries `rawler` first (~2,500 modern cameras including Sony A7 IV),
/// falling back to `rawloader` if needed.
pub struct DefaultRawDecoder;

impl RawDecoder for DefaultRawDecoder {
    fn decode_bytes(&self, bytes: &[u8]) -> Result<CfaImage> {
        RawlerBackend
            .decode_bytes(bytes)
            .or_else(|first| rawloader_only(bytes).map_err(|second| both_failed(first, second)))
    }

    fn decode_file(&self, path: &Path) -> Result<CfaImage> {
        RawlerBackend.decode_file(path).or_else(|first| {
            let bytes = std::fs::read(path)
                .with_context(|| format!("{}: could not be read", path.display()))?;
            rawloader_only(&bytes).map_err(|second| both_failed(first, second))
        })
    }
}

/// Each decoder is tried once. The error keeps both reasons: rawler's is usually the one
/// that explains a modern camera, rawloader's an old one.
fn both_failed(rawler: anyhow::Error, rawloader: anyhow::Error) -> anyhow::Error {
    log::debug!("rawler: {rawler:#}; rawloader: {rawloader:#}");
    anyhow::anyhow!("{rawler:#} (rawloader could not read it either: {rawloader:#})")
}

/// rawloader alone, with the DNG white-balance fallback, and no second try with rawler.
fn rawloader_only(bytes: &[u8]) -> Result<CfaImage> {
    Ok(with_dng_colour(
        decode_reader(&mut Cursor::new(bytes))?,
        bytes,
    ))
}

/// Convenience function to decode a RAW file using the default backend.
pub fn decode_raw(path: &Path) -> Result<CfaImage> {
    DefaultRawDecoder.decode_file(path)
}

pub struct RawloaderBackend;

impl RawDecoder for RawloaderBackend {
    fn decode_bytes(&self, bytes: &[u8]) -> Result<CfaImage> {
        rawloader_only(bytes)
    }
}

/// A decode, plus whether the file carried no usable as-shot white balance of its own.
struct Decoded {
    cfa: CfaImage,
    wb_missing: bool,
    /// Where the kept mosaic sits in the active area, which DNG opcodes are relative to.
    /// Only the rawler path knows it.
    area: Option<crate::opcodes::Area>,
}

fn decode_reader(reader: &mut dyn Read) -> Result<Decoded> {
    let mut raw =
        rawloader::decode(reader).map_err(|e| anyhow!("not a supported RAW file: {e:?}"))?;

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

    let samples =
        match std::mem::replace(&mut raw.data, rawloader::RawImageData::Integer(Vec::new())) {
            rawloader::RawImageData::Integer(d) => d,
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

    let data = crop_in_place(samples, src_w, cl, ct, width, height);

    // Cropping by a non-even offset rotates the mosaic phase. `shift` is rawloader's own
    // correction for exactly this; guessing RGGB here swaps red and blue on any BGGR,
    // GRBG or GBRG sensor.
    let cfa = raw.cfa.shift(cl, ct);
    if !cfa.is_valid() {
        bail!("{} {}: unrecognised CFA pattern", raw.make, raw.model);
    }
    let pattern = cfa_pattern(
        (cfa.width, cfa.height),
        |row, col| cfa.color_at(row, col),
        &cfa.to_string(),
        (&raw.make, &raw.model),
    )?;

    let mut black = [0.0f32; 4];
    let mut white = [65535.0f32; 4];
    for i in 0..4 {
        black[i] = raw.blacklevels[i] as f32;
        let w = raw.whitelevels[i] as f32;
        // A white level at or below black would make the normalisation divide by zero.
        white[i] = if w > black[i] { w } else { black[i] + 1.0 };
    }
    let white = measured_white(&data, width, &pattern, &black, &white);

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
            cfa: pattern,
            black,
            white,
            wb,
            cam_to_srgb: cam_to_srgb(&raw.xyz_to_cam),
            daylight_wb: daylight_multipliers(&raw.xyz_to_cam).unwrap_or(wb),
            orientation: orientation_geometry(raw.orientation),
            make: raw.make.clone(),
            model: raw.model.clone(),
            warp: None,
        },
        wb_missing,
        area: None,
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

/// The EXIF orientation of a TIFF-structured RAW (most of them: ARW, NEF, CR2, DNG,
/// PEF…), read without decoding anything. `None` for other containers.
/// The geometry for an EXIF orientation value (1-8).
pub(crate) fn orientation_from_exif(value: u16) -> Geometry {
    orientation_geometry_rawler(rawler::Orientation::from_u16(value))
}

pub(crate) fn tiff_orientation(bytes: &[u8]) -> Option<Geometry> {
    const ORIENTATION: u16 = 0x0112;
    let v = dng::tag(bytes, ORIENTATION)?;
    Some(orientation_geometry_rawler(rawler::Orientation::from_u16(
        v as u16,
    )))
}

/// The EXIF orientation of a JPEG, PNG, TIFF or WebP. Phones write every photo in the
/// sensor's own landscape frame and record which way up it was held only here, so a
/// raster that ignores it shows every portrait on its side.
pub fn raster_orientation(bytes: &[u8]) -> Geometry {
    use image::ImageDecoder;
    let exif = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_decoder().ok())
        .and_then(|mut d| d.orientation().ok())
        .map(|o| o.to_exif());
    exif.map(|v| orientation_geometry_rawler(rawler::Orientation::from_u16(v as u16)))
        .unwrap_or_default()
}

fn orientation_geometry_rawler(o: rawler::Orientation) -> Geometry {
    use rawler::Orientation as O;
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

fn extract_xyz_to_cam_rawler(raw: &rawler::RawImage) -> [[f32; 3]; 4] {
    let is_nonzero = |m: &[[f32; 3]; 4]| m[..3].iter().any(|row| row.iter().any(|&v| v != 0.0));
    if is_nonzero(&raw.xyz_to_cam) {
        return raw.xyz_to_cam;
    }
    let flat_opt = raw
        .color_matrix
        .get(&Illuminant::D65)
        .or_else(|| raw.color_matrix.get(&Illuminant::Daylight))
        .or_else(|| raw.camera.color_matrix.get(&Illuminant::D65))
        .or_else(|| raw.camera.color_matrix.get(&Illuminant::Daylight))
        .or_else(|| raw.color_matrix.values().next())
        .or_else(|| raw.camera.color_matrix.values().next());

    if let Some(flat) = flat_opt {
        if flat.len() >= 9 {
            return [
                [flat[0], flat[1], flat[2]],
                [flat[3], flat[4], flat[5]],
                [flat[6], flat[7], flat[8]],
                if flat.len() >= 12 {
                    [flat[9], flat[10], flat[11]]
                } else {
                    [0.0; 3]
                },
            ];
        }
    }
    [[0.0; 3]; 4]
}

fn decode_reader_rawler(raw_source: &RawSource) -> Result<Decoded> {
    let mut raw = rawler::decode(raw_source, &RawDecodeParams::default())
        .map_err(|e| anyhow!("not a supported RAW file (rawler): {e:?}"))?;

    if raw.cpp != 1 {
        bail!(
            "{} {}: {} components per pixel — non-mosaic sensors are not supported yet (@stub)",
            raw.make,
            raw.model,
            raw.cpp
        );
    }

    let samples = match std::mem::replace(&mut raw.data, rawler::RawImageData::Integer(Vec::new()))
    {
        rawler::RawImageData::Integer(d) => d,
        rawler::RawImageData::Float(_) => bail!(
            "{} {}: float-encoded RAW is not supported yet (@stub)",
            raw.make,
            raw.model
        ),
    };

    let src_w = raw.width;
    let src_h = raw.height;

    if samples.len() < src_w * src_h {
        bail!(
            "truncated sensor data: {} samples for {src_w}×{src_h}",
            samples.len()
        );
    }

    let crop = raw.crop_area.or(raw.active_area);
    let (cl, ct, width, height) = if let Some(rect) = crop {
        if !rect.is_empty() && rect.p.x + rect.d.w <= src_w && rect.p.y + rect.d.h <= src_h {
            (rect.p.x, rect.p.y, rect.d.w, rect.d.h)
        } else {
            (0, 0, src_w, src_h)
        }
    } else {
        (0, 0, src_w, src_h)
    };

    if width == 0 || height == 0 {
        bail!("empty sensor dimensions after crop: {width}x{height}");
    }

    // DNG opcodes are in active-area coordinates, and the kept window may be a crop of it.
    let area = {
        let (ax, ay, aw, ah) = match raw.active_area {
            Some(a) if !a.is_empty() => (a.p.x, a.p.y, a.d.w, a.d.h),
            _ => (0, 0, src_w, src_h),
        };
        crate::opcodes::Area {
            offset: (cl as i64 - ax as i64, ct as i64 - ay as i64),
            size: (aw as u32, ah as u32),
        }
    };
    let data = crop_in_place(samples, src_w, cl, ct, width, height);

    let raw_cfa = match &raw.photometric {
        rawler::rawimage::RawPhotometricInterpretation::Cfa(cfg) => cfg.cfa.clone(),
        _ => raw.camera.cfa.clone(),
    };
    if !raw_cfa.is_valid() {
        bail!("{} {}: unrecognised CFA pattern", raw.make, raw.model);
    }
    let cfa = raw_cfa.shift(cl, ct);
    let pattern = cfa_pattern(
        (cfa.width, cfa.height),
        |row, col| cfa.color_at(row, col),
        &cfa.to_string(),
        (&raw.make, &raw.model),
    )?;

    // rawler gives black per 2×2 *position* (hence the shift); the shader wants it per
    // colour. White is per component, so every position of a mosaic already agrees.
    let raw_black = by_colour(
        raw.blacklevel.shift(cl, ct).as_bayer_array(),
        &pattern.tile2x2(),
    );
    let raw_white = raw.whitelevel.as_bayer_array();
    let mut black = [0.0f32; 4];
    let mut white = [65535.0f32; 4];
    for i in 0..4 {
        black[i] = raw_black[i];
        let w = raw_white[i];
        white[i] = if w > black[i] { w } else { black[i] + 1.0 };
    }
    let white = measured_white(&data, width, &pattern, &black, &white);

    let xyz_to_cam = extract_xyz_to_cam_rawler(&raw);

    let wb_missing = raw.wb_coeffs[..3]
        .iter()
        .any(|c| !c.is_finite() || *c <= 0.0);
    let mut wb = neutral_multipliers(&raw.wb_coeffs);
    if wb_missing {
        wb = daylight_multipliers(&xyz_to_cam).unwrap_or(wb);
    }

    let make = if !raw.clean_make.is_empty() {
        raw.clean_make
    } else {
        raw.make
    };
    let model = if !raw.clean_model.is_empty() {
        raw.clean_model
    } else {
        raw.model
    };

    Ok(Decoded {
        cfa: CfaImage {
            data,
            width: width as u32,
            height: height as u32,
            cfa: pattern,
            black,
            white,
            wb,
            cam_to_srgb: cam_to_srgb(&xyz_to_cam),
            daylight_wb: daylight_multipliers(&xyz_to_cam).unwrap_or(wb),
            orientation: orientation_geometry_rawler(raw.orientation),
            make,
            model,
            warp: None,
        },
        wb_missing,
        area: Some(area),
    })
}

/// The CFA as a pattern the demosaic shaders know: a 2×2 Bayer tile, or a 6×6 X-Trans one.
///
/// Anything else is refused here rather than demosaiced wrongly: taking an unknown
/// pattern's top-left 2×2 as if it were Bayer is how X-Trans files used to open with maze
/// artefacts, the wrong colour and no error at all. A larger pattern that does repeat
/// every 2×2 (some files spell Bayer as 2×8) is Bayer.
fn cfa_pattern(
    (width, height): (usize, usize),
    colour_at: impl Fn(usize, usize) -> usize,
    name: &str,
    (make, model): (&str, &str),
) -> Result<CfaPattern> {
    let periodic = |pw: usize, ph: usize| {
        width % pw == 0
            && height % ph == 0
            && (0..height).all(|r| (0..width).all(|c| colour_at(r, c) == colour_at(r % ph, c % pw)))
    };
    let (pw, ph) = if periodic(2, 2) {
        (2, 2)
    } else if periodic(6, 6) {
        (6, 6)
    } else {
        bail!("{make} {model}: {width}×{height} CFA pattern '{name}' — not supported yet (@stub)");
    };
    let colours_used: Vec<usize> = (0..ph)
        .flat_map(|r| (0..pw).map(move |c| (r, c)))
        .map(|(r, c)| colour_at(r, c))
        .collect();
    if colours_used.iter().any(|c| *c > 2) {
        bail!(
            "{make} {model}: CFA pattern '{name}' has a fourth colour — not supported yet (@stub)"
        );
    }
    if (0..3).any(|c| !colours_used.contains(&c)) {
        bail!("{make} {model}: CFA pattern '{name}' lacks a colour — not supported yet (@stub)");
    }
    let mut colours = [0u8; 36];
    for (slot, c) in colours.iter_mut().zip(&colours_used) {
        *slot = *c as u8;
    }
    Ok(CfaPattern {
        width: pw as u32,
        height: ph as u32,
        colours,
    })
}

/// Per-position values of a 2×2 block as per-colour ones, in the `CfaImage::black` layout.
/// The two greens, which can differ by a count or two, are averaged; slot 3 repeats green.
fn by_colour(per_position: [f32; 4], cfa2x2: &[u32; 4]) -> [f32; 4] {
    let mut sum = [0.0f32; 3];
    let mut n = [0u32; 3];
    for (v, &c) in per_position.iter().zip(cfa2x2) {
        sum[c as usize] += v;
        n[c as usize] += 1;
    }
    let colour = |c: usize| {
        if n[c] > 0 {
            sum[c] / n[c] as f32
        } else {
            per_position[0]
        }
    };
    [colour(0), colour(1), colour(2), colour(1)]
}

/// Cut the `width × height` window at (`left`, `top`) out of a `src_w`-wide mosaic,
/// reusing its buffer. Each row only ever moves toward the front, so a forward pass of
/// overlapping moves is safe, and a full-frame "crop" costs nothing at all — rather than
/// a second sensor-sized allocation and copy per open.
fn crop_in_place(
    mut samples: Vec<u16>,
    src_w: usize,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
) -> Vec<u16> {
    if left == 0 && top == 0 && width == src_w {
        samples.truncate(width * height);
        return samples;
    }
    for row in 0..height {
        let from = (row + top) * src_w + left;
        samples.copy_within(from..from + width, row * width);
    }
    samples.truncate(width * height);
    samples
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
    cfa: &CfaPattern,
    black: &[f32; 4],
    declared: &[f32; 4],
) -> [f32; 4] {
    let hist = top_histogram(data, width, cfa, black, declared);
    plateau_white(&hist, black, declared)
}

/// Photosite counts per value and CFA colour, for the values the plateau search in
/// `plateau_white` can ever look at; zero below that.
///
/// That search gives up on any colour whose top sits in the bottom half of its range, and
/// then looks at most 8 tolerances (≤ 8 × 66 values) below the top. So nothing under the
/// lowest half-range mark, less that margin, can change its answer — and in a normally
/// exposed photo that is nearly every photosite. Skipping them avoids a scattered
/// increment per photosite, and rows are split across threads; together this takes the
/// pass from the single largest CPU cost of an open to a small one.
fn top_histogram(
    data: &[u16],
    width: usize,
    cfa: &CfaPattern,
    black: &[f32; 4],
    declared: &[f32; 4],
) -> Vec<[u32; 3]> {
    use rayon::prelude::*;

    let floor = (0..3)
        .map(|c| black[c] + 0.5 * (declared[c] - black[c]))
        .fold(f32::MAX, f32::min);
    let lo = (floor.floor() as i64 - 8 * 66 - 2).clamp(0, 65535) as u16;
    let bins = 65536 - lo as usize;

    // A multiple of every pattern's height (2 and 6), so every job starts on the same CFA
    // row phase.
    let rows_per_job = 96;
    let mut top = data
        .par_chunks((width * rows_per_job).max(1))
        .fold(
            || vec![[0u32; 3]; bins],
            |mut hist, block| {
                for (row, line) in block.chunks_exact(width).enumerate() {
                    for (col, &v) in line.iter().enumerate() {
                        if v >= lo {
                            hist[(v - lo) as usize][cfa.colour_at(row, col) as usize] += 1;
                        }
                    }
                }
                hist
            },
        )
        .reduce_with(|mut a, b| {
            for (x, y) in a.iter_mut().zip(&b) {
                for c in 0..3 {
                    x[c] += y[c];
                }
            }
            a
        })
        .unwrap_or_else(|| vec![[0u32; 3]; bins]);

    let mut hist = vec![[0u32; 3]; lo as usize];
    hist.append(&mut top);
    hist
}

/// The white level per CFA colour, given a histogram of the mosaic.
fn plateau_white(hist: &[[u32; 3]], black: &[f32; 4], declared: &[f32; 4]) -> [f32; 4] {
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

    const OPCODE_LIST_2: u16 = 51009;
    const OPCODE_LIST_3: u16 = 51022;

    /// The lens opcodes a DNG carries: OpcodeList2's (applied to the mosaic) and
    /// OpcodeList3's (after demosaic), as far as `opcodes` understands them.
    pub fn opcodes(bytes: &[u8]) -> (Vec<crate::opcodes::Opcode>, Vec<crate::opcodes::Opcode>) {
        let Some(tiff) = Tiff::new(bytes) else {
            return (Vec::new(), Vec::new());
        };
        let list = |tag| {
            tiff.find_bytes(tag)
                .map(crate::opcodes::parse_list)
                .unwrap_or_default()
        };
        (list(OPCODE_LIST_2), list(OPCODE_LIST_3))
    }

    /// The first value of a numeric tag in any TIFF-structured file, DNG or not.
    pub fn tag(bytes: &[u8], tag: u16) -> Option<f64> {
        Tiff::new(bytes)?.find(tag)?.first().copied()
    }

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
            let values = |e: usize, kind: u16, count: usize| self.values(e, kind, count);
            self.search(self.u32(4)? as usize, tag, &mut seen, 0, &values)
        }

        /// A tag's bytes as they are, for the byte and undefined types (opcode lists).
        fn find_bytes(&self, tag: u16) -> Option<&'a [u8]> {
            let mut seen = Vec::new();
            let bytes = |e: usize, kind: u16, count: usize| {
                if !matches!(kind, 1 | 7) {
                    return None;
                }
                let at = if count <= 4 {
                    e + 8
                } else {
                    self.u32(e + 8)? as usize
                };
                self.b.get(at..at.checked_add(count)?)
            };
            self.search(self.u32(4)? as usize, tag, &mut seen, 0, &bytes)
        }

        fn search<T>(
            &self,
            ifd: usize,
            tag: u16,
            seen: &mut Vec<usize>,
            depth: u32,
            read: &dyn Fn(usize, u16, usize) -> Option<T>,
        ) -> Option<T> {
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
                    return read(e, kind, count);
                }
                if t == SUB_IFDS {
                    subs = self.values(e, kind, count).unwrap_or_default();
                }
            }
            for sub in subs {
                if let Some(v) = self.search(sub as usize, tag, seen, depth + 1, read) {
                    return Some(v);
                }
            }
            let next = self.u32(ifd + 2 + 12 * n)? as usize;
            self.search(next, tag, seen, depth + 1, read)
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
/// `xyz_to_cam` comes from the file. Composed with sRGB → XYZ it gives sRGB → camera, and
/// inverting that gives camera → sRGB.
///
/// The camera side has to be normalised so a neutral lands on a neutral, and it is the
/// rows of sRGB → camera that get scaled — one gain per *camera* channel, which is exactly
/// what white balance already is, so the matrix only has to carry the primaries. Scaling
/// the rows of the finished camera → sRGB matrix instead also maps neutral to neutral,
/// but it is a gain per *display* channel, which distorts every colour that is not grey:
/// on a Canon 7D it turned the green row from dcraw's [-0.25, 1.76, -0.51] into
/// [-1.17, 3.56, -1.39], and every photo came out with pink skin and purple water. This is
/// dcraw's construction, and why the ×10000 scaling of its integer matrices does not need
/// undoing — a uniform scale cancels in the normalisation.
fn cam_to_srgb(xyz_to_cam: &[[f32; 3]; 4]) -> [[f32; 3]; 3] {
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let m = [xyz_to_cam[0], xyz_to_cam[1], xyz_to_cam[2]];
    let srgb_to_xyz = invert3(&XYZ_TO_SRGB).expect("the sRGB matrix is invertible");

    let mut srgb_to_cam = mul3(&m, &srgb_to_xyz);
    for row in srgb_to_cam.iter_mut() {
        let sum: f32 = row.iter().sum();
        if sum.is_finite() && sum.abs() > 1e-6 {
            for v in row.iter_mut() {
                *v /= sum;
            }
        }
    }

    let Some(out) = invert3(&srgb_to_cam) else {
        log::warn!("camera has no colour matrix; falling back to identity primaries");
        return identity;
    };

    if out.iter().flatten().any(|v| !v.is_finite()) {
        log::warn!("degenerate colour matrix; falling back to identity primaries");
        return identity;
    }
    out
}

/// Matrix × vector, in f64 for code that inverts what it computes.
pub(crate) fn mul3v(m: &[[f32; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|row| (0..3).map(|k| row[k] as f64 * v[k]).sum())
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

pub(crate) fn invert3(m: &[[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
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
    fn raster_pixels_become_linear_half_floats() {
        let img = image::RgbImage::from_fn(3, 2, |x, y| {
            image::Rgb([(x * 100) as u8, (y * 255) as u8, 188])
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img.clone())
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let out = decode_raster(&png).unwrap();
        assert_eq!((out.width, out.height), (3, 2));
        let to_linear = |v: u8| {
            let c = v as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        for (i, p) in img.pixels().enumerate() {
            let texel = &out.data[i * 8..i * 8 + 8];
            let half = |k: usize| half::f16::from_le_bytes([texel[k], texel[k + 1]]).to_f32();
            for c in 0..3 {
                assert!(
                    (half(c * 2) - to_linear(p[c])).abs() < 1e-3,
                    "pixel {i} channel {c}"
                );
            }
            assert_eq!(half(6), 0.0);
        }
    }

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
    fn camera_matrix_matches_dcraw() {
        // Canon EOS 7D, dcraw's adobe_coeff. Expected: dcraw's rgb_cam for the same
        // matrix. Normalising the wrong side still passes the neutral test above, and
        // gives a green row of [-1.17, 3.56, -1.39]: pink skin, purple water.
        let xyz_to_cam = [
            [6844.0, -996.0, -856.0],
            [-3876.0, 11761.0, 2396.0],
            [-593.0, 1772.0, 6198.0],
            [0.0, 0.0, 0.0],
        ];
        let want = [
            [1.9205, -1.0414, 0.1209],
            [-0.2470, 1.7594, -0.5124],
            [-0.0070, -0.4896, 1.4965],
        ];
        let got = cam_to_srgb(&xyz_to_cam);
        for i in 0..3 {
            for j in 0..3 {
                assert!((got[i][j] - want[i][j]).abs() < 1e-3, "{got:?}");
            }
        }
    }

    #[test]
    fn black_levels_move_from_positions_to_colours() {
        // GBRG, as a Canon 7D crop lands: positions (G, B, R, G).
        let black = by_colour([2047.0, 2049.0, 2051.0, 2045.0], &[1, 2, 0, 1]);
        assert_eq!(black, [2051.0, 2046.0, 2049.0, 2046.0]);
        assert_eq!(by_colour([512.0; 4], &RGGB), [512.0; 4]);
    }

    /// The Fuji X-T2's X-Trans layout, as rawloader's camera table records it.
    const XTRANS: &str = "GGRGGBGGBGGRBRGRBGGGBGGRGGRGGBRBGBRG";

    fn rawloader_tile(pattern: &str, shift: (usize, usize)) -> Result<CfaPattern> {
        let cfa = rawloader::CFA::new(pattern).shift(shift.0, shift.1);
        cfa_pattern(
            (cfa.width, cfa.height),
            |r, c| cfa.color_at(r, c),
            &cfa.to_string(),
            ("FUJIFILM", "X-T2"),
        )
    }

    fn rawler_tile(pattern: &str, shift: (usize, usize)) -> Result<CfaPattern> {
        let cfa = rawler::CFA::new(pattern).shift(shift.0, shift.1);
        cfa_pattern(
            (cfa.width, cfa.height),
            |r, c| cfa.color_at(r, c),
            &cfa.to_string(),
            ("FUJIFILM", "X-T2"),
        )
    }

    #[test]
    fn an_xtrans_pattern_is_read_as_its_whole_6x6_tile_at_every_phase() {
        // Every phase of it, since a crop can land the 6×6 tile anywhere: its top-left 2×2
        // is a plausible Bayer tile at some offsets, which is how it used to slip through
        // as Bayer.
        let tile = CfaPattern::xtrans(XTRANS).unwrap();
        for shift in [(0, 0), (1, 0), (0, 1), (3, 2), (5, 5)] {
            for got in [rawloader_tile(XTRANS, shift), rawler_tile(XTRANS, shift)] {
                let got = got.expect("X-Trans refused");
                assert_eq!((got.width, got.height), (6, 6));
                for r in 0..6 {
                    for c in 0..6 {
                        assert_eq!(
                            got.colour_at(r, c),
                            tile.colour_at(r + shift.1, c + shift.0),
                            "shift {shift:?} at ({r}, {c})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn bayer_patterns_pass_with_their_phase() {
        let tile = |p: Result<CfaPattern>| {
            let p = p.unwrap();
            assert!(p.is_bayer());
            p.tile2x2()
        };
        assert_eq!(tile(rawloader_tile("RGGB", (0, 0))), RGGB);
        assert_eq!(tile(rawler_tile("RGGB", (0, 0))), RGGB);
        // An odd crop offset on both axes turns RGGB into BGGR.
        assert_eq!(tile(rawloader_tile("RGGB", (1, 1))), [2, 1, 1, 0]);
        assert_eq!(tile(rawler_tile("RGGB", (1, 1))), [2, 1, 1, 0]);
        // Bayer spelled as a 2×8 repeat is still Bayer.
        assert_eq!(tile(rawler_tile("RGGBRGGBRGGBRGGB", (0, 0))), RGGB);
    }

    #[test]
    fn other_unsupported_patterns_say_what_they_are() {
        // Not 2×2-periodic, and not X-Trans either.
        let err = rawler_tile("RGGBGRBGRGGBRGGB", (0, 0)).unwrap_err();
        assert!(err.to_string().contains("2×8 CFA pattern"), "{err}");
        // A fourth colour (RGBE) still has its own refusal.
        let err = rawler_tile("RGEB", (0, 0)).unwrap_err();
        assert!(err.to_string().contains("fourth colour"), "{err}");
    }

    #[test]
    fn a_16_bit_png_keeps_its_levels() {
        use image::ImageEncoder;
        // A ramp finer than 8 bits can hold: 1024 distinct values across a row.
        let (w, h) = (1024u32, 2u32);
        let px: Vec<u16> = (0..w * h)
            .flat_map(|i| {
                let v = ((i % w) * 16 + 20000) as u16;
                [v, v, v]
            })
            .collect();
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                bytemuck::cast_slice(&px),
                w,
                h,
                image::ExtendedColorType::Rgb16,
            )
            .unwrap();
        let raster = decode_raster(&png).unwrap();
        let reds: std::collections::HashSet<[u8; 2]> = raster.data[..w as usize * 8]
            .chunks(8)
            .map(|t| [t[0], t[1]])
            .collect();
        assert!(reds.len() > 700, "{} levels", reds.len());
    }

    #[test]
    fn a_dngs_own_lens_opcodes_are_applied() {
        use crate::opcodes::tests::{f64s, gain_map_params, list};
        let (w, h) = (64, 48);
        let plain = DefaultRawDecoder
            .decode_bytes(&crate::testkit::synth_dng(w, h, false).unwrap())
            .unwrap();
        assert!(plain.warp.is_none());
        // Every photosite's light doubled, and a barrel correction.
        let list2 = list(&[(9, gain_map_params((0, 0, 1), (2, 2), &[2.0; 4]))]);
        let warp = [
            &1u32.to_be_bytes()[..],
            &f64s(&[1.0, -0.08, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5]),
        ]
        .concat();
        let list3 = list(&[(1, warp)]);
        let bytes =
            crate::testkit::synth_dng_with(w, h, false, &[(51009, list2), (51022, list3)]).unwrap();
        let fixed = DefaultRawDecoder.decode_bytes(&bytes).unwrap();
        let black = plain.black[1];
        let (a, b) = (
            plain.data[w * 20 + 21] as f32,
            fixed.data[w * 20 + 21] as f32,
        );
        assert!(((b - black) - 2.0 * (a - black)).abs() <= 1.0, "{a} -> {b}");
        let warp = fixed.warp.expect("the warp was not kept");
        assert_eq!(warp.planes[1], [1.0, 0.0, -0.08, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(
            (warp.centre[0] - w as f32 * 0.5).abs() < 1.0,
            "{:?}",
            warp.centre
        );
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
        let white = measured_white(
            &data,
            w,
            &CfaPattern::bayer(RGGB),
            &[2048.0; 4],
            &[12277.0; 4],
        );
        for (c, w) in white.iter().take(3).enumerate() {
            assert_eq!(*w, 11000.0, "colour {c}");
        }
    }

    #[test]
    fn an_unclipped_image_keeps_the_declared_white() {
        // A smooth ramp that never reaches a single repeated top value.
        let (w, h) = (200, 200);
        let data = flat_mosaic(w, h, |i| 2048 + (i % 9000) as u16);
        let white = measured_white(
            &data,
            w,
            &CfaPattern::bayer(RGGB),
            &[2048.0; 4],
            &[12277.0; 4],
        );
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
        let white = measured_white(
            &data,
            w,
            &CfaPattern::bayer(RGGB),
            &[2048.0; 4],
            &[12277.0; 4],
        );
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
        let white = measured_white(
            &data,
            w,
            &CfaPattern::bayer(RGGB),
            &[2048.0; 4],
            &[12277.0; 4],
        );
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
        let white = measured_white(
            &data,
            w,
            &CfaPattern::bayer(RGGB),
            &[2048.0; 4],
            &[12277.0; 4],
        );
        assert_eq!(white[0], 13583.0);
        assert_eq!(white[1], 13583.0);
        assert_eq!(white[2], 13583.0);
    }

    /// The histogram as it was built before `top_histogram`: every photosite, one thread.
    fn full_histogram(data: &[u16], width: usize, cfa: &CfaPattern) -> Vec<[u32; 3]> {
        let mut hist = vec![[0u32; 3]; 65536];
        for (row, line) in data.chunks_exact(width).enumerate() {
            for (col, &v) in line.iter().enumerate() {
                hist[v as usize][cfa.colour_at(row, col) as usize] += 1;
            }
        }
        hist
    }

    #[test]
    fn the_partial_histogram_measures_exactly_what_the_full_one_does() {
        // Tall enough to span several parallel jobs, with a clip, a hot pixel, a
        // dark frame and a frame that clips only just above the half-range mark.
        let (w, h) = (130, 500);
        let cases: Vec<Vec<u16>> = vec![
            flat_mosaic(w, h, |i| match i {
                555 => 16383,
                _ if i % 10 == 0 => 11000,
                _ => 2048 + (i % 7919) as u16,
            }),
            flat_mosaic(w, h, |i| 2048 + (i % 97) as u16),
            flat_mosaic(w, h, |i| {
                if i % 3 == 0 {
                    7200
                } else {
                    2048 + (i % 5000) as u16
                }
            }),
            flat_mosaic(w, h, |i| (i % 65536) as u16),
        ];
        for data in cases {
            let (black, declared) = ([2048.0; 4], [12277.0; 4]);
            let full = plateau_white(
                &full_histogram(&data, w, &CfaPattern::bayer(RGGB)),
                &black,
                &declared,
            );
            let part = measured_white(&data, w, &CfaPattern::bayer(RGGB), &black, &declared);
            assert_eq!(full, part);
        }
    }

    #[test]
    fn cropping_in_place_matches_a_copy() {
        let (src_w, src_h) = (9, 7);
        let src: Vec<u16> = (0..(src_w * src_h) as u16).collect();
        for (left, top, w, h) in [(0, 0, 9, 7), (1, 2, 6, 4), (0, 1, 9, 6), (3, 0, 6, 7)] {
            let want: Vec<u16> = (0..h)
                .flat_map(|r| src[(r + top) * src_w + left..][..w].to_vec())
                .collect();
            assert_eq!(crop_in_place(src.clone(), src_w, left, top, w, h), want);
        }
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

    #[test]
    fn exif_orientation_rawler_maps_to_geometry() {
        use rawler::Orientation as O;
        assert_eq!(orientation_geometry_rawler(O::Rotate90).quarter_turns, 1);
        assert_eq!(orientation_geometry_rawler(O::Rotate270).quarter_turns, 3);
        assert!(orientation_geometry_rawler(O::Normal).is_identity());
        let t = orientation_geometry_rawler(O::Transpose);
        assert!(t.quarter_turns == 1 && t.mirror_h && !t.mirror_v);
    }

    #[test]
    fn a_phone_jpeg_held_upright_opens_upright() {
        let mut jpeg = Vec::new();
        image::RgbImage::new(8, 4)
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        assert!(raster_orientation(&jpeg).is_identity());

        // APP1 with one IFD entry: Orientation (0x0112), SHORT, 6 = turn 90° clockwise.
        let tiff: &[u8] = &[
            b'M', b'M', 0, 42, 0, 0, 0, 8, // header, IFD at 8
            0, 1, // one entry
            0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, // orientation = 6
            0, 0, 0, 0, // no next IFD
        ];
        let mut app1 = vec![0xFF, 0xE1];
        app1.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(tiff);
        jpeg.splice(2..2, app1);

        let g = raster_orientation(&jpeg);
        assert_eq!(g.quarter_turns, 1);
        assert!(!g.mirror_h && !g.mirror_v);
        assert_eq!(decode_raster(&jpeg).unwrap().orientation.quarter_turns, 1);
    }

    #[test]
    fn extract_xyz_to_cam_extracts_d65_matrix() {
        use std::collections::HashMap;
        let mut raw = rawler::RawImage {
            camera: rawler::decoders::Camera::default(),
            make: "SONY".to_string(),
            model: "ILCE-7M4".to_string(),
            clean_make: "Sony".to_string(),
            clean_model: "ILCE-7M4".to_string(),
            width: 100,
            height: 100,
            cpp: 1,
            bps: 14,
            wb_coeffs: [2.0, 1.0, 1.5, f32::NAN],
            whitelevel: rawler::rawimage::WhiteLevel::default(),
            blacklevel: rawler::rawimage::BlackLevel::default(),
            xyz_to_cam: [[0.0; 3]; 4],
            photometric: rawler::rawimage::RawPhotometricInterpretation::BlackIsZero,
            active_area: None,
            crop_area: None,
            blackareas: Vec::new(),
            orientation: rawler::Orientation::Normal,
            data: rawler::RawImageData::Integer(vec![0; 10000]),
            color_matrix: HashMap::new(),
            dng_tags: HashMap::new(),
            fuji_rotation_width: None,
        };

        // Initially zero
        assert_eq!(extract_xyz_to_cam_rawler(&raw), [[0.0; 3]; 4]);

        // Insert Sony A7 IV D65 matrix
        let d65_matrix = vec![
            0.7374, -0.2389, -0.0551, -0.5435, 1.3162, 0.2519, -0.1006, 0.1795, 0.6552,
        ];
        raw.color_matrix
            .insert(rawler::imgop::xyz::Illuminant::D65, d65_matrix);

        let extracted = extract_xyz_to_cam_rawler(&raw);
        assert_eq!(extracted[0], [0.7374, -0.2389, -0.0551]);
        assert_eq!(extracted[1], [-0.5435, 1.3162, 0.2519]);
        assert_eq!(extracted[2], [-0.1006, 0.1795, 0.6552]);
        assert_eq!(extracted[3], [0.0; 3]);

        // cam_to_srgb converts it cleanly without panic
        let srgb = cam_to_srgb(&extracted);
        for row in srgb {
            let sum: f32 = row.iter().sum();
            assert!((sum - 1.0).abs() < 1e-4, "row sum is {sum}");
        }
    }
}
