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
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 4] = [Self::Jpeg, Self::Png, Self::Tiff, Self::Webp];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Tiff => "tif",
            Self::Webp => "webp",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Tiff => "TIFF",
            Self::Webp => "WebP",
        }
    }

    /// Whether the quality setting means anything for this format.
    pub fn is_lossy(self) -> bool {
        matches!(self, Self::Jpeg | Self::Webp)
    }

    /// Whether it can hold 16 bits a channel.
    pub fn has_16_bit(self) -> bool {
        matches!(self, Self::Png | Self::Tiff)
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
            Some(other) => bail!(
                "{}: cannot export .{other} — choose .jpg, .png, .tif or .webp",
                path.display()
            ),
            None => bail!(
                "{}: no file extension — choose .jpg, .png, .tif or .webp",
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

    /// This image scaled to `width × height` (Lanczos), or unchanged if it already is.
    pub fn resized(self, width: u32, height: u32) -> Result<Self>
    where
        image::Rgba<P>: image::Pixel<Subpixel = P>,
    {
        if (width, height) == (self.width, self.height) {
            return Ok(self);
        }
        self.check_size()?;
        let (w, h) = (self.width, self.height);
        let img = image::ImageBuffer::<image::Rgba<P>, Vec<P>>::from_raw(w, h, self.rgba)
            .ok_or_else(|| anyhow!("{w}x{h} does not fit the buffer"))?;
        let out = image::imageops::resize(
            &img,
            width.max(1),
            height.max(1),
            image::imageops::FilterType::Lanczos3,
        );
        Ok(Self {
            width: out.width(),
            height: out.height(),
            rgba: out.into_raw(),
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

    /// Resized and output-sharpened as `settings` say.
    pub fn finished(self, settings: &ExportSettings) -> Result<Self> {
        let (w, h) = settings.sizing.target(self.width(), self.height());
        let strength = settings.sharpen.strength(settings.sharpen_amount);
        Ok(match self {
            Exported::Eight(i) => Exported::Eight(i.resized(w, h)?.sharpened(strength)),
            Exported::Sixteen(i) => Exported::Sixteen(i.resized(w, h)?.sharpened(strength)),
        })
    }

    /// Finish and write: what the single export and every batch recipe do.
    pub fn save(
        self,
        path: &Path,
        settings: &ExportSettings,
        info: Option<&PhotoInfo>,
    ) -> Result<()> {
        match self.finished(settings)? {
            Exported::Eight(i) => i.write(path, settings, info),
            Exported::Sixteen(i) => i.write(path, settings, info),
        }
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
    let icc = settings.color_space.icc().to_vec();
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
            let mut enc = image::codecs::png::PngEncoder::new(out);
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
        }
        ExportFormat::Tiff => write_tiff(out, w, h, samples, &fields, &icc)?,
        ExportFormat::Jpeg => {
            let px = eight.expect("JPEG is always 8-bit");
            // JPEG has no alpha. The encoder reads each pixel as RGB, so the alpha —
            // always an opaque 255 here — is dropped without an RGB copy of the frame.
            let view = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(w, h, px)
                .ok_or_else(|| anyhow!("{w}x{h} does not fit the buffer"))?;
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(out, quality);
            enc.set_icc_profile(icc)
                .map_err(|e| anyhow!("JPEG colour profile: {e}"))?;
            if let Some(exif) = exif {
                enc.set_exif_metadata(exif)
                    .map_err(|e| anyhow!("JPEG EXIF: {e}"))?;
            }
            enc.encode_image(&view)?
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
            out.write_all(&webp_with_metadata(&bytes, &icc, exif.as_deref())?)?;
        }
    }
    Ok(())
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
fn webp_with_metadata(file: &[u8], icc: &[u8], exif: Option<&[u8]>) -> Result<Vec<u8>> {
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
    vp8x[0] = 0x20 | if alpha { 0x10 } else { 0 } | if exif.is_some() { 0x08 } else { 0 };
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
        if !matches!(id, b"VP8X" | b"ICCP" | b"EXIF") {
            chunk(id, data);
        }
    }
    if let Some(exif) = exif {
        chunk(b"EXIF", exif);
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
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
    fn every_format_carries_the_exif_and_the_colour_profile() {
        let img = noise(40, 30);
        for (i, format) in ExportFormat::ALL.into_iter().enumerate() {
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
    fn a_resize_keeps_the_aspect_the_caller_asked_for() {
        let img = noise(64, 48).resized(32, 24).unwrap();
        assert_eq!((img.width, img.height), (32, 24));
        assert_eq!(img.rgba.len(), 32 * 24 * 4);
    }
}
