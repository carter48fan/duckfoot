//! Reading the finished image back off the GPU and writing it to disk.
//!
//! DESIGN.md invariant 2 says one render path serves preview and export. Here that is not
//! a convention being maintained by discipline — export copies *the same texture the
//! screen is showing*, produced by the same shader from the same uniforms. There is no
//! second code path that could drift, because there is no second code path.
//!
//! This is also the only sanctioned GPU→CPU readback outside the scopes (invariant 4).

use std::io::{BufWriter, Seek, Write};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use image::ImageEncoder;
use serde::{Deserialize, Serialize};

use crate::exif_out::{self, MetadataPolicy};
use crate::metadata::PhotoInfo;

/// JPEG quality for export. `image`'s own default is 75, which bands skies and smears fine
/// texture, and a photo editor's export is the last place to throw quality away.
pub const JPEG_QUALITY: u8 = 92;

/// A finished image, tightly packed RGBA, ready to encode: 8 bits a channel, or 16.
pub struct ExportedImage<P = u8> {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` samples, no row padding.
    pub rgba: Vec<P>,
}

/// An export at the depth it was asked for.
pub enum Exported {
    Eight(ExportedImage<u8>),
    Sixteen(ExportedImage<u16>),
}

/// A file format export can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    Jpeg,
    Png,
    Tiff,
    /// Lossy at any quality below 100, lossless at 100.
    Webp,
    /// AV1 still, sRGB only: its encoder tags every file sRGB and carries no ICC profile.
    Avif,
    /// JPEG XL: lossy below quality 100, lossless at 100 (and always in Adobe RGB, which
    /// its lossy mode cannot describe); 8 or 16 bits.
    Jxl,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 6] = [
        Self::Jpeg,
        Self::Png,
        Self::Tiff,
        Self::Webp,
        Self::Avif,
        Self::Jxl,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Tiff => "tif",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Jxl => "jxl",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Tiff => "TIFF",
            Self::Webp => "WebP",
            Self::Avif => "AVIF",
            Self::Jxl => "JPEG XL",
        }
    }

    /// Whether the quality setting means anything for this format.
    pub fn is_lossy(self) -> bool {
        matches!(self, Self::Jpeg | Self::Webp | Self::Avif | Self::Jxl)
    }

    /// Whether quality 100 means lossless rather than merely the best lossy.
    pub fn lossless_at_100(self) -> bool {
        matches!(self, Self::Webp | Self::Jxl)
    }

    /// Whether it can hold 16 bits a channel.
    pub fn has_16_bit(self) -> bool {
        matches!(self, Self::Png | Self::Tiff | Self::Jxl)
    }

    /// Whether a file of it can say it is in `space`.
    pub fn holds(self, space: crate::gamut::OutputSpace) -> bool {
        self != Self::Avif || space == crate::gamut::OutputSpace::Srgb
    }

    /// Picks the encoder from the filename the user chose.
    ///
    /// An extension it cannot write is an error, not a guess: this used to write PNG bytes
    /// under any name, so a `.tif` export was a file that TIFF readers refuse.
    pub fn from_path(path: &Path) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("png") => Ok(Self::Png),
            Some("jpg" | "jpeg") => Ok(Self::Jpeg),
            Some("tif" | "tiff") => Ok(Self::Tiff),
            Some("webp") => Ok(Self::Webp),
            Some("avif") => Ok(Self::Avif),
            Some("jxl") => Ok(Self::Jxl),
            Some(other) => bail!(
                "{}: cannot export .{other} — choose .jpg, .png, .tif, .webp, .avif or .jxl",
                path.display()
            ),
            None => bail!(
                "{}: no file extension — choose .jpg, .png, .tif, .webp, .avif or .jxl",
                path.display()
            ),
        }
    }
}

/// Bits per channel in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BitDepth {
    #[default]
    Eight,
    /// PNG and TIFF only; the others are written at 8.
    Sixteen,
}

/// How big the export is. Never larger than the photo: upscaling adds pixels, not detail.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Resize {
    #[default]
    Original,
    LongEdge(u32),
    ShortEdge(u32),
    /// Fit inside a box, keeping the aspect.
    FitDimensions {
        width: u32,
        height: u32,
    },
    Megapixels(f32),
    Percentage(f32),
}

impl Resize {
    /// The size a `src_w × src_h` photo exports at.
    pub fn target(&self, src_w: u32, src_h: u32) -> (u32, u32) {
        let (w, h) = (src_w.max(1) as f64, src_h.max(1) as f64);
        let scale = match *self {
            Self::Original => 1.0,
            Self::LongEdge(long) => long as f64 / w.max(h),
            Self::ShortEdge(short) => short as f64 / w.min(h),
            Self::FitDimensions { width, height } => (width as f64 / w).min(height as f64 / h),
            Self::Megapixels(mp) => (mp as f64 * 1e6 / (w * h)).sqrt(),
            Self::Percentage(pct) => pct as f64 / 100.0,
        };
        let scale = scale.clamp(0.0, 1.0);
        if scale >= 1.0 {
            return (src_w, src_h);
        }
        (
            ((w * scale).round() as u32).max(1),
            ((h * scale).round() as u32).max(1),
        )
    }
}

/// Sharpening for where the picture is going, applied after the resize: a downscaled
/// image is softened by the filter, and ink on paper spreads. Off by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SharpenFor {
    #[default]
    Off,
    Screen,
    Matte,
    Glossy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SharpenAmount {
    Low,
    #[default]
    Standard,
    High,
}

impl SharpenFor {
    pub const ALL: [SharpenFor; 4] = [Self::Off, Self::Screen, Self::Matte, Self::Glossy];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Screen => "Screen",
            Self::Matte => "Matte paper",
            Self::Glossy => "Glossy paper",
        }
    }

    /// Unsharp-mask strength: matte paper swallows the most edge contrast.
    fn strength(self, amount: SharpenAmount) -> f32 {
        let medium = match self {
            Self::Off => return 0.0,
            Self::Screen => 1.0,
            Self::Glossy => 1.25,
            Self::Matte => 1.5,
        };
        medium
            * match amount {
                SharpenAmount::Low => 0.3,
                SharpenAmount::Standard => 0.6,
                SharpenAmount::High => 1.0,
            }
    }
}

impl SharpenAmount {
    pub const ALL: [SharpenAmount; 3] = [Self::Low, Self::Standard, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Standard => "Standard",
            Self::High => "High",
        }
    }
}

fn yes() -> bool {
    true
}

/// Everything about how a photo is exported. One set for the single export and every
/// batch recipe, so the two cannot drift apart again (they did: one wrote JPEG at 75, the
/// other at the recipe's quality, and only one could write TIFF). Fields added since the
/// first recipes were saved default, so older settings files still load.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ExportSettings {
    pub format: ExportFormat,
    /// 1..=100, for the lossy formats.
    pub quality: u8,
    #[serde(default)]
    pub sizing: Resize,
    #[serde(default)]
    pub depth: BitDepth,
    #[serde(default)]
    pub sharpen: SharpenFor,
    #[serde(default)]
    pub sharpen_amount: SharpenAmount,
    #[serde(default)]
    pub metadata: MetadataPolicy,
    /// GPS left out whatever `metadata` says: a photo posted from home says where home is.
    #[serde(default = "yes")]
    pub strip_location: bool,
    /// The colours it is written in, and the profile it carries to say so (W-09).
    #[serde(default)]
    pub color_space: crate::gamut::OutputSpace,
    /// Whether to strip the background using cross-platform neural inference.
    #[serde(default)]
    pub remove_background: bool,
    /// Matte color to replace background with (e.g. Some([255, 255, 255]) for catalog white).
    /// If None and format supports alpha (PNG, WebP, TIFF), produces transparent alpha channel.
    #[serde(default)]
    pub matte_color: Option<[u8; 3]>,
    /// Reserved for HDR gain map export (ISO 21496-1 / Ultra HDR). Not written yet: the
    /// pipeline has no HDR rendition to measure a gain map against (`finish()` clamps to
    /// 0..1), so the export path ignores this and the UI does not offer it. Kept so saved
    /// settings that carry it still load.
    #[serde(default)]
    pub hdr_gain_map: bool,
    /// Fine-tuning parameters for background cutout (threshold, feather, edge shift, defringing).
    #[serde(default)]
    pub cutout_params: crate::rembg::CutoutParams,
}

impl ExportSettings {
    pub fn new(format: ExportFormat) -> Self {
        Self {
            format,
            quality: JPEG_QUALITY,
            sizing: Resize::Original,
            depth: BitDepth::Eight,
            sharpen: SharpenFor::Off,
            sharpen_amount: SharpenAmount::Standard,
            metadata: MetadataPolicy::All,
            strip_location: true,
            color_space: crate::gamut::OutputSpace::Srgb,
            remove_background: false,
            matte_color: None,
            hdr_gain_map: false,
            cutout_params: crate::rembg::CutoutParams::default(),
        }
    }

    /// The options for a file named `path`, at the default quality.
    pub fn for_path(path: &Path) -> Result<Self> {
        Ok(Self::new(ExportFormat::from_path(path)?))
    }

    /// The depth the file will really have: 16 only where the format holds it.
    pub fn effective_depth(&self) -> BitDepth {
        if self.format.has_16_bit() {
            self.depth
        } else {
            BitDepth::Eight
        }
    }

    /// The colour space the file will really be in: sRGB where the format holds no other,
    /// so a recipe saved with one is written correctly rather than mislabelled.
    pub fn effective_space(&self) -> crate::gamut::OutputSpace {
        if self.format.holds(self.color_space) {
            self.color_space
        } else {
            crate::gamut::OutputSpace::Srgb
        }
    }
}

/// ISO 21496-1 metadata for HDR gain map still images (Ultra HDR).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GainMapMetadata {
    pub version: String,
    pub gain_map_min: f32,
    pub gain_map_max: f32,
    pub gamma: f32,
    pub offset_sdr: f32,
    pub offset_hdr: f32,
    pub hdr_capacity_min: f32,
    pub hdr_capacity_max: f32,
    pub base_rendition_is_sdr: bool,
}

impl Default for GainMapMetadata {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            gain_map_min: 0.0,
            gain_map_max: 2.0,
            gamma: 1.0,
            offset_sdr: 1.0 / 64.0,
            offset_hdr: 1.0 / 64.0,
            hdr_capacity_min: 0.0,
            hdr_capacity_max: 2.0,
            base_rendition_is_sdr: true,
        }
    }
}

/// An HDR gain map and ISO 21496-1 metadata payload.
#[derive(Debug, Clone, PartialEq)]
pub struct GainMap {
    pub width: u32,
    pub height: u32,
    /// 8-bit grayscale luminance gain values [0..255]
    pub data: Vec<u8>,
    pub metadata: GainMapMetadata,
}

impl GainMap {
    /// Compute a gain map from an SDR image (8-bit RGBA) and an HDR source (float RGB in linear light).
    pub fn compute(
        sdr_rgba: &[u8],
        hdr_rgb_f32: &[f32],
        width: u32,
        height: u32,
        max_headroom_stops: f32,
    ) -> Self {
        let metadata = GainMapMetadata {
            gain_map_max: max_headroom_stops,
            hdr_capacity_max: max_headroom_stops,
            ..Default::default()
        };
        let mut data = Vec::with_capacity((width * height) as usize);
        let denom = (metadata.gain_map_max - metadata.gain_map_min).max(1e-4);

        for i in 0..(width * height) as usize {
            let r_sdr = sdr_rgba[i * 4] as f32 / 255.0;
            let g_sdr = sdr_rgba[i * 4 + 1] as f32 / 255.0;
            let b_sdr = sdr_rgba[i * 4 + 2] as f32 / 255.0;
            let luma_sdr = 0.2126 * r_sdr + 0.7152 * g_sdr + 0.0722 * b_sdr;

            let r_hdr = hdr_rgb_f32.get(i * 3).copied().unwrap_or(r_sdr);
            let g_hdr = hdr_rgb_f32.get(i * 3 + 1).copied().unwrap_or(g_sdr);
            let b_hdr = hdr_rgb_f32.get(i * 3 + 2).copied().unwrap_or(b_sdr);
            let luma_hdr = (0.2126 * r_hdr + 0.7152 * g_hdr + 0.0722 * b_hdr).max(0.0);

            let gain = ((luma_hdr + metadata.offset_hdr) / (luma_sdr + metadata.offset_sdr)).log2();
            let norm = ((gain - metadata.gain_map_min) / denom).clamp(0.0, 1.0);
            let val = norm.powf(1.0 / metadata.gamma);
            data.push((val * 255.0).round() as u8);
        }

        Self {
            width,
            height,
            data,
            metadata,
        }
    }

    /// Format the ISO 21496-1 XMP metadata packet for embedding in image files.
    pub fn iso_21496_1_xmp(&self) -> String {
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/">
      <hdrgm:Version>{}</hdrgm:Version>
      <hdrgm:GainMapMin>{:.6}</hdrgm:GainMapMin>
      <hdrgm:GainMapMax>{:.6}</hdrgm:GainMapMax>
      <hdrgm:Gamma>{:.6}</hdrgm:Gamma>
      <hdrgm:OffsetSDR>{:.6}</hdrgm:OffsetSDR>
      <hdrgm:OffsetHDR>{:.6}</hdrgm:OffsetHDR>
      <hdrgm:HDRCapacityMin>{:.6}</hdrgm:HDRCapacityMin>
      <hdrgm:HDRCapacityMax>{:.6}</hdrgm:HDRCapacityMax>
      <hdrgm:BaseRenditionIsSDR>{}</hdrgm:BaseRenditionIsSDR>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>"#,
            self.metadata.version,
            self.metadata.gain_map_min,
            self.metadata.gain_map_max,
            self.metadata.gamma,
            self.metadata.offset_sdr,
            self.metadata.offset_hdr,
            self.metadata.hdr_capacity_min,
            self.metadata.hdr_capacity_max,
            if self.metadata.base_rendition_is_sdr {
                "True"
            } else {
                "False"
            }
        )
    }
}

/// A channel sample: 8 or 16 bits.
pub trait Sample: image::Primitive + Send + Sync + 'static {
    const MAX: f32;
    fn to_f32(self) -> f32;
    fn from_f32(v: f32) -> Self;
}

impl Sample for u8 {
    const MAX: f32 = 255.0;
    fn to_f32(self) -> f32 {
        f32::from(self)
    }
    fn from_f32(v: f32) -> Self {
        v.round().clamp(0.0, 255.0) as u8
    }
}

impl Sample for u16 {
    const MAX: f32 = 65535.0;
    fn to_f32(self) -> f32 {
        f32::from(self)
    }
    fn from_f32(v: f32) -> Self {
        v.round().clamp(0.0, 65535.0) as u16
    }
}

impl<P: Sample> ExportedImage<P> {
    pub fn megapixels(&self) -> f32 {
        (self.width as f32 * self.height as f32) / 1_000_000.0
    }

    /// This image scaled to `width × height` (Lanczos), or unchanged if it already is,
    /// its samples taken as encoded with sRGB's curve.
    pub fn resized(self, width: u32, height: u32) -> Result<Self> {
        self.resized_in(width, height, None)
    }

    /// [`Self::resized`], the samples encoded with a pure power `gamma` (Adobe RGB's) or,
    /// `None`, sRGB's curve. Filtered in linear light, as light adds up: averaged as
    /// encoded values, a fine light-and-dark pattern comes out darker than it is (a black
    /// and white checkerboard halved to 128 rather than the 188 its light averages to).
    /// Through linear 16-bit, which an 8-bit export loses nothing to and a 16-bit one only
    /// in its deepest shadows, rather than floats, which would be 16 bytes a pixel.
    pub fn resized_in(self, width: u32, height: u32, gamma: Option<f32>) -> Result<Self> {
        if (width, height) == (self.width, self.height) {
            return Ok(self);
        }
        self.check_size()?;
        let (w, h) = (self.width, self.height);
        let levels = P::MAX as usize + 1;
        let decode = |e: f32| match gamma {
            Some(g) => e.powf(g),
            None if e <= 0.04045 => e / 12.92,
            None => ((e + 0.055) / 1.055).powf(2.4),
        };
        let encode = |l: f32| match gamma {
            Some(g) => l.powf(1.0 / g),
            None if l <= 0.003_130_8 => l * 12.92,
            None => 1.055 * l.powf(1.0 / 2.4) - 0.055,
        };
        // Every code the samples can take, to linear 16-bit; every linear 16-bit value
        // back to a code. Alpha is coverage, already linear: only rescaled.
        let to_linear: Vec<u16> = (0..levels)
            .map(|v| (decode(v as f32 / P::MAX) * 65535.0).round() as u16)
            .collect();
        let to_code: Vec<P> = (0..65536u32)
            .map(|v| P::from_f32(encode(v as f32 / 65535.0) * P::MAX))
            .collect();
        let linear: Vec<u16> = self
            .rgba
            .chunks_exact(4)
            .flat_map(|p| {
                let lin = |v: P| to_linear[(v.to_f32() as usize).min(levels - 1)];
                let a = (p[3].to_f32() / P::MAX * 65535.0).round() as u16;
                [lin(p[0]), lin(p[1]), lin(p[2]), a]
            })
            .collect();
        let img = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(w, h, linear)
            .ok_or_else(|| anyhow!("{w}x{h} does not fit the buffer"))?;
        let out = image::imageops::resize(
            &img,
            width.max(1),
            height.max(1),
            image::imageops::FilterType::Lanczos3,
        );
        let (ow, oh) = (out.width(), out.height());
        let rgba = out
            .into_raw()
            .chunks_exact(4)
            .flat_map(|p| {
                let a = P::from_f32(f32::from(p[3]) / 65535.0 * P::MAX);
                [
                    to_code[usize::from(p[0])],
                    to_code[usize::from(p[1])],
                    to_code[usize::from(p[2])],
                    a,
                ]
            })
            .collect();
        Ok(Self {
            width: ow,
            height: oh,
            rgba,
        })
    }

    /// Unsharp mask: each channel pushed away from a small blur of itself (a 5-tap
    /// binomial, about one pixel). Rows are blurred as they are needed, so the extra
    /// memory is five rows, not a second copy of the image.
    pub fn sharpened(mut self, strength: f32) -> Self {
        if strength <= 0.0 || self.width < 3 || self.height < 3 {
            return self;
        }
        const K: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
        let (w, h) = (self.width as usize, self.height as usize);
        let row_len = w * 4;
        let blur_row = |src: &[P], y: usize| -> Vec<f32> {
            let row = &src[y * row_len..(y + 1) * row_len];
            let mut out = vec![0.0f32; row_len];
            for x in 0..w {
                for c in 0..3 {
                    let mut acc = 0.0;
                    for (k, weight) in K.iter().enumerate() {
                        let xx = (x as isize + k as isize - 2).clamp(0, w as isize - 1) as usize;
                        acc += weight * row[xx * 4 + c].to_f32();
                    }
                    out[x * 4 + c] = acc;
                }
            }
            out
        };
        // Row y is sharpened only after rows up to y + 2 have been blurred, and each row is
        // blurred before it is itself sharpened, so reading `rgba` in place is exact.
        let mut window: std::collections::VecDeque<(usize, Vec<f32>)> = Default::default();
        for y in 0..h {
            for k in 0..5 {
                let yy = (y as isize + k as isize - 2).clamp(0, h as isize - 1) as usize;
                if !window.iter().any(|(r, _)| *r == yy) {
                    window.push_back((yy, blur_row(&self.rgba, yy)));
                }
            }
            while window.len() > 5 {
                window.pop_front();
            }
            let rows: [&Vec<f32>; 5] = std::array::from_fn(|k| {
                let yy = (y as isize + k as isize - 2).clamp(0, h as isize - 1) as usize;
                &window
                    .iter()
                    .find(|(r, _)| *r == yy)
                    .expect("blurred above")
                    .1
            });
            let out = &mut self.rgba[y * row_len..(y + 1) * row_len];
            for (i, sample) in out.iter_mut().enumerate() {
                if i % 4 == 3 {
                    continue;
                }
                let blur: f32 = K.iter().zip(rows).map(|(w, r)| w * r[i]).sum();
                let v = sample.to_f32();
                *sample = P::from_f32(v + strength * (v - blur));
            }
        }
        self
    }

    fn check_size(&self) -> Result<()> {
        let needed = self.width as usize * self.height as usize * 4;
        if self.rgba.len() != needed {
            bail!(
                "buffer is {} samples, not the {needed} that {}x{} needs",
                self.rgba.len(),
                self.width,
                self.height
            );
        }
        Ok(())
    }
}
/// Samples to encode, borrowed.
#[derive(Clone, Copy)]
enum Samples<'a> {
    Eight(&'a [u8]),
    Sixteen(&'a [u16]),
}

impl ExportedImage<u8> {
    /// Encode to `path`, with the photo's `info` as its EXIF (subject to `settings`).
    ///
    /// Encodes straight from `rgba`: at 100 MP a copy of the buffer is 400 MB, which is
    /// the difference between an export and a phone killing the app.
    pub fn write(
        &self,
        path: &Path,
        settings: &ExportSettings,
        info: Option<&PhotoInfo>,
    ) -> Result<()> {
        self.check_size()?;
        write_atomically(path, |out| {
            encode(
                self.width,
                self.height,
                Samples::Eight(&self.rgba),
                settings,
                info,
                out,
            )
        })
    }

    /// Encode into any seekable writer (TIFF needs to seek back to patch its offsets).
    pub fn encode<W: Write + Seek>(
        &self,
        settings: &ExportSettings,
        info: Option<&PhotoInfo>,
        out: W,
    ) -> Result<()> {
        self.check_size()?;
        encode(
            self.width,
            self.height,
            Samples::Eight(&self.rgba),
            settings,
            info,
            out,
        )
    }

    pub fn remove_background(
        &mut self,
        session: &crate::rembg::RembgSession,
        matte: Option<[u8; 3]>,
        params: &crate::rembg::CutoutParams,
        touchups: &[crate::rembg::TouchupDab],
    ) -> Result<()> {
        let (w, h) = (self.width, self.height);
        let img = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(w, h, self.rgba.clone())
            .ok_or_else(|| anyhow!("{w}x{h} does not fit the buffer"))?;
        let dyn_img = image::DynamicImage::ImageRgba8(img);
        let mask = session.compute_mask_with_params(&dyn_img, params, touchups)?;

        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize * 4;
                let a = mask.get_pixel(x, y)[0];
                if let Some(bg) = matte {
                    let a_norm = a as f32 / 255.0;
                    let inv_a = 1.0 - a_norm;
                    self.rgba[idx] =
                        (self.rgba[idx] as f32 * a_norm + bg[0] as f32 * inv_a).round() as u8;
                    self.rgba[idx + 1] =
                        (self.rgba[idx + 1] as f32 * a_norm + bg[1] as f32 * inv_a).round() as u8;
                    self.rgba[idx + 2] =
                        (self.rgba[idx + 2] as f32 * a_norm + bg[2] as f32 * inv_a).round() as u8;
                    self.rgba[idx + 3] = 255;
                } else {
                    self.rgba[idx + 3] = a;
                }
            }
        }

        if params.defringe_white && matte.is_none() {
            crate::rembg::defringe_rgba(&mut self.rgba, w, h, [255, 255, 255]);
        }
        Ok(())
    }
}

impl ExportedImage<u16> {
    pub fn write(
        &self,
        path: &Path,
        settings: &ExportSettings,
        info: Option<&PhotoInfo>,
    ) -> Result<()> {
        self.check_size()?;
        write_atomically(path, |out| {
            encode(
                self.width,
                self.height,
                Samples::Sixteen(&self.rgba),
                settings,
                info,
                out,
            )
        })
    }

    pub fn encode<W: Write + Seek>(
        &self,
        settings: &ExportSettings,
        info: Option<&PhotoInfo>,
        out: W,
    ) -> Result<()> {
        self.check_size()?;
        encode(
            self.width,
            self.height,
            Samples::Sixteen(&self.rgba),
            settings,
            info,
            out,
        )
    }

    pub fn remove_background(
        &mut self,
        session: &crate::rembg::RembgSession,
        matte: Option<[u8; 3]>,
        params: &crate::rembg::CutoutParams,
        touchups: &[crate::rembg::TouchupDab],
    ) -> Result<()> {
        let (w, h) = (self.width, self.height);
        let eight = to_eight(&self.rgba);
        let img = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(w, h, eight)
            .ok_or_else(|| anyhow!("{w}x{h} does not fit the buffer"))?;
        let dyn_img = image::DynamicImage::ImageRgba8(img);
        let mask = session.compute_mask_with_params(&dyn_img, params, touchups)?;

        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize * 4;
                let a = mask.get_pixel(x, y)[0];
                let a16 = (a as u16) << 8 | a as u16;
                if let Some(bg) = matte {
                    let a_norm = a as f32 / 255.0;
                    let inv_a = 1.0 - a_norm;
                    let bg16 = [
                        ((bg[0] as u16) << 8 | bg[0] as u16) as f32,
                        ((bg[1] as u16) << 8 | bg[1] as u16) as f32,
                        ((bg[2] as u16) << 8 | bg[2] as u16) as f32,
                    ];
                    self.rgba[idx] =
                        (self.rgba[idx] as f32 * a_norm + bg16[0] * inv_a).round() as u16;
                    self.rgba[idx + 1] =
                        (self.rgba[idx + 1] as f32 * a_norm + bg16[1] * inv_a).round() as u16;
                    self.rgba[idx + 2] =
                        (self.rgba[idx + 2] as f32 * a_norm + bg16[2] * inv_a).round() as u16;
                    self.rgba[idx + 3] = u16::MAX;
                } else {
                    self.rgba[idx + 3] = a16;
                }
            }
        }

        if params.defringe_white && matte.is_none() {
            for y in 0..h {
                for x in 0..w {
                    let idx = (y * w + x) as usize * 4;
                    let a16 = self.rgba[idx + 3];
                    if a16 > 0 && a16 < u16::MAX {
                        let a = a16 as f32 / 65535.0;
                        let r = (self.rgba[idx] as f32 - 65535.0 * (1.0 - a)) / a;
                        let g = (self.rgba[idx + 1] as f32 - 65535.0 * (1.0 - a)) / a;
                        let b = (self.rgba[idx + 2] as f32 - 65535.0 * (1.0 - a)) / a;
                        self.rgba[idx] = r.clamp(0.0, 65535.0).round() as u16;
                        self.rgba[idx + 1] = g.clamp(0.0, 65535.0).round() as u16;
                        self.rgba[idx + 2] = b.clamp(0.0, 65535.0).round() as u16;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Exported {
    pub fn width(&self) -> u32 {
        match self {
            Exported::Eight(i) => i.width,
            Exported::Sixteen(i) => i.width,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            Exported::Eight(i) => i.height,
            Exported::Sixteen(i) => i.height,
        }
    }

    /// Resized and output-sharpened as `settings` say, with optional manual touch-up dabs.
    pub fn finished_with_touchups(
        self,
        settings: &ExportSettings,
        touchups: &[crate::rembg::TouchupDab],
    ) -> Result<Self> {
        let (w, h) = settings.sizing.target(self.width(), self.height());
        let strength = settings.sharpen.strength(settings.sharpen_amount);
        let gamma = settings.effective_space().gamma();
        let mut out = match self {
            Exported::Eight(i) => Exported::Eight(i.resized_in(w, h, gamma)?.sharpened(strength)),
            Exported::Sixteen(i) => {
                Exported::Sixteen(i.resized_in(w, h, gamma)?.sharpened(strength))
            }
        };
        if settings.remove_background {
            let matte = if settings.format == ExportFormat::Jpeg && settings.matte_color.is_none() {
                Some([255, 255, 255])
            } else {
                settings.matte_color
            };
            let session = crate::rembg::RembgSession::auto()?;
            match &mut out {
                Exported::Eight(i) => {
                    i.remove_background(&session, matte, &settings.cutout_params, touchups)?
                }
                Exported::Sixteen(i) => {
                    i.remove_background(&session, matte, &settings.cutout_params, touchups)?
                }
            }
        }
        Ok(out)
    }

    /// Resized and output-sharpened as `settings` say.
    pub fn finished(self, settings: &ExportSettings) -> Result<Self> {
        self.finished_with_touchups(settings, &[])
    }

    /// Finish and write with touch-ups applied to background removal.
    pub fn save_with_touchups(
        self,
        path: &Path,
        settings: &ExportSettings,
        info: Option<&PhotoInfo>,
        touchups: &[crate::rembg::TouchupDab],
    ) -> Result<()> {
        match self.finished_with_touchups(settings, touchups)? {
            Exported::Eight(i) => i.write(path, settings, info),
            Exported::Sixteen(i) => i.write(path, settings, info),
        }
    }

    /// Finish and write: what the single export and every batch recipe do.
    pub fn save(
        self,
        path: &Path,
        settings: &ExportSettings,
        info: Option<&PhotoInfo>,
    ) -> Result<()> {
        self.save_with_touchups(path, settings, info, &[])
    }
}

/// Beside the target and renamed over it once complete: a full disk or a crash mid-encode
/// leaves any earlier export of the same name intact, never half of one.
fn write_atomically(
    path: &Path,
    encode: impl FnOnce(&mut BufWriter<&mut std::fs::File>) -> Result<()>,
) -> Result<()> {
    crate::fsio::atomic_write(path, |file| {
        let mut out = BufWriter::new(file);
        encode(&mut out)?;
        out.flush()?;
        Ok(())
    })
    .with_context(|| format!("writing {}", path.display()))
}

/// 16-bit to 8, rounding.
fn to_eight(samples: &[u16]) -> Vec<u8> {
    samples
        .iter()
        .map(|&v| ((u32::from(v) * 255 + 32767) / 65535) as u8)
        .collect()
}

fn encode<W: Write + Seek>(
    w: u32,
    h: u32,
    samples: Samples,
    settings: &ExportSettings,
    info: Option<&PhotoInfo>,
    mut out: W,
) -> Result<()> {
    let quality = settings.quality.clamp(1, 100);
    let fields = exif_out::fields(
        info,
        settings.metadata,
        settings.strip_location,
        &exif_out::now(),
    );
    let exif = exif_out::block(&fields);
    let xmp = xmp_for(info, settings.metadata);
    let space = settings.effective_space();
    let icc = space.icc().to_vec();
    // JPEG and WebP are 8-bit formats whatever was asked.
    let eight_owned;
    let eight: Option<&[u8]> = match samples {
        Samples::Eight(s) => Some(s),
        Samples::Sixteen(s) if !settings.format.has_16_bit() => {
            eight_owned = to_eight(s);
            Some(&eight_owned)
        }
        Samples::Sixteen(_) => None,
    };
    match settings.format {
        ExportFormat::Png => {
            // Written to memory first when there is XMP to add: `image` cannot write the
            // chunk that holds it.
            let mut png = Vec::new();
            let mut enc = image::codecs::png::PngEncoder::new(&mut png);
            enc.set_icc_profile(icc)
                .map_err(|e| anyhow!("PNG colour profile: {e}"))?;
            if let Some(exif) = exif {
                enc.set_exif_metadata(exif)
                    .map_err(|e| anyhow!("PNG EXIF: {e}"))?;
            }
            match (eight, samples) {
                (Some(px), _) => enc.write_image(px, w, h, image::ExtendedColorType::Rgba8)?,
                (None, Samples::Sixteen(px)) => enc.write_image(
                    bytemuck::cast_slice(px),
                    w,
                    h,
                    image::ExtendedColorType::Rgba16,
                )?,
                (None, Samples::Eight(_)) => unreachable!("eight is Some for 8-bit samples"),
            }
            match &xmp {
                Some(xmp) => out.write_all(&png_with_xmp(&png, xmp)?)?,
                None => out.write_all(&png)?,
            }
        }
        ExportFormat::Tiff => write_tiff(out, w, h, samples, &fields, &icc, xmp.as_deref())?,
        ExportFormat::Jpeg => {
            let px = eight.expect("JPEG is always 8-bit");
            // JPEG has no alpha. The encoder reads each pixel as RGB, so the alpha —
            // always an opaque 255 here — is dropped without an RGB copy of the frame.
            let view = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(w, h, px)
                .ok_or_else(|| anyhow!("{w}x{h} does not fit the buffer"))?;
            let mut jpeg = Vec::new();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality);
            enc.set_icc_profile(icc)
                .map_err(|e| anyhow!("JPEG colour profile: {e}"))?;
            if let Some(exif) = exif {
                enc.set_exif_metadata(exif)
                    .map_err(|e| anyhow!("JPEG EXIF: {e}"))?;
            }
            enc.encode_image(&view)?;
            match &xmp {
                Some(xmp) => out.write_all(&jpeg_with_xmp(&jpeg, xmp)?)?,
                None => out.write_all(&jpeg)?,
            }
        }
        ExportFormat::Webp => {
            let px = eight.expect("WebP is always 8-bit");
            // `image` writes only lossless WebP; libwebp does both, but not metadata,
            // which goes in as RIFF chunks afterwards.
            let encoder = webp::Encoder::from_rgba(px, w, h);
            let bytes = if quality >= 100 {
                encoder.encode_lossless()
            } else {
                encoder.encode(f32::from(quality))
            };
            out.write_all(&webp_with_metadata(
                &bytes,
                &icc,
                exif.as_deref(),
                xmp.as_deref(),
            )?)?;
        }
        ExportFormat::Avif => {
            let px = eight.expect("AVIF is written from 8-bit samples");
            out.write_all(&encode_avif(px, w, h, quality, exif.as_deref())?)?;
        }
        ExportFormat::Jxl => {
            // Its lossy mode names its colour encoding, and Adobe RGB is not one it can name.
            let lossless = quality >= 100 || space == crate::gamut::OutputSpace::AdobeRgb;
            let pixels = match (eight, samples) {
                (Some(px), _) => JxlPixels::Eight(px),
                (None, Samples::Sixteen(px)) => JxlPixels::Sixteen(px),
                (None, Samples::Eight(_)) => unreachable!("eight is Some for 8-bit samples"),
            };
            let meta = JxlMeta {
                space,
                icc: &icc,
                exif: exif.as_deref(),
                xmp: xmp.as_deref(),
            };
            out.write_all(&encode_jxl(pixels, w, h, quality, lossless, &meta)?)?;
        }
    }
    Ok(())
}

/// How long rav1e looks for a smaller file: 1 (slowest) to 10. 6 writes a 24 MP photo in
/// seconds rather than minutes, a few percent larger than the slowest search.
const AVIF_SPEED: u8 = 6;

/// AVIF from RGBA8, through ravif (4:4:4, 10-bit AV1, tagged sRGB). EXIF goes in its own
/// item; the muxer has no place for XMP or an ICC profile, which is why AVIF is sRGB only.
fn encode_avif(px: &[u8], w: u32, h: u32, quality: u8, exif: Option<&[u8]>) -> Result<Vec<u8>> {
    let pixels: Vec<ravif::RGBA8> = px
        .chunks_exact(4)
        .map(|c| ravif::RGBA8::new(c[0], c[1], c[2], c[3]))
        .collect();
    let q = f32::from(quality.clamp(1, 100));
    let mut enc = ravif::Encoder::new()
        .with_quality(q)
        .with_alpha_quality(q)
        .with_speed(AVIF_SPEED);
    if let Some(exif) = exif {
        enc = enc.with_exif(exif);
    }
    let img = ravif::Img::new(pixels.as_slice(), w as usize, h as usize);
    // rav1e is mature, but an export that fails must never take the app with it.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| enc.encode_rgba(img)))
        .map_err(|_| anyhow!("the AVIF encoder failed"))?
        .map(|r| r.avif_file)
        .map_err(|e| anyhow!("AVIF: {e}"))
}

enum JxlPixels<'a> {
    Eight(&'a [u8]),
    Sixteen(&'a [u16]),
}

struct JxlMeta<'a> {
    space: crate::gamut::OutputSpace,
    icc: &'a [u8],
    exif: Option<&'a [u8]>,
    xmp: Option<&'a str>,
}

/// JPEG XL from RGBA samples through jixel. Lossy files declare their colour encoding
/// (sRGB or Display P3) and lossless ones carry the ICC profile; EXIF and XMP go in boxes
/// of their own. Alpha is kept only when some pixel is not opaque. As LightCraft does,
/// jixel runs under catch_unwind: it asserts internally.
fn encode_jxl(
    pixels: JxlPixels,
    w: u32,
    h: u32,
    quality: u8,
    lossless: bool,
    meta: &JxlMeta,
) -> Result<Vec<u8>> {
    use crate::gamut::OutputSpace;
    use jixel::{ColorEncoding, EncodeConfig, Speed};
    let colour = match meta.space {
        OutputSpace::DisplayP3 => ColorEncoding::display_p3(),
        OutputSpace::Srgb | OutputSpace::AdobeRgb => ColorEncoding::srgb(),
    };
    let mut c = EncodeConfig::default()
        .with_quality(f32::from(quality.clamp(1, 100)))
        .with_color_encoding(colour)
        .with_lossless(lossless)
        .with_speed(Speed::Fast);
    if lossless && !meta.icc.is_empty() {
        c = c.with_icc_profile(meta.icc.to_vec());
    }
    if let Some(exif) = meta.exif {
        c = c.with_exif(exif.to_vec());
    }
    if let Some(xmp) = meta.xmp {
        c = c.with_xmp(xmp.as_bytes().to_vec());
    }
    /// The samples as jixel takes them: RGB, or RGBA where alpha says something.
    fn channels<T: Copy + PartialEq>(rgba: &[T], opaque: T) -> (Vec<T>, bool) {
        let alpha = rgba.chunks_exact(4).any(|p| p[3] != opaque);
        let n = if alpha { 4 } else { 3 };
        let v = rgba.chunks_exact(4).flat_map(|p| p[..n].to_vec()).collect();
        (v, alpha)
    }
    let (w, h) = (w as usize, h as usize);
    let run = || match pixels {
        JxlPixels::Eight(px) => match channels(px, u8::MAX) {
            (v, false) => jixel::encode_image(&v, w, h, &c),
            (v, true) => jixel::encode_image_with_alpha(&v, w, h, &c),
        },
        JxlPixels::Sixteen(px) => match channels(px, u16::MAX) {
            (v, false) => jixel::encode_image_16bit(&v, w, h, &c),
            (v, true) => jixel::encode_image_with_alpha_16bit(&v, w, h, &c),
        },
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
        .map_err(|_| anyhow!("the JPEG XL encoder failed"))?
        .map_err(|e| anyhow!("JPEG XL: {e}"))
}

/// TIFF through the `tiff` crate itself, which `image`'s encoder wraps: that is what can
/// write an EXIF directory as well as the image's own.
fn write_tiff<W: Write + Seek>(
    out: W,
    w: u32,
    h: u32,
    samples: Samples,
    fields: &exif_out::Fields,
    icc: &[u8],
    xmp: Option<&str>,
) -> Result<()> {
    use tiff::encoder::{colortype, DirectoryEncoder, Rational, TiffEncoder, TiffKind};
    use tiff::tags::Tag;

    fn put<W: Write + Seek, K: TiffKind>(
        dir: &mut DirectoryEncoder<'_, W, K>,
        tag: u16,
        value: &exif_out::Value,
    ) -> tiff::TiffResult<()> {
        use exif_out::Value as V;
        let tag = Tag::from_u16_exhaustive(tag);
        match value {
            V::Ascii(s) => dir.write_tag(tag, s.as_str()),
            V::Short(v) => dir.write_tag(tag, v.as_slice()),
            V::Long(v) => dir.write_tag(tag, *v),
            V::Rational(v) => {
                let r: Vec<Rational> = v.iter().map(|&(n, d)| Rational { n, d }).collect();
                dir.write_tag(tag, r.as_slice())
            }
            V::Byte(v) | V::Undefined(v) => dir.write_tag(tag, v.as_slice()),
        }
    }

    let mut tiff = TiffEncoder::new(out)?;
    let sub = |tiff: &mut TiffEncoder<W>, entries: &[(u16, exif_out::Value)]| {
        if entries.is_empty() {
            return Ok(None);
        }
        let mut dir = tiff.extra_directory()?;
        for (tag, value) in entries {
            put(&mut dir, *tag, value)?;
        }
        dir.finish_with_offsets().map(|o| Some(o.offset))
    };
    let exif_at = sub(&mut tiff, &fields.exif)?;
    let gps_at = sub(&mut tiff, &fields.gps)?;

    macro_rules! image {
        ($colour:ty, $data:expr) => {{
            let mut img = tiff.new_image::<$colour>(w, h)?;
            let dir = img.encoder();
            dir.write_tag(Tag::IccProfile, icc)?;
            for (tag, value) in &fields.ifd0 {
                put(dir, *tag, value)?;
            }
            if let Some(xmp) = xmp {
                // XMLPacket (700), BYTE, as TIFF/EP and Adobe's XMP Part 3 have it.
                dir.write_tag(Tag::Unknown(700), xmp.as_bytes())?;
            }
            if let Some(at) = exif_at {
                dir.write_tag(Tag::ExifDirectory, at)?;
            }
            if let Some(at) = gps_at {
                dir.write_tag(Tag::GpsDirectory, at)?;
            }
            img.write_data($data)?;
        }};
    }
    match samples {
        Samples::Eight(px) => image!(colortype::RGBA8, px),
        Samples::Sixteen(px) => image!(colortype::RGBA16, px),
    }
    Ok(())
}

/// A simple WebP file (VP8 or VP8L) rewrapped as an extended one carrying the colour
/// profile and EXIF: VP8X first, ICCP before the image, EXIF after it.
fn webp_with_metadata(
    file: &[u8],
    icc: &[u8],
    exif: Option<&[u8]>,
    xmp: Option<&str>,
) -> Result<Vec<u8>> {
    if file.len() < 12 || &file[0..4] != b"RIFF" || &file[8..12] != b"WEBP" {
        bail!("libwebp returned something that is not a WebP file");
    }
    let mut chunks: Vec<([u8; 4], &[u8])> = Vec::new();
    let mut at = 12;
    while at + 8 <= file.len() {
        let id: [u8; 4] = file[at..at + 4].try_into()?;
        let len = u32::from_le_bytes(file[at + 4..at + 8].try_into()?) as usize;
        let body = file
            .get(at + 8..at + 8 + len)
            .ok_or_else(|| anyhow!("truncated WebP chunk"))?;
        chunks.push((id, body));
        at += 8 + len + len % 2;
    }
    let (mut width, mut height, mut alpha) = (0u32, 0u32, false);
    for (id, body) in &chunks {
        match id {
            b"VP8X" if body.len() >= 10 => {
                alpha |= body[0] & 0x10 != 0;
                width = u32::from_le_bytes([body[4], body[5], body[6], 0]) + 1;
                height = u32::from_le_bytes([body[7], body[8], body[9], 0]) + 1;
            }
            b"VP8 " if body.len() >= 10 && width == 0 => {
                width = u32::from(u16::from_le_bytes([body[6], body[7]]) & 0x3fff);
                height = u32::from(u16::from_le_bytes([body[8], body[9]]) & 0x3fff);
            }
            b"VP8L" if body.len() >= 5 && width == 0 => {
                let bits = u32::from_le_bytes(body[1..5].try_into()?);
                width = (bits & 0x3fff) + 1;
                height = ((bits >> 14) & 0x3fff) + 1;
                alpha |= (bits >> 28) & 1 == 1;
            }
            b"ALPH" => alpha = true,
            _ => {}
        }
    }
    if width == 0 || height == 0 {
        bail!("could not read the WebP's size");
    }

    let mut vp8x = vec![0u8; 10];
    vp8x[0] = 0x20
        | if alpha { 0x10 } else { 0 }
        | if exif.is_some() { 0x08 } else { 0 }
        | if xmp.is_some() { 0x04 } else { 0 };
    vp8x[4..7].copy_from_slice(&(width - 1).to_le_bytes()[..3]);
    vp8x[7..10].copy_from_slice(&(height - 1).to_le_bytes()[..3]);

    let mut body = b"WEBP".to_vec();
    let mut chunk = |id: &[u8], data: &[u8]| {
        body.extend_from_slice(id);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    };
    chunk(b"VP8X", &vp8x);
    chunk(b"ICCP", icc);
    for (id, data) in &chunks {
        if !matches!(id, b"VP8X" | b"ICCP" | b"EXIF" | b"XMP ") {
            chunk(id, data);
        }
    }
    if let Some(exif) = exif {
        chunk(b"EXIF", exif);
    }
    if let Some(xmp) = xmp {
        chunk(b"XMP ", xmp.as_bytes());
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// The XMP an export carries under `policy`: the photo's rating, label and keywords (all
/// metadata only) and its credit (all, or copyright only), or nothing.
fn xmp_for(info: Option<&PhotoInfo>, policy: MetadataPolicy) -> Option<String> {
    let info = info?;
    fn credit(s: &Option<String>) -> Option<&str> {
        s.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
    let marks = match policy {
        MetadataPolicy::None => return None,
        MetadataPolicy::CopyrightOnly => None,
        MetadataPolicy::All => info.catalog.as_ref(),
    };
    crate::xmp::packet(&crate::xmp::ExportXmp {
        marks,
        creator: credit(&info.artist),
        rights: credit(&info.copyright),
    })
}

/// The namespace that opens a JPEG's XMP segment (APP1), NUL included.
const JPEG_XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// A JPEG with `xmp` in an APP1 segment after its other application segments (EXIF and
/// ICC among them). A packet too big for one segment is left out: extended XMP, which
/// splits one across several, is more than a rating and keywords ever need.
fn jpeg_with_xmp(jpeg: &[u8], xmp: &str) -> Result<Vec<u8>> {
    let len = 2 + JPEG_XMP.len() + xmp.len();
    if len > usize::from(u16::MAX) {
        log::warn!(
            "the export's XMP is {} bytes, more than a JPEG segment holds; left out",
            xmp.len()
        );
        return Ok(jpeg.to_vec());
    }
    if jpeg.get(..2) != Some(&[0xFF, 0xD8]) {
        bail!("the JPEG encoder wrote something that is not a JPEG");
    }
    let mut at = 2;
    while let (Some(&0xFF), Some(&m)) = (jpeg.get(at), jpeg.get(at + 1)) {
        if !(0xE0..=0xEF).contains(&m) {
            break;
        }
        let seg = jpeg
            .get(at + 2..at + 4)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
            .ok_or_else(|| anyhow!("truncated JPEG segment"))?;
        at += 2 + seg;
    }
    let at = at.min(jpeg.len());
    let mut out = Vec::with_capacity(jpeg.len() + len + 2);
    out.extend_from_slice(&jpeg[..at]);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(JPEG_XMP);
    out.extend_from_slice(xmp.as_bytes());
    out.extend_from_slice(&jpeg[at..]);
    Ok(out)
}

/// A PNG with `xmp` in an `iTXt` chunk keyed `XML:com.adobe.xmp`, uncompressed, before
/// the image data, as Adobe's XMP Part 3 has it.
fn png_with_xmp(png: &[u8], xmp: &str) -> Result<Vec<u8>> {
    let mut at = 8;
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into()?) as usize;
        if &png[at + 4..at + 8] == b"IDAT" {
            break;
        }
        at += 12 + len;
    }
    if png.get(..8) != Some(b"\x89PNG\r\n\x1a\n".as_slice()) || at + 8 > png.len() {
        bail!("the PNG encoder wrote something that is not a PNG");
    }
    let mut data = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
    data.extend_from_slice(xmp.as_bytes());
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(b"iTXt");
    chunk.extend_from_slice(&data);
    let crc = crc32fast::hash(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..at]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[at..]);
    Ok(out)
}

/// wgpu requires each row of a texture→buffer copy to start on a 256-byte boundary, so a
/// readback buffer is wider than the image whenever `width * 4` is not already a multiple
/// of 256. 6048 px is 24192 bytes and pads to 24320 — 128 bytes of slack on every one of
/// 4024 rows. Handing that buffer straight to an encoder produces a picture with a
/// diagonal shear, which is the classic symptom of forgetting this.
pub fn padded_bytes_per_row(width: u32) -> u32 {
    padded_row_bytes(width, 4)
}

/// [`padded_bytes_per_row`] for texels of `texel` bytes (8 for RGBA16F).
pub fn padded_row_bytes(width: u32, texel: u32) -> u32 {
    let unpadded = width * texel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    unpadded.div_ceil(align) * align
}

/// Strips the per-row padding, producing a tightly packed RGBA buffer.
pub fn unpad_rows(padded: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let row = (width * 4) as usize;
    let stride = padded_bytes_per_row(width) as usize;
    let needed = stride * height as usize;

    if padded.len() < needed {
        bail!(
            "readback buffer is {} bytes, need {needed} for {width}x{height}",
            padded.len()
        );
    }

    let mut out = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        let start = y * stride;
        out.extend_from_slice(&padded[start..start + row]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_up_to_the_copy_alignment() {
        // 6048 px, the Sony A7 III width: 24192 bytes, which is not 256-aligned.
        assert_eq!(padded_bytes_per_row(6048), 24320);
        // Already aligned widths must not grow.
        assert_eq!(padded_bytes_per_row(64), 256);
        assert_eq!(padded_bytes_per_row(128), 512);
        // Every result is a multiple of the alignment and at least the unpadded size.
        for w in [1u32, 3, 63, 100, 1023, 4080, 6048, 8192] {
            let p = padded_bytes_per_row(w);
            assert_eq!(p % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0, "w={w}");
            assert!(p >= w * 4, "w={w}");
            assert!(p - w * 4 < wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, "w={w}");
        }
    }

    #[test]
    fn unpadding_drops_exactly_the_slack() {
        let (w, h) = (3u32, 2u32);
        let stride = padded_bytes_per_row(w) as usize; // 256, from 12 real bytes
        let mut padded = vec![0xAAu8; stride * h as usize];
        // Row 0 real pixels, then row 1, leaving the slack as 0xAA.
        padded[0..12].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        padded[stride..stride + 12]
            .copy_from_slice(&[13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24]);

        let out = unpad_rows(&padded, w, h).unwrap();
        assert_eq!(out.len(), 24);
        assert_eq!(out, (1u8..=24).collect::<Vec<u8>>());
        assert!(!out.contains(&0xAA), "padding leaked into the image");
    }

    #[test]
    fn unpadding_is_a_no_op_on_an_already_aligned_width() {
        let (w, h) = (64u32, 3u32);
        let src: Vec<u8> = (0..(w * 4 * h)).map(|i| (i % 251) as u8).collect();
        assert_eq!(unpad_rows(&src, w, h).unwrap(), src);
    }

    #[test]
    fn unpadding_rejects_a_short_buffer() {
        assert!(unpad_rows(&[0u8; 10], 6048, 4024).is_err());
    }

    #[test]
    fn format_follows_the_chosen_filename() {
        let format = |p: &str| ExportFormat::from_path(Path::new(p)).ok();
        assert_eq!(format("a.png"), Some(ExportFormat::Png));
        assert_eq!(format("a.JPG"), Some(ExportFormat::Jpeg));
        assert_eq!(format("a.jpeg"), Some(ExportFormat::Jpeg));
        assert_eq!(format("a.tif"), Some(ExportFormat::Tiff));
        assert_eq!(format("a.TIFF"), Some(ExportFormat::Tiff));
        assert_eq!(format("a.webp"), Some(ExportFormat::Webp));
        // Anything else is refused, rather than written as PNG bytes under a wrong name.
        assert_eq!(format("a"), None);
        assert_eq!(format("a.bmp"), None);
    }

    /// Deterministic noise, which is what makes JPEG quality show in the bytes.
    /// The formats `image` and kamadak-exif read back, which the tests below that decode
    /// an export, or read its EXIF, go through. AVIF and JPEG XL have tests of their own.
    const DECODABLE: [ExportFormat; 4] = [
        ExportFormat::Jpeg,
        ExportFormat::Png,
        ExportFormat::Tiff,
        ExportFormat::Webp,
    ];

    /// Every box at the top level of an ISOBMFF file (AVIF, HEIF) or inside `parent`'s
    /// body: type and body.
    fn boxes(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
        let mut out = Vec::new();
        let mut at = 0;
        while at + 8 <= b.len() {
            let size = u32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as usize;
            let kind: [u8; 4] = b[at + 4..at + 8].try_into().unwrap();
            if size < 8 || at + size > b.len() {
                break;
            }
            out.push((kind, &b[at + 8..at + size]));
            at += size;
        }
        out
    }

    #[test]
    fn an_avif_export_is_an_srgb_avif_with_its_exif() {
        let img = noise(40, 30);
        // Asked for in P3: AVIF cannot say so, so it is rendered and written as sRGB.
        let settings = ExportSettings {
            color_space: crate::gamut::OutputSpace::DisplayP3,
            ..ExportSettings::new(ExportFormat::Avif)
        };
        assert_eq!(settings.effective_space(), crate::gamut::OutputSpace::Srgb);
        let bytes = encoded(&img, &settings);
        let top = boxes(&bytes);
        let (_, ftyp) = top.iter().find(|(k, _)| k == b"ftyp").expect("no ftyp");
        assert_eq!(&ftyp[..4], b"avif");
        let (_, meta) = top.iter().find(|(k, _)| k == b"meta").expect("no meta");
        // `meta` is a full box: four bytes of version and flags before its children.
        let meta = boxes(&meta[4..]);
        let (_, iprp) = meta.iter().find(|(k, _)| k == b"iprp").expect("no iprp");
        let (_, ipco) = boxes(iprp)
            .into_iter()
            .find(|(k, _)| k == b"ipco")
            .expect("no ipco");
        let props = boxes(ipco);
        let (_, ispe) = props.iter().find(|(k, _)| k == b"ispe").expect("no ispe");
        let wh = (
            u32::from_be_bytes(ispe[4..8].try_into().unwrap()),
            u32::from_be_bytes(ispe[8..12].try_into().unwrap()),
        );
        assert_eq!(wh, (40, 30));
        assert!(
            props.iter().any(|(k, _)| k == b"av1C"),
            "no AV1 configuration"
        );
        // The muxer leaves `colr` out when it would only state the defaults (BT.709
        // primaries, the sRGB curve), which the AV1 sequence header states too; if it is
        // there it must say sRGB.
        if let Some((_, colr)) = props.iter().find(|(k, _)| k == b"colr") {
            assert_eq!(&colr[..4], b"nclx");
            assert_eq!(u16::from_be_bytes([colr[4], colr[5]]), 1);
            assert_eq!(u16::from_be_bytes([colr[6], colr[7]]), 13);
        }
        assert!(
            memchr::memmem::find(&bytes, b"ILCE-7M4").is_some(),
            "the EXIF item is missing"
        );
    }

    fn decode_jxl(bytes: &[u8]) -> (jxl_oxide::JxlImage, Vec<f32>) {
        let image = jxl_oxide::JxlImage::builder()
            .read(std::io::Cursor::new(bytes))
            .unwrap();
        let px = image
            .render_frame(0)
            .unwrap()
            .image_all_channels()
            .buf()
            .to_vec();
        (image, px)
    }

    #[test]
    fn a_jpeg_xl_export_reads_back_lossy_and_lossless() {
        let img = noise(40, 30);
        let lossy = encoded(&img, &ExportSettings::new(ExportFormat::Jxl));
        let (image, px) = decode_jxl(&lossy);
        assert_eq!((image.width(), image.height()), (40, 30));
        assert_eq!(
            px.len(),
            40 * 30 * 3,
            "an opaque image grew an alpha channel"
        );
        assert!(
            memchr::memmem::find(&lossy, b"ILCE-7M4").is_some(),
            "no EXIF box"
        );
        // Lossless at quality 100: every sample back exactly.
        let settings = ExportSettings {
            quality: 100,
            ..ExportSettings::new(ExportFormat::Jxl)
        };
        let (_, px) = decode_jxl(&encoded(&img, &settings));
        let want: Vec<f32> = img
            .rgba
            .chunks_exact(4)
            .flat_map(|p| p[..3].iter().map(|&v| f32::from(v) / 255.0))
            .collect();
        assert!(
            px.iter()
                .zip(&want)
                .all(|(a, b)| (a - b).abs() < 0.5 / 255.0),
            "lossless JPEG XL changed samples"
        );
    }

    #[test]
    fn a_jpeg_xl_export_says_its_colour_space() {
        let img = noise(16, 16);
        let p3 = ExportSettings {
            color_space: crate::gamut::OutputSpace::DisplayP3,
            ..ExportSettings::new(ExportFormat::Jxl)
        };
        let (image, _) = decode_jxl(&encoded(&img, &p3));
        let header = format!("{:?}", image.image_header().metadata.colour_encoding);
        assert!(header.contains("P3"), "a P3 export declared {header}");
        // Adobe RGB, which the lossy mode cannot name, is written lossless with its profile.
        let adobe = ExportSettings {
            color_space: crate::gamut::OutputSpace::AdobeRgb,
            ..ExportSettings::new(ExportFormat::Jxl)
        };
        let (image, _) = decode_jxl(&encoded(&img, &adobe));
        assert_eq!(
            image.original_icc(),
            Some(crate::gamut::OutputSpace::AdobeRgb.icc())
        );
    }

    fn noise(width: u32, height: u32) -> ExportedImage {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let rgba = (0..width * height)
            .flat_map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let [r, g, b, ..] = seed.to_le_bytes();
                [r, g, b, 255]
            })
            .collect();
        ExportedImage {
            width,
            height,
            rgba,
        }
    }

    fn scratch_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("duckfoot-export-{}-{name}", std::process::id()))
    }

    #[test]
    fn jpeg_is_encoded_at_the_export_quality() {
        let img = noise(64, 48);
        let mut ours = std::io::Cursor::new(Vec::new());
        img.encode(&ExportSettings::new(ExportFormat::Jpeg), None, &mut ours)
            .unwrap();

        let rgb: Vec<u8> = img
            .rgba
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let at = |quality: u8| {
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
                .encode(&rgb, 64, 48, image::ExtendedColorType::Rgb8)
                .unwrap();
            out
        };
        // The same pixels as a bare encoder at the export quality: the profile and EXIF
        // segments change the file, not the picture.
        let decode = |b: &[u8]| image::load_from_memory(b).unwrap().into_rgb8().into_raw();
        assert_eq!(
            decode(ours.get_ref()),
            decode(&at(JPEG_QUALITY)),
            "not encoded at q{JPEG_QUALITY}"
        );
        // Noise is incompressible, so quality shows directly as size.
        assert!(ours.get_ref().len() > at(75).len() * 5 / 4);
    }

    #[test]
    fn tif_writes_a_real_lossless_tiff() {
        let img = noise(37, 11);
        let path = scratch_path("real.tif");
        img.write(&path, &ExportSettings::for_path(&path).unwrap(), None)
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert!(bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*"));
        let back = image::load_from_memory_with_format(&bytes, image::ImageFormat::Tiff)
            .unwrap()
            .into_rgba8();
        assert_eq!(back.dimensions(), (37, 11));
        assert_eq!(back.into_raw(), img.rgba);
    }

    #[test]
    fn an_unknown_extension_is_an_error_and_writes_nothing() {
        let path = scratch_path("nope.bmp");
        let err = ExportSettings::for_path(&path).unwrap_err();
        assert!(err.to_string().contains("cannot export .bmp"), "{err}");
        assert!(!path.exists());
    }

    #[test]
    fn a_short_buffer_is_an_error_not_a_panic() {
        let mut img = noise(8, 8);
        img.rgba.truncate(10);
        let mut out = std::io::Cursor::new(Vec::new());
        for format in ExportFormat::ALL {
            assert!(img
                .encode(&ExportSettings::new(format), None, &mut out)
                .is_err());
        }
    }

    #[test]
    fn webp_is_lossy_below_100_and_lossless_at_it() {
        let img = noise(64, 48);
        let encode = |quality| {
            let mut out = std::io::Cursor::new(Vec::new());
            img.encode(
                &ExportSettings {
                    quality,
                    ..ExportSettings::new(ExportFormat::Webp)
                },
                None,
                &mut out,
            )
            .unwrap();
            out.into_inner()
        };
        let (lossy, lossless) = (encode(80), encode(100));
        assert!(lossy.starts_with(b"RIFF") && lossless.starts_with(b"RIFF"));
        assert!(lossy.len() < lossless.len(), "quality 80 was not lossy");
        let back = image::load_from_memory_with_format(&lossless, image::ImageFormat::WebP)
            .unwrap()
            .into_rgba8();
        assert_eq!(back.into_raw(), img.rgba, "quality 100 was not lossless");
    }

    fn sample_info() -> PhotoInfo {
        PhotoInfo {
            make: "SONY".into(),
            model: "ILCE-7M4".into(),
            exposure_time: Some((1, 250)),
            f_number: Some(2.8),
            iso: Some(400),
            captured: Some("2026:05:04 10:11:12".into()),
            gps: Some((48.8584, 2.2945)),
            copyright: Some("(c) 2026 A. Photographer".into()),
            ..Default::default()
        }
    }

    fn encoded(img: &ExportedImage, settings: &ExportSettings) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        img.encode(settings, Some(&sample_info()), &mut out)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn every_format_carries_the_photos_marks_as_xmp_and_still_reads() {
        let img = noise(40, 30);
        let mut marks = crate::sidecar::AssetMetadata {
            rating: 4,
            color_label: Some("red".into()),
            ..Default::default()
        };
        marks.add_keywords("harbour, boats");
        let info = PhotoInfo {
            catalog: Some(marks.clone()),
            ..sample_info()
        };
        for format in DECODABLE {
            let at = |policy| {
                let settings = ExportSettings {
                    metadata: policy,
                    ..ExportSettings::new(format)
                };
                let mut out = std::io::Cursor::new(Vec::new());
                img.encode(&settings, Some(&info), &mut out).unwrap();
                out.into_inner()
            };
            let all = at(MetadataPolicy::All);
            // The file is still one: it decodes (a PNG's chunk CRCs checked), EXIF and all.
            image::load_from_memory(&all).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            exif::Reader::new()
                .read_from_container(&mut std::io::Cursor::new(&all))
                .unwrap_or_else(|e| panic!("{format:?}: no EXIF beside the XMP ({e})"));
            let x = crate::xmp::find_packet(&all).unwrap_or_else(|| panic!("{format:?}: no XMP"));
            assert_eq!(x.marks().as_ref(), Some(&marks), "{format:?}");
            assert_eq!(
                x.properties
                    .get("dc:rights")
                    .and_then(|v| v.first())
                    .map(String::as_str),
                Some("(c) 2026 A. Photographer"),
                "{format:?}"
            );
            // Copyright only: the credit, not the marks.
            let credit = crate::xmp::find_packet(&at(MetadataPolicy::CopyrightOnly)).unwrap();
            assert!(
                credit.marks().is_none(),
                "{format:?}: marks under copyright only"
            );
            assert!(credit.properties.contains_key("dc:rights"), "{format:?}");
            assert!(
                crate::xmp::find_packet(&at(MetadataPolicy::None)).is_none(),
                "{format:?}: XMP under no metadata"
            );
        }
    }

    #[test]
    fn every_format_carries_the_exif_and_the_colour_profile() {
        let img = noise(40, 30);
        for (i, format) in DECODABLE.into_iter().enumerate() {
            // Each format in a different space, so every profile goes out in some file.
            let space = crate::gamut::OutputSpace::ALL[i % 3];
            let settings = ExportSettings {
                strip_location: false,
                color_space: space,
                ..ExportSettings::new(format)
            };
            let bytes = encoded(&img, &settings);
            let exif = exif::Reader::new()
                .read_from_container(&mut std::io::Cursor::new(&bytes))
                .unwrap_or_else(|e| panic!("{format:?}: no EXIF ({e})"));
            let field = |tag| {
                exif.get_field(tag, exif::In::PRIMARY)
                    .map(|f| f.display_value().to_string())
            };
            assert_eq!(
                field(exif::Tag::Model).as_deref(),
                Some("\"ILCE-7M4\""),
                "{format:?}"
            );
            assert_eq!(
                field(exif::Tag::ExposureTime).as_deref(),
                Some("1/250"),
                "{format:?}"
            );
            assert!(field(exif::Tag::GPSLatitude).is_some(), "{format:?}");
            assert_eq!(
                exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                    .and_then(|f| f.value.get_uint(0)),
                Some(1),
                "{format:?}"
            );

            // `image` does not report a TIFF's profile (tag 34675), though it is there
            // for every colour-managed reader; read that one with `tiff` itself.
            let icc = if format == ExportFormat::Tiff {
                tiff::decoder::Decoder::new(std::io::Cursor::new(&bytes))
                    .unwrap()
                    .get_tag_u8_vec(tiff::tags::Tag::IccProfile)
                    .ok()
            } else {
                use image::ImageDecoder;
                image::ImageReader::new(std::io::Cursor::new(&bytes))
                    .with_guessed_format()
                    .unwrap()
                    .into_decoder()
                    .unwrap()
                    .icc_profile()
                    .unwrap()
            };
            assert_eq!(icc.as_deref(), Some(space.icc()), "{format:?} in {space:?}");
            // And the picture still decodes as it was.
            let back = image::load_from_memory(&bytes).unwrap();
            assert_eq!((back.width(), back.height()), (40, 30), "{format:?}");
        }
    }

    #[test]
    fn location_is_left_out_unless_asked_for() {
        let img = noise(16, 16);
        let bytes = encoded(&img, &ExportSettings::new(ExportFormat::Jpeg));
        let exif = exif::Reader::new()
            .read_from_container(&mut std::io::Cursor::new(&bytes))
            .unwrap();
        assert!(exif
            .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
            .is_none());
        assert!(exif
            .get_field(exif::Tag::Model, exif::In::PRIMARY)
            .is_some());
    }

    #[test]
    fn sixteen_bit_png_and_tiff_keep_more_than_256_levels() {
        // A ramp through all 65536 values, 256 wide.
        let (w, h) = (256u32, 256u32);
        let rgba: Vec<u16> = (0..w * h)
            .flat_map(|i| {
                let v = i as u16;
                [v, v, v, 65535]
            })
            .collect();
        let img = ExportedImage {
            width: w,
            height: h,
            rgba,
        };
        for format in [ExportFormat::Png, ExportFormat::Tiff] {
            let settings = ExportSettings {
                depth: BitDepth::Sixteen,
                ..ExportSettings::new(format)
            };
            let mut out = std::io::Cursor::new(Vec::new());
            img.encode(&settings, None, &mut out).unwrap();
            let back = image::load_from_memory(out.get_ref())
                .unwrap()
                .into_rgba16();
            let levels: std::collections::BTreeSet<u16> = back.pixels().map(|p| p.0[0]).collect();
            assert_eq!(levels.len(), 65536, "{format:?}");
            assert_eq!(back.into_raw(), img.rgba, "{format:?} is lossless");
        }
        // Asked of JPEG, it quietly writes 8 bits.
        let settings = ExportSettings {
            depth: BitDepth::Sixteen,
            ..ExportSettings::new(ExportFormat::Jpeg)
        };
        assert_eq!(settings.effective_depth(), BitDepth::Eight);
        let mut out = std::io::Cursor::new(Vec::new());
        img.encode(&settings, None, &mut out).unwrap();
        assert!(image::load_from_memory(out.get_ref()).is_ok());
    }

    #[test]
    fn a_resize_never_enlarges_and_keeps_the_aspect() {
        let (w, h) = (6000, 4000);
        assert_eq!(Resize::Original.target(w, h), (6000, 4000));
        assert_eq!(Resize::LongEdge(2048).target(w, h), (2048, 1365));
        assert_eq!(Resize::LongEdge(2048).target(h, w), (1365, 2048));
        assert_eq!(Resize::ShortEdge(1000).target(w, h), (1500, 1000));
        assert_eq!(Resize::Percentage(50.0).target(w, h), (3000, 2000));
        let (mw, mh) = Resize::Megapixels(6.0).target(w, h);
        assert!(((mw * mh) as f32 / 1e6 - 6.0).abs() < 0.01, "{mw}x{mh}");
        assert_eq!(
            Resize::FitDimensions {
                width: 1000,
                height: 1000
            }
            .target(w, h),
            (1000, 667)
        );
        // Asked for more than the photo has, it gets the photo.
        assert_eq!(Resize::LongEdge(10_000).target(w, h), (6000, 4000));
        assert_eq!(Resize::Percentage(200.0).target(w, h), (6000, 4000));
    }

    #[test]
    fn output_sharpening_lifts_edges_and_leaves_flat_areas_alone() {
        // Left half 64, right half 192.
        let (w, h) = (16u32, 8u32);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let v = if i % w < w / 2 { 64 } else { 192 };
                [v, v, v, 255]
            })
            .collect();
        let flat_before = rgba[0];
        let img = ExportedImage {
            width: w,
            height: h,
            rgba,
        }
        .sharpened(0.6);
        let at = |x: u32, y: u32| img.rgba[((y * w + x) * 4) as usize];
        assert_eq!(at(0, 4), flat_before, "flat stays flat");
        assert!(at(w / 2 - 1, 4) < 64, "the dark side of the edge darkens");
        assert!(at(w / 2, 4) > 192, "the light side brightens");
        assert!(
            img.rgba.chunks_exact(4).all(|p| p[3] == 255),
            "alpha untouched"
        );
    }

    #[test]
    fn settings_saved_before_the_new_options_still_load() {
        let old: ExportSettings =
            serde_json::from_str(r#"{"format":"Jpeg","quality":85}"#).unwrap();
        assert_eq!(old.sizing, Resize::Original);
        assert_eq!(old.depth, BitDepth::Eight);
        assert!(old.strip_location);
        assert_eq!(old.metadata, MetadataPolicy::All);
    }

    #[test]
    fn a_resize_averages_light_not_codes() {
        // A black and white checkerboard, halved: each output pixel is half white light,
        // which sRGB encodes as about 188, not the 128 the codes average to.
        let rgba = (0..64 * 64u32)
            .flat_map(|i| {
                let v = if (i % 64 + i / 64) % 2 == 0 { 255 } else { 0 };
                [v, v, v, 255]
            })
            .collect();
        let img = ExportedImage::<u8> {
            width: 64,
            height: 64,
            rgba,
        };
        let out = img.resized(16, 16).unwrap();
        let centre = out.rgba[(8 * 16 + 8) * 4];
        assert!(
            centre.abs_diff(188) <= 3,
            "half white light came out as {centre}"
        );
        assert_eq!(out.rgba[(8 * 16 + 8) * 4 + 3], 255, "alpha moved");
    }

    #[test]
    fn a_resize_keeps_the_aspect_the_caller_asked_for() {
        let img = noise(64, 48).resized(32, 24).unwrap();
        assert_eq!((img.width, img.height), (32, 24));
        assert_eq!(img.rgba.len(), 32 * 24 * 4);
    }

    #[test]
    fn gain_map_computes_headroom_and_formats_iso_21496_1_xmp() {
        // SDR image: 2 pixels. First is mid-grey, second is blown SDR white (1.0).
        let sdr_rgba = vec![
            128, 128, 128, 255, // mid-grey
            255, 255, 255, 255, // 1.0 SDR white
        ];
        // HDR image: First pixel is mid-grey (same), second pixel is HDR specular (4.0 = +2 stops headroom).
        let hdr_rgb = vec![0.5, 0.5, 0.5, 4.0, 4.0, 4.0];
        let gm = GainMap::compute(&sdr_rgba, &hdr_rgb, 2, 1, 2.0);
        assert_eq!(gm.width, 2);
        assert_eq!(gm.height, 1);
        assert_eq!(gm.data.len(), 2);

        // Pixel 0 has ~0 gain (same brightness)
        assert!(
            gm.data[0] < 10,
            "pixel 0 gain should be near 0, got {}",
            gm.data[0]
        );
        // Pixel 1 has max gain (4x = 2 stops headroom on a 2-stop max map)
        assert!(
            gm.data[1] > 240,
            "pixel 1 gain should be near 255, got {}",
            gm.data[1]
        );

        // Verify ISO 21496-1 XMP metadata
        let xmp = gm.iso_21496_1_xmp();
        assert!(xmp.contains("<hdrgm:Version>1.0</hdrgm:Version>"));
        assert!(xmp.contains("<hdrgm:GainMapMax>2.000000</hdrgm:GainMapMax>"));
        assert!(xmp.contains("<hdrgm:BaseRenditionIsSDR>True</hdrgm:BaseRenditionIsSDR>"));
    }
}
