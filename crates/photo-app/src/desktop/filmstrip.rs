//! The stage toolbar and the filmstrip under the stage.

use crate::*;

impl PhotoApp {
    // ── Stage toolbar, canvas and filmstrip (centre) ────────────────────────────────────

    /// CROP to start framing; APPLY and CANCEL while framing.
    pub(crate) fn crop_chips(&mut self, ui: &mut egui::Ui) {
        if self.cropping {
            if widgets::chip(ui, true, "apply crop")
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
                let loaded = self.ready_image().is_some();
                let zoom_key = egui::Id::new("stage_zoom_width");
                let zoom_w = widgets::trailing_width(ui.ctx(), zoom_key);
                let mut zoom_beside = false;
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(loaded, |ui| {
                        self.crop_chips(ui);
                        if widgets::chip(ui, self.split_active, "a / b")
                            .on_hover_text("Before / after split (\\). Drag the divider; double-click it to centre")
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

                    // Zoom at the right end when it fits beside the rest, as measured last
                    // frame; otherwise on a row of its own below.
                    zoom_beside = zoom_w + 12.0 <= ui.available_width();
                    if zoom_beside {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.scope(|ui| {
                                self.zoom_controls(ui, loaded);
                                widgets::remember_trailing_width(ui, zoom_key);
                            });
                        });
                    }
                });
                if !zoom_beside {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.scope(|ui| {
                                self.zoom_controls(ui, loaded);
                                widgets::remember_trailing_width(ui, zoom_key);
                            });
                        });
                    });
                }
                // Framing a crop: its shapes and guide on a row of their own, which the
                // row above has no room for.
                if self.cropping {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| self.crop_shape_chips(ui));
                }
            });
    }

    /// 1:1, FIT and the zoom readout, laid out right to left.
    fn zoom_controls(&mut self, ui: &mut egui::Ui, loaded: bool) {
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
                // A virtual copy follows its photo.
                dam::SortBy::FileName => displayed.sort_by(|&a, &b| {
                    order(
                        (&shelf[a].sort_name, shelf[a].copy)
                            .cmp(&(&shelf[b].sort_name, shelf[b].copy)),
                    )
                }),
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
                let actions_key = egui::Id::new("filmstrip_actions_width");
                let actions_w = widgets::trailing_width(ui.ctx(), actions_key);
                let mut beside = false;
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
                    // Beside the title when they fit there, as last frame measured them.
                    beside = actions_w + 12.0 <= ui.available_width();
                    if beside {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.scope(|ui| {
                                self.filmstrip_actions(ui, true);
                                widgets::remember_trailing_width(ui, actions_key);
                            });
                        });
                    }
                });
                if !beside {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        self.filmstrip_actions(ui, false);
                        widgets::remember_trailing_width(ui, actions_key);
                    });
                }
                ui.add_space(6.0);

                // Two rows: what is shown, then how it is ordered and what to do with the
                // selection. One row with a right-aligned half ran the halves into each other
                // on a narrow stage.
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    self.rating_filter(ui);
                    ui.separator();
                    self.flag_filter(ui);
                });
                ui.add_space(4.0);
                let selected_key = egui::Id::new("filmstrip_selection_width");
                let selected_w = widgets::trailing_width(ui.ctx(), selected_key);
                let mut selection_beside = false;
                let count = self.selection.count();
                let selection_actions = |ui: &mut egui::Ui, export: &mut bool, clear: &mut bool| {
                    if widgets::button(ui, Kind::Primary, &format!("Export Batch ({count})"), 0.0)
                        .clicked()
                    {
                        *export = true;
                    }
                    if widgets::button(ui, Kind::Secondary, "Deselect", 0.0).clicked() {
                        *clear = true;
                    }
                };
                let mut clear = false;
                // The label filter after the sort when it fits there, as last frame
                // measured it, else on a row of its own.
                let label_key = egui::Id::new("filmstrip_label_width");
                let label_w = widgets::trailing_width(ui.ctx(), label_key);
                let mut label_beside = false;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    self.search_field(ui, 140.0);
                    ui.separator();
                    self.sort_controls(ui, false);
                    if label_w + 12.0 <= ui.available_width() {
                        label_beside = true;
                        ui.separator();
                        ui.scope(|ui| {
                            ui.horizontal(|ui| self.label_filter(ui));
                            widgets::remember_trailing_width(ui, label_key);
                        });
                    }
                    if count > 0 && selected_w + 12.0 <= ui.available_width() {
                        selection_beside = true;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.scope(|ui| {
                                selection_actions(ui, &mut action_export, &mut clear);
                                widgets::remember_trailing_width(ui, selected_key);
                            });
                        });
                    }
                });
                if !label_beside {
                    ui.add_space(4.0);
                    ui.scope(|ui| {
                        ui.horizontal(|ui| self.label_filter(ui));
                        widgets::remember_trailing_width(ui, label_key);
                    });
                }
                if !self.saved_filters.is_empty()
                    || self.filtering()
                    || self.saved_filter_draft.is_some()
                {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        self.saved_filters_row(ui);
                    });
                }
                if count > 0 && !selection_beside {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        selection_actions(ui, &mut action_export, &mut clear);
                        widgets::remember_trailing_width(ui, selected_key);
                    });
                }
                if clear {
                    self.selection.clear();
                }
                ui.add_space(6.0);

                self.film_tiles(ui, gpu, &displayed);
            });

        if action_export {
            self.batch_dialog_open = true;
        }
    }

    /// Whether anything hides photos: sorting is not filtering.
    pub(crate) fn filtering(&self) -> bool {
        self.filter
            != dam::FilterState {
                sort_by: self.filter.sort_by,
                sort_descending: self.filter.sort_descending,
                ..Default::default()
            }
    }

    /// Saved filters as chips (a tap shows one; right-click or a long press deletes it),
    /// and "+ save" to keep the filter showing under a name.
    pub(crate) fn saved_filters_row(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::dim("Saved:"));
        let mut apply = None;
        let mut delete = None;
        for (i, saved) in self.saved_filters.iter().enumerate() {
            let on = saved.filter() == self.filter;
            let r = widgets::chip(ui, on, &saved.name)
                .on_hover_text("Show this filter; right-click to delete it");
            if r.clicked() {
                apply = Some(i);
            }
            r.context_menu(|ui| {
                if ui.button("Delete").clicked() {
                    delete = Some(i);
                    ui.close();
                }
            });
        }
        let mut save = None;
        match &mut self.saved_filter_draft {
            None => {
                if self.filtering()
                    && widgets::chip(ui, false, "+ save")
                        .on_hover_text("Keep this filter under a name")
                        .clicked()
                {
                    self.saved_filter_draft = Some(String::new());
                }
            }
            Some(name) => {
                let field = ui.add(
                    egui::TextEdit::singleline(name)
                        .hint_text("name, then Enter")
                        .desired_width(130.0),
                );
                if field.lost_focus() {
                    let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if entered && !name.trim().is_empty() {
                        save = Some(name.clone());
                    }
                    if entered
                        || name.trim().is_empty()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        self.saved_filter_draft = None;
                    }
                } else if name.is_empty() && !field.has_focus() {
                    field.request_focus();
                }
            }
        }
        if let Some(name) = save {
            let saved = dam::SavedFilter::new(&name, &self.filter);
            self.set_status(format!("Saved the filter as “{}”", saved.name));
            match self.saved_filters.iter_mut().find(|s| s.name == saved.name) {
                Some(old) => *old = saved,
                None => self.saved_filters.push(saved),
            }
            self.persist_session();
        }
        if let Some(i) = apply {
            self.filter = self.saved_filters[i].filter();
        }
        if let Some(i) = delete {
            let gone = self.saved_filters.remove(i);
            self.set_status(format!("Deleted the saved filter “{}”", gone.name));
            self.persist_session();
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
        let mut action_copy = None;
        let mut action_label = None;

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
                // Thumbnails for what is in view and a stretch either side, as they are
                // needed rather than a whole roll's at once (WP5.2).
                const PREFETCH: usize = 24;
                let wanted: Vec<PathBuf> = displayed
                    [visible.start.saturating_sub(PREFETCH)..(visible.end + PREFETCH).min(total)]
                    .iter()
                    .map(|&i| self.shelf[i].path.clone())
                    .collect();
                for (k, path) in wanted.iter().enumerate() {
                    self.request_thumbnail(path);
                    let in_view = (visible.start.saturating_sub(PREFETCH) + k) >= visible.start
                        && (visible.start.saturating_sub(PREFETCH) + k) < visible.end;
                    if in_view {
                        self.thumb_used.insert(path.clone(), self.frame_no);
                    }
                }

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
                    let tile_resp = match item.info.as_ref().map(crate::info::one_line) {
                        Some(line) if !line.is_empty() => {
                            tile_resp.on_hover_text(format!("{}\n{line}", item.name))
                        }
                        _ => tile_resp,
                    };
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
                        ui.label(theme::dim("Label:"));
                        ui.horizontal(|ui| {
                            for (name, fill) in labels::LABELS {
                                let on = item.metadata.color_label.as_deref() == Some(name);
                                if labels::swatch(ui, name, fill, on).clicked() {
                                    action_label = Some((item.path.clone(), name));
                                    ui.close();
                                }
                            }
                        });
                        if ui.button("Virtual copy (Ctrl+')").clicked() {
                            action_copy = Some(item.path.clone());
                            ui.close();
                        }
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
        if let Some((path, label)) = action_label {
            if self.selection.is_selected(&path) {
                self.set_label_for_selection(Some(label));
            } else {
                self.set_label_for_paths(&[path], Some(label));
            }
        }
        if let Some(path) = action_copy {
            self.make_virtual_copy(gpu, &path);
            return;
        }
        if let Some(path) = to_open {
            self.open_path(gpu, path);
        }
    }

    /// Undo · Redo | Reset all · Copy stack · Paste · Modules… · Previous · Paste to all. `right_to_left` callers
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
            Choose,
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
            Action::Choose,
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
                Action::Choose => {
                    let tip = format!(
                        "Choose what Paste, Previous and Paste to all carry: now {} \
                         (Ctrl+Shift+V)",
                        self.paste_set.describe()
                    );
                    if widgets::button(ui, Kind::Secondary, "Modules…", 0.0)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        self.paste_picker_open = !self.paste_picker_open;
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
