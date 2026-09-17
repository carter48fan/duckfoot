//! Reading the finished image back off the GPU and writing it to disk.
//!
//! DESIGN.md invariant 2 says one render path serves preview and export. Here that is not
//! a convention being maintained by discipline — export copies *the same texture the
//! screen is showing*, produced by the same shader from the same uniforms. There is no
//! second code path that could drift, because there is no second code path.
//!
//! This is also the only sanctioned GPU→CPU readback outside the scopes (invariant 4).

use std::path::Path;

use anyhow::{bail, Context, Result};

/// A finished image, tightly packed RGBA8, ready to encode.
pub struct ExportedImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, no row padding.
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
}

impl ExportFormat {
    /// Picks the encoder from the filename the user chose, defaulting to PNG.
    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("jpg" | "jpeg") => Self::Jpeg,
            _ => Self::Png,
        }
    }
}

impl ExportedImage {
    pub fn megapixels(&self) -> f32 {
        (self.width as f32 * self.height as f32) / 1_000_000.0
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let buffer = image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "buffer is {} bytes, not the {} that {}x{} needs",
                    self.rgba.len(),
                    self.width as usize * self.height as usize * 4,
                    self.width,
                    self.height
                )
            })?;

        match ExportFormat::from_path(path) {
            ExportFormat::Png => buffer
                .save_with_format(path, image::ImageFormat::Png)
                .with_context(|| format!("writing {}", path.display()))?,
            ExportFormat::Jpeg => {
                // JPEG has no alpha channel; dropping it is not a loss because the
                // pipeline writes an opaque alpha of 255 for every pixel.
                image::DynamicImage::ImageRgba8(buffer)
                    .to_rgb8()
                    .save_with_format(path, image::ImageFormat::Jpeg)
                    .with_context(|| format!("writing {}", path.display()))?
            }
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
        assert_eq!(
            ExportFormat::from_path(Path::new("a.png")),
            ExportFormat::Png
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("a.JPG")),
            ExportFormat::Jpeg
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("a.jpeg")),
            ExportFormat::Jpeg
        );
        // Anything unrecognised writes a PNG rather than guessing or failing.
        assert_eq!(ExportFormat::from_path(Path::new("a")), ExportFormat::Png);
        assert_eq!(
            ExportFormat::from_path(Path::new("a.tif")),
            ExportFormat::Png
        );
    }
}
