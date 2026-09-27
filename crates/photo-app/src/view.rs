//! Crop mode, and zoom and pan over the stage.

use crate::*;

impl PhotoApp {
    // ── Crop mode ───────────────────────────────────────────────────────────────────────

    /// Show the whole frame again, with the crop drawn over it, to reframe.
    pub fn begin_crop(&mut self) {
        if self.cropping || self.ready_image().is_none() {
            return;
        }
        self.cropping = true;
        self.crop_before = self.stack.geometry.crop;
        self.crop_drag = None;
        // The shown region just changed size, so a zoom into the old one means nothing.
        self.view = View::default();
    }

    /// Keep the new framing and go back to editing inside it. The whole session is one
    /// undo step, however many drags it took.
    pub(crate) fn apply_crop(&mut self) {
        if !self.cropping {
            return;
        }
        self.cropping = false;
        self.crop_drag = None;
        if self.stack.geometry.crop != self.crop_before {
            let mut before = self.stack.clone();
            before.geometry.crop = self.crop_before;
            self.history.checkpoint(&before);
        }
        self.view = View::default();
        self.needs_render = true;
    }

    /// Leave crop mode with the crop as it was on entering.
    pub(crate) fn cancel_crop(&mut self) {
        if !self.cropping {
            return;
        }
        self.cropping = false;
        self.crop_drag = None;
        self.stack.geometry.crop = self.crop_before;
        self.view = View::default();
        self.needs_render = true;
    }

    /// Fix the crop's shape (or free it); a fixed one is refitted about its centre at once.
    pub fn set_crop_aspect(&mut self, aspect: crop_tool::Aspect) {
        self.crop_aspect = aspect;
        let Some(frame) = self.frame_pixels() else {
            return;
        };
        if let Some(ratio) = aspect.pixel_ratio(frame) {
            let fitted = crop_tool::fitted(self.stack.geometry.crop.sanitised(), ratio, frame);
            if fitted != self.stack.geometry.crop {
                self.stack.geometry.crop = fitted;
                self.needs_render = true;
            }
        }
    }

    /// The same shape turned a quarter (X): 3:2 becomes 2:3, the photo's own shape its
    /// upright or sideways twin.
    pub(crate) fn swap_crop_aspect(&mut self) {
        let turned = match self.crop_aspect {
            crop_tool::Aspect::Original => match self.frame_pixels() {
                Some((w, h)) => crop_tool::Aspect::Ratio(h.round() as u32, w.round() as u32),
                None => return,
            },
            crop_tool::Aspect::Free => {
                // A free crop turns too: its own shape, sideways.
                let Some((fw, fh)) = self.frame_pixels() else {
                    return;
                };
                let c = self.stack.geometry.crop.sanitised();
                let (w, h) = (c.width * fw, c.height * fh);
                self.set_crop_aspect(crop_tool::Aspect::Ratio(h.round() as u32, w.round() as u32));
                self.crop_aspect = crop_tool::Aspect::Free;
                return;
            }
            other => other.swapped(),
        };
        self.set_crop_aspect(turned);
    }

    /// The frame the crop lives in, in pixels: after turns, before the crop.
    pub(crate) fn frame_pixels(&self) -> Option<(f32, f32)> {
        let image = self.ready_image()?;
        let (w, h) = self
            .stack
            .geometry
            .output_size(image.sensor_width, image.sensor_height);
        Some((w as f32, h as f32))
    }

    /// The aspect chips, the swap and the guide, for either layout's crop controls.
    pub(crate) fn crop_shape_chips(&mut self, ui: &mut egui::Ui) {
        for (aspect, label) in crop_tool::Aspect::CHOICES {
            if widgets::chip(ui, self.crop_aspect == aspect, label).clicked() {
                self.set_crop_aspect(aspect);
            }
        }
        // A shape swapped to one without a chip of its own (2:3) still shows which it is.
        if !crop_tool::Aspect::CHOICES
            .iter()
            .any(|(a, _)| *a == self.crop_aspect)
        {
            let _ = widgets::chip(ui, true, &self.crop_aspect.label());
        }
        if widgets::chip(ui, false, "⇄")
            .on_hover_text("Turn the shape a quarter: 3:2 to 2:3 (X)")
            .clicked()
        {
            self.swap_crop_aspect();
        }
        if widgets::chip(ui, false, self.crop_guide.label())
            .on_hover_text("Next guide: thirds, golden, diagonals, grid, none (O)")
            .clicked()
        {
            self.crop_guide = self.crop_guide.next();
        }
    }

    pub(crate) fn toggle_crop(&mut self) {
        if self.cropping {
            self.apply_crop();
        } else {
            self.begin_crop();
        }
    }

    /// A turn or flip mid-session moves the crop into a new frame, and is its own undo
    /// step; Cancel returns to the crop as it stands after it, not to one in the old frame.
    pub(crate) fn rebase_crop(&mut self) {
        if self.cropping {
            self.crop_before = self.stack.geometry.crop;
        }
    }

    /// The part of the frame on screen: all of it while framing, otherwise the crop.
    pub(crate) fn shown_region(&self) -> Crop {
        if self.cropping {
            Crop::default()
        } else {
            self.stack.geometry.crop.sanitised()
        }
    }

    /// Size of the shown region, in full-resolution output pixels.
    pub(crate) fn shown_size(&self) -> Option<egui::Vec2> {
        let image = self.ready_image()?;
        let region = self.shown_region();
        Some(egui::vec2(
            image.width as f32 * region.width,
            image.height as f32 * region.height,
        ))
    }

    /// Kelvin and tint for the open photo's camera.
    pub(crate) fn white_scale(&self) -> WhiteScale {
        self.ready_image()
            .map_or_else(WhiteScale::srgb, |image| image.white_scale())
    }

    /// Scale at which the whole frame fits the panel it had last frame.
    pub(crate) fn fit_scale(&self) -> Option<f32> {
        let img = self.shown_size()?;
        let panel = self.last_panel;
        if panel.x < 1.0 || panel.y < 1.0 {
            return None;
        }
        Some((panel.x / img.x).min(panel.y / img.y))
    }

    pub(crate) fn zoom_by(&mut self, factor: f32) {
        let max_zoom = self
            .fit_scale()
            .map_or(1.0, |fit| (MAX_ZOOM_SCALE / fit).max(1.0));
        self.view.zoom = (self.view.zoom * factor).clamp(1.0, max_zoom);
    }

    pub(crate) fn zoom_actual(&mut self) {
        // 1:1 is expressed relative to fit, which depends on the panel size, so it is
        // resolved against the screen rect the workspace actually got last frame.
        if let Some(percent) = self.zoom_percent(self.last_panel) {
            if percent > 0.0 {
                self.view.zoom *= 100.0 / percent;
                self.view.zoom = self.view.zoom.max(1.0);
            }
        }
    }
}
