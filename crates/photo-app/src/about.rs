//! About: the version, the licence, and every notice the binary must carry
//! (THIRD_PARTY_LICENSES.txt, built in), so they can be read in the app on every
//! platform, a phone included, where a file beside the binary cannot be.

use crate::*;

/// The notices, as `tools/generate_licenses.py` writes them.
const NOTICES: &str = include_str!("../../../THIRD_PARTY_LICENSES.txt");
/// Duckfoot's own licence.
const LICENSE: &str = include_str!("../../../LICENSE");

impl PhotoApp {
    pub(crate) fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(egui::Id::new("about-backdrop"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let r = ui.allocate_rect(screen, Sense::click());
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
                r
            })
            .inner;
        let mut close = backdrop.clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let width = 640.0_f32.min(screen.width() - 32.0);
        let height = (screen.height() - 80.0).max(200.0);
        egui::Area::new(egui::Id::new("about"))
            .order(egui::Order::Tooltip)
            .fade_in(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(4.0)
                    .inner_margin(Margin::same(20))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("Duckfoot {}", crate::VERSION))
                                    .family(theme::display())
                                    .size(16.0)
                                    .color(theme::TEXT_HI),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if widgets::icon(ui, "×", false, "Close (Esc)").clicked() {
                                        close = true;
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);
                        ui.label(theme::dim(
                            "A photo editor that keeps your pictures on your device. Free \
                             software under the MIT licence; it is built on the open-source \
                             work below, under the licences given with it. Not affiliated \
                             with Adobe or any camera maker; their names are used only to say \
                             what Duckfoot opens or works like.",
                        ));
                        ui.add_space(10.0);
                        egui::ScrollArea::vertical()
                            .max_height(height - 140.0)
                            .show(ui, |ui| {
                                for text in [LICENSE, NOTICES] {
                                    ui.label(
                                        RichText::new(text)
                                            .family(egui::FontFamily::Monospace)
                                            .size(11.0)
                                            .color(theme::TEXT_DIM),
                                    );
                                    ui.add_space(12.0);
                                }
                            });
                    });
            });
        if close {
            self.about_open = false;
        }
    }
}
