//! The desktop status bar.

use crate::*;

impl PhotoApp {
    // ── Status bar ──────────────────────────────────────────────────────────────────────

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let mut notice_action = None;
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(14, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(path) = &self.loaded_path {
                            ui.label(
                                theme::num(file_name(path))
                                    .size(11.5)
                                    .color(Color32::from_gray(0xcc)),
                            );
                            let (state, colour) = self.sidecar_state().describe();
                            ui.label(theme::tag(state).color(colour));
                            widgets::dot(ui, colour);
                            vsep(ui);
                        }

                        match &self.export {
                            Export::Idle => {}
                            Export::Writing { path, started, .. } => {
                                readout(
                                    ui,
                                    "writing",
                                    &format!(
                                        "{}  {:.1}s",
                                        file_name(path),
                                        started.elapsed().as_secs_f32()
                                    ),
                                    theme::ACCENT,
                                );
                            }
                            Export::Done { path, seconds } => {
                                readout(
                                    ui,
                                    "exported",
                                    &format!("{}  {seconds:.1}s", file_name(path)),
                                    theme::LIME,
                                );
                            }
                            Export::Failed(e) => {
                                ui.label(theme::num(e).size(11.5).color(theme::DANGER));
                            }
                        }

                        if self.ready_image().is_some() {
                            if let Some(percent) = self.zoom_percent(self.last_panel) {
                                readout(ui, "zoom", &format!("{percent:.0}%"), theme::TEXT_HI);
                            }
                            readout(
                                ui,
                                "submit",
                                &format!("{:.2} ms", self.render_ms),
                                theme::TEXT_HI,
                            );
                            readout(
                                ui,
                                "decode",
                                &format!("{:.0} ms", self.decode_ms),
                                theme::TEXT_HI,
                            );
                        }

                        // Whatever width is left, from the left edge: errors win, then the
                        // latest notification.
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if let Some(err) = &self.error {
                                widgets::dot(ui, theme::DANGER);
                                ui.add(
                                    egui::Label::new(
                                        theme::num(err).size(11.5).color(theme::DANGER),
                                    )
                                    .truncate(),
                                );
                            } else if let Some((msg, undo, left)) = self.live_notice() {
                                // Wake to take it down: nothing else may repaint.
                                ui.ctx().request_repaint_after(left);
                                widgets::dot(ui, theme::ACCENT);
                                let msg = msg.to_string();
                                if let Some(action) = undo {
                                    if widgets::chip(ui, true, action.label()).clicked() {
                                        notice_action = Some(action);
                                    }
                                }
                                ui.add(
                                    egui::Label::new(
                                        theme::num(msg).size(11.5).color(theme::ACCENT),
                                    )
                                    .truncate(),
                                );
                            } else {
                                ui.label(theme::tag("ready").color(theme::TEXT_FAINT));
                            }
                        });
                    });
                });
            });
        if let Some(action) = notice_action {
            self.run_notice_action(action);
        }
    }

    /// Display scale as a percentage of 1:1, for the status bar.
    pub(crate) fn zoom_percent(&self, panel: egui::Vec2) -> Option<f32> {
        let img = self.shown_size()?;
        if panel.x < 1.0 || panel.y < 1.0 {
            return None;
        }
        let fit = (panel.x / img.x).min(panel.y / img.y);
        Some(fit * self.view.zoom * 100.0)
    }
}
