//! The photo being edited: choosing it, saving it, rating it, and pasting onto it.

use crate::*;

/// How many photos' undo histories are kept after stepping away from them. Each is at
/// most 256 stacks of a few hundred bytes.
pub(crate) const HISTORY_KEPT: usize = 20;

impl PhotoApp {
    /// Save the open photo's edits now (Ctrl+S), whether or not autosave would have.
    pub fn save_current_sidecar(&mut self) {
        let Some(path) = self.loaded_path.clone() else {
            return;
        };
        if self.write_open_sidecar() {
            let name = photo_engine::sidecar_path(&path);
            self.set_status(format!("Saved sidecar: {}", file_name(&name)));
        }
    }

    /// Write the open photo's edits to its sidecar if they differ from what is on disk.
    /// Called when leaving a photo, a second after an edit settles, and by the shell on
    /// close, suspend and memory warnings: DESIGN.md forbids "are you sure", so nothing
    /// may depend on the user remembering to save. Returns false if something could not
    /// be written, and says why.
    pub fn flush(&mut self) -> bool {
        self.autosave_due = None;
        self.persist_session();
        if self.loaded_path.is_none() {
            return true;
        }
        let on_disk = self.saved_stack.as_ref().unwrap_or(&self.baseline);
        if self.committed_stack() == *on_disk {
            return true;
        }
        self.write_open_sidecar()
    }

    /// The stack as it stands outside any gesture still in progress: a crop being framed
    /// is not part of it until it is applied.
    pub(crate) fn committed_stack(&self) -> PhotoStack {
        let mut stack = self.stack.clone();
        if self.cropping {
            stack.geometry.crop = self.crop_before;
        }
        stack
    }

    pub(crate) fn write_open_sidecar(&mut self) -> bool {
        let Some(path) = self.loaded_path.clone() else {
            return true;
        };
        if self.decode_failed {
            return true;
        }
        if self.sidecar_locked {
            self.error = Some(format!(
                "{}: edits are not being saved — its sidecar could not be read or is from a \
                 newer Duckfoot, and is left untouched",
                file_name(&path)
            ));
            return false;
        }
        // The camera orientation is the baseline of an unsaved photo; writing before it is
        // known would record the photo as unrotated for good.
        if self.orientation_pending && self.saved_stack.is_none() {
            let orientation = photo_engine::file_orientation(&path);
            self.apply_orientation(orientation);
        }
        let metadata = self
            .shelf
            .iter()
            .find(|it| it.path == path)
            .map(|it| it.metadata.clone())
            .unwrap_or_default();
        let stack = self.committed_stack();
        match photo_engine::save_sidecar_full(&path, Some(&stack), &metadata) {
            Ok(_) => {
                if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
                    item.has_sidecar = true;
                    item.is_edited = false;
                }
                self.saved_stack = Some(stack);
                true
            }
            Err(e) => {
                self.error = Some(format!(
                    "Could not save edits to {}: {e:#}",
                    file_name(&path)
                ));
                false
            }
        }
    }

    /// Flush, and mark the session as closed cleanly. The shell calls this on the way out.
    pub fn shutdown(&mut self) {
        self.flush();
        if let Some(lock) = session_lock_path(self.settings_dir.as_deref()) {
            let _ = std::fs::remove_file(lock);
        }
    }

    /// Write a rating or flag without touching the stack on disk. A photo that was never
    /// opened keeps a sidecar with no stack, so its camera orientation still decides how
    /// it opens.
    pub(crate) fn update_metadata(
        &mut self,
        p: &Path,
        change: impl Fn(&mut photo_engine::AssetMetadata),
    ) {
        let Some(item) = self.shelf.iter_mut().find(|it| it.path == p) else {
            return;
        };
        change(&mut item.metadata);
        let metadata = item.metadata.clone();
        match photo_engine::update_sidecar(p, |data| data.metadata = metadata.clone()) {
            Ok(_) => item.has_sidecar = true,
            Err(e) => {
                self.error = Some(format!("Could not save to {}: {e:#}", file_name(p)));
            }
        }
    }

    pub fn set_rating_for_path(&mut self, p: &Path, rating: u8) {
        self.update_metadata(p, |m| m.rating = rating.min(5));
    }

    pub fn set_flag_for_path(&mut self, p: &Path, flag: photo_engine::Flag) {
        self.update_metadata(p, |m| m.flag = flag);
    }

    pub fn set_rating_for_selection(&mut self, rating: u8) {
        let paths: Vec<PathBuf> = if !self.selection.is_empty() {
            self.selection.selected.iter().cloned().collect()
        } else if let Some(p) = &self.loaded_path {
            vec![p.clone()]
        } else {
            return;
        };

        let count = paths.len();
        for p in &paths {
            self.set_rating_for_path(p, rating);
        }
        self.set_status(format!("Set {rating}★ on {count} photo(s)"));
    }

    pub fn set_flag_for_selection(&mut self, flag: photo_engine::Flag) {
        let paths: Vec<PathBuf> = if !self.selection.is_empty() {
            self.selection.selected.iter().cloned().collect()
        } else if let Some(p) = &self.loaded_path {
            vec![p.clone()]
        } else {
            return;
        };

        let count = paths.len();
        let flag_str = match flag {
            photo_engine::Flag::Pick => "Pick",
            photo_engine::Flag::Reject => "Reject",
            photo_engine::Flag::Unflagged => "Unflagged",
        };
        for p in &paths {
            self.set_flag_for_path(p, flag);
        }
        self.set_status(format!("Set {flag_str} on {count} photo(s)"));
    }

    pub fn open_session_folder(&mut self, gpu: &Gpu, folder: PathBuf) {
        let images = dam::FolderTree::scan_supported_images(&folder);
        if !images.is_empty() {
            self.folder_tree.current_folder = Some(folder);
            self.add_to_shelf(gpu, images);
            self.folder_browser_open = false;
        } else {
            self.set_status("No supported images found in folder");
        }
    }

    pub fn revert_to_sidecar(&mut self) {
        let Some(path) = self.loaded_path.clone() else {
            return;
        };
        match load_sidecar(&path) {
            Ok(Some(saved)) => {
                self.history.checkpoint(&self.stack);
                self.saved_stack = Some(saved.clone());
                self.stack = saved;
                self.needs_render = true;
                if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
                    item.is_edited = false;
                }
                self.set_status("Reverted to saved sidecar");
            }
            Ok(None) => {
                self.set_status("No saved edits to revert to");
            }
            Err(e) => {
                self.error = Some(format!("Failed to load sidecar: {e:#}"));
            }
        }
    }

    pub fn reset_to_default(&mut self) {
        self.history.checkpoint(&self.stack);
        self.stack = self.baseline.clone();
        self.needs_render = true;
        self.set_status("Reset all adjustments to default");
    }

    pub fn copy_adjustments(&mut self) {
        if self.loaded_path.is_none() {
            return;
        }
        self.clipboard_stack = Some(self.stack.clone());
        // A new look on the clipboard: the last paste to all is settled.
        self.bulk_paste_undo = None;
        self.set_status(format!(
            "Copied adjustments (paste carries {})",
            self.paste_set.describe()
        ));
    }

    /// The clipboard's look on `target`, as far as the paste set carries it: by default
    /// every module on `target`'s own framing, since rotation, mirroring, straighten and
    /// crop belong to a photo, not to a look, and pasting them turns and crops photos
    /// that were never framed that way.
    pub(crate) fn pasted_onto(&self, copied: &PhotoStack, target: &PhotoStack) -> PhotoStack {
        self.paste_set.apply(copied, target)
    }

    pub fn paste_adjustments(&mut self) {
        let Some(copied) = &self.clipboard_stack else {
            self.set_status("Clipboard is empty");
            return;
        };
        if self.loaded_path.is_none() {
            return;
        }
        let pasted = self.pasted_onto(copied, &self.stack);
        self.history.checkpoint(&self.stack);
        self.stack = pasted;
        self.needs_render = true;
        self.set_status("Pasted adjustments from clipboard");
    }

    /// The edits of the photo open before this one, on this one's framing: stepping
    /// through a burst, each frame takes the last one's look in one tap, and the
    /// clipboard is left alone. A previous photo with no edits passes on the defaults.
    pub fn same_as_previous(&mut self) {
        let Some(prev) = self.previous_photo.clone() else {
            return;
        };
        if self.loaded_path.is_none() {
            return;
        }
        let look = match photo_engine::load_sidecar(&prev) {
            Ok(stack) => stack.unwrap_or_default(),
            Err(e) => {
                self.error = Some(format!("{}: {e:#}", file_name(&prev)));
                return;
            }
        };
        let pasted = self.pasted_onto(&look, &self.stack);
        if pasted == self.stack {
            self.set_status(format!("Already the same as {}", file_name(&prev)));
            return;
        }
        self.history.checkpoint(&self.stack);
        self.stack = pasted;
        self.needs_render = true;
        self.set_status(format!("Same edits as {}", file_name(&prev)));
    }

    pub fn can_same_as_previous(&self) -> bool {
        self.previous_photo.is_some() && self.can_edit()
    }

    /// Paste onto every photo on the shelf. Each keeps its own framing, the open photo
    /// takes the paste as an undoable edit, and the sidecars it replaced are kept so the
    /// whole paste can be taken back with [`Self::undo_paste_to_all`].
    pub fn paste_to_all_in_shelf(&mut self) {
        let Some(copied) = self.clipboard_stack.clone() else {
            self.set_status("Clipboard is empty");
            return;
        };
        let mut before = Vec::new();
        let mut count = 0;
        let mut failed = Vec::new();
        let paths: Vec<PathBuf> = self.shelf.iter().map(|it| it.path.clone()).collect();
        for path in paths {
            if self.loaded_path.as_ref() == Some(&path) {
                let pasted = self.pasted_onto(&copied, &self.stack);
                if pasted != self.stack {
                    self.history.checkpoint(&self.stack);
                    self.stack = pasted;
                    self.needs_render = true;
                }
                count += 1;
                continue;
            }
            let sidecar = photo_engine::sidecar_path(&path);
            let previous = std::fs::read(&sidecar).ok();
            let result = photo_engine::update_sidecar(&path, |data| {
                let own = data.stack.clone().unwrap_or_else(|| PhotoStack {
                    geometry: photo_engine::file_orientation(&path),
                    ..PhotoStack::default()
                });
                data.stack = Some(self.pasted_onto(&copied, &own));
            });
            match result {
                Ok(_) => {
                    before.push((path.clone(), previous));
                    if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
                        item.has_sidecar = true;
                    }
                    count += 1;
                }
                Err(_) => failed.push(file_name(&path)),
            }
        }
        self.bulk_paste_undo = Some(before);
        if failed.is_empty() {
            self.set_status_undoable(
                format!("Pasted onto {count} photos"),
                NoticeAction::PasteToAll,
            );
        } else {
            self.error = Some(format!(
                "Pasted onto {count} photos; could not write {}",
                failed.join(", ")
            ));
        }
    }

    /// Put back every sidecar the last paste to all replaced.
    pub fn undo_paste_to_all(&mut self) {
        let Some(before) = self.bulk_paste_undo.take() else {
            return;
        };
        let count = before.len();
        for (path, previous) in before {
            let sidecar = photo_engine::sidecar_path(&path);
            let restored = match previous {
                Some(bytes) => photo_engine::fsio::atomic_write_bytes(&sidecar, &bytes),
                None => std::fs::remove_file(&sidecar).map_err(anyhow::Error::from),
            };
            if restored.is_err() {
                self.error = Some(format!("Could not restore {}", file_name(&sidecar)));
            }
            if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
                item.has_sidecar = find_sidecar(&path).is_some();
            }
        }
        self.set_status(format!("Took the paste back off {count} photos"));
    }

    pub fn can_undo_paste_to_all(&self) -> bool {
        self.bulk_paste_undo.is_some()
    }

    pub fn add_to_shelf(&mut self, gpu: &Gpu, paths: Vec<PathBuf>) {
        let open_first = self.loaded_path.is_none();
        let mut to_open = None;
        for path in paths {
            if !self.shelf.iter().any(|it| it.path == path) {
                if open_first && to_open.is_none() {
                    to_open = Some(path.clone());
                }
                self.request_thumbnail(&path);
                self.shelf.push(ShelfItem::new(path));
            }
        }
        if let Some(first) = to_open {
            self.edit_when_opened = true;
            self.open_path(gpu, first);
        }
    }

    pub(crate) fn request_thumbnail(&mut self, path: &std::path::Path) {
        if !path.exists() {
            return;
        }
        if self.thumbs_requested.insert(path.to_path_buf()) {
            self.loader.thumbnail(path.to_path_buf());
        }
    }

    /// Step to the previous (-1) or next (+1) photo as the shelf shows them: through the
    /// active filter and sort, so stepping with "picks only" on stays on picks.
    pub(crate) fn step_photo(&mut self, gpu: &Gpu, dir: isize) {
        let order = self.displayed_indices();
        if order.is_empty() {
            return;
        }
        let current = self
            .loading
            .as_ref()
            .map(|(p, _)| p)
            .or(self.loaded_path.as_ref())
            .and_then(|p| order.iter().position(|&i| &self.shelf[i].path == p));
        let n = order.len() as isize;
        let next = match current {
            Some(i) => (i as isize + dir).rem_euclid(n) as usize,
            None => 0,
        };
        // Past any photo whose file is missing, unless every one is.
        let n = order.len();
        let step = if dir < 0 { n - 1 } else { 1 };
        let found = (0..n)
            .map(|k| order[(next + k * step) % n])
            .find(|&i| !self.shelf[i].missing);
        let path = self.shelf[found.unwrap_or(order[next])].path.clone();
        self.open_path(gpu, path);
    }

    /// Make `path` the photo being edited, and start decoding it.
    ///
    /// The controls move at once (U-01): the photo being left is saved, and the sliders
    /// show the new photo's own saved edits — or defaults, if it has none — from this
    /// frame on, while the decode runs in the background. Tone and colour can be edited
    /// straight away and appear when the pixels land; rotate, flip, crop and white balance
    /// wait for the decode, which is what knows the photo's true orientation and camera.
    pub fn open_path(&mut self, _gpu: &Gpu, path: PathBuf) {
        if !path.exists() {
            if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
                item.missing = true;
            }
            self.set_status(format!(
                "{} is missing: reconnect its drive, or remove it from the shelf",
                file_name(&path)
            ));
            return;
        }
        if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
            item.missing = false;
        }
        if self.loaded_path.as_ref() == Some(&path) {
            let decoding_it = self.loading.as_ref().is_some_and(|(p, _)| *p == path);
            if decoding_it || self.engine_path.as_ref() == Some(&path) {
                return;
            }
        } else {
            // An unapplied crop is never saved, and a slider or crop drag in progress
            // belongs to the photo it started on.
            self.cancel_crop();
            self.flush();
            self.begin_editing(&path);
        }
        self.open_generation += 1;
        self.loading = Some((path.clone(), Instant::now()));
        self.stage_preview = None;
        self.loader.open(path, self.open_generation);
    }

    /// Point every control at `path`: its sidecar's stack, or defaults.
    pub(crate) fn begin_editing(&mut self, path: &Path) {
        self.error = None;
        // A mask belongs to the photo it was drawn on.
        self.selected_mask = None;
        self.mask_drag = None;
        self.sidecar_locked = false;
        self.decode_failed = false;
        let (saved, metadata, snapshots) = match photo_engine::load_sidecar_full(path) {
            Ok(Some(data)) => {
                if data.is_from_newer_version() {
                    self.sidecar_locked = true;
                    self.error = Some(format!(
                        "{}: its sidecar is from a newer Duckfoot. Shown as saved; edits \
                         here will not be written over it.",
                        file_name(path)
                    ));
                }
                let snapshots = data
                    .snapshots
                    .into_iter()
                    .map(|s| (s.name, s.stack))
                    .collect();
                (data.stack, data.metadata, snapshots)
            }
            Ok(None) => (None, photo_engine::AssetMetadata::default(), Vec::new()),
            Err(e) => {
                // Holding edits this build cannot read is not the same as holding none:
                // show it, and never write over it.
                self.sidecar_locked = true;
                self.error = Some(format!(
                    "{}: {e:#}. Its edits were not loaded, and the file is left untouched.",
                    file_name(path)
                ));
                (None, photo_engine::AssetMetadata::default(), Vec::new())
            }
        };

        if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
            item.metadata = metadata;
            item.is_edited = false;
        } else {
            self.request_thumbnail(path);
            let mut item = ShelfItem::new(path.to_path_buf());
            item.metadata = metadata;
            self.shelf.push(item);
        }
        // Stepping through a selection built for a batch keeps it.
        let keep_selection =
            self.selection.selected.len() > 1 && self.selection.selected.contains(path);
        if !keep_selection {
            self.selection.select_single(path.to_path_buf());
        }

        self.baseline = PhotoStack::default();
        self.orientation_pending = true;
        self.stack = saved.clone().unwrap_or_else(|| self.baseline.clone());
        self.saved_stack = saved;
        self.autosave_seen = self.stack.clone();
        self.autosave_due = None;
        // The history of the photo being left is parked, and this photo's taken back if
        // it was edited earlier in the session: undo belongs to a photo, not to the app.
        let history = self
            .parked_histories
            .iter()
            .position(|(p, _)| p == path)
            .map(|i| self.parked_histories.remove(i).1)
            .unwrap_or_else(History::new);
        let left = std::mem::replace(&mut self.history, history);
        if let Some(previous) = self.loaded_path.take() {
            self.previous_photo = Some(previous.clone());
            if !left.past.is_empty() || !left.future.is_empty() {
                self.parked_histories.push((previous, left));
                if self.parked_histories.len() > HISTORY_KEPT {
                    self.parked_histories.remove(0);
                }
            }
        }
        self.snapshots = snapshots;
        self.loaded_path = Some(path.to_path_buf());
        self.view = View::default();
        self.cropping = false;
        self.crop_drag = None;
        self.needs_render = true;
    }

    /// The camera's orientation for the open photo is now known. It is what "no edits"
    /// means for it, so the baseline takes it — and so does a stack that is still the
    /// baseline, along with its undo history. A saved stack already carries its own.
    pub(crate) fn apply_orientation(&mut self, orientation: Geometry) {
        self.orientation_pending = false;
        let guessed = self.baseline.geometry;
        if guessed == orientation {
            return;
        }
        self.baseline.geometry = orientation;
        if self.saved_stack.is_none() {
            if self.stack.geometry == guessed {
                self.stack.geometry = orientation;
                self.needs_render = true;
            }
            if self.autosave_seen.geometry == guessed {
                self.autosave_seen.geometry = orientation;
            }
            self.history.rebase_geometry(guessed, orientation);
        }
    }

    /// The open photo, once its pixels are on the GPU. While a newly chosen photo decodes
    /// the engine still holds the last one, which must not be drawn, measured or exported
    /// as if it were the photo the controls now describe.
    pub(crate) fn ready_image(&self) -> Option<&photo_engine::LoadedImage> {
        if self.engine_path.is_some() && self.engine_path == self.loaded_path {
            self.engine.image()
        } else {
            None
        }
    }

    /// The photo's pixels are on the GPU and match the controls.
    pub fn is_photo_ready(&self) -> bool {
        self.ready_image().is_some()
    }

    /// Collect whatever the workers have finished.
    pub(crate) fn poll_loader(&mut self, ctx: &egui::Context, gpu: &Gpu) {
        while let Ok(done) = self.loader.done.try_recv() {
            match done {
                loader::Done::Opened {
                    path,
                    generation,
                    result,
                    decode_ms,
                } if generation == self.open_generation => {
                    self.loading = None;
                    self.stage_preview = None;
                    match result {
                        Ok(source) => {
                            self.decode_ms = decode_ms;
                            self.finish_open(gpu, path, source);
                        }
                        // DESIGN.md: failures surface in the UI, with the filename.
                        Err(e) => {
                            self.error = Some(e);
                            self.edit_when_opened = false;
                            if self.loaded_path.as_ref() == Some(&path) {
                                self.decode_failed = true;
                            }
                        }
                    }
                }
                // Superseded by a newer request while it was decoding.
                loader::Done::Opened { .. } => {}
                loader::Done::Preview {
                    path,
                    generation,
                    image,
                    orientation,
                } => {
                    // Only while its own decode is still running; a preview that loses the
                    // race to the real image has nothing left to stand in for.
                    let current = self.loading.as_ref().is_some_and(|(p, _)| *p == path);
                    if generation == self.open_generation && current {
                        if self.orientation_pending {
                            self.apply_orientation(orientation);
                        }
                        let texture =
                            ctx.load_texture("stage preview", image, egui::TextureOptions::LINEAR);
                        self.stage_preview = Some((path, texture));
                    }
                }
                loader::Done::Thumbnail { path, image, info } => {
                    self.thumbs_answered += 1;
                    if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
                        item.info = info;
                    }
                    if let Some(image) = image {
                        let name = format!("thumb:{}", path.display());
                        let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
                        self.thumbs.insert(path, texture);
                    }
                }
            }
        }
    }

    pub(crate) fn finish_open(
        &mut self,
        gpu: &Gpu,
        path: PathBuf,
        uploaded: Box<photo_engine::UploadedImage>,
    ) {
        if self.loaded_path.as_ref() != Some(&path) {
            // The user has moved on; the generation check makes this rare, not impossible.
            return;
        }
        let orientation = uploaded.orientation;
        self.release_texture(gpu);
        self.engine_path = None;

        // The pixels are already on the GPU (the loader uploaded them); what is left here
        // is recording the demosaic.
        let load_res = self.engine.load_uploaded(gpu.device, gpu.queue, *uploaded);

        if let Err(e) = load_res {
            // DESIGN.md: failures surface in the UI, with the filename.
            self.error = Some(format!("{}: {e:#}", path.display()));
            self.edit_when_opened = false;
            self.decode_failed = true;
            return;
        }
        self.engine_path = Some(path.clone());
        self.apply_orientation(orientation);
        if std::mem::take(&mut self.edit_when_opened) {
            self.compact_tab = Some(compact::Tab::Edit);
            self.compact_controls_open = true;
        }
        if self.saved_stack.is_some() {
            self.set_status(format!("Loaded sidecar for {}", file_name(&path)));
        }
        self.view = View::default();
        self.needs_render = true;
    }

    pub(crate) fn release_texture(&mut self, gpu: &Gpu) {
        if let Some(id) = self.texture_id.take() {
            renderer_write(gpu).free_texture(&id);
        }
        self.engine.unload();
    }

    /// Write the open photo's snapshots into its sidecar, leaving the rest of it alone.
    pub(crate) fn save_snapshots(&mut self) {
        let Some(path) = self.loaded_path.clone() else {
            return;
        };
        if self.sidecar_locked || self.decode_failed {
            return;
        }
        let snapshots: Vec<photo_engine::Snapshot> = self
            .snapshots
            .iter()
            .map(|(name, stack)| photo_engine::Snapshot {
                name: name.clone(),
                stack: stack.clone(),
            })
            .collect();
        match photo_engine::update_sidecar(&path, |data| data.snapshots = snapshots.clone()) {
            Ok(_) => {
                if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
                    item.has_sidecar = true;
                }
            }
            Err(e) => {
                self.error = Some(format!(
                    "Could not save snapshots for {}: {e:#}",
                    file_name(&path)
                ));
            }
        }
    }

    /// Save the open photo's current stack as a named snapshot, in its sidecar.
    pub fn take_snapshot(&mut self, name: &str) {
        if self.ready_image().is_none() {
            return;
        }
        let name = if name.trim().is_empty() {
            format!("snapshot {}", self.snapshots.len() + 1)
        } else {
            name.trim().to_string()
        };
        self.snapshots.push((name, self.stack.clone()));
        self.save_snapshots();
    }

    /// Snapshot `i`'s stack, as one undo step.
    pub fn recall_snapshot(&mut self, i: usize) {
        let Some((_, stack)) = self.snapshots.get(i) else {
            return;
        };
        if *stack == self.stack {
            return;
        }
        let stack = stack.clone();
        self.history.checkpoint(&self.stack);
        self.stack = stack;
        self.needs_render = true;
    }

    /// Delete snapshot `i`; the notice's Undo puts it back.
    pub fn delete_snapshot(&mut self, i: usize) {
        let (Some(path), true) = (self.loaded_path.clone(), i < self.snapshots.len()) else {
            return;
        };
        let snapshot = self.snapshots.remove(i);
        self.save_snapshots();
        let msg = format!("Deleted snapshot \u{201c}{}\u{201d}", snapshot.0);
        self.deleted_snapshot = Some((path, i, snapshot));
        self.set_status_undoable(msg, NoticeAction::DeleteSnapshot);
    }

    pub fn undo_delete_snapshot(&mut self) {
        let Some((path, i, snapshot)) = self.deleted_snapshot.take() else {
            return;
        };
        if self.loaded_path.as_ref() != Some(&path) {
            // Another photo is open now: put it back in the file it came from.
            let _ = photo_engine::update_sidecar(&path, |data| {
                let at = i.min(data.snapshots.len());
                data.snapshots.insert(
                    at,
                    photo_engine::Snapshot {
                        name: snapshot.0.clone(),
                        stack: snapshot.1.clone(),
                    },
                );
            });
            return;
        }
        let at = i.min(self.snapshots.len());
        self.snapshots.insert(at, snapshot);
        self.save_snapshots();
    }

    /// Undo or redo to step `k` of the open photo's history.
    pub fn jump_history(&mut self, k: usize) {
        if self.history.jump_to(&mut self.stack, k) {
            self.needs_render = true;
        }
    }

    /// A file or folder dropped on the window. The shell sends one call per file, so they
    /// are collected here and shelved together on the next frame, instead of each
    /// superseding the one before and only the last surviving.
    pub fn drop_file(&mut self, path: PathBuf) {
        self.pending_drops.push(path);
    }

    pub(crate) fn take_drops(&mut self, gpu: &Gpu) {
        if self.pending_drops.is_empty() {
            return;
        }
        let mut photos = Vec::new();
        for path in std::mem::take(&mut self.pending_drops) {
            if path.is_dir() {
                photos.extend(dam::FolderTree::scan_supported_images(&path));
            } else if photo_engine::is_supported_path(&path) {
                photos.push(path);
            }
        }
        let Some(first) = photos.first().cloned() else {
            self.set_status("Nothing dropped that Duckfoot can open");
            return;
        };
        let count = photos.len();
        self.add_to_shelf(gpu, photos);
        self.open_path(gpu, first);
        if count > 1 {
            self.set_status(format!("Added {count} photos to the shelf"));
        }
    }

    /// Take photos off the shelf. Nothing on disk changes; the notice's Undo puts them
    /// back where they were.
    pub fn remove_from_shelf(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        if self.loaded_path.as_ref().is_some_and(|p| paths.contains(p)) {
            self.flush();
        }
        let mut removed = Vec::new();
        let mut i = 0;
        let mut index = 0;
        while i < self.shelf.len() {
            if paths.contains(&self.shelf[i].path) {
                removed.push((index, self.shelf.remove(i)));
            } else {
                i += 1;
            }
            index += 1;
        }
        for (_, item) in &removed {
            self.selection.selected.remove(&item.path);
            // The texture goes with its handle.
            self.thumbs.remove(&item.path);
            self.thumbs_requested.remove(&item.path);
        }
        let n = removed.len();
        self.removed_from_shelf = removed;
        self.set_status_undoable(
            if n == 1 {
                "Removed 1 photo from the shelf".to_string()
            } else {
                format!("Removed {n} photos from the shelf")
            },
            NoticeAction::Remove,
        );
    }

    pub fn undo_remove_from_shelf(&mut self) {
        let removed = std::mem::take(&mut self.removed_from_shelf);
        let n = removed.len();
        for (index, item) in removed {
            let path = item.path.clone();
            self.shelf.insert(index.min(self.shelf.len()), item);
            self.request_thumbnail(&path);
        }
        self.set_status(format!("Put {n} back on the shelf"));
    }
}
