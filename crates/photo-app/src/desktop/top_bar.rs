//! The desktop top bar, one line: the logo (the app's one menu), the stage's view tools, the
//! open photo's name, then zoom, undo and redo, and Export.

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
                    self.app_menu(ui);
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

    /// The logo, as a button: the app's one menu. Opening and adding photos, Cutout, Help,
    /// the shortcuts and Preferences, and which barrel this is. The other two barrels exist
    /// in the design, not in the code: listing them disabled is honest; clickable would not
    /// be. A dot on the mark until the tour has been seen, as Help was: offered, never
    /// started for you.
    fn app_menu(&mut self, ui: &mut egui::Ui) {
        let (rect, response) =
            widgets::allocate_control(ui, egui::Vec2::splat(TOP_BAR_LINE), Sense::click());
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if open || response.hovered() {
            ui.painter().rect_filled(rect, 3.0, theme::BG_850);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        widgets::paint_brand_mark(
            ui.painter(),
            Rect::from_center_size(rect.center(), egui::Vec2::splat(22.0)),
        );
        if !self.settings.desk_tour_done {
            ui.painter()
                .circle_filled(rect.right_top() + vec2(-6.0, 6.0), 3.0, theme::LIME);
        }
        widgets::describe(ui, &response, rect, || {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Duckfoot menu")
        });
        let response = response.on_hover_text("Duckfoot: open, Cutout, Help and Preferences");

        let cutout_ready = self.loaded_path.is_some();
        widgets::menu_popup(&response, MENU_W, |ui| {
            if menu_item(ui, "Open RAW…", "", true) {
                self.pick_and_open(ui.ctx());
                ui.close();
            }
            if menu_item(ui, "Add photos…", "", true) {
                self.pick_and_shelve(ui.ctx());
                ui.close();
            }
            if menu_item(ui, "Add a folder…", "", true) {
                self.pick_folder_and_shelve(ui.ctx());
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
            if menu_item(ui, "Preferences…", "Ctrl+,", true) {
                self.preferences_open = true;
                ui.close();
            }
            ui.separator();
            menu_item(ui, "✓ Photo", "", true);
            menu_item(ui, "   Audio", "design only", false);
            menu_item(ui, "   Video", "design only", false);
        });
    }
}

/// The width of the app's menus: wide enough for the longest name and its keys.
pub(crate) const MENU_W: f32 = 220.0;

/// A menu row: `name ········ shortcut`. True when chosen.
pub(crate) fn menu_item(ui: &mut egui::Ui, name: &str, keys: &str, enabled: bool) -> bool {
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
