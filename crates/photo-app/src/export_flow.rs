//! Exporting: the single-photo export, and the batch dialog and queue.

use crate::*;

impl PhotoApp {
    /// Read the current frame back and write it, encoding off the UI thread.
    pub(crate) fn export_to(&mut self, gpu: &Gpu, path: PathBuf) {
        // Renders the current stack at full resolution, whatever the preview is at, and
        // never with the split.
        let settings = photo_engine::ExportSettings {
            format: match photo_engine::ExportFormat::from_path(&path) {
                Ok(f) => f,
                Err(e) => {
                    self.export = Export::Failed(format!("{e:#}"));
                    return;
                }
            },
            ..self.export_settings
        };
        let depth = settings.effective_depth();
        let exported = match self.engine.export_in(
            gpu.device,
            gpu.queue,
            &self.stack,
            depth,
            settings.effective_space(),
        ) {
            Ok(e) => e,
            Err(e) => {
                self.export = Export::Failed(format!("{e:#}"));
                return;
            }
        };
        let gain = if settings.wants_gain_map() {
            match self.engine.gain_map(
                gpu.device,
                gpu.queue,
                &self.stack,
                settings.effective_space(),
            ) {
                Ok(g) => Some(g),
                Err(e) => {
                    self.export = Export::Failed(format!("the HDR gain map: {e:#}"));
                    self.needs_render = true;
                    return;
                }
            }
        } else {
            None
        };

        let (tx, rx) = mpsc::channel();
        let target = path.clone();
        let source = self.loaded_path.clone();
        let touchups = self.cutout_studio.touchups.clone();
        std::thread::spawn(move || {
            let info = source.as_deref().and_then(photo_engine::read_export_info);
            let outcome = exported
                .save_with(&target, &settings, info.as_ref(), &touchups, gain.as_ref())
                .map_err(|e| format!("{e:#}"));
            // The receiver is gone only if the app is shutting down; nothing to report to.
            let _ = tx.send(outcome);
        });

        self.export = Export::Writing {
            path,
            source: self.loaded_path.clone(),
            started: Instant::now(),
            result: rx,
        };
        // The export overwrote the preview's uniforms; put the preview back.
        self.needs_render = true;
    }

    /// Default filename beside the source: DSC08450.ARW -> DSC08450.jpg. JPEG at the export
    /// quality: what a photo is shared as, at a tenth of a PNG's size.
    pub(crate) fn export_suggestion(&self) -> (PathBuf, String) {
        let ext = self.export_settings.format.extension();
        match &self.loaded_path {
            Some(src) => {
                let stem = src
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("untitled");
                // Where the last export went, in its format; else beside the photo.
                let dir = self
                    .settings
                    .export_dir
                    .clone()
                    .filter(|d| d.is_dir())
                    .or_else(|| src.parent().map(PathBuf::from))
                    .unwrap_or_default();
                (dir, format!("{stem}.{ext}"))
            }
            None => (PathBuf::new(), format!("untitled.{ext}")),
        }
    }

    pub(crate) fn poll_export(&mut self, ctx: &egui::Context) {
        let (outcome, path, seconds, source) = {
            let Export::Writing {
                path,
                source,
                started,
                result,
            } = &mut self.export
            else {
                return;
            };
            match result.try_recv() {
                Ok(Ok(())) => (
                    Some(Ok(())),
                    path.clone(),
                    started.elapsed().as_secs_f32(),
                    source.take(),
                ),
                Ok(Err(e)) => (
                    Some(Err(e)),
                    path.clone(),
                    started.elapsed().as_secs_f32(),
                    None,
                ),
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint();
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => (
                    Some(Err("export thread stopped unexpectedly".to_string())),
                    path.clone(),
                    started.elapsed().as_secs_f32(),
                    None,
                ),
            }
        };

        match outcome {
            Some(Ok(())) => {
                self.platform.export_completed(&path);
                if let Some(source) = source {
                    self.exported_copy(&source);
                }
                let msg = crate::i18n::tr_args(
                    "exported",
                    &[
                        ("name", file_name(&path).into()),
                        ("seconds", format!("{seconds:.1}").into()),
                    ],
                );
                if self.platform.can_share() {
                    self.set_status_undoable(msg, NoticeAction::Share);
                } else {
                    self.set_status(msg);
                }
                self.export = Export::Done { path, seconds };
            }
            Some(Err(e)) => {
                self.set_status(format!("Export failed: {e}"));
                self.export = Export::Failed(e);
            }
            None => {}
        }
    }

    pub(crate) fn poll_batch_export(&mut self, ctx: &egui::Context) {
        let Some(queue) = &mut self.export_queue else {
            return;
        };
        if let Some(batch::ExportStatus::Finished {
            total,
            success,
            errors,
        }) = queue.take_finished()
        {
            if success + errors.len() < total {
                self.set_status(format!(
                    "Batch export cancelled: {success} of {total} written"
                ));
            } else if errors.is_empty() {
                self.set_status(format!(
                    "Batch export complete: {success}/{total} photos rendered."
                ));
            } else {
                self.set_status(format!(
                    "Batch export finished with {} errors out of {total} jobs",
                    errors.len()
                ));
            }
        } else if queue.is_running() {
            // The worker does not wake the event loop; keep the progress bar moving.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    pub(crate) fn batch_export_dialog(&mut self, ui: &mut egui::Ui) {
        if !self.batch_dialog_open {
            return;
        }

        let mut open = self.batch_dialog_open;
        let mut close_dialog = false;
        let mut start_export = false;

        egui::Window::new("Batch Multi-Export & Recipes")
            .open(&mut open)
            .resizable(true)
            // Inset past the corner resize grips (they reach 10 pt inside the frame), so
            // the last row's buttons are never under one.
            .frame(egui::Frame::window(&ui.ctx().global_style()).inner_margin(Margin::same(14)))
            // No bigger than the screen: on a phone it is most of it.
            .default_size(vec2(520.0, 480.0).min(ui.ctx().content_rect().size() - vec2(16.0, 16.0)))
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                // Selection Info
                let count = if !self.selection.is_empty() {
                    self.selection.count()
                } else if self.loaded_path.is_some() {
                    1
                } else {
                    self.shelf.len()
                };
                let target_desc = if !self.selection.is_empty() {
                    format!("{count} selected photos")
                } else if self.loaded_path.is_some() {
                    "Currently open photo".to_string()
                } else {
                    format!("All {count} photos on shelf")
                };

                ui.horizontal(|ui| {
                    ui.label(theme::head("Export Source:"));
                    ui.label(
                        RichText::new(target_desc)
                            .family(theme::mono_bold())
                            .color(theme::ACCENT),
                    );
                });

                ui.separator();

                // Destination Folder
                ui.label(theme::head("Destination Folder"));
                let default_dir = self
                    .loaded_path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(|p| p.join("Export"))
                    .unwrap_or_else(|| PathBuf::from("Export"));
                let default_dir = self.platform.default_output_dir().unwrap_or(default_dir);

                if self.batch_output_dir.is_none() {
                    self.batch_output_dir = Some(default_dir);
                }

                ui.horizontal(|ui| {
                    let dir_str = self
                        .batch_output_dir
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    ui.label(RichText::new(dir_str).monospace().color(theme::TEXT));
                    if widgets::button(ui, Kind::Secondary, "Browse...", 80.0).clicked() {
                        let from = self.batch_output_dir.clone();
                        self.request(ui.ctx(), platform::Request::BatchOutputDir { from });
                    }
                });

                ui.separator();

                // Recipes (Multi-Export Targets)
                ui.label(theme::head("Process Recipes (Multi-Export)"));
                ui.label(
                    RichText::new(
                        "Select multiple export formats to generate concurrently from a single decode pass:",
                    )
                    .size(11.0)
                    .color(theme::TEXT_FAINT),
                );

                let mut duplicate = None;
                let mut delete = None;
                for (i, recipe) in self.export_recipes.iter_mut().enumerate() {
                    let editing = self.editing_recipe == Some(i);
                    ui.horizontal(|ui| {
                        widgets::checkbox(ui, &mut recipe.enabled);
                        ui.label(
                            RichText::new(format!("[{}]", recipe.settings.format.label()))
                                .family(theme::mono_bold())
                                .color(theme::ACCENT),
                        );
                        ui.label(RichText::new(&recipe.name).color(if recipe.enabled {
                            theme::TEXT_HI
                        } else {
                            theme::TEXT_DIM
                        }));
                        if let Some(sub) = &recipe.subfolder {
                            ui.label(
                                RichText::new(format!("↳ /{sub}"))
                                    .monospace()
                                    .color(theme::TEXT_FAINT),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::chip(ui, editing, if editing { "done" } else { "edit" })
                                .clicked()
                            {
                                self.editing_recipe = if editing { None } else { Some(i) };
                            }
                        });
                    });
                    if editing {
                        egui::Frame::new()
                            .fill(theme::BG_925)
                            .stroke(Stroke::new(1.0, theme::BORDER))
                            .corner_radius(theme::RADIUS)
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(theme::dim("name"));
                                    ui.text_edit_singleline(&mut recipe.name);
                                });
                                ui.horizontal(|ui| {
                                    ui.label(theme::dim("subfolder"));
                                    let mut sub = recipe.subfolder.clone().unwrap_or_default();
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut sub)
                                                .hint_text("none: straight into the folder"),
                                        )
                                        .changed()
                                    {
                                        // Kept as typed; made one safe folder name where
                                        // the batch builds the path.
                                        recipe.subfolder = (!sub.trim().is_empty()).then_some(sub);
                                    }
                                });
                                crate::export_ui::settings_editor(ui, &mut recipe.settings, None);
                                ui.horizontal(|ui| {
                                    if widgets::button(ui, Kind::Secondary, "Duplicate", 0.0)
                                        .clicked()
                                    {
                                        duplicate = Some(i);
                                    }
                                    if widgets::button(ui, Kind::Secondary, "Delete", 0.0)
                                        .clicked()
                                    {
                                        delete = Some(i);
                                    }
                                });
                            });
                    }
                }
                if let Some(i) = duplicate {
                    let mut copy = self.export_recipes[i].clone();
                    copy.id = format!("{}_copy_{}", copy.id, self.export_recipes.len());
                    copy.name = format!("{} (copy)", copy.name);
                    self.export_recipes.insert(i + 1, copy);
                    self.editing_recipe = Some(i + 1);
                }
                if let Some(i) = delete {
                    self.export_recipes.remove(i);
                    self.editing_recipe = None;
                }
                ui.horizontal(|ui| {
                    if widgets::button(ui, Kind::Secondary, "+ New recipe", 0.0).clicked() {
                        let n = self.export_recipes.len();
                        self.export_recipes.push(batch::ExportRecipe {
                            id: format!("recipe_{n}"),
                            name: format!("Recipe {}", n + 1),
                            enabled: true,
                            settings: self.export_settings,
                            subfolder: None,
                        });
                        self.editing_recipe = Some(n);
                    }
                    if widgets::button(ui, Kind::Secondary, "Reset recipes", 0.0)
                        .on_hover_text("Back to the four built-in recipes")
                        .clicked()
                    {
                        self.export_recipes = batch::ExportRecipe::default_recipes();
                        self.editing_recipe = None;
                    }
                });

                ui.separator();

                // Renaming Pattern
                ui.label(theme::head("Batch Renaming Pattern"));
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.batch_renamer.pattern);
                });
                ui.horizontal(|ui| {
                    ui.label(theme::tag("Tokens:").color(theme::TEXT_FAINT));
                    for token in ["{filename}", "{seq:3}", "{date}", "{camera}", "{rating}"] {
                        if widgets::chip(ui, false, token).clicked() {
                            self.batch_renamer.pattern.push_str(token);
                        }
                    }
                });

                // Renaming Preview
                let sample_path = self
                    .loaded_path
                    .as_ref()
                    .or_else(|| self.shelf.first().map(|it| &it.path))
                    .cloned()
                    .unwrap_or_else(|| PathBuf::from("DSC08450.ARW"));

                // The preview uses the sample photo's own metadata, as the batch will.
                let sample_info = photo_engine::read_info(&photo_engine::copies::source_file(&sample_path));
                let sample_date = sample_info
                    .as_ref()
                    .and_then(|i| i.capture_date_compact())
                    .or_else(|| photo_engine::file_date_compact(&photo_engine::copies::source_file(&sample_path)));
                let sample_ctx = batch::RenameContext {
                    source_path: &sample_path,
                    sequence_index: 0,
                    camera_model: sample_info.as_ref().map(|i| i.model.as_str()),
                    rating: 5,
                    custom_text: "",
                    target_extension: "jpg",
                    date: sample_date.as_deref(),
                };
                let preview_name = self.batch_renamer.format_filename(&sample_ctx);
                ui.horizontal(|ui| {
                    ui.label(theme::tag("Preview:").color(theme::TEXT_FAINT));
                    ui.label(
                        RichText::new(preview_name)
                            .family(theme::mono_bold())
                            .color(theme::TEXT_HI),
                    );
                });

                ui.separator();

                // Progress or Action buttons
                let is_running = self
                    .export_queue
                    .as_ref()
                    .map(|q| q.is_running())
                    .unwrap_or(false);

                if is_running {
                    if let Some(queue) = &mut self.export_queue {
                        if let batch::ExportStatus::Running {
                            total,
                            completed,
                            current_file,
                        } = queue.poll()
                        {
                            let frac = if *total > 0 {
                                *completed as f32 / *total as f32
                            } else {
                                0.0
                            };
                            ui.add(
                                egui::ProgressBar::new(frac)
                                    .text(format!("{completed}/{total} - {current_file}")),
                            );
                        }
                    }
                    let cancelling = self
                        .export_queue
                        .as_ref()
                        .is_some_and(|q| q.is_cancelling());
                    if cancelling {
                        ui.label(
                            theme::tag("stopping after the photo being written…")
                                .color(theme::TEXT_DIM),
                        );
                    } else if widgets::button(ui, Kind::Secondary, "Cancel Export", 120.0)
                        .clicked()
                    {
                        if let Some(queue) = &mut self.export_queue {
                            queue.cancel();
                        }
                    }
                } else {
                    let active_recipes_count =
                        self.export_recipes.iter().filter(|r| r.enabled).count();
                    let total_outputs = count * active_recipes_count;

                    ui.horizontal(|ui| {
                        let btn_text = format!("START EXPORT ({total_outputs} files)");
                        let can_start = total_outputs > 0;
                        ui.add_enabled_ui(can_start, |ui| {
                            if widgets::button(ui, Kind::Primary, &btn_text, 180.0).clicked() {
                                start_export = true;
                            }
                        });
                        if widgets::button(ui, Kind::Secondary, "Close", 80.0).clicked() {
                            close_dialog = true;
                        }
                    });
                }
            });

        self.batch_dialog_open = open && !close_dialog;

        if start_export {
            self.execute_batch_export();
        }
    }

    pub(crate) fn execute_batch_export(&mut self) {
        let Some(out_dir) = self.batch_output_dir.clone() else {
            return;
        };

        // In the order the shelf shows them (its filter and sort), then any selected photo
        // the filter hides: {seq} numbers photos the way they are laid out, not the way a
        // hash set happens to iterate.
        let selected_paths: Vec<PathBuf> = if !self.selection.is_empty() {
            let shown = self.displayed_indices();
            let hidden = (0..self.shelf.len()).filter(|i| !shown.contains(i));
            shown
                .iter()
                .copied()
                .chain(hidden)
                .map(|i| &self.shelf[i].path)
                .filter(|p| self.selection.is_selected(p))
                .cloned()
                .collect()
        } else if let Some(p) = &self.loaded_path {
            vec![p.clone()]
        } else {
            self.shelf.iter().map(|it| it.path.clone()).collect()
        };

        let active_recipes: Vec<batch::ExportRecipe> = self
            .export_recipes
            .iter()
            .filter(|r| r.enabled)
            .cloned()
            .collect();

        if selected_paths.is_empty() || active_recipes.is_empty() {
            return;
        }

        let mut jobs = Vec::new();
        let mut claimed = std::collections::HashSet::new();

        for (seq_idx, src_path) in selected_paths.iter().enumerate() {
            let info = photo_engine::read_info(&photo_engine::copies::source_file(src_path));
            let date = info
                .as_ref()
                .and_then(|i| i.capture_date_compact())
                .or_else(|| {
                    photo_engine::file_date_compact(&photo_engine::copies::source_file(src_path))
                });
            let (stack, crop, rating) = if Some(src_path) == self.loaded_path.as_ref() {
                let rating = self
                    .shelf
                    .iter()
                    .find(|it| &it.path == src_path)
                    .map(|it| it.metadata.rating)
                    .unwrap_or(0);
                (self.stack.clone(), self.stack.geometry.crop, rating)
            } else {
                let (stack, meta) = match photo_engine::load_sidecar_full(src_path) {
                    Ok(Some(sc)) => (sc.stack, sc.metadata),
                    _ => (None, photo_engine::AssetMetadata::default()),
                };
                // Never opened, or only rated: exported as shot, which includes the way the
                // camera was held.
                let stack = stack.unwrap_or_else(|| PhotoStack {
                    geometry: photo_engine::file_orientation(&photo_engine::copies::source_file(
                        src_path,
                    )),
                    ..PhotoStack::default()
                });
                let crop = stack.geometry.crop;
                (stack, crop, meta.rating)
            };

            for recipe in &active_recipes {
                let target_dir = match &recipe.subfolder {
                    Some(sub) => out_dir.join(batch::renamer::sanitize_component(sub)),
                    None => out_dir.clone(),
                };

                let ctx = batch::RenameContext {
                    source_path: src_path,
                    sequence_index: seq_idx,
                    camera_model: info.as_ref().map(|i| i.model.as_str()),
                    rating,
                    custom_text: "",
                    target_extension: recipe.settings.format.extension(),
                    date: date.as_deref(),
                };

                if let Some(target_path) =
                    self.batch_renamer
                        .resolve_target_path(&target_dir, &ctx, &mut claimed)
                {
                    jobs.push(batch::ExportJob {
                        source_path: src_path.clone(),
                        stack: stack.clone(),
                        crop,
                        output_path: target_path,
                        recipe: recipe.clone(),
                    });
                }
            }
        }

        if let Some(queue) = &mut self.export_queue {
            queue.start_batch(jobs);
        }
    }
}
