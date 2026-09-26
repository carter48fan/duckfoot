//! The inspector's Customize tab and its footer: format line and sidecar buttons.

use crate::*;

impl PhotoApp {
    pub(crate) fn customize_body(&mut self, ui: &mut egui::Ui) {
        self.ui_scale_row(ui);
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if widgets::button(ui, Kind::Secondary, "Reset order", 0.0)
                .on_hover_text("Restore the default pipeline order")
                .clicked()
            {
                self.inspector_sections = InspectorSectionConfig::default_sections();
            }
            if widgets::button(ui, Kind::Secondary, "Show all", 0.0).clicked() {
                for s in &mut self.inspector_sections {
                    s.visible = true;
                }
            }
            if widgets::button(ui, Kind::Secondary, "Essentials", 0.0)
                .on_hover_text("Scope, crop, white balance, exposure and filmic only")
                .clicked()
            {
                for s in &mut self.inspector_sections {
                    s.visible = matches!(
                        s.id,
                        InspectorSectionId::Scope
                            | InspectorSectionId::Geometry
                            | InspectorSectionId::WhiteBalance
                            | InspectorSectionId::Exposure
                            | InspectorSectionId::Filmic3D
                    );
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            widgets::checkbox(ui, &mut self.single_open);
            ui.label(
                RichText::new("One module open at a time")
                    .size(12.5)
                    .color(theme::TEXT),
            );
        })
        .response
        .on_hover_text("Opening a module closes the others. Shift-click to open one alongside.");
        ui.add_space(2.0);
        ui.label(
            theme::tag("order is display only · the pipeline itself is fixed")
                .color(theme::TEXT_FAINT),
        );
        ui.add_space(2.0);

        let mut move_act = None;
        let total = self.inspector_sections.len();
        for (i, sec) in self.inspector_sections.iter_mut().enumerate() {
            widgets::card(ui, false, |ui| {
                ui.horizontal(|ui| {
                    widgets::checkbox(ui, &mut sec.visible);
                    let colour = if sec.visible {
                        theme::TEXT_HI
                    } else {
                        theme::TEXT_DIM
                    };
                    ui.label(
                        RichText::new(sec.id.title())
                            .family(theme::sans_strong())
                            .size(13.0)
                            .color(colour),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if sec.id == InspectorSectionId::Scope {
                            ui.label(theme::tag("pinned").color(theme::TEXT_FAINT));
                            return;
                        }
                        ui.add_enabled_ui(i + 1 < total, |ui| {
                            if widgets::icon(ui, "↓", false, "Move down").clicked() {
                                move_act = Some((i, 1));
                            }
                        });
                        ui.add_enabled_ui(i > 1, |ui| {
                            if widgets::icon(ui, "↑", false, "Move up").clicked() {
                                move_act = Some((i, -1));
                            }
                        });
                    });
                });
            });
        }

        if let Some((idx, dir)) = move_act {
            let new_idx = (idx as isize + dir) as usize;
            if new_idx < self.inspector_sections.len() {
                self.inspector_sections.swap(idx, new_idx);
            }
        }
    }

    pub(crate) fn inspector_footer(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("inspector_footer")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(12, 10)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                self.export_format_line(ui);
                self.sidecar_buttons(ui);
                ui.label(
                    RichText::new("edits are non-destructive · the RAW is never written")
                        .monospace()
                        .size(10.0)
                        .color(theme::TEXT_DIM),
                );
            });
    }

    /// "PNG / JPEG · 8-BIT · sRGB ········ 6048 × 4024": what an export writes.
    pub(crate) fn export_format_line(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Not `theme::tag`: that uppercases, and it is sRGB, not SRGB.
            ui.label(
                RichText::new("PNG / JPEG · 8-BIT · sRGB")
                    .family(theme::mono_bold())
                    .size(10.5)
                    .extra_letter_spacing(0.3)
                    .color(theme::TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(image) = self.ready_image() {
                    let (_, _, w, h) = self
                        .stack
                        .geometry
                        .crop
                        .sanitised()
                        .to_pixels(image.width, image.height);
                    ui.label(
                        RichText::new(format!("{w} × {h}"))
                            .family(theme::mono_bold())
                            .size(11.5)
                            .color(theme::ACCENT),
                    );
                }
            });
        });
    }

    /// SAVE SIDECAR and REVERT, filling the width.
    pub(crate) fn sidecar_buttons(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let loaded = self.loaded_path.is_some();
            let has_sidecar = self.saved_stack.is_some();
            let revert_w = 76.0;
            let w = ui.available_width() - revert_w - ui.spacing().item_spacing.x;
            ui.add_enabled_ui(loaded, |ui| {
                if widgets::button(ui, Kind::Accent, "SAVE SIDECAR  →", w)
                    .on_hover_text("Write this stack beside the RAW (Ctrl+S)")
                    .clicked()
                {
                    self.save_current_sidecar();
                }
            });
            ui.add_enabled_ui(loaded && has_sidecar, |ui| {
                if widgets::button(ui, Kind::Secondary, "REVERT", revert_w)
                    .on_hover_text("Reload the saved sidecar (undoable)")
                    .clicked()
                {
                    self.revert_to_sidecar();
                }
            });
        });
    }

    /// Text size for the whole interface: S M L XL XXL.
    pub(crate) fn ui_scale_row(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::tag("text size").color(theme::TEXT_DIM));
            for (scale, label) in settings::UI_SCALES {
                let on = (self.settings.ui_scale - scale).abs() < 0.01;
                if widgets::chip(ui, on, label).clicked() {
                    chosen = Some(scale);
                }
            }
        });
        if let Some(scale) = chosen {
            self.set_ui_scale(scale);
        }
    }
}
