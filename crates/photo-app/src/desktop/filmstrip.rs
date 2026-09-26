//! The stage toolbar and the filmstrip under the stage.

use crate::*;

impl PhotoApp {
    // ── Stage toolbar, canvas and filmstrip (centre) ────────────────────────────────────

    /// CROP to start framing; APPLY and CANCEL while framing.
    pub(crate) fn crop_chips(&mut self, ui: &mut egui::Ui) {
        if self.cropping {
            if widgets::chip(ui, true, "✓ apply crop")
                .on_hover_text("Keep this framing and carry on editing inside it (Enter or C)")
                .clicked()
            {
                self.apply_crop();
            }
            if widgets::chip(ui, false, "× cancel")
                .on_hover_text("Put the crop back the way it was (Esc)")
                .clicked()
            {
                self.cancel_crop();
            }
        } else if widgets::chip(ui, false, "crop")
            .on_hover_text("Frame a crop on the whole picture (C)")
            .clicked()
        {
            self.begin_crop();
        }
    }

    pub(crate) fn stage_toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("stage_toolbar")
            .frame(egui::Frame::new().fill(theme::BG_950).inner_margin(Margin::symmetric(14, 7)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let loaded = self.ready_image().is_some();
                    ui.add_enabled_ui(loaded, |ui| {
                        self.crop_chips(ui);
                        if widgets::chip(ui, self.split_active, "a / b")
                            .on_hover_text("Before / after split (\\). Drag the divider; double-click it to centre")
                            .clicked()
                        {
                            self.split_active = !self.split_active;
                            self.needs_render = true;
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_enabled_ui(loaded, |ui| {
                            if widgets::button(ui, Kind::Secondary, "1:1", 0.0)
                                .on_hover_text("Actual pixels (Ctrl+1, or Z to toggle)")
                                .clicked()
                            {
                                self.zoom_actual();
                            }
                            if widgets::button(ui, Kind::Secondary, "FIT", 0.0)
                                .on_hover_text("Fit to window (Ctrl+0, or Z to toggle)")
                                .clicked()
                            {
                                self.view = View::default();
                            }
                            // Right-to-left: + then the readout then −.
                            pod(ui, |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if widgets::icon(ui, "+", false, "Zoom in").clicked() {
                                    self.zoom_by(1.25);
                                }
                                let percent = self
                                    .zoom_percent(self.last_panel)
                                    .map_or("—".to_string(), |p| format!("{p:.0}%"));
                                ui.add_sized(
                                    [44.0, 20.0],
                                    egui::Label::new(
                                        RichText::new(percent)
                                            .family(theme::mono_bold())
                                            .size(11.0)
                                            .color(theme::TEXT_HI),
                                    ),
                                );
                                if widgets::icon(ui, "−", false, "Zoom out").clicked() {
                                    self.zoom_by(0.8);
                                }
                            });
                        });
                    });
                });
            });
    }

    /// The shelf as the filter and sort leave it: indices into `self.shelf`, not copies of
    /// it, since this runs every frame and cloning every item grew with the folder.
    pub(crate) fn displayed_indices(&self) -> Vec<usize> {
        let is_stack_edited = self.is_stack_edited();
        let loaded_path = self.loaded_path.clone();

        let mut displayed: Vec<usize> = self
            .shelf
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                let is_edited = if loaded_path.as_ref() == Some(&it.path) {
                    is_stack_edited
                } else {
                    it.is_edited
                };
                self.filter
                    .matches(&it.path, &it.metadata, is_edited, it.has_sidecar)
            })
            .map(|(i, _)| i)
            .collect();

        {
            let shelf = &self.shelf;
            let descending = self.filter.sort_descending;
            let order = |ord: std::cmp::Ordering| if descending { ord.reverse() } else { ord };
            match self.filter.sort_by {
                dam::SortBy::FileName => {
                    displayed.sort_by(|&a, &b| order(shelf[a].name.cmp(&shelf[b].name)))
                }
                dam::SortBy::Rating => displayed.sort_by(|&a, &b| {
                    order(shelf[a].metadata.rating.cmp(&shelf[b].metadata.rating))
                }),
                dam::SortBy::FileSize => {
                    displayed.sort_by(|&a, &b| order(shelf[a].bytes.cmp(&shelf[b].bytes)))
                }
            }
        }

        displayed
    }

    pub(crate) fn filmstrip(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        let displayed = self.displayed_indices();
        let mut action_export = false;

        egui::Panel::bottom("filmstrip")
            .frame(egui::Frame::new().fill(theme::BG_950).inner_margin(Margin {
                left: 14,
                right: 14,
                top: 8,
                bottom: 10,
            }))
            .show(ui, |ui| {
                let compact = ui.available_width() < 700.0;
                ui.horizontal(|ui| {
                    ui.label(theme::head("Filmstrip").color(theme::ACCENT));
                    let developed = self.shelf.iter().filter(|it| it.has_sidecar).count();
                    let visible_count = displayed.len();
                    ui.label(
                        theme::num(format!(
                            "{visible_count}/{} stills · {developed} with sidecar",
                            self.shelf.len()
                        ))
                        .size(11.0)
                        .color(Color32::from_gray(0xcc)),
                    );
                    if !compact {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.filmstrip_actions(ui, true);
                        });
                    }
                });
                if compact {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| self.filmstrip_actions(ui, false));
                }
                ui.add_space(6.0);

                // Two rows: what is shown, then how it is ordered and what to do with the
                // selection. One row with a right-aligned half ran the halves into each other
                // on a narrow stage.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    self.rating_filter(ui);
                    ui.separator();
                    self.flag_filter(ui);
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    self.search_field(ui, 140.0);
                    ui.separator();
                    self.sort_controls(ui, false);
                    if !self.selection.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::button(
                                ui,
                                Kind::Primary,
                                &format!("Export Batch ({})", self.selection.count()),
                                0.0,
                            )
                            .clicked()
                            {
                                action_export = true;
                            }
                            if widgets::button(ui, Kind::Secondary, "Deselect", 0.0).clicked() {
                                self.selection.clear();
                            }
                        });
                    }
                });
                ui.add_space(6.0);

                self.film_tiles(ui, gpu, &displayed);
            });

        if action_export {
            self.batch_dialog_open = true;
        }
    }

    pub(crate) fn rating_filter(&mut self, ui: &mut egui::Ui) {
        // Rating filter chips
        ui.label(theme::dim("Rating:"));
        pod(ui, |ui| {
            let is_all = matches!(self.filter.rating, dam::RatingFilter::All);
            if widgets::chip(ui, is_all, "All").clicked() {
                self.filter.rating = dam::RatingFilter::All;
            }
            for min in 1..=5 {
                let active =
                    matches!(self.filter.rating, dam::RatingFilter::MinStars(m) if m == min);
                let label = if min == 5 {
                    "5★".to_string()
                } else {
                    format!("{min}★+")
                };
                if widgets::chip(ui, active, &label).clicked() {
                    self.filter.rating = if active {
                        dam::RatingFilter::All
                    } else {
                        dam::RatingFilter::MinStars(min)
                    };
                }
            }
        });
    }

    pub(crate) fn flag_filter(&mut self, ui: &mut egui::Ui) {
        // Flag filter chips
        ui.label(theme::dim("Flags:"));
        pod(ui, |ui| {
            let f = self.filter.flag;
            if widgets::chip(ui, f == dam::FlagFilter::All, "All").clicked() {
                self.filter.flag = dam::FlagFilter::All;
            }
            if widgets::chip(ui, f == dam::FlagFilter::PicksOnly, "Picks").clicked() {
                self.filter.flag = if f == dam::FlagFilter::PicksOnly {
                    dam::FlagFilter::All
                } else {
                    dam::FlagFilter::PicksOnly
                };
            }
            if widgets::chip(ui, f == dam::FlagFilter::HideRejects, "Hide Rej").clicked() {
                self.filter.flag = if f == dam::FlagFilter::HideRejects {
                    dam::FlagFilter::All
                } else {
                    dam::FlagFilter::HideRejects
                };
            }
            if widgets::chip(ui, f == dam::FlagFilter::RejectsOnly, "Rejects").clicked() {
                self.filter.flag = if f == dam::FlagFilter::RejectsOnly {
                    dam::FlagFilter::All
                } else {
                    dam::FlagFilter::RejectsOnly
                };
            }
        });
    }

    pub(crate) fn search_field(&mut self, ui: &mut egui::Ui, width: f32) {
        ui.add(
            egui::TextEdit::singleline(&mut self.filter.search_query)
                .hint_text("Search…")
                .desired_width(width)
                .font(egui::TextStyle::Monospace),
        );
        if !self.filter.search_query.is_empty()
            && ui.button("×").on_hover_text("Clear search").clicked()
        {
            self.filter.search_query.clear();
        }
    }

    /// "Sort: Name · Rating · Size ↑". `right_to_left` callers get it reversed, so it reads
    /// the same way on screen either way.
    pub(crate) fn sort_controls(&mut self, ui: &mut egui::Ui, right_to_left: bool) {
        let arrow = |ui: &mut egui::Ui, filter: &mut dam::FilterState| {
            let glyph = if filter.sort_descending { "↓" } else { "↑" };
            if widgets::button(ui, Kind::Secondary, glyph, 22.0)
                .on_hover_text(if filter.sort_descending {
                    "Descending order"
                } else {
                    "Ascending order"
                })
                .clicked()
            {
                filter.sort_descending = !filter.sort_descending;
            }
        };
        let chips = |ui: &mut egui::Ui, filter: &mut dam::FilterState| {
            pod(ui, |ui| {
                let mut order = [
                    (dam::SortBy::FileName, "Name"),
                    (dam::SortBy::Rating, "Rating"),
                    (dam::SortBy::FileSize, "Size"),
                ];
                if right_to_left {
                    order.reverse();
                }
                for (by, label) in order {
                    if widgets::chip(ui, filter.sort_by == by, label).clicked() {
                        filter.sort_by = by;
                    }
                }
            });
        };
        if right_to_left {
            arrow(ui, &mut self.filter);
            chips(ui, &mut self.filter);
            ui.label(theme::dim("Sort:"));
        } else {
            ui.label(theme::dim("Sort:"));
            chips(ui, &mut self.filter);
            arrow(ui, &mut self.filter);
        }
    }

    /// The strip of thumbnails, and what clicking and long-pressing them does.
    pub(crate) fn film_tiles(&mut self, ui: &mut egui::Ui, gpu: &Gpu, displayed: &[usize]) {
        let mut to_open = None;
        let mut action_shift = None;
        let mut action_ctrl = None;
        let mut action_single = None;
        let mut action_rating = None;
        let mut action_flag = None;
        let mut action_export = false;

        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            .show_viewport(ui, |ui, viewport| {
                if self.shelf.is_empty() {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 70.0), Sense::hover());
                    ui.painter().text(
                        rect.left_center() + vec2(4.0, 0.0),
                        Align2::LEFT_CENTER,
                        if widgets::is_touch(ui.ctx()) {
                            "Open or add photos to start a roll."
                        } else {
                            "Drop photos anywhere in the window to start a roll."
                        },
                        FontId::proportional(12.0),
                        theme::TEXT_DIM,
                    );
                    return;
                }

                if displayed.is_empty() {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 70.0), Sense::hover());
                    ui.painter().text(
                        rect.left_center() + vec2(4.0, 0.0),
                        Align2::LEFT_CENTER,
                        "No photos match the active filter criteria.",
                        FontId::proportional(12.0),
                        theme::TEXT_DIM,
                    );
                    return;
                }

                // Only the tiles in view are laid out and painted; the rest of the
                // strip is empty space the scroll area measures. Tiles are all one
                // size, so which ones are in view is arithmetic.
                let origin = ui.max_rect().left_top();
                let total = displayed.len();
                ui.set_min_size(vec2(total as f32 * FILM_STRIDE - FILM_GAP, FILM_TILE.y));
                let visible = visible_tiles(viewport.x_range(), total);

                let loading = self.loading.as_ref().map(|(p, _)| p);
                for (slot, &index) in displayed
                    .iter()
                    .enumerate()
                    .take(visible.end)
                    .skip(visible.start)
                {
                    let item = &self.shelf[index];
                    let active = self.loaded_path.as_ref() == Some(&item.path);
                    let selected = self.selection.is_selected(&item.path);
                    // The open photo's tile shows the live render, once there is one for
                    // it; until its decode lands, its thumbnail like any other.
                    let live = self.ready_image().filter(|_| active).and_then(|img| {
                        self.texture_id
                            .map(|id| (id, img.width as f32 / img.height.max(1) as f32))
                    });
                    let picture = if live.is_some() {
                        live
                    } else {
                        self.thumbs.get(&item.path).map(|t| {
                            let [w, h] = t.size();
                            (t.id(), w as f32 / h.max(1) as f32)
                        })
                    };
                    let pending = loading == Some(&item.path);
                    let tile_rect = Rect::from_min_size(
                        origin + vec2(slot as f32 * FILM_STRIDE, 0.0),
                        FILM_TILE,
                    );
                    // Keyed by the photo, not by position among the tiles drawn this
                    // frame: scrolling changes which tile is drawn first, and a click
                    // or an open context menu must stay with its photo.
                    let tile_resp = ui
                        .scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(tile_rect)
                                .id_salt(&item.path),
                            |ui| film_tile(ui, item, active, selected, pending, picture),
                        )
                        .inner;
                    if tile_resp.clicked() {
                        let (ctrl, shift) = ui.input(|i| {
                            (i.modifiers.command || i.modifiers.ctrl, i.modifiers.shift)
                        });
                        // Select mode: a touch screen's Ctrl-click.
                        let ctrl = ctrl || self.select_mode;
                        if shift {
                            let all_paths: Vec<PathBuf> = displayed
                                .iter()
                                .map(|&i| self.shelf[i].path.clone())
                                .collect();
                            action_shift = Some((item.path.clone(), all_paths));
                        } else if ctrl {
                            action_ctrl = Some(item.path.clone());
                        } else {
                            action_single = Some((item.path.clone(), active));
                        }
                    }

                    tile_resp.context_menu(|ui| {
                        ui.label(
                            RichText::new(&item.name)
                                .family(theme::mono_bold())
                                .color(theme::ACCENT),
                        );
                        ui.separator();
                        ui.label(theme::dim("Flags:"));
                        ui.horizontal(|ui| {
                            if ui.button("Pick (P)").clicked() {
                                action_flag = Some((item.path.clone(), photo_engine::Flag::Pick));
                                ui.close();
                            }
                            if ui.button("Reject (X)").clicked() {
                                action_flag = Some((item.path.clone(), photo_engine::Flag::Reject));
                                ui.close();
                            }
                            if ui.button("Clear (U)").clicked() {
                                action_flag =
                                    Some((item.path.clone(), photo_engine::Flag::Unflagged));
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
                                    action_rating = Some((item.path.clone(), r));
                                    ui.close();
                                }
                            }
                        });
                        if ui.button("Batch Multi-Export…").clicked() {
                            action_export = true;
                            ui.close();
                        }
                    });
                }
            });

        if let Some((target, all_paths)) = action_shift {
            self.selection.select_range(target, &all_paths);
        }
        if let Some(target) = action_ctrl {
            self.selection.toggle_select(target);
        }
        if let Some((target, active)) = action_single {
            self.selection.select_single(target.clone());
            if !active {
                to_open = Some(target);
            }
        }
        if let Some((path, rating)) = action_rating {
            if self.selection.is_selected(&path) {
                self.set_rating_for_selection(rating);
            } else {
                self.set_rating_for_path(&path, rating);
            }
        }
        if let Some((path, flag)) = action_flag {
            if self.selection.is_selected(&path) {
                self.set_flag_for_selection(flag);
            } else {
                self.set_flag_for_path(&path, flag);
            }
        }
        if action_export {
            self.batch_dialog_open = true;
        }
        if let Some(path) = to_open {
            self.open_path(gpu, path);
        }
    }

    /// Undo · Redo | Reset all · Copy stack · Paste · Previous · Paste to all. `right_to_left` callers
    /// get them reversed, so they read the same way on screen either way.
    pub(crate) fn filmstrip_actions(&mut self, ui: &mut egui::Ui, right_to_left: bool) {
        #[derive(Clone, Copy)]
        enum Action {
            Undo,
            Redo,
            Sep,
            Reset,
            Copy,
            Paste,
            Previous,
            PasteAll,
        }
        let mut order = [
            Action::Undo,
            Action::Redo,
            Action::Sep,
            Action::Reset,
            Action::Copy,
            Action::Paste,
            Action::Previous,
            Action::PasteAll,
        ];
        if right_to_left {
            order.reverse();
        }

        let loaded = self.loaded_path.is_some();
        let has_clip = self.clipboard_stack.is_some();
        let n = self.shelf.len();
        for action in order {
            use widgets::button_enabled as button;
            match action {
                Action::Undo => {
                    if button(
                        ui,
                        !self.history.past.is_empty(),
                        Kind::Secondary,
                        "Undo",
                        0.0,
                    )
                    .on_hover_text("Ctrl+Z")
                    .clicked()
                    {
                        self.undo();
                    }
                }
                Action::Redo => {
                    if button(
                        ui,
                        !self.history.future.is_empty(),
                        Kind::Secondary,
                        "Redo",
                        0.0,
                    )
                    .on_hover_text("Ctrl+Shift+Z")
                    .clicked()
                    {
                        self.redo();
                    }
                }
                Action::Sep => vsep(ui),
                Action::Reset => {
                    if button(ui, loaded, Kind::Secondary, "Reset all", 0.0)
                        .on_hover_text("Every module back to default (undoable)")
                        .clicked()
                    {
                        self.reset_to_default();
                    }
                }
                Action::Copy => {
                    if button(ui, loaded, Kind::Secondary, "Copy stack", 0.0)
                        .on_hover_text("Copy this image's adjustments (Ctrl+C)")
                        .clicked()
                    {
                        self.copy_adjustments();
                    }
                }
                Action::Paste => {
                    if button(ui, has_clip && loaded, Kind::Secondary, "Paste", 0.0)
                        .on_hover_text("Paste the copied stack onto this image (Ctrl+V)")
                        .clicked()
                    {
                        self.paste_adjustments();
                    }
                }
                Action::Previous => {
                    if button(
                        ui,
                        self.can_same_as_previous(),
                        Kind::Secondary,
                        "Previous",
                        0.0,
                    )
                    .on_hover_text(
                        "The same edits as the photo open before this one; crop and \
                             rotation stay (Ctrl+Alt+V)",
                    )
                    .clicked()
                    {
                        self.same_as_previous();
                    }
                }
                Action::PasteAll if self.can_undo_paste_to_all() => {
                    if button(ui, true, Kind::Secondary, "Undo paste", 0.0)
                        .on_hover_text("Put back every sidecar the paste to all replaced")
                        .clicked()
                    {
                        self.undo_paste_to_all();
                    }
                }
                Action::PasteAll => {
                    if button(
                        ui,
                        has_clip && n > 0,
                        Kind::Accent,
                        &format!("Paste to all {n}"),
                        0.0,
                    )
                    .on_hover_text(
                        "Paste the copied look onto every shelf photo; each keeps its own \
                         crop and rotation",
                    )
                    .clicked()
                    {
                        self.paste_to_all_in_shelf();
                    }
                }
            }
        }
    }
}
