//! The left panel: this photo's history and snapshots.

use crate::*;

impl PhotoApp {
    // ── History panel (left) ────────────────────────────────────────────────────────────

    /// The left panel: this photo's snapshots and history, one view. Folded, it is a thin
    /// edge with one button to open it again. The photos are on the shelf under the stage.
    pub(crate) fn history_panel(&mut self, ui: &mut egui::Ui) {
        if !self.shelf_open && !self.touring_history() {
            let edge = egui::Panel::left("history_edge")
                .frame(
                    egui::Frame::new()
                        .fill(theme::BG_950)
                        .inner_margin(Margin::symmetric(2, 10)),
                )
                .exact_size(34.0)
                .resizable(false)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        if widgets::icon(ui, "›", false, "Show history and snapshots").clicked() {
                            self.shelf_open = true;
                        }
                    });
                });
            self.desk_spots.history = Some(edge.response.rect);
            return;
        }
        let panel = egui::Panel::left("history")
            .frame(egui::Frame::new().fill(theme::BG_950))
            .resizable(true)
            .default_size(272.0)
            .min_size(220.0)
            .max_size(400.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                widgets::band(ui, |ui| {
                    ui.label(theme::head("History"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon(ui, "‹", false, "Fold this panel away").clicked() {
                            self.shelf_open = false;
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
                                ui.spacing_mut().item_spacing.y = 10.0;
                                self.snapshots_group(ui);
                                self.history_group(ui);
                            });
                    });
            });
        self.desk_spots.history = Some(panel.response.rect);
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
