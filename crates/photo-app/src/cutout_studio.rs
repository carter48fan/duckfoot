//! Interactive Cutout Studio: live background removal preview, edge fine-tuning,
//! white halo defringing, and manual Keep/Erase touch-up brushes.

use egui::RichText;
use image::{DynamicImage, GenericImageView, RgbaImage};
use photo_engine::rembg::{CutoutParams, RembgSession, TouchupDab};
use std::path::{Path, PathBuf};

use crate::theme;
use crate::widgets::{self, Kind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CutoutPreviewMode {
    #[default]
    Checkerboard,
    CatalogWhite,
    Black,
    Rubylith,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CutoutBrushMode {
    #[default]
    Keep,
    Erase,
}

pub struct CutoutStudioState {
    pub open: bool,
    pub preview_mode: CutoutPreviewMode,
    pub brush_mode: CutoutBrushMode,
    pub brush_radius: f32,
    pub brush_hardness: f32,
    pub params: CutoutParams,
    pub touchups: Vec<TouchupDab>,
    pub export_matte_white: bool,
    pub cached_pred_320: Option<Vec<f32>>,
    pub cached_upscaled_pred: Option<image::GrayImage>,
    pub cached_source_path: Option<PathBuf>,
    pub preview_texture: Option<egui::TextureHandle>,
    pub preview_base_rgba: Option<RgbaImage>,
    pub status: Option<String>,
}

impl Default for CutoutStudioState {
    fn default() -> Self {
        Self {
            open: false,
            preview_mode: CutoutPreviewMode::Checkerboard,
            brush_mode: CutoutBrushMode::Keep,
            brush_radius: 0.035,
            brush_hardness: 0.85,
            params: CutoutParams {
                threshold: 0.50,
                feather: 1.0,
                edge_shift: 0,
                defringe_white: true,
            },
            touchups: Vec::new(),
            export_matte_white: false,
            cached_pred_320: None,
            cached_upscaled_pred: None,
            cached_source_path: None,
            preview_texture: None,
            preview_base_rgba: None,
            status: None,
        }
    }
}

impl CutoutStudioState {
    /// Invalidate caches when changing loaded photo.
    pub fn reset_cache(&mut self) {
        self.cached_pred_320 = None;
        self.cached_upscaled_pred = None;
        self.cached_source_path = None;
        self.preview_texture = None;
        self.preview_base_rgba = None;
        self.touchups.clear();
        self.status = None;
    }

    /// Invalidate neural prediction and preview texture without wiping manual touch-up dabs.
    pub fn invalidate_prediction(&mut self) {
        self.cached_source_path = None;
        self.cached_pred_320 = None;
        self.cached_upscaled_pred = None;
        self.preview_texture = None;
        self.preview_base_rgba = None;
        self.status = None;
    }

    /// Prepare Cutout Studio using a pre-rendered image (e.g. from the GPU pipeline with full exposure/edits).
    pub fn prepare_with_image(&mut self, ctx: &egui::Context, path: &Path, img: DynamicImage) {
        let is_same_path = self.cached_source_path.as_deref() == Some(path);
        if is_same_path && self.cached_pred_320.is_some() {
            return;
        }

        if !is_same_path {
            self.touchups.clear();
        }
        self.cached_pred_320 = None;
        self.cached_upscaled_pred = None;
        self.cached_source_path = Some(path.to_path_buf());
        self.preview_texture = None;
        self.preview_base_rgba = None;
        self.status = None;

        // Downscale source for fast 60fps interactive preview (~1400px long side) if needed
        let (w, h) = img.dimensions();
        let scale = 1400.0 / (w.max(h) as f32).max(1.0);
        let (pw, ph) = if scale < 1.0 {
            (
                ((w as f32 * scale).round() as u32).max(1),
                ((h as f32 * scale).round() as u32).max(1),
            )
        } else {
            (w, h)
        };
        let preview_img = if (pw, ph) != (w, h) {
            img.resize_exact(pw, ph, image::imageops::FilterType::Triangle)
        } else {
            img
        };
        self.preview_base_rgba = Some(preview_img.to_rgba8());

        // Run neural inference for 320x320 prediction
        match RembgSession::auto() {
            Ok(session) => match session.raw_prediction_320(&preview_img) {
                Ok(pred) => {
                    self.cached_upscaled_pred =
                        Some(RembgSession::upscale_prediction(&pred, pw, ph));
                    self.cached_pred_320 = Some(pred);
                    self.update_preview_texture(ctx);
                    self.status = Some(
                        "Cutout ready. Use Keep/Erase brushes or sliders to fine-tune.".into(),
                    );
                }
                Err(e) => {
                    self.status = Some(format!("Inference failed: {e}"));
                }
            },
            Err(e) => {
                self.status = Some(format!("Failed to load rembg model: {e}"));
            }
        }
    }

    /// Load source photo into memory, compute 320x320 prediction, and render initial preview.
    /// Supports both raster files (JPEG, PNG, TIFF, WebP) and camera RAW files (Sony ARW, Canon CR2/CR3, Nikon NEF, Fuji RAF, etc.).
    pub fn prepare(&mut self, ctx: &egui::Context, path: &Path) {
        if self.cached_source_path.as_deref() == Some(path) && self.cached_pred_320.is_some() {
            return;
        }

        match Self::load_image_from_file(path) {
            Ok(img) => {
                self.prepare_with_image(ctx, path, img);
            }
            Err(e) => {
                self.reset_cache();
                self.cached_source_path = Some(path.to_path_buf());
                self.status = Some(format!("Failed to open image for cutout: {e}"));
            }
        }
    }

    /// Load an image from file path with automatic support for camera RAW files and EXIF orientation.
    pub fn load_image_from_file(path: &Path) -> Result<DynamicImage, String> {
        // 1. If it's a standard raster format, try standard image loader first
        if photo_engine::is_raster_path(path) {
            if let Ok(img) = image::open(path) {
                return Ok(img);
            }
        }

        // 2. Try camera RAW embedded preview
        if let Some(preview) = photo_engine::load_embedded_preview(path, 1400) {
            let (w, h) = (preview.width, preview.height);
            if let Some(rgb) = image::RgbImage::from_raw(w, h, preview.rgb) {
                let mut dyn_img = DynamicImage::ImageRgb8(rgb);
                for _ in 0..(preview.orientation.quarter_turns % 4) {
                    dyn_img = dyn_img.rotate90();
                }
                if preview.orientation.mirror_h {
                    dyn_img = dyn_img.fliph();
                }
                if preview.orientation.mirror_v {
                    dyn_img = dyn_img.flipv();
                }
                return Ok(dyn_img);
            }
        }

        // 3. Fallback to image::open
        image::open(path).map_err(|e| format!("{e}"))
    }

    /// Re-render the interactive preview texture from the cached prediction, params, and touch-ups.
    pub fn update_preview_texture(&mut self, ctx: &egui::Context) {
        let (Some(pred), Some(base_rgba)) = (&self.cached_pred_320, &self.preview_base_rgba) else {
            return;
        };
        let (w, h) = base_rgba.dimensions();
        if self
            .cached_upscaled_pred
            .as_ref()
            .is_none_or(|up| up.dimensions() != (w, h))
        {
            self.cached_upscaled_pred = Some(RembgSession::upscale_prediction(pred, w, h));
        }
        let Some(upscaled) = &self.cached_upscaled_pred else {
            return;
        };

        let Ok(session) = RembgSession::auto() else {
            return;
        };
        let Ok(mask) =
            session.compute_mask_from_upscaled(upscaled, &self.params, &self.touchups)
        else {
            return;
        };

        let mut rgba = base_rgba.clone();
        for y in 0..h {
            for x in 0..w {
                let p = rgba.get_pixel_mut(x, y);
                p[3] = mask.get_pixel(x, y)[0];
            }
        }

        if self.params.defringe_white {
            photo_engine::rembg::defringe_rgba(rgba.as_mut(), w, h, [255, 255, 255]);
        }

        let pixels = overlay_pixels(&rgba, self.preview_mode);
        let color_image =
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
        if let Some(tex) = &mut self.preview_texture {
            tex.set(color_image, egui::TextureOptions::LINEAR);
        } else {
            self.preview_texture =
                Some(ctx.load_texture("cutout_preview", color_image, egui::TextureOptions::LINEAR));
        }
    }

    /// Add a touchup dab with explicit mode (normalized coordinates [0..1]).
    pub fn add_touchup_with_mode(
        &mut self,
        ctx: &egui::Context,
        at: [f32; 2],
        mode: CutoutBrushMode,
    ) {
        if let Some(last) = self.touchups.last() {
            let dx = last.at[0] - at[0];
            let dy = last.at[1] - at[1];
            if (dx * dx + dy * dy).sqrt() < (self.brush_radius * 0.3).max(0.004)
                && last.keep == (mode == CutoutBrushMode::Keep)
            {
                return;
            }
        }
        self.touchups.push(TouchupDab {
            at,
            radius: self.brush_radius,
            hardness: self.brush_hardness,
            keep: mode == CutoutBrushMode::Keep,
        });
        self.update_preview_texture(ctx);
    }

    /// Add a touchup dab using current brush mode.
    pub fn add_touchup(&mut self, ctx: &egui::Context, at: [f32; 2]) {
        self.add_touchup_with_mode(ctx, at, self.brush_mode);
    }

    /// Undo the last touch-up dab.
    pub fn undo_touchup(&mut self, ctx: &egui::Context) -> bool {
        if self.touchups.pop().is_some() {
            self.update_preview_texture(ctx);
            true
        } else {
            false
        }
    }

    /// Clear all touch-up dabs.
    pub fn clear_touchups(&mut self, ctx: &egui::Context) {
        if !self.touchups.is_empty() {
            self.touchups.clear();
            self.update_preview_texture(ctx);
        }
    }

    /// The studio's controls, wherever they are shown: the desktop's Remove background
    /// tool, or the phone's Tools sheet. Returns whether the preview needs remaking, and whether
    /// export was pressed.
    pub fn controls(&mut self, ui: &mut egui::Ui) -> (bool, bool) {
        let mut trigger_export = false;
        let mut changed = false;
        let caption = |ui: &mut egui::Ui, text: &str| {
            ui.add_space(2.0);
            ui.label(theme::tag(text).color(theme::TEXT_DIM));
        };

        caption(ui, "show it on");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for (mode, label, tip) in [
                (
                    CutoutPreviewMode::Checkerboard,
                    "checker",
                    "Transparent, as a checkerboard",
                ),
                (CutoutPreviewMode::CatalogWhite, "white", "On catalog white"),
                (CutoutPreviewMode::Black, "black", "On black"),
                (
                    CutoutPreviewMode::Rubylith,
                    "mask",
                    "What it drops, washed red",
                ),
            ] {
                if widgets::outlined_chip(ui, true, self.preview_mode == mode, label)
                    .on_hover_text(tip)
                    .clicked()
                    && self.preview_mode != mode
                {
                    self.preview_mode = mode;
                    changed = true;
                }
            }
        });

        caption(ui, "edge");
        if widgets::param_slider(
            ui,
            "threshold",
            &mut self.params.threshold,
            0.05..=0.95,
            2,
            0.50,
        )
        .on_hover_text("Lower keeps more of the photo, higher drops more")
        .changed()
        {
            changed = true;
        }
        let mut shift_f = self.params.edge_shift as f32;
        if widgets::param_slider(ui, "edge shift", &mut shift_f, -10.0..=10.0, 0, 0.0)
            .on_hover_text("Pull the edge in (negative) or push it out (positive), in pixels")
            .changed()
        {
            self.params.edge_shift = shift_f.round() as i32;
            changed = true;
        }
        if widgets::param_slider(ui, "feather", &mut self.params.feather, 0.0..=10.0, 1, 1.0)
            .on_hover_text("How soft the edge is, in pixels")
            .changed()
        {
            changed = true;
        }
        if widgets::outlined_chip(ui, true, self.params.defringe_white, "defringe white")
            .on_hover_text("Take a white backdrop's glow out of the soft edge pixels")
            .clicked()
        {
            self.params.defringe_white = !self.params.defringe_white;
            changed = true;
        }

        caption(ui, "touch up");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if widgets::outlined_chip(ui, true, self.brush_mode == CutoutBrushMode::Keep, "keep")
                .on_hover_text("Paint on the photo to bring back what it cut away")
                .clicked()
            {
                self.brush_mode = CutoutBrushMode::Keep;
            }
            if widgets::outlined_chip(ui, true, self.brush_mode == CutoutBrushMode::Erase, "erase")
                .on_hover_text("Paint on the photo to take away what it kept (Alt too)")
                .clicked()
            {
                self.brush_mode = CutoutBrushMode::Erase;
            }
        });
        let mut brush_size_pct = self.brush_radius * 100.0;
        if widgets::param_slider(ui, "brush size", &mut brush_size_pct, 0.5..=20.0, 1, 3.5)
            .changed()
        {
            self.brush_radius = brush_size_pct / 100.0;
        }
        let mut hardness_pct = self.brush_hardness * 100.0;
        if widgets::param_slider(ui, "hardness", &mut hardness_pct, 10.0..=100.0, 0, 85.0).changed()
        {
            self.brush_hardness = hardness_pct / 100.0;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let has_touchups = !self.touchups.is_empty();
            if widgets::outlined_chip(ui, has_touchups, false, "undo stroke").clicked() {
                self.undo_touchup(ui.ctx());
            }
            if widgets::outlined_chip(
                ui,
                has_touchups,
                false,
                &format!("clear ({})", self.touchups.len()),
            )
            .clicked()
            {
                self.clear_touchups(ui.ctx());
            }
            if widgets::outlined_chip(ui, true, false, "recompute")
                .on_hover_text(
                    "Find the subject again, from the photo with its edits as they are now",
                )
                .clicked()
            {
                self.invalidate_prediction();
            }
        });

        caption(ui, "save as png");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if widgets::outlined_chip(ui, true, !self.export_matte_white, "transparent").clicked() {
                self.export_matte_white = false;
            }
            if widgets::outlined_chip(ui, true, self.export_matte_white, "on white").clicked() {
                self.export_matte_white = true;
            }
        });
        ui.add_space(2.0);
        if widgets::button(ui, Kind::Primary, "Save cutout…", ui.available_width())
            .on_hover_text("A full-size PNG, with the edge settings and touch-ups")
            .clicked()
        {
            trigger_export = true;
        }

        if let Some(status) = &self.status {
            ui.label(RichText::new(status).size(11.0).color(theme::TEXT_DIM));
        }
        let hint = if widgets::is_touch(ui.ctx()) {
            "Paint on the photo to keep or erase · pinch to zoom"
        } else {
            "Paint on the photo · Alt erases · Space-drag pans · Esc closes"
        };
        ui.label(theme::dim(hint).size(11.0));
        (changed, trigger_export)
    }
}

/// The preview as an overlay for the stage's own image of the photo: each pixel the cutout
/// drops or softens is composited over `mode`'s backdrop, and each pixel it keeps whole is
/// left transparent. The stage's image re-renders sharper as it zooms in and this one is
/// fixed at about 1400px, so the kept subject shows through at full detail.
///
/// Defringing only touches the softened pixels, so it shows here too.
fn overlay_pixels(rgba: &RgbaImage, mode: CutoutPreviewMode) -> Vec<u8> {
    let (w, h) = rgba.dimensions();
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let p = rgba.get_pixel(x, y);
            if p[3] == 255 {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let alpha = p[3] as f32 / 255.0;
            let inv_a = 1.0 - alpha;
            let over = |bg: [f32; 3]| {
                [0, 1, 2].map(|c| (p[c] as f32 * alpha + bg[c] * inv_a).round() as u8)
            };
            let [r, g, b] = match mode {
                CutoutPreviewMode::Checkerboard => {
                    // 16px checkerboard pattern
                    let check = ((x / 16) + (y / 16)) % 2 == 0;
                    let bg = if check { 220.0 } else { 160.0 };
                    over([bg; 3])
                }
                CutoutPreviewMode::CatalogWhite => over([255.0; 3]),
                CutoutPreviewMode::Black => over([0.0; 3]),
                // Red mask overlay on background
                CutoutPreviewMode::Rubylith => over([220.0 * 0.75, 30.0 * 0.75, 30.0 * 0.75]),
            };
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_shows_the_photo_through_what_the_cutout_keeps() {
        let mut rgba = RgbaImage::new(3, 1);
        rgba.put_pixel(0, 0, image::Rgba([10, 20, 30, 255])); // kept
        rgba.put_pixel(1, 0, image::Rgba([10, 20, 30, 0])); // dropped
        rgba.put_pixel(2, 0, image::Rgba([100, 100, 100, 128])); // edge
        let px = overlay_pixels(&rgba, CutoutPreviewMode::CatalogWhite);
        assert_eq!(&px[0..4], &[0, 0, 0, 0], "a kept pixel is the stage's");
        assert_eq!(
            &px[4..8],
            &[255, 255, 255, 255],
            "a dropped pixel is the backdrop"
        );
        // 100·0.502 + 255·0.498 ≈ 177, opaque.
        assert_eq!(&px[8..12], &[177, 177, 177, 255]);
    }

    #[test]
    fn default_state_has_white_defringe_enabled() {
        let state = CutoutStudioState::default();
        assert!(!state.open);
        assert!(state.params.defringe_white);
        assert_eq!(state.preview_mode, CutoutPreviewMode::Checkerboard);
        assert_eq!(state.brush_mode, CutoutBrushMode::Keep);
        assert!(state.touchups.is_empty());
    }

    #[test]
    fn touchup_brush_management() {
        let mut state = CutoutStudioState {
            brush_radius: 0.05,
            ..Default::default()
        };

        // Push Keep dab
        state.touchups.push(TouchupDab {
            at: [0.5, 0.5],
            radius: state.brush_radius,
            hardness: 0.8,
            keep: true,
        });
        assert_eq!(state.touchups.len(), 1);
        assert!(state.touchups[0].keep);

        // Push Erase dab
        state.touchups.push(TouchupDab {
            at: [0.6, 0.7],
            radius: state.brush_radius,
            hardness: 0.8,
            keep: false,
        });
        assert_eq!(state.touchups.len(), 2);
        assert!(!state.touchups[1].keep);

        // Undo
        assert!(state.touchups.pop().is_some());
        assert_eq!(state.touchups.len(), 1);

        // Clear
        state.touchups.clear();
        assert!(state.touchups.is_empty());
    }

    #[test]
    fn invalidate_prediction_preserves_touchups() {
        let mut state = CutoutStudioState {
            cached_source_path: Some(PathBuf::from("/test/mandolin.arw")),
            cached_pred_320: Some(vec![0.5; 320 * 320]),
            ..Default::default()
        };
        state.touchups.push(TouchupDab {
            at: [0.5, 0.5],
            radius: 0.05,
            hardness: 0.8,
            keep: true,
        });

        state.invalidate_prediction();
        assert!(state.cached_source_path.is_none());
        assert!(state.cached_pred_320.is_none());
        assert_eq!(
            state.touchups.len(),
            1,
            "Touchups should be preserved on invalidate"
        );
    }

    #[test]
    fn prepare_with_image_populates_prediction() {
        let ctx = egui::Context::default();
        let mut state = CutoutStudioState::default();
        let path = Path::new("/test/DSC05280.ARW");

        // Synthetic 100x100 RGB image with a bright center (like an instrument) on dark background
        let mut img = image::RgbaImage::new(100, 100);
        for y in 0..100 {
            for x in 0..100 {
                if (30..=70).contains(&x) && (20..=80).contains(&y) {
                    img.put_pixel(x, y, image::Rgba([220, 160, 40, 255]));
                } else {
                    img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
                }
            }
        }
        let dyn_img = DynamicImage::ImageRgba8(img);

        state.prepare_with_image(&ctx, path, dyn_img);
        assert_eq!(state.cached_source_path.as_deref(), Some(path));
        assert!(state.cached_pred_320.is_some());
        assert!(state.preview_base_rgba.is_some());
        assert!(state.preview_texture.is_some());
        assert!(state
            .status
            .as_ref()
            .is_some_and(|s| s.contains("Cutout ready")));
    }
}
