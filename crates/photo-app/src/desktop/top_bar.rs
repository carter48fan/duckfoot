//! The desktop top bar, one line: the barrel, the stage's view tools, the open photo's name,
//! then zoom, undo and redo, Export and a menu for the rest.

use crate::*;

/// The top bar's line: the zoom pod's height, the tallest thing on it.
const TOP_BAR_LINE: f32 = 36.0;

impl PhotoApp {
    // ── Top bar ─────────────────────────────────────────────────────────────────────────

    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui) {
        let shown = egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(12, 6)),
            )
            // Exactly its line and margins: grown by the spacing after its last row it
            // would sit its items above its middle.
            .exact_size(TOP_BAR_LINE + 12.0)
            .resizable(false)
            .show(ui, |ui| {
                widgets::bar_line(ui, TOP_BAR_LINE, |ui| {
                    self.barrel_menu(ui);
                    ui.add_space(4.0);
                    separator(ui);
                    ui.add_space(4.0);

                    // The stage's view tools: what you look at the photo with.
                    let loaded = self.ready_image().is_some();
                    ui.add_enabled_ui(loaded, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        self.crop_chips(ui);
                        if widgets::chip(ui, self.split_active, "a / b")
                            .on_hover_text(
                                "Before / after split (\\). Drag the divider; double-click it to centre",
                            )
                            .clicked()
                        {
                            self.split_active = !self.split_active;
                            self.needs_render = true;
                        }
                        if widgets::chip(ui, self.clip_overlay, "clip")
                            .on_hover_text("Show clipping: red where blown, blue where crushed (J)")
                            .clicked()
                        {
                            self.clip_overlay = !self.clip_overlay;
                        }
                        self.proof_chips(ui);
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.top_menu(ui);

                        let busy = matches!(self.export, Export::Writing { .. });
                        let ready = loaded && !busy;
                        let export = ui
                            .add_enabled_ui(ready, |ui| {
                                widgets::button(ui, Kind::Primary, "Export", 0.0).on_hover_text(
                                    "Choose size, format and metadata, then where (Ctrl+E)",
                                )
                            })
                            .inner;
                        self.desk_spots.export = Some(export.rect);
                        if export.clicked() {
                            self.export_dialog_open = true;
                        }

                        // Right-to-left, so these read undo · redo on screen.
                        ui.add_enabled_ui(!self.history.future.is_empty(), |ui| {
                            if widgets::icon(ui, "↷", false, "Redo (Ctrl+Shift+Z)").clicked() {
                                self.redo();
                            }
                        });
                        let steps = self.history.past.len();
                        ui.add_enabled_ui(steps > 0, |ui| {
                            let tip = format!("Undo (Ctrl+Z) · {steps} steps");
                            if widgets::icon(ui, "↶", false, &tip).clicked() {
                                self.undo();
                            }
                        });
                        separator(ui);
                        self.zoom_controls(ui, loaded, false);

                        // Whatever is left, centred: which photo this is.
                        ui.allocate_ui_with_layout(
                            ui.available_size(),
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                let name = self
                                    .loaded_path
                                    .as_ref()
                                    .and_then(|p| p.file_name())
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                ui.add(
                                    egui::Label::new(
                                        theme::num(name).size(12.0).color(theme::TEXT_DIM),
                                    )
                                    .truncate(),
                                );
                            },
                        );
                    });
                });
            });
        self.desk_spots.toolbar = Some(shown.response.rect);
    }

    /// The brand mark and which barrel this is. The other two barrels exist in the design,
    /// not in the code: listing them disabled is honest; clickable would not be.
    fn barrel_menu(&mut self, ui: &mut egui::Ui) {
        widgets::brand_mark(ui, 20.0);
        let title = RichText::new("PHOTO ▾")
            .family(theme::mono_bold())
            .size(12.0)
            .extra_letter_spacing(1.0)
            .color(theme::TEXT_HI);
        ui.menu_button(title, |ui| {
            let _ = widgets::chip(ui, true, "photo");
            ui.add_enabled_ui(false, |ui| {
                widgets::chip(ui, false, "audio")
                    .on_disabled_hover_text("Barrel 2 is design only — see DESIGN.md");
                widgets::chip(ui, false, "video")
                    .on_disabled_hover_text("Barrel 3 is design only — see DESIGN.md");
            });
        })
        .response
        .on_hover_text("Duckfoot: three barrels, one stock");
    }

    /// ⋯: opening a photo, Cutout, Help, the shortcuts and Preferences, out of the bar's
    /// way. Lit until the tour has been seen, as Help was: offered, never started for you.
    fn top_menu(&mut self, ui: &mut egui::Ui) {
        let lit = !self.settings.desk_tour_done;
        let dots =
            RichText::new("⋯")
                .size(16.0)
                .color(if lit { theme::ACCENT } else { theme::TEXT });
        let cutout_ready = self.loaded_path.is_some();
        ui.menu_button(dots, |ui| {
            ui.set_min_width(200.0);
            if menu_item(ui, "Open RAW…", "", true) {
                self.pick_and_open(ui.ctx());
                ui.close();
            }
            let cutout = if self.cutout_studio.open {
                "Close Cutout"
            } else {
                "Cutout…"
            };
            if menu_item(ui, cutout, "", cutout_ready) {
                self.cutout_studio.open = !self.cutout_studio.open;
                ui.close();
            }
            ui.separator();
            if menu_item(ui, "Help", "F1", true) {
                self.open_help();
                ui.close();
            }
            if menu_item(ui, "Shortcuts", "?", true) {
                self.shortcuts_open = !self.shortcuts_open;
                ui.close();
            }
            ui.separator();
            if menu_item(ui, "Preferences…", "Ctrl+,", true) {
                self.preferences_open = true;
                ui.close();
            }
        })
        .response
        .on_hover_text("Open, Cutout, Help, shortcuts and Preferences")
        // "⋯" is no name to hear: a reader is told what it is.
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Menu"));
    }
}

/// A menu row: `name ········ shortcut`. True when chosen.
fn menu_item(ui: &mut egui::Ui, name: &str, keys: &str, enabled: bool) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        ui.add(
            egui::Button::new(name)
                .shortcut_text(theme::num(keys).size(11.0).color(theme::TEXT_FAINT))
                .frame(false)
                .min_size(vec2(ui.available_width(), 26.0)),
        )
        .clicked()
    })
    .inner
}

/// A hairline between groups on the bar.
fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, theme::BORDER),
    );
}
