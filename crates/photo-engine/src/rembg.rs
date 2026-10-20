//! Cross-platform neural network background removal (rembg) using tract-onnx.
//!
//! 100% pure Rust neural network inference with zero external C++ dependencies or Python.
//! Runs identically across Linux, Windows, macOS, and Android (aarch64).

use anyhow::{bail, Context, Result};
use image::{
    imageops::FilterType, DynamicImage, GenericImageView, GrayImage, Luma, Rgb, RgbImage, RgbaImage,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
            Self::Custom(p) => p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("custom.onnx"),
        }
    }
}

/// Comprehensive parameters for fine-tuning background cutout.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CutoutParams {
    /// Foreground threshold cutoff (0.05..=0.95, default 0.50).
    pub threshold: f32,
    /// Edge feather in relative radius (0.0..=20.0 px, default 1.0).
    pub feather: f32,
    /// Edge shift (-20..=20 px, default 0: negative erodes/contracts, positive dilates/expands).
    pub edge_shift: i32,
    /// Whether to decontaminate/defringe white studio background from edge pixels (default true).
    pub defringe_white: bool,
}

impl Default for CutoutParams {
    fn default() -> Self {
        Self {
            threshold: 0.50,
            feather: 1.0,
            edge_shift: 0,
            defringe_white: true,
        }
    }
}

/// One manual touch-up dab drawn by the user on the stage.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TouchupDab {
    /// Normalized coordinates (0.0..=1.0) on the original image.
    pub at: [f32; 2],
    /// Radius in normalized coordinates relative to max dimension.
    pub radius: f32,
    /// Hardness (0.0 = all feather, 1.0 = hard edge).
    pub hardness: f32,
    /// If true, restore foreground (alpha = 255); if false, erase background (alpha = 0).
    pub keep: bool,
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
        let p = PathBuf::from(&dir)
            .join("models")
            .join(p.file_stem().unwrap_or_default())
            .join(fname);
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
        let p = PathBuf::from(&home)
            .join(".rembg")
            .join("models")
            .join(fname);
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
#[derive(Clone)]
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
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, 320, 320)),
            )
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
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, 320, 320)),
            )
            .with_context(|| "Failed to configure model input shape [1, 3, 320, 320]")?
            .into_optimized()
            .with_context(|| "Failed to optimize ONNX graph")?
            .into_runnable()
            .with_context(|| "Failed to convert ONNX graph to runnable plan")?;

        Ok(Self { model })
    }

    /// Embedded U2Netp weights (4.4 MB) bundled directly in the binary for zero-setup offline usage.
    pub const BUNDLED_U2NETP: &'static [u8] = include_bytes!("../data/u2netp.onnx");

    /// Automatically find and load a supported model, falling back to embedded U2Netp if no
    /// file exists. Loaded once a run and shared after that (a clone is an `Arc`): parsing
    /// and optimising the graph is most of a cutout's time, and a batch export with
    /// cutouts asked for it once a photo. A model dropped in place later is used from the
    /// next run.
    pub fn auto() -> Result<Self> {
        static LOADED: std::sync::OnceLock<std::result::Result<RembgSession, String>> =
            std::sync::OnceLock::new();
        LOADED
            .get_or_init(|| Self::load_auto().map_err(|e| format!("{e:#}")))
            .clone()
            .map_err(|e| anyhow::anyhow!(e))
    }

    fn load_auto() -> Result<Self> {
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
            bail!(
                "Could not find model {:?}. Ensure it is present in ~/.u2net/ or specified path.",
                model_type.filename()
            );
        }
    }

    /// Run neural inference on 320x320 and return the raw predicted foreground probabilities (0.0..=1.0).
    pub fn raw_prediction_320(&self, img: &DynamicImage) -> Result<Vec<f32>> {
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
        const PLANE_SIZE: usize = 320 * 320;
        let mut input_data = vec![0.0f32; 3 * PLANE_SIZE];
        for y in 0..320 {
            for x in 0..320 {
                let pixel = rgb.get_pixel(x as u32, y as u32);
                let r = (pixel[0] as f32 / 255.0 - mean[0]) / std[0];
                let g = (pixel[1] as f32 / 255.0 - mean[1]) / std[1];
                let b = (pixel[2] as f32 / 255.0 - mean[2]) / std[2];

                let idx_base = y * 320 + x;
                input_data[idx_base] = r;
                input_data[PLANE_SIZE + idx_base] = g;
                input_data[2 * PLANE_SIZE + idx_base] = b;
            }
        }

        let tensor = tract_ndarray::Array4::from_shape_vec((1, 3, 320, 320), input_data)
            .with_context(|| "Failed to shape ndarray for tensor")?
            .into_tensor();

        // 3. Run model inference
        let outputs = self
            .model
            .run(tvec!(tensor.into()))
            .with_context(|| "Failed to run neural inference")?;

        let output_tensor = &outputs[0];
        let view = output_tensor
            .to_plain_array_view::<f32>()
            .with_context(|| "Failed to view output tensor as f32 array")?;
        let slice = view.as_slice().unwrap_or(&[]);

        // 4. Extract 320x320 mask and min-max normalize
        let mut min_val = f32::MAX;
        let mut max_val = f32::MIN;

        for &val in slice.iter() {
            if val < min_val {
                min_val = val;
            }
            if val > max_val {
                max_val = val;
            }
        }

        let range = if (max_val - min_val).abs() > 1e-6 {
            max_val - min_val
        } else {
            1.0
        };

        let mut pred_320 = vec![0.0f32; 320 * 320];
        if slice.len() >= 320 * 320 {
            for i in 0..320 * 320 {
                pred_320[i] = ((slice[i] - min_val) / range).clamp(0.0, 1.0);
            }
        }

        Ok(pred_320)
    }

    /// Upscale a 320x320 normalized probability map once to `(orig_w, orig_h)` so interactive
    /// slider drags and brush strokes do not re-run bicubic interpolation on every frame.
    pub fn upscale_prediction(pred_320: &[f32], orig_w: u32, orig_h: u32) -> GrayImage {
        let mut raw_320 = GrayImage::new(320, 320);
        for (dst, &v) in raw_320.as_mut().iter_mut().zip(pred_320.iter()) {
            *dst = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        if orig_w == 320 && orig_h == 320 {
            raw_320
        } else {
            DynamicImage::ImageLuma8(raw_320)
                .resize_exact(orig_w, orig_h, FilterType::CatmullRom)
                .to_luma8()
        }
    }

    /// Compute an alpha mask from a pre-upscaled probability image with fine-tuning parameters and touch-ups.
    pub fn compute_mask_from_upscaled(
        &self,
        pred_upscaled: &GrayImage,
        params: &CutoutParams,
        touchups: &[TouchupDab],
    ) -> Result<GrayImage> {
        let t = params.threshold.clamp(0.01, 0.99);
        // Tight anti-aliasing span in probability space; spatial edge softening in pixels
        // is performed after morphological shift and brush touch-ups via `spatial_feather`.
        let aa_half = 0.015;
        let t_low = (t - aa_half).max(0.0);
        let t_high = (t + aa_half).min(1.0);
        let span = (t_high - t_low).max(1e-5);

        let (orig_w, orig_h) = pred_upscaled.dimensions();
        let mut mask_orig = GrayImage::new(orig_w, orig_h);
        for (dst, &src_u8) in mask_orig.as_mut().iter_mut().zip(pred_upscaled.as_raw()) {
            let v = src_u8 as f32 / 255.0;
            let smooth = if v <= t_low {
                0.0
            } else if v >= t_high {
                1.0
            } else {
                let s = (v - t_low) / span;
                s * s * (3.0 - 2.0 * s)
            };
            *dst = (smooth * 255.0).round() as u8;
        }

        if params.edge_shift != 0 {
            morphological_shift(&mut mask_orig, params.edge_shift);
        }

        if !touchups.is_empty() {
            apply_touchups(&mut mask_orig, touchups);
        }

        if params.feather >= 0.5 {
            spatial_feather(&mut mask_orig, params.feather);
        }

        Ok(mask_orig)
    }

    /// Compute an alpha mask from a cached 320x320 prediction with fine-tuning parameters and touch-ups.
    pub fn compute_mask_from_prediction(
        &self,
        pred_320: &[f32],
        orig_w: u32,
        orig_h: u32,
        params: &CutoutParams,
        touchups: &[TouchupDab],
    ) -> Result<GrayImage> {
        let upscaled = Self::upscale_prediction(pred_320, orig_w, orig_h);
        self.compute_mask_from_upscaled(&upscaled, params, touchups)
    }

    /// Compute an alpha mask with full fine-tuning parameters and touch-up points.
    pub fn compute_mask_with_params(
        &self,
        img: &DynamicImage,
        params: &CutoutParams,
        touchups: &[TouchupDab],
    ) -> Result<GrayImage> {
        let pred = self.raw_prediction_320(img)?;
        let (w, h) = img.dimensions();
        self.compute_mask_from_prediction(&pred, w, h, params, touchups)
    }

    /// Compute default alpha mask (0 = background, 255 = foreground) at original image dimensions.
    pub fn compute_mask(&self, img: &DynamicImage) -> Result<GrayImage> {
        self.compute_mask_with_params(img, &CutoutParams::default(), &[])
    }

    /// Remove background with fine-tuning parameters and touch-ups, returning an RGBA image.
    pub fn remove_background_with_params(
        &self,
        img: &DynamicImage,
        params: &CutoutParams,
        touchups: &[TouchupDab],
    ) -> Result<RgbaImage> {
        let mask = self.compute_mask_with_params(img, params, touchups)?;
        let mut rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();

        for y in 0..h {
            for x in 0..w {
                let p = rgba.get_pixel_mut(x, y);
                p[3] = mask.get_pixel(x, y)[0];
            }
        }

        if params.defringe_white {
            defringe_rgba(rgba.as_mut(), w, h, [255, 255, 255]);
        }

        Ok(rgba)
    }

    /// Remove background, returning an RGBA image with transparent background.
    pub fn remove_background(&self, img: &DynamicImage) -> Result<RgbaImage> {
        self.remove_background_with_params(img, &CutoutParams::default(), &[])
    }

    /// Remove background with fine-tuning parameters, replacing it with a solid color matte.
    pub fn remove_background_with_params_and_matte(
        &self,
        img: &DynamicImage,
        params: &CutoutParams,
        touchups: &[TouchupDab],
        matte: [u8; 3],
    ) -> Result<RgbImage> {
        let mask = self.compute_mask_with_params(img, params, touchups)?;
        let mut rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();

        for y in 0..h {
            for x in 0..w {
                let p = rgba.get_pixel_mut(x, y);
                p[3] = mask.get_pixel(x, y)[0];
            }
        }

        if params.defringe_white {
            defringe_rgba(rgba.as_mut(), w, h, [255, 255, 255]);
        }

        let mut out = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let p = rgba.get_pixel(x, y);
                let a = p[3] as f32 / 255.0;
                let inv_a = 1.0 - a;

                let r = (p[0] as f32 * a + matte[0] as f32 * inv_a).round() as u8;
                let g = (p[1] as f32 * a + matte[1] as f32 * inv_a).round() as u8;
                let b = (p[2] as f32 * a + matte[2] as f32 * inv_a).round() as u8;

                out.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
        Ok(out)
    }

    /// Remove background, replacing it with a solid color matte (e.g. [255, 255, 255] for catalog white).
    pub fn remove_background_with_matte(
        &self,
        img: &DynamicImage,
        matte: [u8; 3],
    ) -> Result<RgbImage> {
        self.remove_background_with_params_and_matte(img, &CutoutParams::default(), &[], matte)
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1e-5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Decontaminate / defringe semi-transparent edge pixels by mathematically subtracting
/// the background color (e.g. pure white [255, 255, 255] from a studio backdrop).
pub fn defringe_rgba(rgba: &mut [u8], w: u32, h: u32, bg: [u8; 3]) {
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize * 4;
            let a = rgba[idx + 3];
            if a > 0 && a < 255 {
                let a_norm = a as f32 / 255.0;
                let inv_a = 1.0 - a_norm;
                for c in 0..3 {
                    let observed = rgba[idx + c] as f32;
                    let unblended = (observed - bg[c] as f32 * inv_a) / a_norm;
                    rgba[idx + c] = unblended.clamp(0.0, 255.0).round() as u8;
                }
            }
        }
    }
}

/// Separable morphological min (erosion / contract) or max (dilation / expand).
fn morphological_shift(mask: &mut GrayImage, shift: i32) {
    if shift == 0 {
        return;
    }
    let (w, h) = mask.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let r = shift.unsigned_abs().min(30) as usize;
    let erode = shift < 0;

    let src = mask.as_raw();
    let mut temp = vec![0u8; (w * h) as usize];

    // Pass 1: Horizontal min/max
    for y in 0..h as usize {
        let row_start = y * w as usize;
        for x in 0..w as usize {
            let x_min = x.saturating_sub(r);
            let x_max = (x + r).min(w as usize - 1);
            let mut val = src[row_start + x];
            if erode {
                for kx in x_min..=x_max {
                    val = val.min(src[row_start + kx]);
                }
            } else {
                for kx in x_min..=x_max {
                    val = val.max(src[row_start + kx]);
                }
            }
            temp[row_start + x] = val;
        }
    }

    // Pass 2: Vertical min/max
    let dst = mask.as_mut();
    for y in 0..h as usize {
        let y_min = y.saturating_sub(r);
        let y_max = (y + r).min(h as usize - 1);
        let row_start = y * w as usize;
        for x in 0..w as usize {
            let mut val = temp[row_start + x];
            if erode {
                for ky in y_min..=y_max {
                    val = val.min(temp[ky * w as usize + x]);
                }
            } else {
                for ky in y_min..=y_max {
                    val = val.max(temp[ky * w as usize + x]);
                }
            }
            dst[row_start + x] = val;
        }
    }
}

/// Two-pass separable box blur (tent filter) across a radius of `feather` pixels.
fn spatial_feather(mask: &mut GrayImage, feather: f32) {
    let r = (feather.round() as usize).clamp(1, 20);
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    if w == 0 || h == 0 {
        return;
    }
    let mut temp = vec![0u8; w * h];
    for _ in 0..2 {
        let src = mask.as_raw();
        for y in 0..h {
            let row = y * w;
            for x in 0..w {
                let x_min = x.saturating_sub(r);
                let x_max = (x + r).min(w - 1);
                let mut sum = 0u32;
                for kx in x_min..=x_max {
                    sum += src[row + kx] as u32;
                }
                let count = (x_max - x_min + 1) as u32;
                temp[row + x] = ((sum + count / 2) / count) as u8;
            }
        }
        let dst = mask.as_mut();
        for y in 0..h {
            let y_min = y.saturating_sub(r);
            let y_max = (y + r).min(h - 1);
            let count = (y_max - y_min + 1) as u32;
            let row = y * w;
            for x in 0..w {
                let mut sum = 0u32;
                for ky in y_min..=y_max {
                    sum += temp[ky * w + x] as u32;
                }
                dst[row + x] = ((sum + count / 2) / count) as u8;
            }
        }
    }
}

/// Apply user touch-up dabs (keep restores foreground to 255, erase removes to 0).
fn apply_touchups(mask: &mut GrayImage, touchups: &[TouchupDab]) {
    let (w, h) = mask.dimensions();
    let max_dim = w.max(h) as f32;
    for dab in touchups {
        let cx = dab.at[0] * w as f32;
        let cy = dab.at[1] * h as f32;
        let r_px = (dab.radius * max_dim).max(1.0);
        let x_min = (cx - r_px).floor().max(0.0) as u32;
        let x_max = (cx + r_px).ceil().min(w as f32 - 1.0) as u32;
        let y_min = (cy - r_px).floor().max(0.0) as u32;
        let y_max = (cy + r_px).ceil().min(h as f32 - 1.0) as u32;

        let target_a = if dab.keep { 255.0 } else { 0.0 };
        for py in y_min..=y_max {
            for px in x_min..=x_max {
                let dx = px as f32 - cx;
                let dy = py as f32 - cy;
                let dist = (dx * dx + dy * dy).sqrt() / r_px;
                if dist <= 1.0 {
                    let cov = 1.0 - smoothstep(dab.hardness.clamp(0.0, 0.99), 1.0, dist);
                    let curr_a = mask.get_pixel(px, py)[0] as f32;
                    let blended = curr_a * (1.0 - cov) + target_a * cov;
                    mask.put_pixel(px, py, Luma([blended.round() as u8]));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_model_resolves_custom_path() {
        // The search directories (~/.u2net, U2NET_HOME, ...) differ per machine, so resolve
        // the model this repository ships instead of whatever one may be installed.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/u2netp.onnx");
        let found = find_model(&RembgModel::Custom(path.clone()));
        assert_eq!(found, Some(path));
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/missing.onnx");
        assert_eq!(find_model(&RembgModel::Custom(missing)), None);
    }

    #[test]
    fn test_u2netp_inference_pure_rust() {
        let model_path = find_model(&RembgModel::U2Netp);
        if let Some(path) = model_path {
            let session =
                RembgSession::from_file(&path).expect("Model should load cleanly in tract");

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
            let mask = session
                .compute_mask(&dynamic)
                .expect("Inference should succeed");
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
        let mask = session
            .compute_mask(&dynamic)
            .expect("Bundled inference should succeed");
        assert_eq!(mask.width(), 64);
        assert_eq!(mask.height(), 64);
        assert!(mask.get_pixel(32, 32)[0] > mask.get_pixel(0, 0)[0]);
    }

    #[test]
    fn test_defringe_white_unblends_background() {
        // Pixel is 50% opacity, observed RGB is (200, 200, 200) blended over white (255, 255, 255).
        // True foreground should be: (200 - 255 * 0.5) / 0.5 = (200 - 127.5) / 0.5 = 145.
        let mut rgba = [200u8, 200, 200, 128];
        defringe_rgba(&mut rgba, 1, 1, [255, 255, 255]);
        // Unblended value should be around 145
        assert!((rgba[0] as i32 - 145).abs() <= 2);
        assert!((rgba[1] as i32 - 145).abs() <= 2);
        assert!((rgba[2] as i32 - 145).abs() <= 2);
    }

    #[test]
    fn test_touchup_keep_and_erase() {
        let mut mask = GrayImage::new(100, 100);
        // Initially all 0
        let touchups = vec![
            TouchupDab {
                at: [0.5, 0.5],
                radius: 0.1,
                hardness: 1.0,
                keep: true,
            },
            TouchupDab {
                at: [0.8, 0.8],
                radius: 0.1,
                hardness: 1.0,
                keep: false,
            },
        ];
        apply_touchups(&mut mask, &touchups);
        // Center should now be 255
        assert_eq!(mask.get_pixel(50, 50)[0], 255);
        // Corner was 0, stays 0
        assert_eq!(mask.get_pixel(80, 80)[0], 0);
    }

    #[test]
    #[ignore = "manual validation on user's real desktop photo"]
    fn test_dsc05273_cutout_real_photo() {
        let test_path = std::path::Path::new("/home/carterw/Desktop/DSC05273.jpg");
        if !test_path.exists() {
            return;
        }
        let session = RembgSession::auto().expect("Auto session should load");
        let img = image::open(test_path).expect("Image should open");
        // Mandolin tailpiece is around x: ~0.505, y: ~0.865 in normalized coords
        let params = CutoutParams {
            threshold: 0.50,
            feather: 1.0,
            edge_shift: 0,
            defringe_white: true,
        };
        let touchups = vec![TouchupDab {
            at: [0.505, 0.865],
            radius: 0.045,
            hardness: 0.85,
            keep: true,
        }];
        let cutout = session
            .remove_background_with_params(&img, &params, &touchups)
            .expect("Background removal should succeed");
        assert_eq!(cutout.width(), img.width());
        assert_eq!(cutout.height(), img.height());
        // Save result so user can visually verify
        let out_path = std::path::Path::new("/home/carterw/Desktop/DSC05273_duckfoot_cutout.png");
        cutout.save(out_path).expect("Saving cutout should succeed");
    }
}
