//! The left rail, and the drawer it opens: folders, history and snapshots.

use crate::*;

impl PhotoApp {
    // ── Shelf bin (left) ────────────────────────────────────────────────────────────────

    /// The left rail. The shelf is one thing, under the stage: the top two buttons show it
    /// as a strip or a grid, or hide it. Below them, the drawer's two views (folders, and
    /// this photo's history and snapshots) and adding photos.
    pub(crate) fn shelf_rail(&mut self, ui: &mut egui::Ui) {
        let rail = egui::Panel::left("shelf_rail")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(8, 10)),
            )
            .exact_size(48.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.vertical_centered(|ui| {
                    let shown = self.strip != StripMode::Hidden;
                    let tip = if shown {
                        "Hide the shelf (F)"
                    } else {
                        "Show the shelf (F)"
                    };
                    if widgets::icon(ui, "▤", shown, tip).clicked() {
                        self.strip = if shown {
                            StripMode::Hidden
                        } else {
                            StripMode::Strip
                        };
                    }
                    let grid = self.strip == StripMode::Grid;
                    let tip = if grid {
                        "Back to the strip"
                    } else {
                        "The shelf as a grid"
                    };
                    if widgets::icon(ui, "▦", grid, tip).clicked() {
                        self.strip = if grid {
                            StripMode::Strip
                        } else {
                            StripMode::Grid
                        };
                    }
                    ui.label(theme::tag(self.shelf.len().to_string()).color(theme::TEXT_FAINT))
                        .on_hover_text(format!("{} photos on the shelf", self.shelf.len()));

                    ui.add_space(4.0);
                    let r = ui.available_rect_before_wrap();
                    ui.painter().hline(
                        (r.center().x - 12.0)..=(r.center().x + 12.0),
                        r.top(),
                        Stroke::new(1.0, theme::BORDER),
                    );
                    ui.add_space(8.0);

                    let folders = self.shelf_open && self.folder_browser_open;
                    if widgets::icon(ui, "🗁", folders, "Folders").clicked() {
                        self.shelf_open = !folders;
                        self.folder_browser_open = true;
                    }
                    let history = self.shelf_open && !self.folder_browser_open;
                    if widgets::icon(ui, "🕘", history, "History and snapshots").clicked() {
                        self.shelf_open = !history;
                        self.folder_browser_open = false;
                    }
                    if widgets::icon(ui, "✚", false, "Add photos to the shelf").clicked() {
                        self.pick_and_shelve(ui.ctx());
                    }
                });
            });
        self.desk_spots.rail = Some(rail.response.rect);
    }

    /// The drawer the rail opens: folders to bring photos in from, or this photo's history
    /// and snapshots. The photos themselves are on the shelf under the stage.
    pub(crate) fn shelf(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        if !self.shelf_open {
            return;
        }
        egui::Panel::left("shelf")
            .frame(egui::Frame::new().fill(theme::BG_950))
            .resizable(true)
            .default_size(272.0)
            .min_size(220.0)
            .max_size(400.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let title = if self.folder_browser_open {
                    "Folders"
                } else {
                    "History"
                };
                widgets::band(ui, |ui| {
                    ui.label(theme::head(title));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon(ui, "×", false, "Close").clicked() {
                            self.shelf_open = false;
                        }
                        if self.folder_browser_open
                            && widgets::icon(ui, "✚", false, "Add photos to the shelf").clicked()
                        {
                            self.pick_and_shelve(ui.ctx());
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(Margin {
                                left: 10,
                                right: 10,
                                top: 8,
                                bottom: 10,
                            })
                            .show(ui, |ui| {
                                if self.folder_browser_open {
                                    self.shelf_folders(ui, gpu);
                                } else {
                                    ui.spacing_mut().item_spacing.y = 10.0;
                                    self.snapshots_group(ui);
                                    self.history_group(ui);
                                }
                            });
                    });
            });
    }

    pub(crate) fn shelf_folders(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        let mut load_folder = None;
        let mut change_folder = None;

        group(
            ui,
            "Bookmarks",
            |_ui| {},
            |ui| {
                for bm in &self.folder_tree.bookmarks {
                    let is_cur = self.folder_tree.current_folder.as_ref() == Some(&bm.path);
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    let painter = ui.painter();
                    if resp.hovered() {
                        painter.rect_filled(rect, 3.0, Color32::from_gray(0x18));
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let col = if is_cur { theme::ACCENT } else { theme::TEXT };
                    painter.text(
                        pos2(rect.left() + 8.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        format!("📁 {}", bm.name),
                        FontId::new(12.0, theme::sans_strong()),
                        col,
                    );
                    if resp.clicked() {
                        change_folder = Some(bm.path.clone());
                    }
                }

                ui.add_space(4.0);
                if widgets::button(ui, Kind::Secondary, "Browse Folder…", 0.0).clicked() {
                    self.request(
                        ui.ctx(),
                        platform::Request::BrowseFolder {
                            from: self.settings.import_dir.clone(),
                        },
                    );
                }
            },
        );

        if let Some(cur) = &self.folder_tree.current_folder.clone() {
            let subs = self.cached_subfolders(cur);
            ui.add_space(8.0);
            let folder_name = cur.file_name().and_then(|n| n.to_str()).unwrap_or("Folder");
            let title = format!("Folder: {folder_name}");
            group(
                ui,
                &title,
                |_ui| {},
                |ui| {
                    ui.label(
                        RichText::new(cur.display().to_string())
                            .family(egui::FontFamily::Monospace)
                            .size(10.5)
                            .color(theme::TEXT_DIM),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if widgets::button(ui, Kind::Primary, "Scan & Load", 0.0)
                            .on_hover_text(
                                "Load all supported images from this folder into the shelf",
                            )
                            .clicked()
                        {
                            load_folder = Some(cur.clone());
                        }
                        if let Some(parent) = cur.parent() {
                            if widgets::button(ui, Kind::Secondary, "⬆ Parent", 0.0).clicked() {
                                change_folder = Some(parent.to_path_buf());
                            }
                        }
                    });

                    ui.add_space(6.0);
                    ui.label(theme::dim("Subfolders:"));
                    if subs.is_empty() {
                        ui.label(theme::dim("No subdirectories").size(11.0));
                    } else {
                        for sub in subs {
                            let name = sub.file_name().and_then(|n| n.to_str()).unwrap_or("dir");
                            let (rect, resp) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 24.0),
                                Sense::click(),
                            );
                            let painter = ui.painter();
                            if resp.hovered() {
                                painter.rect_filled(rect, 2.0, Color32::from_gray(0x18));
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            painter.text(
                                pos2(rect.left() + 8.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                format!("📁 {name}"),
                                FontId::proportional(11.5),
                                theme::TEXT,
                            );
                            if resp.clicked() {
                                change_folder = Some(sub);
                            }
                        }
                    }
                },
            );
        } else {
            ui.add_space(8.0);
            ui.label(theme::dim(
                "Select a bookmark or browse to a session folder to scan photos.",
            ));
        }

        if let Some(cf) = change_folder {
            self.folder_tree.current_folder = Some(cf);
        }
        if let Some(lf) = load_folder {
            self.open_session_folder(gpu, lf);
        }
    }

    pub(crate) fn snapshots_group(&mut self, ui: &mut egui::Ui) {
        let count = self.snapshots.len();
        let mut restore = None;
        let mut delete = None;
        let mut add = false;
        // A snapshot records the framing too, which is only known once the photo decoded.
        let can_snapshot = self.ready_image().is_some();
        group(
            ui,
            "Snapshots",
            |ui| {
                ui.label(theme::tag(count.to_string()).color(theme::TEXT_DIM));
            },
            |ui| {
                list_row(ui, "current", RowStyle::Now, Some(("LIVE", theme::LIME)));
                for (i, (name, stack)) in self.snapshots.iter().enumerate() {
                    let key = snapshot_key(i);
                    let style = if *stack == self.stack {
                        RowStyle::Match
                    } else {
                        RowStyle::Normal
                    };
                    let row = list_row(ui, name, style, Some((&key, theme::TEXT_DIM)));
                    if row.clicked() {
                        restore = Some(i);
                    }
                    row.context_menu(|ui| {
                        if ui.button("Delete snapshot").clicked() {
                            delete = Some(i);
                            ui.close();
                        }
                    });
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    // `desired_width` is the text area alone; the field's own margin and
                    // the gap to the button come on top of it.
                    let field_w = ui.available_width()
                        - 52.0
                        - ui.spacing().item_spacing.x
                        - 2.0 * ui.spacing().button_padding.x;
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.new_snapshot_name)
                            .hint_text("name this state…")
                            .font(egui::TextStyle::Monospace)
                            .desired_width(field_w.max(40.0)),
                    );
                    let submitted =
                        edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if widgets::button_enabled(ui, can_snapshot, Kind::Secondary, "+ save", 52.0)
                        .on_hover_text("Save the current stack as a snapshot")
                        .clicked()
                        || (submitted && can_snapshot)
                    {
                        add = true;
                    }
                });
                ui.add_space(2.0);
            },
        );

        if add {
            let name = std::mem::take(&mut self.new_snapshot_name);
            self.take_snapshot(&name);
        }
        if let Some(i) = restore {
            self.recall_snapshot(i);
        }
        if let Some(i) = delete {
            self.delete_snapshot(i);
        }
    }

    pub(crate) fn history_group(&mut self, ui: &mut egui::Ui) {
        let steps = self.history.timeline(&self.stack).len() - 1;
        group(
            ui,
            "History",
            |ui| {
                ui.label(theme::tag(format!("{steps} steps")).color(theme::ACCENT));
            },
            |ui| self.history_rows(ui),
        );
    }

    /// Every step of the open photo's history, newest first; a click jumps to it.
    pub(crate) fn history_rows(&mut self, ui: &mut egui::Ui) {
        let now = self.history.past.len();
        let mut jump = None;
        {
            let timeline = self.history.timeline(&self.stack);
            // Labels between stored states change only with the history; the two beside
            // the live stack follow it (WP5.4).
            if self.history_labels.0 != self.history.revision {
                let labels = (0..timeline.len())
                    .map(|k| {
                        if k == 0 || k == now || k == now + 1 {
                            String::new()
                        } else {
                            describe_change(timeline[k - 1], timeline[k])
                        }
                    })
                    .collect();
                self.history_labels = (self.history.revision, labels);
            }
            // Rows above NOW are redo steps: not applied, so faint.
            for k in (0..timeline.len()).rev() {
                let label = if k == 0 {
                    if self.history.truncated {
                        "oldest kept"
                    } else {
                        "opened"
                    }
                    .to_string()
                } else if k == now || k == now + 1 {
                    describe_change(timeline[k - 1], timeline[k])
                } else {
                    self.history_labels
                        .1
                        .get(k)
                        .cloned()
                        .unwrap_or_else(|| describe_change(timeline[k - 1], timeline[k]))
                };
                let (style, right) = if k == now {
                    (RowStyle::Now, "NOW".to_string())
                } else if k > now {
                    (RowStyle::Faint, k.to_string())
                } else {
                    (RowStyle::Normal, k.to_string())
                };
                let colour = if k == now {
                    theme::ACCENT
                } else {
                    theme::TEXT_FAINT
                };
                if list_row(ui, &label, style, Some((&right, colour))).clicked() && k != now {
                    jump = Some(k);
                }
            }
        }
        if let Some(k) = jump {
            self.jump_history(k);
        }
    }
}
