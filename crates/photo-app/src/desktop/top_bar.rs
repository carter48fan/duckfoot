//! The desktop top bar: brand, barrel tabs, the photo pill, open and export.

use crate::*;

/// The top bar's line: the brand's two lines of text, the tallest thing on it.
const TOP_BAR_LINE: f32 = 32.0;

impl PhotoApp {
    // ── Top bar ─────────────────────────────────────────────────────────────────────────

    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(14, 9)),
            )
            // Exactly its line and margins: grown by the spacing after its last row it
            // would sit its items above its middle.
            .exact_size(TOP_BAR_LINE + 18.0)
            .resizable(false)
            .show(ui, |ui| {
                widgets::bar_line(ui, TOP_BAR_LINE, |ui| {
                    widgets::brand_mark(ui, 26.0);
                    ui.add_space(2.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(
                            RichText::new("DUCKFOOT")
                                .family(theme::display())
                                .size(15.0)
                                .extra_letter_spacing(1.4)
                                .color(theme::TEXT_HI),
                        );
                        ui.label(
                            RichText::new("THREE BARRELS, ONE STOCK")
                                .monospace()
                                .size(9.0)
                                .extra_letter_spacing(0.6)
                                .color(Color32::from_gray(0xcc)),
                        );
                    });
                    ui.add_space(12.0);

                    // The other two barrels exist in the design, not in the code. Showing
                    // them disabled is honest; showing them clickable would not be.
                    pod(ui, |ui| {
                        let _ = widgets::chip(ui, true, "photo");
                        ui.add_enabled_ui(false, |ui| {
                            widgets::chip(ui, false, "audio")
                                .on_disabled_hover_text("Barrel 2 is design only — see DESIGN.md");
                            widgets::chip(ui, false, "video")
                                .on_disabled_hover_text("Barrel 3 is design only — see DESIGN.md");
                        });
                    });

                    if ui.available_width() > 720.0 {
                        ui.add_space(10.0);
                        self.telemetry_pill(ui);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let busy = matches!(self.export, Export::Writing { .. });
                        let ready = self.ready_image().is_some() && !busy;
                        let export = ui
                            .add_enabled_ui(ready, |ui| {
                                widgets::button(ui, Kind::Primary, "Export", 84.0).on_hover_text(
                                    "Choose size, format and metadata, then where (Ctrl+E)",
                                )
                            })
                            .inner;
                        if export.clicked() {
                            self.export_dialog_open = true;
                        }
                        if widgets::button(ui, Kind::Secondary, "Open RAW…", 0.0).clicked() {
                            self.pick_and_open(ui.ctx());
                        }
                        ui.add_space(4.0);

                        let cutout_active = self.cutout_studio.open;
                        let cutout_ready = self.loaded_path.is_some();
                        let cutout_btn = ui.add_enabled_ui(cutout_ready, |ui| {
                            widgets::button(
                                ui,
                                if cutout_active { Kind::Primary } else { Kind::Secondary },
                                "✂ Cutout",
                                0.0,
                            ).on_hover_text("Open Cutout Studio for background removal & touch-up")
                        }).inner;
                        if cutout_btn.clicked() {
                            self.cutout_studio.open = !self.cutout_studio.open;
                        }
                        ui.add_space(4.0);

                        if widgets::icon(ui, "?", self.shortcuts_open, "Shortcuts (? or F1)")
                            .clicked()
                        {
                            self.shortcuts_open = !self.shortcuts_open;
                        }

                        // Right-to-left, so these read shelf · split · inspector on screen.
                        pod(ui, |ui| {
                            let wide = self.inspector_width >= 400.0;
                            if widgets::icon(
                                ui,
                                "◨",
                                wide,
                                if wide {
                                    "Narrow the inspector (300px)"
                                } else {
                                    "Widen the inspector for the 3D views (460px)"
                                },
                            )
                            .clicked()
                            {
                                let target = if wide { 300.0 } else { 460.0 };
                                self.inspector_width = target;
                                self.inspector_force_width = Some(target);
                            }
                            if widgets::icon(
                                ui,
                                "◫",
                                self.split_active,
                                "Before / after split (\\)",
                            )
                            .clicked()
                            {
                                self.split_active = !self.split_active;
                                self.needs_render = true;
                            }
                            if widgets::icon(ui, "◧", self.shelf_open, "Show or hide the shelf")
                                .clicked()
                            {
                                self.shelf_open = !self.shelf_open;
                            }
                        });
                    });
                });
            });
    }

    /// The session readout: camera, dimensions, undo depth. Real values only.
    pub(crate) fn telemetry_pill(&self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .stroke(Stroke::new(1.0, Color32::from_gray(0x2a)))
            .corner_radius(theme::RADIUS)
            .inner_margin(Margin::symmetric(12, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let sep = |ui: &mut egui::Ui| {
                        ui.label(
                            RichText::new("|")
                                .monospace()
                                .color(Color32::from_gray(0x33)),
                        );
                    };
                    match self.ready_image() {
                        Some(image) => {
                            widgets::dot(ui, theme::ACCENT);
                            let camera = format!("{} {}", image.make, image.model);
                            ui.label(
                                RichText::new(camera.trim())
                                    .family(theme::mono_bold())
                                    .size(11.5)
                                    .color(theme::ACCENT),
                            );
                            sep(ui);
                            ui.label(
                                theme::num(format!("{} × {}", image.width, image.height))
                                    .size(11.5),
                            );
                            sep(ui);
                            ui.label(
                                theme::num(format!("{:.1} MP", image.megapixels()))
                                    .size(11.5)
                                    .color(Color32::from_gray(0xcc)),
                            );
                            sep(ui);
                            ui.label(
                                theme::num(format!("undo ({})", self.history.past.len()))
                                    .size(11.5)
                                    .color(theme::TEXT_DIM),
                            );
                        }
                        None => {
                            widgets::dot(ui, theme::TEXT_FAINT);
                            ui.label(
                                theme::num("no image loaded")
                                    .size(11.5)
                                    .color(theme::TEXT_DIM),
                            );
                        }
                    }
                });
            });
    }
}
