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

/// JPEG quality for export. `image`'s own default is 75, which bands skies and smears fine
/// texture, and a photo editor's export is the last place to throw quality away.
pub const JPEG_QUALITY: u8 = 92;

/// A finished image, tightly packed RGBA8, ready to encode.
pub struct ExportedImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, no row padding.
    pub rgba: Vec<u8>,
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

/// How to encode an export. One set of options for the single export and every batch
/// recipe, so the two cannot drift apart again (they did: one wrote JPEG at 75, the
/// other at the recipe's quality, and only one could write TIFF).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodeOptions {
    pub format: ExportFormat,
    /// 1..=100, for the lossy formats.
    pub quality: u8,
}

impl EncodeOptions {
    pub fn new(format: ExportFormat) -> Self {
        Self {
            format,
            quality: JPEG_QUALITY,
        }
    }

    /// The options for a file named `path`, at the default quality.
    pub fn for_path(path: &Path) -> Result<Self> {
        Ok(Self::new(ExportFormat::from_path(path)?))
    }
}

impl ExportedImage {
    pub fn megapixels(&self) -> f32 {
        (self.width as f32 * self.height as f32) / 1_000_000.0
    }

    /// This image scaled to `width × height` (Lanczos), or unchanged if it already is.
    pub fn resized(self, width: u32, height: u32) -> Result<Self> {
        if (width, height) == (self.width, self.height) {
            return Ok(self);
        }
        self.check_size()?;
        let (w, h) = (self.width, self.height);
        let img = image::RgbaImage::from_raw(w, h, self.rgba)
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

    /// Encode to `path`.
    ///
    /// Encodes straight from `rgba`: at 100 MP a copy of the buffer is 400 MB, which is
    /// the difference between an export and a phone killing the app.
    pub fn write(&self, path: &Path, options: &EncodeOptions) -> Result<()> {
        self.check_size()?;
        // Beside the target and renamed over it once complete: a full disk or a crash
        // mid-encode leaves any earlier export of the same name intact, never half of one.
        crate::fsio::atomic_write(path, |file| {
            let mut out = BufWriter::new(file);
            self.encode(options, &mut out)?;
            out.flush()?;
            Ok(())
        })
        .with_context(|| format!("writing {}", path.display()))
    }

    /// Encode into any seekable writer (TIFF needs to seek back to patch its offsets).
    pub fn encode<W: Write + Seek>(&self, options: &EncodeOptions, mut out: W) -> Result<()> {
        self.check_size()?;
        let (w, h) = (self.width, self.height);
        let quality = options.quality.clamp(1, 100);
        match options.format {
            ExportFormat::Png => image::codecs::png::PngEncoder::new(out).write_image(
                &self.rgba,
                w,
                h,
                image::ExtendedColorType::Rgba8,
            )?,
            ExportFormat::Tiff => image::codecs::tiff::TiffEncoder::new(out).write_image(
                &self.rgba,
                w,
                h,
                image::ExtendedColorType::Rgba8,
            )?,
            ExportFormat::Jpeg => {
                // JPEG has no alpha. The encoder reads each pixel as RGB, so the alpha —
                // always an opaque 255 here — is dropped without an RGB copy of the frame.
                let view = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(
                    w,
                    h,
                    self.rgba.as_slice(),
                )
                .ok_or_else(|| anyhow!("{w}x{h} does not fit the buffer"))?;
                image::codecs::jpeg::JpegEncoder::new_with_quality(out, quality)
                    .encode_image(&view)?
            }
            ExportFormat::Webp => {
                // `image` writes only lossless WebP; libwebp does both.
                let encoder = webp::Encoder::from_rgba(&self.rgba, w, h);
                let bytes = if quality >= 100 {
                    encoder.encode_lossless()
                } else {
                    encoder.encode(f32::from(quality))
                };
                out.write_all(&bytes)?;
            }
        }
        Ok(())
    }

    fn check_size(&self) -> Result<()> {
        let needed = self.width as usize * self.height as usize * 4;
        if self.rgba.len() != needed {
            bail!(
                "buffer is {} bytes, not the {needed} that {}x{} needs",
                self.rgba.len(),
                self.width,
                self.height
            );
        }
        Ok(())
    }
}

/// wgpu requires each row of a texture→buffer copy to start on a 256-byte boundary, so a
/// readback buffer is wider than the image whenever `width * 4` is not already a multiple
/// of 256. 6048 px is 24192 bytes and pads to 24320 — 128 bytes of slack on every one of
/// 4024 rows. Handing that buffer straight to an encoder produces a picture with a
/// diagonal shear, which is the classic symptom of forgetting this.
pub fn padded_bytes_per_row(width: u32) -> u32 {
    let unpadded = width * 4;
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
        img.encode(&EncodeOptions::new(ExportFormat::Jpeg), &mut ours)
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
        assert_eq!(
            ours.get_ref(),
            &at(JPEG_QUALITY),
            "not encoded at q{JPEG_QUALITY}"
        );
        // Noise is incompressible, so quality shows directly as size.
        assert!(ours.get_ref().len() > at(75).len() * 5 / 4);
    }

    #[test]
    fn tif_writes_a_real_lossless_tiff() {
        let img = noise(37, 11);
        let path = scratch_path("real.tif");
        img.write(&path, &EncodeOptions::for_path(&path).unwrap())
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
        let err = EncodeOptions::for_path(&path).unwrap_err();
        assert!(err.to_string().contains("cannot export .bmp"), "{err}");
        assert!(!path.exists());
    }

    #[test]
    fn a_short_buffer_is_an_error_not_a_panic() {
        let mut img = noise(8, 8);
        img.rgba.truncate(10);
        let mut out = std::io::Cursor::new(Vec::new());
        for format in ExportFormat::ALL {
            assert!(img.encode(&EncodeOptions::new(format), &mut out).is_err());
        }
    }

    #[test]
    fn webp_is_lossy_below_100_and_lossless_at_it() {
        let img = noise(64, 48);
        let encode = |quality| {
            let mut out = std::io::Cursor::new(Vec::new());
            img.encode(
                &EncodeOptions {
                    format: ExportFormat::Webp,
                    quality,
                },
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

    #[test]
    fn a_resize_keeps_the_aspect_the_caller_asked_for() {
        let img = noise(64, 48).resized(32, 24).unwrap();
        assert_eq!((img.width, img.height), (32, 24));
        assert_eq!(img.rgba.len(), 32 * 24 * 4);
    }
}
