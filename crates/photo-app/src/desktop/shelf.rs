//! The shelf bin (left): photos, folders, snapshots and history.

use crate::*;

impl PhotoApp {
    // ── Shelf bin (left) ────────────────────────────────────────────────────────────────

    /// The shelf folded to a rail of icons, always there. The filmstrip already shows the
    /// roll, so the full shelf is a drawer opened from here for files and folders.
    pub(crate) fn shelf_rail(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("shelf_rail")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_925)
                    .inner_margin(Margin::symmetric(8, 10)),
            )
            .exact_size(48.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.vertical_centered(|ui| {
                    let stills = self.shelf_open && !self.folder_browser_open;
                    let tip = format!("Stills on the shelf ({})", self.shelf.len());
                    if widgets::icon(ui, "▦", stills, &tip).clicked() {
                        self.shelf_open = !stills;
                        self.folder_browser_open = false;
                    }
                    ui.label(theme::tag(self.shelf.len().to_string()).color(theme::TEXT_FAINT));
                    let folders = self.shelf_open && self.folder_browser_open;
                    if widgets::icon(ui, "🗁", folders, "Folders").clicked() {
                        self.shelf_open = !folders;
                        self.folder_browser_open = true;
                    }
                    ui.add_space(6.0);
                    if widgets::icon(ui, "✚", false, "Add photos to the shelf").clicked() {
                        self.pick_and_shelve(ui.ctx());
                    }
                });
            });
    }

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

                // Footer first, so it pins to the bottom of the panel.
                egui::Panel::bottom("shelf_footer")
                    .frame(
                        egui::Frame::new()
                            .fill(theme::BG_925)
                            .inner_margin(Margin::symmetric(12, 8)),
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(theme::tag("shelf total").color(Color32::from_gray(0xcc)));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let total: u64 = self.shelf.iter().map(|it| it.bytes).sum();
                                    ui.label(
                                        RichText::new(format_bytes(total))
                                            .family(theme::mono_bold())
                                            .size(12.0)
                                            .color(theme::ACCENT),
                                    );
                                },
                            );
                        });
                    });

                widgets::band(ui, |ui| {
                    ui.label(RichText::new("🗁").size(15.0).color(theme::ACCENT));
                    ui.label(theme::head("Shelf bin"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon(ui, "✚", false, "Add photos to the shelf").clicked() {
                            self.pick_and_shelve(ui.ctx());
                        }
                        if !self.shelf.is_empty()
                            && widgets::icon(ui, "🗑", false, "Remove all photos from the shelf")
                                .clicked()
                        {
                            self.clear_shelf();
                        }
                    });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let total_shelf = self.shelf.len();
                    if widgets::chip(
                        ui,
                        !self.folder_browser_open,
                        &format!("Stills ({total_shelf})"),
                    )
                    .clicked()
                    {
                        self.folder_browser_open = false;
                    }
                    if widgets::chip(ui, self.folder_browser_open, "Folders").clicked() {
                        self.folder_browser_open = true;
                    }
                });
                ui.add_space(4.0);

                let mut to_open = None;
                let mut to_remove = None;

                if self.folder_browser_open {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                        )
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(Margin {
                                    left: 10,
                                    right: 10,
                                    top: 4,
                                    bottom: 10,
                                })
                                .show(ui, |ui| {
                                    self.shelf_folders(ui, gpu);
                                });
                        });
                } else {
                    // Drop target. Dropping works anywhere in the window; this is where it's
                    // advertised, and clicking it is the same as the + above.
                    egui::Frame::new()
                        .inner_margin(Margin {
                            left: 10,
                            right: 10,
                            top: 6,
                            bottom: 6,
                        })
                        .show(ui, |ui| {
                            let (rect, response) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 34.0),
                                Sense::click(),
                            );
                            let dragging_in = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
                            let hot = response.hovered() || dragging_in;
                            widgets::dashed_rect(
                                ui.painter(),
                                rect,
                                if hot {
                                    theme::ACCENT
                                } else {
                                    Color32::from_gray(0x2a)
                                },
                            );
                            let painter = ui.painter();
                            let label = painter.layout_no_wrap(
                                "Drop Photos (RAW / Raster)".to_owned(),
                                FontId::proportional(12.5),
                                if hot {
                                    theme::TEXT_HI
                                } else {
                                    Color32::from_gray(0xcc)
                                },
                            );
                            let left = rect.center().x - (label.size().x + 20.0) / 2.0;
                            painter.text(
                                pos2(left + 6.0, rect.center().y),
                                Align2::CENTER_CENTER,
                                "⬆",
                                FontId::proportional(14.0),
                                theme::ACCENT,
                            );
                            painter.galley(
                                pos2(left + 20.0, rect.center().y - label.size().y / 2.0),
                                label,
                                theme::TEXT,
                            );
                            if hot {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if response.clicked() {
                                self.pick_and_shelve(ui.ctx());
                            }
                        });

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                        )
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(Margin {
                                    left: 10,
                                    right: 10,
                                    top: 4,
                                    bottom: 10,
                                })
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 10.0;
                                    self.shelf_group(ui, &mut to_open, &mut to_remove);
                                    self.snapshots_group(ui);
                                    self.history_group(ui);
                                });
                        });
                }

                if let Some(path) = to_open {
                    self.open_path(gpu, path);
                }
                if let Some(idx) = to_remove {
                    let path = self.shelf[idx].path.clone();
                    self.remove_from_shelf(&[path]);
                }
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

    pub(crate) fn shelf_group(
        &mut self,
        ui: &mut egui::Ui,
        to_open: &mut Option<PathBuf>,
        to_remove: &mut Option<usize>,
    ) {
        let total: u64 = self.shelf.iter().map(|it| it.bytes).sum();
        let title = format!("Photos ({})", self.shelf.len());
        let mut row_shift = None;
        let mut row_ctrl = None;
        let mut row_single = None;
        let mut row_rating = None;
        let mut row_flag = None;

        group(
            ui,
            &title,
            |ui| {
                ui.label(
                    theme::num(format_bytes(total))
                        .size(11.0)
                        .color(theme::TEXT_DIM),
                );
            },
            |ui| {
                if self.shelf.is_empty() {
                    ui.add_space(6.0);
                    ui.label(theme::dim("Nothing shelved yet.").size(12.0));
                    ui.add_space(6.0);
                    return;
                }
                let mut any_visible = false;
                let query = self.filter.search_query.to_lowercase();
                for (i, item) in self.shelf.iter().enumerate() {
                    let active = self.loaded_path.as_ref() == Some(&item.path);
                    let is_edited = if active {
                        self.is_stack_edited()
                    } else {
                        item.is_edited
                    };
                    if !self.filter.matches_lowered(
                        &query,
                        &item.search_name,
                        &item.metadata,
                        is_edited,
                        item.has_sidecar,
                    ) {
                        continue;
                    }
                    any_visible = true;
                    // Rows are all one height, so a row scrolled out of view only has to
                    // hold its space: the same allocation `shelf_row` makes, so the scroll
                    // extent and every later row's id are unchanged. Laying out and
                    // painting all of a 2000-photo roll every frame is what this avoids.
                    let row = Rect::from_min_size(
                        ui.cursor().min,
                        vec2(ui.available_width(), SHELF_ROW_HEIGHT),
                    );
                    if !ui.is_rect_visible(row) {
                        ui.allocate_space(row.size());
                        continue;
                    }
                    let selected = self.selection.is_selected(&item.path);
                    let status = if active {
                        match self.sidecar_state() {
                            SidecarState::Synced => Some(theme::LIME),
                            SidecarState::Unsaved | SidecarState::Modified => Some(theme::ACCENT),
                            _ => None,
                        }
                    } else if item.has_sidecar {
                        Some(theme::LIME)
                    } else if item.is_edited {
                        Some(theme::ACCENT)
                    } else {
                        None
                    };
                    let (resp, remove) = shelf_row(ui, item, active, selected, status);
                    if resp.clicked() && !remove {
                        let (ctrl, shift) = ui.input(|i| {
                            (i.modifiers.command || i.modifiers.ctrl, i.modifiers.shift)
                        });
                        // Select mode: a touch screen's Ctrl-click.
                        let ctrl = ctrl || self.select_mode;
                        if shift {
                            row_shift = Some(item.path.clone());
                        } else if ctrl {
                            row_ctrl = Some(item.path.clone());
                        } else {
                            row_single = Some((item.path.clone(), active));
                        }
                    }
                    if remove {
                        *to_remove = Some(i);
                    }
                    resp.context_menu(|ui| {
                        ui.label(
                            RichText::new(&item.name)
                                .family(theme::mono_bold())
                                .color(theme::ACCENT),
                        );
                        if ui.button("Remove from shelf").clicked() {
                            *to_remove = Some(i);
                            ui.close();
                        }
                        ui.separator();
                        ui.label(theme::dim("Flags:"));
                        ui.horizontal(|ui| {
                            if ui.button("Pick (P)").clicked() {
                                row_flag = Some((item.path.clone(), photo_engine::Flag::Pick));
                                ui.close();
                            }
                            if ui.button("Reject (X)").clicked() {
                                row_flag = Some((item.path.clone(), photo_engine::Flag::Reject));
                                ui.close();
                            }
                            if ui.button("Clear (U)").clicked() {
                                row_flag = Some((item.path.clone(), photo_engine::Flag::Unflagged));
                                ui.close();
                            }
                        });
                        ui.label(theme::dim("Rating:"));
                        ui.horizontal(|ui| {
                            for r in 0..=5 {
                                let label = if r == 0 {
                                    "0★".to_string()
                                } else {
                                    format!("{r}★")
                                };
                                if ui.button(label).clicked() {
                                    row_rating = Some((item.path.clone(), r));
                                    ui.close();
                                }
                            }
                        });
                    });
                }

                if !any_visible {
                    ui.add_space(6.0);
                    ui.label(theme::dim("No photos match filters.").size(12.0));
                    ui.add_space(6.0);
                }
            },
        );

        if let Some(target) = row_shift {
            // The range runs through the photos as listed, filter and sort included.
            let all_paths: Vec<PathBuf> = self
                .displayed_indices()
                .into_iter()
                .map(|i| self.shelf[i].path.clone())
                .collect();
            self.selection.select_range(target, &all_paths);
        }
        if let Some(target) = row_ctrl {
            self.selection.toggle_select(target);
        }
        if let Some((target, active)) = row_single {
            self.selection.select_single(target.clone());
            if !active {
                *to_open = Some(target);
            }
        }
        if let Some((path, rating)) = row_rating {
            if self.selection.is_selected(&path) {
                self.set_rating_for_selection(rating);
            } else {
                self.set_rating_for_path(&path, rating);
            }
        }
        if let Some((path, flag)) = row_flag {
            if self.selection.is_selected(&path) {
                self.set_flag_for_selection(flag);
            } else {
                self.set_flag_for_path(&path, flag);
            }
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
