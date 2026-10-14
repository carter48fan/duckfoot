//! The stage: the photo, crop framing, split view and the empty state.

use crate::*;

/// How long the stage's corner readouts stay after the pointer stops moving, in seconds.
const HUD_LINGER: f32 = 1.5;

impl PhotoApp {
    pub(crate) fn workspace(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::same(16)),
            )
            .show(ui, |ui| {
                if let Some((path, preview)) = &self.stage_preview {
                    if self.loading.as_ref().is_some_and(|(p, _)| p == path) {
                        stage_preview(ui, preview, path);
                        return;
                    }
                }
                if self.texture_id.is_none() || self.ready_image().is_none() {
                    self.empty_stage(ui);
                    return;
                }
                let (Some(id), Some((image_w, image_h))) = (
                    self.texture_id,
                    self.ready_image().map(|i| (i.width, i.height)),
                ) else {
                    return;
                };

                // Only the crop is on screen outside crop mode: `img` is the size of what is
                // shown, and `frame_of` finds where the whole frame would sit around it.
                let full = egui::vec2(image_w as f32, image_h as f32);
                let region = self.shown_region();
                let img = full * egui::vec2(region.width, region.height);
                let frame_of = |shown: egui::Rect| {
                    let size = shown.size() / egui::vec2(region.width, region.height);
                    egui::Rect::from_min_size(
                        shown.min - size * egui::vec2(region.x, region.y),
                        size,
                    )
                };
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                self.last_panel = rect.size();
                self.compact_rects.stage = rect;
                if rect.width() < 1.0 || rect.height() < 1.0 {
                    return;
                }

                if self.cutout_studio.open {
                    let needs_prepare = self.loaded_path.as_ref().is_some_and(|p| {
                        self.cutout_studio.cached_source_path.as_deref() != Some(p.as_path())
                            || self.cutout_studio.cached_pred_320.is_none()
                    });
                    if needs_prepare {
                        self.prepare_cutout_studio(ui.ctx(), gpu);
                    }
                }

                // Scale at which the whole frame fits. Everything is expressed relative to
                // this so that resizing the window never changes what you are looking at.
                let fit = (rect.width() / img.x).min(rect.height() / img.y);
                let max_zoom = (MAX_ZOOM_SCALE / fit).max(1.0);

                let scale_now = fit * self.view.zoom;
                let dest_now = egui::Rect::from_center_size(
                    rect.center() - self.view.pan * scale_now,
                    img * scale_now,
                );
                // At fit the whole frame is visible and there is nothing to pan to.
                let at_fit = self.view.zoom <= 1.0 + 1e-3;

                // The before/after divider is its own widget, registered after the image so
                // it sits on top: grabbing it can never start a pan. The hit strip is wider
                // than the line it draws.
                // The split is a fraction of the whole frame (that is what the shader tests),
                // so it is placed against the frame, not the crop.
                let frame_now = frame_of(dest_now);
                let divider = (self.split_active && !self.cropping).then(|| {
                    let x = (frame_now.left() + self.split_pos * frame_now.width())
                        .clamp(dest_now.left(), dest_now.right());
                    let strip = Rect::from_x_y_ranges(x - 12.0..=x + 12.0, dest_now.y_range())
                        .intersect(rect);
                    ui.interact(
                        strip,
                        ui.id().with("split_divider"),
                        Sense::click_and_drag(),
                    )
                });
                let mut move_divider_to = None;
                if let Some(div) = &divider {
                    if div.hovered() || div.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if div.double_clicked() {
                        self.split_pos = 0.5;
                        self.needs_render = true;
                    } else if div.is_pointer_button_down_on() {
                        move_divider_to = div.interact_pointer_pos();
                    }
                }
                // A chosen local mask's handles take a drag that starts on them, before a pan
                // or the divider can.
                let sensor = self
                    .ready_image()
                    .map(|i| (i.sensor_width, i.sensor_height));
                if let (Some(i), Some(sensor), false) = (self.selected_mask, sensor, self.cropping) {
                    let mapping = crate::mask_tool::Mapping {
                        geometry: self.stack.geometry,
                        sensor,
                        frame: frame_now,
                    };
                    let tolerance = if widgets::is_touch(ui.ctx()) { 24.0 } else { 9.0 };
                    let painting_brush = self
                        .stack
                        .local
                        .params
                        .masks
                        .get(i)
                        .is_some_and(|m| matches!(m.shape, photo_engine::local::Shape::Brush { .. }));
                    // A pin under a click chooses that mask, even while a brush is out.
                    let on_pin = response.clicked()
                        && response.interact_pointer_pos().is_some_and(|p| {
                            self.stack
                                .local
                                .params
                                .masks
                                .iter()
                                .enumerate()
                                .any(|(j, m)| j != i && mapping.pin(m).distance(p) <= tolerance)
                        });
                    if painting_brush && !on_pin {
                        // A brush mask is painted: a drag lays a stroke (one undo step), a
                        // tap one dab. Alt or the right button erases.
                        let erase = self.brush.erase
                            || ui.input(|i| i.modifiers.alt)
                            || response.dragged_by(egui::PointerButton::Secondary);
                        if response.drag_started() {
                            let origin = ui.input(|i| i.pointer.press_origin());
                            if let Some(at) = origin.or(response.interact_pointer_pos()) {
                                self.history.checkpoint(&self.stack);
                                let s = mapping.to_sensor(at);
                                self.painting = Some(self.paint(i, s, s, true, erase));
                            }
                        } else if response.clicked() {
                            if let Some(at) = response.interact_pointer_pos() {
                                self.history.checkpoint(&self.stack);
                                let s = mapping.to_sensor(at);
                                self.paint(i, s, s, true, erase);
                            }
                        }
                        if let (Some(last), Some(p), true) =
                            (self.painting, response.interact_pointer_pos(), response.dragged())
                        {
                            let to = mapping.to_sensor(p);
                            self.painting = Some(self.paint(i, last, to, false, erase));
                        }
                        if response.hover_pos().is_some() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                        }
                    } else if let Some(mask) = self.stack.local.params.masks.get(i).cloned() {
                        if response.drag_started() {
                            let origin = ui.input(|i| i.pointer.press_origin());
                            if let Some(at) = origin.or(response.interact_pointer_pos()) {
                                if let Some(handle) = mapping.hit(&mask, at, tolerance) {
                                    self.history.checkpoint(&self.stack);
                                    self.mask_drag = Some((handle, at, mask.clone()));
                                }
                            }
                        }
                        if let (Some((handle, from, start)), Some(p)) =
                            (self.mask_drag.clone(), response.interact_pointer_pos())
                        {
                            let moved = mapping.dragged(&start, handle, from, p);
                            if self.stack.local.params.masks[i] != moved {
                                self.stack.local.params.masks[i] = moved;
                                self.needs_render = true;
                            }
                        }
                        if let Some(p) = response.hover_pos() {
                            let cursor = match self.mask_drag.as_ref().map(|d| d.0).or(mapping.hit(&mask, p, tolerance)) {
                                Some(crate::mask_tool::Handle::Move) => Some(egui::CursorIcon::Grab),
                                Some(_) => Some(egui::CursorIcon::Crosshair),
                                None => None,
                            };
                            if let Some(c) = cursor {
                                ui.ctx().set_cursor_icon(c);
                            }
                        }
                    }
                    // A click on another mask's pin chooses it.
                    if response.clicked() {
                        if let Some(p) = response.interact_pointer_pos() {
                            let pins = self.stack.local.params.masks.iter().enumerate();
                            let hit = pins
                                .map(|(j, m)| (j, mapping.pin(m).distance(p)))
                                .filter(|(j, d)| *j != i && *d <= tolerance)
                                .min_by(|a, b| a.1.total_cmp(&b.1));
                            if let Some((j, _)) = hit {
                                self.selected_mask = Some(j);
                            }
                        }
                    }
                }
                // Healing: a tap heals a spot there; a spot's circle, or its source's, drags.
                if let (true, Some(sensor), false) = (self.healing, sensor, self.cropping) {
                    let mapping = crate::mask_tool::Mapping {
                        geometry: self.stack.geometry,
                        sensor,
                        frame: frame_now,
                    };
                    let (ow, _) = self.stack.geometry.output_size(sensor.0, sensor.1);
                    let per_px = frame_now.width() / ow as f32;
                    let long = sensor.0.max(sensor.1) as f32;
                    let slack = if widgets::is_touch(ui.ctx()) { 12.0 } else { 4.0 };
                    let spots = self.stack.heal.params.spots.clone();
                    let selected = self.selected_spot;
                    // The circle under a point: the chosen spot's source first, then any spot.
                    let hit = |p: egui::Pos2| -> Option<(usize, bool)> {
                        let within = |at: [f32; 2], r: f32| {
                            mapping.to_screen(at).distance(p) <= r * long * per_px + slack
                        };
                        if let Some(i) = selected {
                            // A Remove spot has no source to drag.
                            if let Some(s) = spots
                                .get(i)
                                .filter(|s| s.mode != photo_engine::heal::Mode::Remove)
                            {
                                if within(s.src, s.radius) {
                                    return Some((i, true));
                                }
                            }
                        }
                        spots
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, s)| within(s.dst, s.radius))
                            .map(|(i, _)| (i, false))
                    };
                    if response.drag_started() {
                        let origin = ui.input(|i| i.pointer.press_origin());
                        if let Some(at) = origin.or(response.interact_pointer_pos()) {
                            if let Some((i, is_src)) = hit(at) {
                                self.history.checkpoint(&self.stack);
                                self.selected_spot = Some(i);
                                self.spot_drag = Some((is_src, at, spots[i]));
                            }
                        }
                    }
                    if let (Some((is_src, from, start)), Some(p), Some(i)) =
                        (self.spot_drag, response.interact_pointer_pos(), self.selected_spot)
                    {
                        let (a, b) = (mapping.to_sensor(from), mapping.to_sensor(p));
                        let d = [b[0] - a[0], b[1] - a[1]];
                        let mut moved = start;
                        let target = if is_src { &mut moved.src } else { &mut moved.dst };
                        *target = [target[0] + d[0], target[1] + d[1]];
                        if let Some(spot) = self.stack.heal.params.spots.get_mut(i) {
                            if *spot != moved {
                                *spot = moved;
                                self.needs_render = true;
                            }
                        }
                    }
                    if response.clicked() {
                        if let Some(p) = response.interact_pointer_pos() {
                            match hit(p) {
                                Some((i, _)) => self.selected_spot = Some(i),
                                None if frame_now.contains(p)
                                    && spots.len() < photo_engine::heal::MAX_SPOTS =>
                                {
                                    self.pending_spot =
                                        Some((mapping.to_sensor(p), self.spot_size / 100.0));
                                }
                                None => {}
                            }
                        }
                    }
                    if let Some(p) = response.hover_pos() {
                        ui.ctx().set_cursor_icon(if hit(p).is_some() {
                            egui::CursorIcon::Grab
                        } else {
                            egui::CursorIcon::Crosshair
                        });
                    }
                }
                if response.drag_stopped() {
                    self.mask_drag = None;
                    self.painting = None;
                    self.spot_drag = None;
                }
                let mask_dragging =
                    self.mask_drag.is_some() || self.painting.is_some() || self.spot_drag.is_some();

                // With nothing to pan at fit, a drag anywhere on the image moves the divider.
                if self.split_active
                    && !self.cropping
                    && at_fit
                    && response.dragged()
                    && !mask_dragging
                {
                    move_divider_to = response.interact_pointer_pos();
                }
                self.split_dragging = move_divider_to.is_some();
                if let Some(p) = move_divider_to {
                    let x = p.x.clamp(dest_now.left(), dest_now.right());
                    let fraction = ((x - frame_now.left()) / frame_now.width()).clamp(0.0, 1.0);
                    if (fraction - self.split_pos).abs() > 1e-4 {
                        self.split_pos = fraction;
                        self.needs_render = true;
                    }
                }

                // The eyedropper: the next click on the photo picks white balance there.
                if self.picking_wb && !self.cropping {
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    }
                    if response.clicked() {
                        if let Some(p) = response.interact_pointer_pos() {
                            if frame_now.contains(p) {
                                let n = (p - frame_now.min) / frame_now.size();
                                self.pending_sample = Some(crate::Sample::Pick(pos2(n.x, n.y)));
                                self.picking_wb = false;
                            }
                        }
                    }
                }

                // Targeted Adjustment Tool (TAT): click/drag on photo to adjust the targeted color band
                if let Some(mode) = self.tat_mode {
                    if !self.cropping {
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                        }
                        if response.drag_started() {
                            if let Some(p) = response.interact_pointer_pos() {
                                if frame_now.contains(p) {
                                    let n = (p - frame_now.min) / frame_now.size();
                                    self.pending_sample = Some(crate::Sample::Tat(pos2(n.x, n.y), mode, p));
                                }
                            }
                        }
                        if let Some(drag) = &mut self.tat_drag {
                            if response.dragged() {
                                if let Some(p) = response.interact_pointer_pos() {
                                    let delta_y = drag.start_pos.y - p.y; // drag up is positive
                                    drag.current_delta = delta_y * 0.4;
                                    let target_name = drag.band_name;
                                    let new_val = drag.initial_val + drag.current_delta;
                                    for (name, band) in self.stack.color_eq.params.bands_mut() {
                                        if name == target_name {
                                            match mode {
                                                crate::tat::TatMode::ColorEq(crate::ColorEqTab::Hue) => {
                                                    band.hue = new_val.clamp(-30.0, 30.0);
                                                }
                                                crate::tat::TatMode::ColorEq(crate::ColorEqTab::Saturation) => {
                                                    band.saturation = new_val.clamp(-100.0, 100.0);
                                                }
                                                crate::tat::TatMode::ColorEq(crate::ColorEqTab::Luminance) => {
                                                    band.luminance = new_val.clamp(-100.0, 100.0);
                                                }
                                            }
                                        }
                                    }
                                    self.needs_render = true;
                                }
                            }
                            if response.drag_stopped() {
                                self.history.checkpoint(&self.stack);
                                self.tat_drag = None;
                            }
                        }
                    }
                }
                if self.cropping {
                    // In crop mode a drag frames the picture instead of moving it: on a
                    // handle it moves that edge or corner, inside the crop it slides it,
                    // outside it turns the photo, and on an uncropped frame it draws one.
                    // Nothing is checkpointed per drag - Apply makes the session one step
                    // - except a turn, which is its own step as a quarter turn is.
                    let unclamped = |p: egui::Pos2| {
                        egui::pos2(
                            (p.x - frame_now.left()) / frame_now.width(),
                            (p.y - frame_now.top()) / frame_now.height(),
                        )
                    };
                    let to_norm = |p: egui::Pos2| {
                        let n = unclamped(p);
                        egui::pos2(n.x.clamp(0.0, 1.0), n.y.clamp(0.0, 1.0))
                    };
                    let frame_size = (frame_now.width(), frame_now.height());
                    let tolerance = if widgets::is_touch(ui.ctx()) { 22.0 } else { 10.0 };
                    let current = self.stack.geometry.crop.sanitised();
                    let framed_now = egui::Rect::from_min_size(
                        frame_now.min
                            + egui::vec2(current.x * frame_now.width(), current.y * frame_now.height()),
                        egui::vec2(
                            current.width * frame_now.width(),
                            current.height * frame_now.height(),
                        ),
                    );
                    let shift = ui.input(|i| i.modifiers.shift);
                    let ratio_now = |c: Crop| {
                        self.crop_aspect.pixel_ratio(frame_size).or_else(|| {
                            shift.then(|| (c.width * frame_size.0) / (c.height * frame_size.1))
                        })
                    };
                    let grip_under = |p: egui::Pos2| {
                        crop_tool::grip_at(current, unclamped(p), frame_size, tolerance)
                    };
                    if response.drag_started() {
                        self.crop_drag = response.interact_pointer_pos().map(|p| {
                            let on_frame = frame_now.contains(p);
                            match grip_under(p) {
                                // An uncropped frame is all "inside": a drag across it
                                // draws, and only its edges and corners are handles.
                                Some(crop_tool::Grip::Move) if current.is_full_frame() => {
                                    CropDrag::Draw { anchor: to_norm(p) }
                                }
                                Some(grip) => CropDrag::Grip {
                                    grip,
                                    grab: unclamped(p),
                                    start: current,
                                    ratio: ratio_now(current),
                                },
                                None if current.is_full_frame() && on_frame => {
                                    CropDrag::Draw { anchor: to_norm(p) }
                                }
                                None => {
                                    self.history.checkpoint(&self.stack);
                                    let v = p - framed_now.center();
                                    CropDrag::Rotate {
                                        centre: framed_now.center(),
                                        from_angle: v.y.atan2(v.x),
                                        from_deg: self.stack.geometry.straighten_deg,
                                    }
                                }
                            }
                        });
                    }
                    if let (Some(drag), Some(pointer)) =
                        (self.crop_drag, response.interact_pointer_pos())
                    {
                        if response.dragged() || response.drag_stopped() {
                            match drag {
                                CropDrag::Rotate {
                                    centre,
                                    from_angle,
                                    from_deg,
                                } => {
                                    let v = pointer - centre;
                                    let turned = (v.y.atan2(v.x) - from_angle).to_degrees();
                                    // Positive straighten turns the picture clockwise on
                                    // screen, so it follows the hand that turns it.
                                    let deg = (from_deg + turned).clamp(-45.0, 45.0);
                                    if (deg - self.stack.geometry.straighten_deg).abs() > 1e-3 {
                                        self.stack.geometry.straighten_deg = deg;
                                        self.needs_render = true;
                                    }
                                }
                                CropDrag::Draw { anchor } => {
                                    let crop = crop_tool::drawn(
                                        anchor,
                                        to_norm(pointer),
                                        ratio_now(current),
                                        frame_size,
                                    )
                                    .sanitised();
                                    if crop != self.stack.geometry.crop {
                                        self.stack.geometry.crop = crop;
                                        self.needs_render = true;
                                    }
                                }
                                CropDrag::Grip {
                                    grip,
                                    grab,
                                    start,
                                    ratio,
                                } => {
                                    let crop = crop_tool::dragged(
                                        start,
                                        grip,
                                        grab,
                                        unclamped(pointer),
                                        ratio,
                                        frame_size,
                                    )
                                    .sanitised();
                                    if crop != self.stack.geometry.crop {
                                        self.stack.geometry.crop = crop;
                                        self.needs_render = true;
                                    }
                                }
                            }
                        }
                    }
                    if response.drag_stopped() {
                        self.crop_drag = None;
                    }
                    if let Some(p) = response.hover_pos() {
                        let cursor = match self.crop_drag {
                            Some(CropDrag::Grip { grip, .. }) => grip.cursor(),
                            Some(CropDrag::Rotate { .. }) => egui::CursorIcon::Grabbing,
                            Some(CropDrag::Draw { .. }) => egui::CursorIcon::Crosshair,
                            None => match grip_under(p) {
                                Some(crop_tool::Grip::Move) if current.is_full_frame() => {
                                    egui::CursorIcon::Crosshair
                                }
                                Some(grip) => grip.cursor(),
                                None if current.is_full_frame() && frame_now.contains(p) => {
                                    egui::CursorIcon::Crosshair
                                }
                                None => egui::CursorIcon::Grab,
                            },
                        };
                        ui.ctx().set_cursor_icon(cursor);
                    }
                } else if self.cutout_studio.open {
                    let space = ui.input(|i| i.key_down(egui::Key::Space));
                    let is_middle = response.dragged_by(egui::PointerButton::Middle);
                    if (space || is_middle) && response.dragged() && !at_fit {
                        self.view.pan -= response.drag_delta() / scale_now;
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    } else if (response.dragged() || response.clicked()) && !space && !is_middle {
                        if let Some(p) = response.interact_pointer_pos() {
                            if dest_now.contains(p) {
                                let norm_x = ((p.x - dest_now.left()) / dest_now.width()).clamp(0.0, 1.0);
                                let norm_y = ((p.y - dest_now.top()) / dest_now.height()).clamp(0.0, 1.0);
                                let erase = ui.input(|i| i.modifiers.alt)
                                    || response.dragged_by(egui::PointerButton::Secondary);
                                let mode = if erase {
                                    cutout_studio::CutoutBrushMode::Erase
                                } else {
                                    self.cutout_studio.brush_mode
                                };
                                self.cutout_studio.add_touchup_with_mode(ui.ctx(), [norm_x, norm_y], mode);
                            }
                        }
                    }
                } else if response.dragged() && !at_fit && !self.split_dragging && !mask_dragging {
                    self.view.pan -= response.drag_delta() / scale_now;
                }
                let cutout_active = self.cutout_studio.open && !self.cropping;
                // On touch, one recogniser reads the photo's touches: it is the Edit
                // sheet's control, or a sideways flick steps to the next photo (G-02).
                let free = !self.cropping
                    && !mask_dragging
                    && !self.split_dragging
                    && self.selected_mask.is_none()
                    && !self.healing
                    && !cutout_active;
                self.stage_gestures(ui.ctx(), rect, at_fit, free);
                if !widgets::is_touch(ui.ctx()) && !cutout_active {
                    self.desktop_photo_drag(ui.ctx(), &response);
                }

                if let Some(pointer) = response.hover_pos() {
                    // Wheel, plus pinch / Ctrl+wheel, which egui reports as a zoom factor.
                    let (scroll, pinch) = ui
                        .ctx()
                        .input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
                    let factor = (scroll * 0.004).exp() * pinch;
                    if factor != 1.0 {
                        let before = fit * self.view.zoom;
                        let centre = rect.center() - self.view.pan * before;
                        // The image point under the cursor, in image pixels from centre.
                        let anchor = (pointer - centre) / before;

                        self.view.zoom = (self.view.zoom * factor).clamp(1.0, max_zoom);

                        // Keep that point under the cursor rather than zooming to centre.
                        let after = fit * self.view.zoom;
                        self.view.pan = (rect.center() - pointer) / after + anchor;
                    }
                }

                let scale = fit * self.view.zoom;
                // Pan only as far as the image still covers the view: at fit that is
                // nowhere, so the frame cannot be knocked off-centre by a stray drag.
                let slack = ((img * scale - rect.size()) * 0.5).max(egui::Vec2::ZERO) / scale;
                self.view.pan = self.view.pan.clamp(-slack, slack);

                let dest = egui::Rect::from_center_size(
                    rect.center() - self.view.pan * scale,
                    img * scale,
                );
                let frame = frame_of(dest);
                self.stage_frame = Some(frame);
                let painter = ui.painter().with_clip_rect(rect);
                painter.image(
                    id,
                    dest,
                    egui::Rect::from_min_size(
                        egui::pos2(region.x, region.y),
                        egui::vec2(region.width, region.height),
                    ),
                    egui::Color32::WHITE,
                );
                // The cutout is drawn over the photo, not in place of it: its preview is
                // ~1400px, so the kept subject is left to the stage's image, which stays
                // sharp however far in it zooms. The preview is of the crop alone.
                if let (true, Some(tex)) = (self.cutout_studio.open, &self.cutout_studio.preview_texture) {
                    painter.image(
                        tex.id(),
                        dest,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                // The frame the image sits in: a hairline, not a shadow.
                let framed_view = dest.intersect(rect);
                painter.rect_stroke(
                    framed_view,
                    0.0,
                    Stroke::new(1.0, theme::RAIL),
                    egui::StrokeKind::Outside,
                );

                // While framing, the whole picture is shown and the crop drawn over it:
                // everything outside is dimmed, and thirds are marked.
                let crop = self.stack.geometry.crop.sanitised();
                if self.cropping {
                    let framed = egui::Rect::from_min_size(
                        dest.min + egui::vec2(crop.x * dest.width(), crop.y * dest.height()),
                        egui::vec2(crop.width * dest.width(), crop.height * dest.height()),
                    );
                    let veil = egui::Color32::from_black_alpha(170);
                    // Four bands rather than a stencil: cheap, exact, and no extra pass.
                    painter.rect_filled(
                        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), framed.top())),
                        0.0,
                        veil,
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(rect.left(), framed.bottom()),
                            rect.max,
                        ),
                        0.0,
                        veil,
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(rect.left(), framed.top()),
                            egui::pos2(framed.left(), framed.bottom()),
                        ),
                        0.0,
                        veil,
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(framed.right(), framed.top()),
                            egui::pos2(rect.right(), framed.bottom()),
                        ),
                        0.0,
                        veil,
                    );
                    painter.rect_stroke(
                        framed,
                        0.0,
                        egui::Stroke::new(1.0, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                    let thin = egui::Stroke::new(1.0, theme::TEXT.linear_multiply(0.25));
                    let at = |(u, v): (f32, f32)| {
                        framed.min + egui::vec2(u * framed.width(), v * framed.height())
                    };
                    for [a, b] in self.crop_guide.lines() {
                        painter.line_segment([at(a), at(b)], thin);
                    }
                    // Handles: an L at each corner and a bar at each edge's middle.
                    let handle = egui::Stroke::new(3.0, theme::ACCENT);
                    let arm = (framed.width().min(framed.height()) * 0.2).clamp(6.0, 18.0);
                    for (corner, dx, dy) in [
                        (framed.left_top(), 1.0, 1.0),
                        (framed.right_top(), -1.0, 1.0),
                        (framed.left_bottom(), 1.0, -1.0),
                        (framed.right_bottom(), -1.0, -1.0),
                    ] {
                        painter.line_segment([corner, corner + egui::vec2(dx * arm, 0.0)], handle);
                        painter.line_segment([corner, corner + egui::vec2(0.0, dy * arm)], handle);
                    }
                    let half = arm * 0.6;
                    for (mid, horizontal) in [
                        (framed.center_top(), true),
                        (framed.center_bottom(), true),
                        (framed.left_center(), false),
                        (framed.right_center(), false),
                    ] {
                        let d = if horizontal {
                            egui::vec2(half, 0.0)
                        } else {
                            egui::vec2(0.0, half)
                        };
                        painter.line_segment([mid - d, mid + d], handle);
                    }
                    hud_badge(
                        &painter,
                        pos2(framed_view.center().x, framed_view.top() + 12.0),
                        Align2::CENTER_TOP,
                        "CROP · handles resize · inside moves · outside turns · X swaps · O guides · Enter / Esc",
                        theme::ACCENT,
                        theme::ACCENT,
                    );
                }

                // The chosen local mask: its outline and handles, and a pin on every other.
                if let (Some(i), Some(sensor), false) = (self.selected_mask, sensor, self.cropping) {
                    let mapping = crate::mask_tool::Mapping {
                        geometry: self.stack.geometry,
                        sensor,
                        frame,
                    };
                    let dark = Stroke::new(3.0, Color32::from_black_alpha(140));
                    let light = Stroke::new(1.5, theme::TEXT_HI);
                    for (j, m) in self.stack.local.params.masks.iter().enumerate() {
                        if j == i {
                            continue;
                        }
                        let at = mapping.pin(m);
                        painter.circle(at, 5.0, Color32::from_black_alpha(160), light);
                    }
                    if let Some(mask) = self.stack.local.params.masks.get(i) {
                        for (k, line) in mapping.outline(mask).into_iter().enumerate() {
                            // The inner ring (feather) and a gradient's outer lines, fainter.
                            let faint = k != 0 && matches!(mask.shape, photo_engine::local::Shape::Radial { .. })
                                || k != 1 && matches!(mask.shape, photo_engine::local::Shape::Linear { .. });
                            let stroke = if faint {
                                Stroke::new(1.0, theme::TEXT_HI.linear_multiply(0.6))
                            } else {
                                light
                            };
                            painter.add(egui::Shape::line(line.clone(), dark));
                            painter.add(egui::Shape::line(line, stroke));
                        }
                        // The brush, where it would paint.
                        if let (photo_engine::local::Shape::Brush { .. }, Some(p)) =
                            (&mask.shape, response.hover_pos())
                        {
                            let (ow, _) = self.stack.geometry.output_size(sensor.0, sensor.1);
                            let per_px = frame.width() / ow as f32;
                            let long = sensor.0.max(sensor.1) as f32;
                            let r = self.brush.size / 100.0 * long * per_px;
                            let colour = if self.brush.erase { theme::DANGER } else { theme::TEXT_HI };
                            painter.circle_stroke(p, r, dark);
                            painter.circle_stroke(p, r, Stroke::new(1.5, colour));
                            let inner = r * (1.0 - self.brush.feather / 100.0).clamp(0.0, 1.0);
                            if inner > 2.0 {
                                painter.circle_stroke(p, inner, Stroke::new(1.0, colour.linear_multiply(0.5)));
                            }
                        }
                        for (handle, at) in mapping.handles(mask) {
                            let r = if handle == crate::mask_tool::Handle::Move { 6.0 } else { 5.0 };
                            painter.circle(at, r, theme::ACCENT, Stroke::new(1.5, Color32::BLACK));
                        }
                    }
                    hud_badge(
                        &painter,
                        pos2(framed_view.center().x, framed_view.top() + 12.0),
                        Align2::CENTER_TOP,
                        if matches!(
                            self.stack.local.params.masks.get(i).map(|m| &m.shape),
                            Some(photo_engine::local::Shape::Brush { .. })
                        ) {
                            "BRUSH · paint to add · Alt or right-drag erases · Esc puts it away"
                        } else {
                            "MASK · drag the handles · Esc puts them away"
                        },
                        theme::ACCENT,
                        theme::ACCENT,
                    );
                }

                // Heal spots: every spot's circle, and the chosen one's source joined to it.
                if let (true, Some(sensor), false) = (self.healing, sensor, self.cropping) {
                    let mapping = crate::mask_tool::Mapping {
                        geometry: self.stack.geometry,
                        sensor,
                        frame,
                    };
                    let (ow, _) = self.stack.geometry.output_size(sensor.0, sensor.1);
                    let per_px = frame.width() / ow as f32;
                    let long = sensor.0.max(sensor.1) as f32;
                    let dark = Stroke::new(3.0, Color32::from_black_alpha(140));
                    for (i, spot) in self.stack.heal.params.spots.iter().enumerate() {
                        let r = spot.radius * long * per_px;
                        let dst = mapping.to_screen(spot.dst);
                        let chosen = self.selected_spot == Some(i);
                        let colour = if chosen { theme::ACCENT } else { theme::TEXT_HI };
                        painter.circle_stroke(dst, r, dark);
                        painter.circle_stroke(dst, r, Stroke::new(1.5, colour));
                        if chosen && spot.mode != photo_engine::heal::Mode::Remove {
                            let src = mapping.to_screen(spot.src);
                            let soft = Stroke::new(1.2, colour.linear_multiply(0.7));
                            painter.circle_stroke(src, r, dark);
                            painter.circle_stroke(src, r, soft);
                            let d = dst - src;
                            if d.length() > 2.0 * r {
                                let n = d.normalized();
                                painter.line_segment([src + n * r, dst - n * r], soft);
                            }
                        }
                    }
                    hud_badge(
                        &painter,
                        pos2(framed_view.center().x, framed_view.top() + 12.0),
                        Align2::CENTER_TOP,
                        "HEAL · tap a spot · drag a circle or its source · Esc stops",
                        theme::ACCENT,
                        theme::ACCENT,
                    );
                }

                if self.picking_wb {
                    hud_badge(
                        &painter,
                        pos2(framed_view.center().x, framed_view.top() + 12.0),
                        Align2::CENTER_TOP,
                        "WHITE BALANCE · click something that should be grey · Esc cancels",
                        theme::ACCENT,
                        theme::ACCENT,
                    );
                }

                if let Some(mode) = self.tat_mode {
                    let label = match mode {
                        crate::tat::TatMode::ColorEq(tab) => format!("TARGETED ADJUSTMENT ({tab:?}) · Drag up/down on photo colors · Esc cancels"),
                    };
                    if let Some(drag) = &self.tat_drag {
                        let val = drag.initial_val + drag.current_delta;
                        let text = format!("🎯 {} {}: {:+.1}", drag.band_name, match mode {
                            crate::tat::TatMode::ColorEq(tab) => format!("{tab:?}"),
                        }, val);
                        hud_badge(
                            &painter,
                            pos2(framed_view.center().x, framed_view.top() + 12.0),
                            Align2::CENTER_TOP,
                            &text,
                            theme::ACCENT,
                            theme::ACCENT,
                        );
                    } else {
                        hud_badge(
                            &painter,
                            pos2(framed_view.center().x, framed_view.top() + 12.0),
                            Align2::CENTER_TOP,
                            &label,
                            theme::ACCENT,
                            theme::ACCENT,
                        );
                    }
                }

                if self.split_active {
                    let split_screen_x = (frame.left() + self.split_pos * frame.width())
                        .clamp(dest.left(), dest.right());
                    let (top, bottom) = (framed_view.top(), framed_view.bottom());
                    painter.vline(
                        split_screen_x,
                        top..=bottom,
                        Stroke::new(2.0, theme::ACCENT),
                    );

                    let badge_y = top + 14.0;
                    if self.split_pos > 0.08 {
                        hud_badge(
                            &painter,
                            pos2(split_screen_x - 8.0, badge_y),
                            Align2::RIGHT_TOP,
                            "BEFORE",
                            theme::TEXT_HI,
                            Color32::from_gray(0x2a),
                        );
                    }
                    if self.split_pos < 0.92 {
                        hud_badge(
                            &painter,
                            pos2(split_screen_x + 8.0, badge_y),
                            Align2::LEFT_TOP,
                            "AFTER",
                            theme::ACCENT,
                            theme::ACCENT,
                        );
                    }

                    let handle = pos2(split_screen_x, (top + bottom) / 2.0);
                    painter.circle(handle, 15.0, theme::BG_950, Stroke::new(2.0, theme::ACCENT));
                    painter.text(
                        handle,
                        Align2::CENTER_CENTER,
                        "⇆",
                        FontId::proportional(15.0),
                        theme::ACCENT,
                    );
                }

                // Corner readouts, only while the pointer is over the photo and moving: the
                // UI must not sit on the media while it is being looked at (DESIGN.md).
                let still_for = ui.ctx().input(|i| i.pointer.time_since_last_movement());
                let hud = response.hovered() && still_for < HUD_LINGER;
                if response.hovered() && still_for < HUD_LINGER {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs_f32(
                            HUD_LINGER - still_for,
                        ));
                }
                // What exports is exactly this texture, cropped; only the display scale
                // differs.
                if hud && framed_view.width() > 420.0 {
                    let (_, _, w, h) = crop.to_pixels(image_w, image_h);
                    hud_badge(
                        &painter,
                        framed_view.left_bottom() + vec2(12.0, -12.0),
                        Align2::LEFT_BOTTOM,
                        &format!(
                            "exports these pixels · {w} × {h} · shown at {:.0}%",
                            scale * 100.0
                        ),
                        Color32::from_gray(0xcc),
                        theme::RAIL,
                    );
                }
                if let Some((path, _)) = &self.loading {
                    hud_badge(
                        &painter,
                        framed_view.left_top() + vec2(12.0, 12.0),
                        Align2::LEFT_TOP,
                        &format!("opening {}…", file_name(path)),
                        theme::ACCENT,
                        theme::ACCENT,
                    );
                }
                let hist = self.engine.histogram();
                if hud && hist.peak > 0 {
                    let clipped = hist.clipped_high().max(hist.clipped_low()) * 100.0;
                    let (text, colour) = if clipped >= 0.1 {
                        (format!("●  clipped {clipped:.1}%"), theme::DANGER)
                    } else {
                        (format!("clipped {clipped:.1}%"), theme::TEXT_DIM)
                    };
                    hud_badge(
                        &painter,
                        framed_view.right_bottom() + vec2(-12.0, -12.0),
                        Align2::RIGHT_BOTTOM,
                        &text,
                        colour,
                        theme::RAIL,
                    );
                }

                self.paint_photo_overlay(ui, rect);

                if cutout_active {
                    if let Some(p) = response.hover_pos() {
                        if dest.contains(p) {
                            let space = ui.input(|i| i.key_down(egui::Key::Space));
                            if space {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                            } else {
                                let brush_screen_r = self.cutout_studio.brush_radius * dest.width().max(dest.height());
                                let erase = ui.input(|i| i.modifiers.alt);
                                let color = if erase || self.cutout_studio.brush_mode == cutout_studio::CutoutBrushMode::Erase {
                                    Color32::from_rgb(0xf8, 0x71, 0x71)
                                } else {
                                    Color32::from_rgb(0x4a, 0xde, 0x80)
                                };
                                painter.circle_stroke(p, brush_screen_r, Stroke::new(2.5, Color32::from_black_alpha(150)));
                                painter.circle_stroke(p, brush_screen_r, Stroke::new(1.2, color));
                                painter.circle_filled(p, 1.5, color);
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                            }
                        }
                    }
                }

                if self.cutout_studio.open {
                    let badge = if self.layout.is_compact() {
                        "CUTOUT · paint to keep or erase"
                    } else {
                        "CUTOUT STUDIO · Paint on photo to Keep/Erase · Alt erases · Space+drag pans · Esc closes"
                    };
                    hud_badge(
                        &painter,
                        pos2(framed_view.center().x, framed_view.top() + 12.0),
                        Align2::CENTER_TOP,
                        badge,
                        theme::ACCENT,
                        theme::ACCENT,
                    );
                }

                // The phone shows the studio's controls in its Tools sheet instead.
                if self.cutout_studio.open && !self.layout.is_compact() {
                    let mut export_clicked = false;
                    self.cutout_studio.show_hud(ui.ctx(), &mut export_clicked);
                    if export_clicked {
                        self.export_cutout(ui.ctx());
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.cutout_studio.open = false;
                    }
                }

                if self.cropping {
                    if ui.ctx().output(|o| o.cursor_icon) != egui::CursorIcon::Move {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    }
                } else if self.split_dragging {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                } else if !at_fit && response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if !at_fit && response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
            });
    }

    /// No image yet: a drop target that is also the Open button, and the reason the last
    /// file failed, in place (DESIGN.md: failures surface in the UI, with the filename).
    pub(crate) fn empty_stage(&mut self, ui: &mut egui::Ui) {
        let area = ui.available_rect_before_wrap();
        let zone = Rect::from_center_size(
            area.center(),
            vec2((area.width() - 32.0).clamp(160.0, 440.0), 180.0),
        );
        let response = ui.interact(zone, ui.id().with("drop_zone"), Sense::click());
        let dragging_in = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let hot = response.hovered() || dragging_in;

        let painter = ui.painter();
        widgets::dashed_rect(
            painter,
            zone,
            if hot {
                theme::ACCENT
            } else {
                Color32::from_gray(0x2a)
            },
        );
        if let Some((path, _)) = &self.loading {
            painter.text(
                zone.center(),
                Align2::CENTER_CENTER,
                format!("decoding {}…", file_name(path)),
                FontId::new(13.0, theme::mono_bold()),
                theme::ACCENT,
            );
            return;
        }
        let c = zone.center();
        let touch = widgets::is_touch(ui.ctx());
        painter.text(
            c - vec2(0.0, 44.0),
            Align2::CENTER_CENTER,
            "⬆",
            FontId::proportional(22.0),
            theme::ACCENT,
        );
        painter.text(
            c - vec2(0.0, 12.0),
            Align2::CENTER_CENTER,
            if touch {
                "Open a photo"
            } else {
                "Drop a camera RAW file"
            },
            FontId::new(15.0, theme::sans_strong()),
            theme::TEXT_HI,
        );
        painter.text(
            c + vec2(0.0, 10.0),
            Align2::CENTER_CENTER,
            if touch {
                "tap here to choose one"
            } else {
                "or click to open one"
            },
            FontId::proportional(12.5),
            theme::TEXT_DIM,
        );
        painter.text(
            c + vec2(0.0, 40.0),
            Align2::CENTER_CENTER,
            "ARW · CR2 · CR3 · NEF · RAF · ORF · RW2 · DNG",
            FontId::monospace(10.5),
            theme::TEXT_FAINT,
        );
        if let Some(err) = &self.error {
            let galley = painter.layout(
                err.clone(),
                FontId::monospace(11.5),
                theme::DANGER,
                zone.width(),
            );
            painter.galley(
                pos2(
                    zone.center().x - galley.size().x / 2.0,
                    zone.bottom() + 16.0,
                ),
                galley,
                theme::DANGER,
            );
        }
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            self.pick_and_open(ui.ctx());
        }
    }

    /// Export the cutout as the studio has it: a PNG, transparent or on catalog white,
    /// with its edge settings and touch-ups.
    pub(crate) fn export_cutout(&mut self, ctx: &egui::Context) {
        self.export_settings.format = photo_engine::ExportFormat::Png;
        self.export_settings.remove_background = true;
        self.export_settings.cutout_params = self.cutout_studio.params;
        self.export_settings.matte_color = if self.cutout_studio.export_matte_white {
            Some([255, 255, 255])
        } else {
            None
        };
        self.pick_and_export(ctx);
    }

    /// Prepare Cutout Studio with the current photo: renders an interactive ~1400px base image
    /// directly from the GPU engine using the current stack edits (exposure, white balance, tone curve, crop),
    /// falling back to raw embedded preview or raster file loader if the GPU engine is not ready.
    pub(crate) fn prepare_cutout_studio(&mut self, ctx: &egui::Context, gpu: &Gpu) {
        let Some(path) = self.loaded_path.clone() else {
            return;
        };
        if self.cutout_studio.cached_source_path.as_deref() == Some(&path)
            && self.cutout_studio.cached_pred_320.is_some()
        {
            return;
        }

        let rendered = if self.ready_image().is_some() {
            let (crop_w, crop_h) = if let Some(image) = self.engine.image() {
                let (out_w, out_h) = self
                    .stack
                    .geometry
                    .output_size(image.sensor_width, image.sensor_height);
                let (_, _, w, h) = self.stack.geometry.crop.to_pixels(out_w, out_h);
                (w, h)
            } else {
                (1400, 1400)
            };

            let scale = 1400.0 / (crop_w.max(crop_h) as f32).max(1.0);
            let target_size = if scale < 1.0 {
                Some((
                    ((crop_w as f32 * scale).round() as u32).max(1),
                    ((crop_h as f32 * scale).round() as u32).max(1),
                ))
            } else {
                None
            };

            match self.engine.export_sized_in(
                gpu.device,
                gpu.queue,
                &self.stack,
                photo_engine::export::BitDepth::Eight,
                photo_engine::gamut::OutputSpace::Srgb,
                target_size,
            ) {
                Ok(photo_engine::export::Exported::Eight(exported)) => {
                    self.needs_render = true; // export_sized_in overwrote preview uniforms; put them back
                    image::RgbaImage::from_raw(exported.width, exported.height, exported.rgba)
                        .map(image::DynamicImage::ImageRgba8)
                }
                _ => {
                    self.needs_render = true;
                    None
                }
            }
        } else {
            None
        };

        if let Some(dyn_img) = rendered {
            self.cutout_studio.prepare_with_image(ctx, &path, dyn_img);
        } else {
            // Fallback to loading from disk (handles camera RAW embedded preview or raster formats)
            self.cutout_studio.prepare(ctx, &path);
        }
    }
}
