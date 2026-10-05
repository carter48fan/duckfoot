//! Interactive Cutout Studio: live background removal preview, edge fine-tuning,
//! white halo defringing, and manual Keep/Erase touch-up brushes.

use egui::{pos2, RichText};
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

        let Ok(session) = RembgSession::auto() else {
            return;
        };
        let Ok(mask) =
            session.compute_mask_from_prediction(pred, w, h, &self.params, &self.touchups)
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

        // Composite over chosen preview background mode
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let p = rgba.get_pixel(x, y);
                let alpha = p[3] as f32 / 255.0;
                let inv_a = 1.0 - alpha;

                let (r, g, b, a_out) = match self.preview_mode {
                    CutoutPreviewMode::Checkerboard => {
                        // 16px checkerboard pattern
                        let check = ((x / 16) + (y / 16)) % 2 == 0;
                        let bg = if check { 220.0 } else { 160.0 };
                        (
                            (p[0] as f32 * alpha + bg * inv_a).round() as u8,
                            (p[1] as f32 * alpha + bg * inv_a).round() as u8,
                            (p[2] as f32 * alpha + bg * inv_a).round() as u8,
                            255,
                        )
                    }
                    CutoutPreviewMode::CatalogWhite => (
                        (p[0] as f32 * alpha + 255.0 * inv_a).round() as u8,
                        (p[1] as f32 * alpha + 255.0 * inv_a).round() as u8,
                        (p[2] as f32 * alpha + 255.0 * inv_a).round() as u8,
                        255,
                    ),
                    CutoutPreviewMode::Black => (
                        (p[0] as f32 * alpha).round() as u8,
                        (p[1] as f32 * alpha).round() as u8,
                        (p[2] as f32 * alpha).round() as u8,
                        255,
                    ),
                    CutoutPreviewMode::Rubylith => {
                        // Red mask overlay on background
                        (
                            (p[0] as f32 * alpha + 220.0 * inv_a * 0.75).round() as u8,
                            (p[1] as f32 * alpha + 30.0 * inv_a * 0.75).round() as u8,
                            (p[2] as f32 * alpha + 30.0 * inv_a * 0.75).round() as u8,
                            255,
                        )
                    }
                };

                pixels.push(r);
                pixels.push(g);
                pixels.push(b);
                pixels.push(a_out);
            }
        }

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

    /// Show the floating Cutout Studio toolbar window.
    pub fn show_hud(&mut self, ctx: &egui::Context, on_export_clicked: &mut bool) {
        if !self.open {
            return;
        }

        let mut open = self.open;
        let mut trigger_export = false;
        let mut changed = false;

        egui::Window::new("✂ Cutout Studio")
            .open(&mut open)
            .fade_in(false)
            .collapsible(true)
            .resizable(false)
            .default_width(320.0)
            .default_pos(pos2(24.0, 60.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 7.0;

                // ── Preview Background ──────────────────────────────────────────────────────────
                ui.label(RichText::new("PREVIEW BACKDROP").monospace().size(10.0).color(theme::TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    if widgets::chip(ui, self.preview_mode == CutoutPreviewMode::Checkerboard, "🏁 Checker").clicked() {
                        self.preview_mode = CutoutPreviewMode::Checkerboard;
                        changed = true;
                    }
                    if widgets::chip(ui, self.preview_mode == CutoutPreviewMode::CatalogWhite, "⬜ White").clicked() {
                        self.preview_mode = CutoutPreviewMode::CatalogWhite;
                        changed = true;
                    }
                    if widgets::chip(ui, self.preview_mode == CutoutPreviewMode::Black, "⬛ Black").clicked() {
                        self.preview_mode = CutoutPreviewMode::Black;
                        changed = true;
                    }
                    if widgets::chip(ui, self.preview_mode == CutoutPreviewMode::Rubylith, "🟥 Mask").clicked() {
                        self.preview_mode = CutoutPreviewMode::Rubylith;
                        changed = true;
                    }
                });

                ui.separator();

                // ── Edge & Halo Tuning ─────────────────────────────────────────────────────────
                ui.label(RichText::new("EDGE & DEFRINGE TUNING").monospace().size(10.0).color(theme::TEXT_DIM));
                if widgets::chip(
                    ui,
                    self.params.defringe_white,
                    "✨ Defringe White Lightbox Halo",
                )
                .on_hover_text("Removes white studio background bleed from semi-transparent perimeter pixels")
                .clicked()
                {
                    self.params.defringe_white = !self.params.defringe_white;
                    changed = true;
                }

                if widgets::param_slider(
                    ui,
                    "threshold",
                    &mut self.params.threshold,
                    0.05..=0.95,
                    2,
                    0.50,
                )
                .on_hover_text("Neural net cutoff sensitivity (lower keeps more, higher erases more)")
                .changed()
                {
                    changed = true;
                }

                let mut shift_f = self.params.edge_shift as f32;
                if widgets::param_slider(
                    ui,
                    "edge shift",
                    &mut shift_f,
                    -10.0..=10.0,
                    0,
                    0.0,
                )
                .on_hover_text("Contract (negative) or expand (positive) the perimeter mask in pixels")
                .changed()
                {
                    self.params.edge_shift = shift_f.round() as i32;
                    changed = true;
                }

                if widgets::param_slider(
                    ui,
                    "feather",
                    &mut self.params.feather,
                    0.0..=10.0,
                    1,
                    1.0,
                )
                .on_hover_text("Gaussian edge feathering radius in pixels")
                .changed()
                {
                    changed = true;
                }

                ui.separator();

                // ── Touch-up Brush ────────────────────────────────────────────────────────────
                ui.label(RichText::new("TOUCH-UP BRUSH").monospace().size(10.0).color(theme::TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    if widgets::chip(ui, self.brush_mode == CutoutBrushMode::Keep, "✚ Keep (Restore)")
                        .on_hover_text("Paint on stage to restore cut off parts (e.g. chrome tailpiece)")
                        .clicked()
                    {
                        self.brush_mode = CutoutBrushMode::Keep;
                    }
                    if widgets::chip(ui, self.brush_mode == CutoutBrushMode::Erase, "➖ Erase (Remove)")
                        .on_hover_text("Paint on stage to erase unwanted objects (e.g. guitar stand legs)")
                        .clicked()
                    {
                        self.brush_mode = CutoutBrushMode::Erase;
                    }
                });

                let mut brush_size_pct = self.brush_radius * 100.0;
                if widgets::param_slider(
                    ui,
                    "brush size",
                    &mut brush_size_pct,
                    0.5..=20.0,
                    1,
                    3.5,
                )
                .changed()
                {
                    self.brush_radius = brush_size_pct / 100.0;
                }

                let mut hardness_pct = self.brush_hardness * 100.0;
                if widgets::param_slider(
                    ui,
                    "hardness",
                    &mut hardness_pct,
                    10.0..=100.0,
                    0,
                    85.0,
                )
                .changed()
                {
                    self.brush_hardness = hardness_pct / 100.0;
                }

                ui.horizontal(|ui| {
                    let has_touchups = !self.touchups.is_empty();
                    if widgets::button_enabled(ui, has_touchups, Kind::Secondary, "↶ Undo Dab", 0.0).clicked() {
                        self.undo_touchup(ctx);
                    }
                    if widgets::button_enabled(
                        ui,
                        has_touchups,
                        Kind::Secondary,
                        &format!("✕ Clear ({})", self.touchups.len()),
                        0.0,
                    )
                    .clicked()
                    {
                        self.clear_touchups(ctx);
                    }
                    if widgets::button(ui, Kind::Secondary, "↻ Recompute", 0.0)
                        .on_hover_text("Re-render base image from current exposure/edits and recompute neural mask")
                        .clicked()
                    {
                        self.invalidate_prediction();
                    }
                });

                ui.separator();

                // ── Export Output ─────────────────────────────────────────────────────────────
                ui.label(RichText::new("EXPORT OUTPUT").monospace().size(10.0).color(theme::TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    if widgets::chip(ui, !self.export_matte_white, "Transparent PNG").clicked() {
                        self.export_matte_white = false;
                    }
                    if widgets::chip(ui, self.export_matte_white, "Catalog White PNG").clicked() {
                        self.export_matte_white = true;
                    }
                });

                ui.add_space(3.0);
                if widgets::button(ui, Kind::Primary, "💾 Export Cutout PNG…", 0.0)
                    .on_hover_text("Save full-resolution cutout image with edge tuning and manual touch-ups applied")
                    .clicked()
                {
                    trigger_export = true;
                }

                if let Some(status) = &self.status {
                    ui.add_space(2.0);
                    ui.label(RichText::new(status).size(11.0).color(theme::TEXT_DIM));
                }

                ui.add_space(2.0);
                ui.label(
                    RichText::new("💡 Paint on image • Alt erases • Space+drag pans • Esc closes")
                        .size(10.0)
                        .color(theme::TEXT_FAINT),
                );
            });

        self.open = open;
        if changed {
            self.update_preview_texture(ctx);
        }
        if trigger_export {
            *on_export_clicked = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut state = CutoutStudioState::default();
        state.cached_source_path = Some(PathBuf::from("/test/mandolin.arw"));
        state.cached_pred_320 = Some(vec![0.5; 320 * 320]);
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
                if x >= 30 && x <= 70 && y >= 20 && y <= 80 {
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
