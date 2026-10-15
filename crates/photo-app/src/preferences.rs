//! Preferences (desktop): the settings that belong to the whole app rather than to a photo,
//! in one place, opened from the top bar's menu or with Ctrl+,.

use crate::*;

impl PhotoApp {
    pub(crate) fn preferences_window(&mut self, ctx: &egui::Context) {
        if !self.preferences_open {
            return;
        }
        let screen = ctx.content_rect();
        // A dimmed backdrop: a click on it closes the window, as Esc does.
        let backdrop = egui::Area::new(egui::Id::new("preferences-backdrop"))
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
        let width = 560.0_f32.min(screen.width() - 32.0);
        egui::Area::new(egui::Id::new("preferences"))
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
                                RichText::new("Preferences")
                                    .family(theme::display())
                                    .size(17.0)
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
                        let max_h = (screen.height() - 140.0).max(200.0);
                        egui::ScrollArea::vertical()
                            .max_height(max_h)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 8.0;
                                close |= self.preferences_body(ui);
                            });
                    });
            });
        if close {
            self.preferences_open = false;
        }
    }

    /// The settings, a heading per kind. True when one of them asks for the window to close
    /// (it opened something else in its place).
    fn preferences_body(&mut self, ui: &mut egui::Ui) -> bool {
        let mut close = false;

        heading(ui, "Interface");
        self.ui_scale_row(ui);
        note(
            ui,
            "Everything grows and shrinks with it: text, controls, and the space inside \
             panels and drawers.",
        );

        heading(ui, "Editing panel");
        ui.horizontal(|ui| {
            widgets::checkbox(ui, &mut self.single_open);
            ui.label(
                RichText::new("One drawer open at a time")
                    .size(12.5)
                    .color(theme::TEXT),
            );
        })
        .response
        .on_hover_text("Opening a drawer closes the others. Shift-click to open one alongside.");
        if widgets::button(ui, Kind::Secondary, "Choose which tools show…", 0.0).clicked() {
            self.tool = Tool::Edit;
            self.customize_open = true;
            close = true;
        }

        heading(ui, "Shelf");
        ui.horizontal(|ui| {
            for (mode, label) in [
                (StripMode::Hidden, "hidden"),
                (StripMode::Strip, "strip"),
                (StripMode::Grid, "grid"),
            ] {
                if widgets::chip(ui, self.strip == mode, label).clicked() {
                    self.strip = mode;
                }
            }
        });
        note(
            ui,
            "Also the gear at the end of the shelf. F hides its photos; G makes it a grid.",
        );
        if widgets::chip(ui, self.settings.xmp_sidecars, "update .xmp sidecars")
            .on_hover_text(
                "Write ratings, labels and keywords into the .xmp sidecar Lightroom or \
                 darktable keeps beside a photo, leaving the rest of it alone. None is made",
            )
            .clicked()
        {
            self.settings.xmp_sidecars = !self.settings.xmp_sidecars;
            self.save_settings();
        }

        heading(ui, "Display");
        self.screen_row(ui);

        heading(ui, "Help");
        if widgets::button(ui, Kind::Secondary, "Take the tour", 0.0)
            .on_hover_text("A minute's walk round the window")
            .clicked()
        {
            self.start_desk_tour();
            close = true;
        }
        close
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(10.0);
    ui.label(theme::tag(text).color(theme::TEXT_DIM));
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(theme::TEXT_DIM));
}
