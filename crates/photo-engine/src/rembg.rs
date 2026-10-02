//! Cross-platform neural network background removal (rembg) using tract-onnx.
//!
//! 100% pure Rust neural network inference with zero external C++ dependencies or Python.
//! Runs identically across Linux, Windows, macOS, and Android (aarch64).

use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};
use image::{imageops::FilterType, DynamicImage, GenericImageView, GrayImage, Luma, Rgb, RgbImage, RgbaImage};
use tract_onnx::prelude::*;

/// Predefined model types supported out of the box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RembgModel {
    /// Full U2-Net (168 MB) - maximum edge accuracy and detail.
    U2Net,
    /// Portable U2-Netp (4.4 MB) - ultra fast, mobile and embedded friendly.
    U2Netp,
    /// Custom ONNX model path.
    Custom(PathBuf),
}

impl RembgModel {
    pub fn filename(&self) -> &str {
        match self {
            Self::U2Net => "u2net.onnx",
            Self::U2Netp => "u2netp.onnx",
            Self::Custom(p) => p.file_name().and_then(|n| n.to_str()).unwrap_or("custom.onnx"),
        }
    }
}

/// Locate a model file across standard locations, cache directories, and environment variables.
pub fn find_model(model: &RembgModel) -> Option<PathBuf> {
    if let RembgModel::Custom(path) = model {
        if path.exists() {
            return Some(path.clone());
        }
    }

    let fname = model.filename();

    // 1. Explicit environment variables
    if let Ok(dir) = std::env::var("U2NET_HOME") {
        let p = PathBuf::from(dir).join(fname);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(dir) = std::env::var("REMBG_HOME") {
        let p = PathBuf::from(&dir).join(fname);
        if p.exists() {
            return Some(p);
        }
        let p = PathBuf::from(&dir).join("models").join(p.file_stem().unwrap_or_default()).join(fname);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. User home ~/.u2net/
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(&home).join(".u2net").join(fname);
        if p.exists() {
            return Some(p);
        }
        let p = PathBuf::from(&home).join(".rembg").join("models").join(fname);
        if p.exists() {
            return Some(p);
        }
    }

    // 3. User profile on Windows
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let p = PathBuf::from(&userprofile).join(".u2net").join(fname);
        if p.exists() {
            return Some(p);
        }
    }

    // 4. Current working directory / models / data
    let local = Path::new("models").join(fname);
    if local.exists() {
        return Some(local);
    }
    let data = Path::new("crates/photo-engine/data").join(fname);
    if data.exists() {
        return Some(data);
    }

    None
}

use std::sync::Arc;

/// Active background removal session loaded in pure Rust.
pub struct RembgSession {
    model: Arc<tract_onnx::tract_core::plan::SimplePlan<TypedFact, Box<dyn TypedOp>>>,
}

impl RembgSession {
    /// Load an ONNX model from a file path.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let model = tract_onnx::onnx()
            .model_for_path(path)
            .with_context(|| format!("Failed to load ONNX model from {:?}", path))?
            .with_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, 320, 320)))
            .with_context(|| "Failed to configure model input shape [1, 3, 320, 320]")?
            .into_optimized()
            .with_context(|| "Failed to optimize ONNX graph")?
            .into_runnable()
            .with_context(|| "Failed to convert ONNX graph to runnable plan")?;

        Ok(Self { model })
    }

    /// Load an ONNX model from an in-memory byte buffer or reader.
    pub fn from_reader<R: std::io::Read>(mut reader: R) -> Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_read(&mut reader)
            .with_context(|| "Failed to load ONNX model from reader")?
            .with_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, 320, 320)))
            .with_context(|| "Failed to configure model input shape [1, 3, 320, 320]")?
            .into_optimized()
            .with_context(|| "Failed to optimize ONNX graph")?
            .into_runnable()
            .with_context(|| "Failed to convert ONNX graph to runnable plan")?;

        Ok(Self { model })
    }

    /// Embedded U2Netp weights (4.4 MB) bundled directly in the binary for zero-setup offline usage.
    pub const BUNDLED_U2NETP: &'static [u8] = include_bytes!("../data/u2netp.onnx");

    /// Automatically find and load a supported model, falling back to embedded U2Netp if no file exists.
    pub fn auto() -> Result<Self> {
        // 1. Prefer local full U2Net if present on disk
        if let Some(path) = find_model(&RembgModel::U2Net) {
            log::info!("Loading rembg full U2Net model from {:?}", path);
            if let Ok(sess) = Self::from_file(&path) {
                return Ok(sess);
            }
        }

        // 2. Check if u2netp is on disk
        if let Some(path) = find_model(&RembgModel::U2Netp) {
            log::info!("Loading rembg U2Netp model from {:?}", path);
            if let Ok(sess) = Self::from_file(&path) {
                return Ok(sess);
            }
        }

        // 3. Bundled zero-setup fallback
        log::info!("Loading rembg embedded U2Netp model");
        Self::bundled()
    }

    /// Load the bundled U2Netp model directly from binary memory.
    pub fn bundled() -> Result<Self> {
        Self::from_reader(std::io::Cursor::new(Self::BUNDLED_U2NETP))
    }

    /// Automatically find and load a specific model variant.
    pub fn load(model_type: RembgModel) -> Result<Self> {
        if let Some(path) = find_model(&model_type) {
            log::info!("Loading rembg ONNX model from {:?}", path);
            Self::from_file(path)
        } else if model_type == RembgModel::U2Netp {
            Self::bundled()
        } else {
            bail!("Could not find model {:?}. Ensure it is present in ~/.u2net/ or specified path.", model_type.filename());
        }
    }

    /// Compute an alpha mask (0 = background, 255 = foreground) at original image dimensions.
    pub fn compute_mask(&self, img: &DynamicImage) -> Result<GrayImage> {
        let (orig_w, orig_h) = img.dimensions();
        if orig_w == 0 || orig_h == 0 {
            bail!("Cannot compute mask for empty image");
        }

        // 1. Resize to 320x320 for U2Net inference
        let resized = img.resize_exact(320, 320, FilterType::Triangle);
        let rgb = resized.to_rgb8();

        // 2. Normalize with ImageNet mean and std:
        // mean: [0.485, 0.456, 0.406], std: [0.229, 0.224, 0.225]
        let mean = [0.485f32, 0.456f32, 0.406f32];
        let std = [0.229f32, 0.224f32, 0.225f32];

        // Tensor shape: [1, 3, 320, 320]
        let mut input_data = vec![0.0f32; 1 * 3 * 320 * 320];
        for y in 0..320 {
            for x in 0..320 {
                let pixel = rgb.get_pixel(x as u32, y as u32);
                let r = (pixel[0] as f32 / 255.0 - mean[0]) / std[0];
                let g = (pixel[1] as f32 / 255.0 - mean[1]) / std[1];
                let b = (pixel[2] as f32 / 255.0 - mean[2]) / std[2];

                let idx_base = y * 320 + x;
                input_data[0 * 320 * 320 + idx_base] = r;
                input_data[1 * 320 * 320 + idx_base] = g;
                input_data[2 * 320 * 320 + idx_base] = b;
            }
        }

        let tensor = tract_ndarray::Array4::from_shape_vec((1, 3, 320, 320), input_data)
            .with_context(|| "Failed to shape ndarray for tensor")?
            .into_tensor();

        // 3. Run model inference
        let outputs = self.model.run(tvec!(tensor.into()))
            .with_context(|| "Failed to run neural inference")?;

        let output_tensor = &outputs[0];
        let view = output_tensor.to_plain_array_view::<f32>()
            .with_context(|| "Failed to view output tensor as f32 array")?;
        let slice = view.as_slice().unwrap_or(&[]);

        // 4. Extract 320x320 mask and min-max normalize
        let mut min_val = f32::MAX;
        let mut max_val = f32::MIN;

        for &val in slice.iter() {
            if val < min_val { min_val = val; }
            if val > max_val { max_val = val; }
        }

        let range = if (max_val - min_val).abs() > 1e-6 {
            max_val - min_val
        } else {
            1.0
        };

        let mut mask_320 = GrayImage::new(320, 320);
        if slice.len() >= 320 * 320 {
            // First 320x320 elements correspond to predicted mask d0
            for y in 0..320 {
                for x in 0..320 {
                    let val = slice[y * 320 + x];
                    let norm = ((val - min_val) / range).clamp(0.0, 1.0);
                    let byte = (norm * 255.0).round() as u8;
                    mask_320.put_pixel(x as u32, y as u32, Luma([byte]));
                }
            }
        }

        // 5. Upscale back to original image size
        let mask_dyn = DynamicImage::ImageLuma8(mask_320);
        let mask_orig = mask_dyn.resize_exact(orig_w, orig_h, FilterType::CatmullRom).to_luma8();

        Ok(mask_orig)
    }

    /// Remove background, returning an RGBA image with transparent background.
    pub fn remove_background(&self, img: &DynamicImage) -> Result<RgbaImage> {
        let mask = self.compute_mask(img)?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();

        let mut out = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut p = *rgba.get_pixel(x, y);
                let alpha = mask.get_pixel(x, y)[0];
                p[3] = alpha;
                out.put_pixel(x, y, p);
            }
        }
        Ok(out)
    }

    /// Remove background, replacing it with a solid color matte (e.g. [255, 255, 255] for catalog white).
    pub fn remove_background_with_matte(&self, img: &DynamicImage, matte: [u8; 3]) -> Result<RgbImage> {
        let mask = self.compute_mask(img)?;
        let rgb = img.to_rgb8();
        let (w, h) = rgb.dimensions();

        let mut out = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let p = rgb.get_pixel(x, y);
                let a = mask.get_pixel(x, y)[0] as f32 / 255.0;
                let inv_a = 1.0 - a;

                let r = (p[0] as f32 * a + matte[0] as f32 * inv_a).round() as u8;
                let g = (p[1] as f32 * a + matte[1] as f32 * inv_a).round() as u8;
                let b = (p[2] as f32 * a + matte[2] as f32 * inv_a).round() as u8;

                out.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_model_in_local_environment() {
        let model = RembgModel::U2Netp;
        let found = find_model(&model);
        assert!(found.is_some(), "Should find u2netp.onnx in ~/.u2net");
        println!("Found u2netp at {:?}", found.unwrap());
    }

    #[test]
    fn test_u2netp_inference_pure_rust() {
        let model_path = find_model(&RembgModel::U2Netp);
        if let Some(path) = model_path {
            let session = RembgSession::from_file(&path).expect("Model should load cleanly in tract");
            
            // Create a 100x100 synthetic test image with a white circle in center on black background
            let mut img = RgbImage::new(100, 100);
            for y in 0..100 {
                for x in 0..100 {
                    let dx = x as i32 - 50;
                    let dy = y as i32 - 50;
                    if dx * dx + dy * dy < 25 * 25 {
                        img.put_pixel(x, y, Rgb([240, 240, 240]));
                    } else {
                        img.put_pixel(x, y, Rgb([10, 10, 10]));
                    }
                }
            }

            let dynamic = DynamicImage::ImageRgb8(img);
            let mask = session.compute_mask(&dynamic).expect("Inference should succeed");
            assert_eq!(mask.width(), 100);
            assert_eq!(mask.height(), 100);

            // Center pixel should have high foreground probability
            let center_val = mask.get_pixel(50, 50)[0];
            // Corner pixel should have low background probability
            let corner_val = mask.get_pixel(5, 5)[0];

            println!("Center alpha: {}, Corner alpha: {}", center_val, corner_val);
            assert!(center_val > corner_val);
        }
    }

    #[test]
    fn test_bundled_u2netp_inference() {
        let session = RembgSession::bundled().expect("Bundled model should load");
        let mut img = RgbImage::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                if (x as i32 - 32).pow(2) + (y as i32 - 32).pow(2) < 16 * 16 {
                    img.put_pixel(x, y, Rgb([255, 255, 255]));
                } else {
                    img.put_pixel(x, y, Rgb([0, 0, 0]));
                }
            }
        }
        let dynamic = DynamicImage::ImageRgb8(img);
        let mask = session.compute_mask(&dynamic).expect("Bundled inference should succeed");
        assert_eq!(mask.width(), 64);
        assert_eq!(mask.height(), 64);
        assert!(mask.get_pixel(32, 32)[0] > mask.get_pixel(0, 0)[0]);
    }
}
