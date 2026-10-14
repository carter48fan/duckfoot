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
            let orientation =
                photo_engine::file_orientation(&photo_engine::copies::source_file(&path));
            self.apply_orientation(orientation);
        }
        let metadata = self
            .shelf
            .iter()
            .find(|it| it.path == path)
            .map(|it| it.metadata.clone())
            .unwrap_or_default();
        let stack = self.committed_stack();
        match photo_engine::save_sidecar_with_history(&path, &stack, &metadata, &self.history.past)
        {
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
        // Marked before its sidecar was read: read it now, so the marks it already has
        // are not written over with nothing.
        if item.pending {
            item.take(ShelfFacts::read(p));
            self.shelf_pending.remove(p);
        }
        change(&mut item.metadata);
        let metadata = item.metadata.clone();
        match photo_engine::update_sidecar(p, |data| data.metadata = metadata.clone()) {
            Ok(_) => item.has_sidecar = true,
            Err(e) => {
                self.error = Some(format!("Could not save to {}: {e:#}", file_name(p)));
            }
        }
        // A virtual copy's marks are its own; the file's .xmp follows the original.
        if self.settings.xmp_sidecars && photo_engine::copies::parse(p).is_none() {
            if let Err(e) = photo_engine::xmp::update_sidecar(p, &metadata) {
                self.error = Some(format!("Could not update the .xmp sidecar: {e:#}"));
            }
        }
        self.bump_shelf_epoch();
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

    /// Shelve a folder's photos, and its folders' four deep (WP5.8), read on the
    /// loader's thread; the folders that held photos are watched.
    pub fn open_session_folder(&mut self, _gpu: &Gpu, folder: PathBuf) {
        self.scans_pending += 1;
        self.reading = Some(file_name(&folder));
        self.reads += 1;
        self.loader.file_job(loader::FileJob::Scan {
            folder,
            purpose: loader::ScanFor::Open,
            depth: 4,
            epoch: Some(self.loader.scan_epoch()),
        });
    }

    /// `dir`'s folders for the browser: the last listing, asked for again off this thread
    /// when it is for another folder or a few seconds old (WP5.3).
    pub(crate) fn cached_subfolders(&mut self, dir: &std::path::Path) -> Vec<PathBuf> {
        let fresh = |t: &Instant| t.elapsed() < Duration::from_secs(3);
        match &mut self.subfolders {
            Some((d, subs, asked)) if d == dir => {
                if !fresh(asked) {
                    *asked = Instant::now();
                    self.loader
                        .file_job(loader::FileJob::Subfolders(dir.to_path_buf()));
                }
                subs.clone()
            }
            _ => {
                self.subfolders = Some((dir.to_path_buf(), Vec::new(), Instant::now()));
                self.loader
                    .file_job(loader::FileJob::Subfolders(dir.to_path_buf()));
                Vec::new()
            }
        }
    }

    /// While a folder is read: how far it has got, with a button that stops it (WP5.8).
    pub(crate) fn show_reading(&mut self) {
        let Some(name) = &self.reading else { return };
        let (folders, photos) = self.loader.scan_progress();
        let msg = match (folders, photos) {
            (0, _) => format!("Reading {name}…"),
            (1, p) => format!("Reading {name}… {p} photos"),
            (f, p) => format!("Reading {name}… {p} photos in {f} folders"),
        };
        self.set_status_undoable(msg, NoticeAction::CancelScan);
    }

    /// Stops the folder reads under way; nothing they found is shelved.
    pub fn cancel_reading(&mut self) {
        if self.reading.take().is_some() {
            self.loader.cancel_scans();
            self.set_status("Stopped reading the folder");
        }
    }

    /// A folder read has come back.
    fn take_scan_result(
        &mut self,
        gpu: &Gpu,
        folder: PathBuf,
        purpose: loader::ScanFor,
        found: Vec<(PathBuf, Vec<PathBuf>)>,
    ) {
        self.scans_pending = self.scans_pending.saturating_sub(1);
        if purpose == loader::ScanFor::Watch {
            let listing = found
                .iter()
                .find(|(d, _)| *d == folder)
                .map(|(_, l)| l.clone())
                .unwrap_or_default();
            if folder.is_dir() {
                self.folder_watch.watch(folder, &listing);
            }
            return;
        }
        let images: Vec<PathBuf> = found.iter().flat_map(|(_, l)| l.iter().cloned()).collect();
        let Some(first) = images.first().cloned() else {
            self.set_status(format!(
                "No photos in {} or its folders",
                file_name(&folder)
            ));
            return;
        };
        let (count, folders) = (images.len(), found.len());
        if purpose == loader::ScanFor::Open {
            for (dir, listing) in &found {
                self.folder_watch.watch(dir.clone(), listing);
            }
            self.folder_tree.current_folder = Some(folder);
            self.folder_browser_open = false;
        }
        let open_first = purpose == loader::ScanFor::Drop || self.loaded_path.is_none();
        self.add_to_shelf(gpu, images);
        if open_first {
            self.open_path(gpu, first);
        }
        self.set_status(if folders > 1 {
            format!("Added {count} photos from {folders} folders")
        } else {
            format!("Added {count} photos")
        });
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
                    geometry: photo_engine::file_orientation(&photo_engine::copies::source_file(
                        &path,
                    )),
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
                // Its thumbnail is asked for when its tile is first in view (WP5.2), its
                // sidecar and size read off this thread (WP5.3).
                self.shelf_pending.insert(path.clone());
                self.loader.file_job(loader::FileJob::Shelf(path.clone()));
                self.shelf.push(ShelfItem::waiting(path));
            }
        }
        self.bump_shelf_epoch();
        if let Some(first) = to_open {
            self.edit_when_opened = true;
            self.open_path(gpu, first);
        }
    }

    /// A virtual copy of `of` (W-05): its edits and marks as they stand, with a history of
    /// its own, shelved after it and its other copies, and opened.
    pub fn make_virtual_copy(&mut self, gpu: &Gpu, of: &std::path::Path) {
        if self.loaded_path.as_deref() == Some(of) {
            self.flush();
        }
        let copy =
            photo_engine::copies::next_copy(of, |p| self.shelf.iter().any(|it| it.path == p));
        let (stack, metadata) = match photo_engine::load_sidecar_full(of) {
            Ok(Some(data)) => (data.stack, data.metadata),
            _ => (None, photo_engine::AssetMetadata::default()),
        };
        // An unedited photo's copy starts from the camera's orientation, as it would.
        let stack = stack.unwrap_or_else(|| PhotoStack {
            geometry: photo_engine::file_orientation(&photo_engine::copies::source_file(of)),
            ..PhotoStack::default()
        });
        if let Err(e) = photo_engine::save_sidecar_full(&copy, Some(&stack), &metadata) {
            self.error = Some(format!("Could not make a copy of {}: {e:#}", file_name(of)));
            return;
        }
        let source = photo_engine::copies::source_file(of).into_owned();
        let same_photo =
            |p: &std::path::Path| photo_engine::copies::source_file(p).as_ref() == source;
        let at = match self.shelf.iter().position(|it| it.path == of) {
            Some(i) => {
                i + 1
                    + self.shelf[i + 1..]
                        .iter()
                        .take_while(|it| same_photo(&it.path))
                        .count()
            }
            None => self.shelf.len(),
        };
        self.request_thumbnail(&copy);
        self.shelf.insert(at, ShelfItem::new(copy.clone()));
        self.bump_shelf_epoch();
        self.open_path(gpu, copy.clone());
        self.set_status(crate::i18n::tr_args(
            "virtual-copy-made",
            &[("name", file_name(&copy).into())],
        ));
    }

    /// Take in what the last read of the watched folders found, and start the next.
    pub(crate) fn poll_folders(&mut self, ctx: &egui::Context, gpu: &Gpu) {
        if self.folder_watch.folders().is_empty() {
            return;
        }
        if let Some(scan) = self.folder_watch.poll(std::time::Instant::now()) {
            self.take_scan(gpu, scan);
        }
        ctx.request_repaint_after(dam::watch::INTERVAL);
    }

    /// Read the watched folders now and take in what they hold.
    pub fn rescan_folders_now(&mut self, gpu: &Gpu) {
        let scan = self.folder_watch.scan_now();
        self.take_scan(gpu, scan);
    }

    fn take_scan(&mut self, gpu: &Gpu, scan: dam::watch::Scan) {
        let mut returned = Vec::new();
        for item in &mut self.shelf {
            let file = photo_engine::copies::source_file(&item.path).into_owned();
            let watched = file
                .parent()
                .is_some_and(|d| scan.folders.iter().any(|f| f == d));
            if !watched {
                continue;
            }
            let missing = !scan.present.contains(&file);
            if item.missing && !missing {
                returned.push(item.path.clone());
            }
            item.missing = missing;
        }
        for path in returned {
            self.thumbs_requested.remove(&path);
            self.request_thumbnail(&path);
        }
        let new: Vec<PathBuf> = scan
            .new
            .into_iter()
            .filter(|p| !self.shelf.iter().any(|it| it.path == *p))
            .collect();
        if !new.is_empty() {
            let n = new.len();
            self.add_to_shelf(gpu, new);
            self.set_status(format!("{n} new photo(s) arrived in a watched folder"));
        }
    }

    pub(crate) fn request_thumbnail(&mut self, path: &std::path::Path) {
        if !photo_engine::copies::source_file(path).exists() {
            return;
        }
        if self.thumbs_requested.insert(path.to_path_buf()) {
            self.thumbs_pending.insert(path.to_path_buf());
            self.loader.thumbnail(path.to_path_buf());
        }
    }

    /// The most thumbnails kept: a phone has far less memory to spare.
    fn thumb_cap(&self) -> usize {
        if self.layout.is_compact() {
            400
        } else {
            1500
        }
    }

    /// Drop the least recently drawn thumbnails until `keep` are left; they are asked for
    /// again when their tiles come back into view.
    pub(crate) fn evict_thumbs(&mut self, keep: usize) {
        if self.thumbs.len() <= keep {
            return;
        }
        let mut by_age: Vec<(u64, PathBuf)> = self
            .thumbs
            .keys()
            .map(|p| (self.thumb_used.get(p).copied().unwrap_or(0), p.clone()))
            .collect();
        by_age.sort();
        let open = self.loaded_path.clone();
        for (_, path) in by_age.into_iter().take(self.thumbs.len() - keep) {
            if Some(&path) == open.as_ref() {
                continue;
            }
            self.thumbs.remove(&path);
            self.thumbs_requested.remove(&path);
            self.thumb_used.remove(&path);
        }
    }

    /// Give memory back when the system asks (Android's onTrimMemory): the engine's spare
    /// textures, and all but a quarter of the thumbnails.
    /// The shell has replaced egui's renderer and context (the window's surface changed
    /// format, L-21): whatever was registered with the old ones is gone, but the app, its
    /// photo and its roll are not. The photo's texture is registered again on the next
    /// render and thumbnails are asked for again as they come into view.
    pub fn renderer_replaced(&mut self) {
        // Not freed: the renderer that held it has gone.
        self.texture_id = None;
        self.thumbs.clear();
        self.thumbs_requested.clear();
        self.thumb_used.clear();
        self.stage_preview = None;
        self.needs_render = true;
    }

    pub fn release_memory(&mut self) {
        self.engine.release_spares();
        self.evict_thumbs(self.thumb_cap() / 4);
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
        if !photo_engine::copies::source_file(&path).exists() {
            if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
                item.missing = true;
            }
            self.set_status(format!(
                "{} is missing: reconnect its drive, or remove it from the shelf",
                file_name(&path)
            ));
            return;
        }
        // Its metadata comes with its thumbnail, and the Info card wants it whether or
        // not its tile is in view.
        self.request_thumbnail(&path);
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
        // Its filmstrip thumbnail, at once, until something sharper comes: the stage never
        // goes blank between photos.
        self.stage_preview = self.thumbs.get(&path).map(|t| (path.clone(), t.clone()));
        self.loader.open(path, self.open_generation);
    }

    /// Point every control at `path`: its sidecar's stack, or defaults.
    pub(crate) fn begin_editing(&mut self, path: &Path) {
        self.error = None;
        // A mask belongs to the photo it was drawn on.
        self.selected_mask = None;
        self.mask_drag = None;
        self.painting = None;
        self.selected_spot = None;
        self.spot_drag = None;
        self.pending_spot = None;
        self.cutout_studio.reset_cache();
        self.sidecar_locked = false;
        self.decode_failed = false;
        let (saved, metadata, snapshots, saved_history) =
            match photo_engine::load_sidecar_full(path) {
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
                    (data.stack, data.metadata, snapshots, data.history)
                }
                Ok(None) => (
                    None,
                    photo_engine::AssetMetadata::default(),
                    Vec::new(),
                    Vec::new(),
                ),
                Err(e) => {
                    // Holding edits this build cannot read is not the same as holding none:
                    // show it, and never write over it.
                    self.sidecar_locked = true;
                    self.error = Some(format!(
                        "{}: {e:#}. Its edits were not loaded, and the file is left untouched.",
                        file_name(path)
                    ));
                    (
                        None,
                        photo_engine::AssetMetadata::default(),
                        Vec::new(),
                        Vec::new(),
                    )
                }
            };

        if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
            if item.pending {
                item.take(ShelfFacts::read(path));
                self.shelf_pending.remove(path);
            }
            item.metadata = metadata;
            item.is_edited = false;
        } else {
            self.request_thumbnail(path);
            let mut item = ShelfItem::new(path.to_path_buf());
            item.metadata = metadata;
            self.shelf.push(item);
            self.bump_shelf_epoch();
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
            // Else what the sidecar kept, so undo reaches back past a relaunch.
            .unwrap_or_else(|| {
                let truncated = saved_history.len() >= photo_engine::sidecar::HISTORY_STEPS;
                History::from_past(saved_history, truncated)
            });
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
        // An edit saved before the current colour math is drawn as it was; say so, and
        // offer the update rather than making it.
        if self.stack.rendering != photo_engine::params::Rendering::V2 {
            self.set_status_undoable(
                "This edit uses the older colour math (highlights, very bright colours)",
                NoticeAction::UpdateRendering,
            );
        }
    }

    /// Draw the open edit with the current colour math (`params::Rendering::V2`): one
    /// undo step, so it can be taken back.
    pub(crate) fn update_rendering(&mut self) {
        if !self.can_edit() || self.stack.rendering == photo_engine::params::Rendering::V2 {
            return;
        }
        self.history.checkpoint(&self.stack);
        self.stack.rendering = photo_engine::params::Rendering::V2;
        self.needs_render = true;
        self.set_status("Updated to the current colour math (undo to go back)");
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

    /// The open photo's own texture is what the stage draws, not a stand-in.
    pub(crate) fn photo_on_stage(&self) -> bool {
        self.texture_id.is_some() && self.ready_image().is_some()
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
                    // The stand-in stays until the photo's own texture is on the stage
                    // (`sync`): the decode is only the CPU's half of opening it.
                    self.loading = None;
                    match result {
                        Ok(source) => {
                            self.decode_ms = decode_ms;
                            self.finish_open(gpu, path, source);
                        }
                        // DESIGN.md: failures surface in the UI, with the filename.
                        Err(e) => {
                            self.stage_preview = None;
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
                    // Until the photo itself is on the stage, which can be well after its
                    // decode: the GPU still has to demosaic it.
                    let current = self.loaded_path.as_ref() == Some(&path)
                        && !self.decode_failed
                        && !self.photo_on_stage();
                    if generation == self.open_generation && current {
                        if self.orientation_pending {
                            self.apply_orientation(orientation);
                        }
                        let texture =
                            ctx.load_texture("stage preview", image, egui::TextureOptions::LINEAR);
                        self.stage_preview = Some((path, texture));
                    }
                }
                loader::Done::Shelf(facts) => {
                    if self.shelf_pending.remove(&facts.path) {
                        if let Some(item) = self
                            .shelf
                            .iter_mut()
                            .find(|it| it.path == facts.path && it.pending)
                        {
                            item.take(facts);
                        }
                    }
                }
                loader::Done::Scanned {
                    folder,
                    purpose,
                    found,
                    cancelled,
                } => {
                    if purpose != loader::ScanFor::Watch {
                        self.reads = self.reads.saturating_sub(1);
                        if self.reads == 0 {
                            self.reading = None;
                        }
                    }
                    if cancelled {
                        self.scans_pending = self.scans_pending.saturating_sub(1);
                    } else {
                        self.take_scan_result(gpu, folder, purpose, found);
                    }
                }
                loader::Done::Subfolders { dir, subs } => {
                    self.subfolders = Some((dir, subs, Instant::now()));
                }
                loader::Done::Library {
                    entries,
                    deleted,
                    failed,
                } => self.take_library(entries, deleted, failed),
                loader::Done::Thumbnail { path, image, info } => {
                    self.thumbs_pending.remove(&path);
                    if let Some(item) = self.shelf.iter_mut().find(|it| it.path == path) {
                        item.info = info.map(|i| *i);
                    }
                    if let Some(image) = image {
                        let name = format!("thumb:{}", path.display());
                        let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
                        self.thumb_used.insert(path.clone(), self.frame_no);
                        self.thumbs.insert(path, texture);
                        let cap = self.thumb_cap();
                        self.evict_thumbs(cap);
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
            // The grid of tools, to choose from.
            self.compact_tab = Some(compact::Tab::Edit);
            self.compact_controls_open = false;
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
        let mut folders = 0;
        for path in std::mem::take(&mut self.pending_drops) {
            if path.is_dir() {
                // Read on the loader's thread, four folders deep.
                folders += 1;
                self.scans_pending += 1;
                self.reading = Some(file_name(&path));
                self.reads += 1;
                self.loader.file_job(loader::FileJob::Scan {
                    folder: path,
                    purpose: loader::ScanFor::Drop,
                    depth: 4,
                    epoch: Some(self.loader.scan_epoch()),
                });
            } else if photo_engine::is_supported_path(&path) {
                photos.push(path);
            }
        }
        let Some(first) = photos.first().cloned() else {
            if folders == 0 {
                self.set_status("Nothing dropped that Duckfoot can open");
            }
            return;
        };
        let count = photos.len();
        self.add_to_shelf(gpu, photos);
        self.open_path(gpu, first);
        if count > 1 {
            self.set_status(format!("Added {count} photos to the shelf"));
        }
    }

    /// Empty the shelf completely. Flushes the open photo if dirty, clears thumbnails
    /// and selections, and registers an undoable action to restore all items with Ctrl+Z.
    pub fn clear_shelf(&mut self) {
        if self.shelf.is_empty() {
            return;
        }
        if self.loaded_path.is_some() {
            self.flush();
        }
        let count = self.shelf.len();
        let removed = self.take_off_shelf(|_| true);
        self.close_photo();
        self.removed_from_shelf = removed;
        self.set_status_undoable(
            format!("Cleared shelf ({count} stills removed)"),
            NoticeAction::Remove,
        );
    }

    /// Take photos off the shelf. Nothing on disk changes; the notice's Undo puts them
    /// back where they were.
    pub fn remove_from_shelf(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let open_removed = self.loaded_path.as_ref().is_some_and(|p| paths.contains(p));
        // The open photo's place goes to the next one shown that stays, or else the one
        // before it.
        let successor = if open_removed {
            self.flush();
            let order = self.displayed_indices();
            let open = order
                .iter()
                .position(|&i| Some(&self.shelf[i].path) == self.loaded_path.as_ref());
            let stays = |&&i: &&usize| !paths.contains(&self.shelf[i].path);
            open.and_then(|at| {
                order[at..]
                    .iter()
                    .find(stays)
                    .or_else(|| order[..at].iter().rev().find(stays))
            })
            .map(|&i| self.shelf[i].path.clone())
        } else {
            None
        };
        let removed = self.take_off_shelf(|p| paths.contains(p));
        if open_removed {
            self.close_photo();
            self.pending_open = successor;
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

    /// Leave the stage empty: nothing being edited, nothing decoding. The pixels are let go
    /// on the next frame, which has the `Gpu` to free them with.
    fn close_photo(&mut self) {
        self.cancel_crop();
        self.loaded_path = None;
        self.engine_path = None;
        self.loading = None;
        self.stage_preview = None;
        // A decode still running for it lands on nothing.
        self.open_generation += 1;
        self.error = None;
        self.decode_failed = false;
        self.edit_when_opened = false;
    }

    /// The shelf's items `gone` picks, taken off it with their thumbnails, each with the
    /// index it stood at.
    fn take_off_shelf(&mut self, gone: impl Fn(&PathBuf) -> bool) -> Vec<(usize, ShelfItem)> {
        let mut removed = Vec::new();
        let mut i = 0;
        let mut index = 0;
        while i < self.shelf.len() {
            if gone(&self.shelf[i].path) {
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
            self.thumbs_pending.remove(&item.path);
            self.thumb_used.remove(&item.path);
        }
        if !removed.is_empty() {
            self.bump_shelf_epoch();
        }
        removed
    }

    /// The phone's Library screen: open it and read the library off this thread.
    pub fn open_storage(&mut self) {
        let Some(dir) = self.platform.library_dir() else {
            return;
        };
        self.storage = Some(crate::dam::storage::StorageView::default());
        self.loader.file_job(loader::FileJob::Library {
            dir,
            delete: Vec::new(),
        });
    }

    /// Whether `path` (or the photo it is a copy of) is one of the library's copies.
    pub(crate) fn is_library_copy(&self, path: &Path) -> bool {
        self.platform
            .library_dir()
            .is_some_and(|dir| crate::dam::storage::is_in(&dir, path))
    }

    /// Deletes these photos of the library, with their sidecars and virtual copies, and
    /// takes them off the shelf. There is no undo, so the buttons that ask for it ask
    /// twice. The open photo is kept: its edits would be written back beside it.
    pub fn delete_copies(&mut self, photos: &[PathBuf]) {
        let Some(dir) = self.platform.library_dir() else {
            return;
        };
        let open = self
            .loaded_path
            .as_deref()
            .map(|p| photo_engine::copies::source_file(p).into_owned());
        let delete: Vec<PathBuf> = photos
            .iter()
            .filter(|p| p.starts_with(&dir) && Some(*p) != open.as_ref())
            .cloned()
            .collect();
        if delete.len() < photos.len() && open.is_some() {
            self.set_status("The open photo is kept: open another to delete it");
        }
        if delete.is_empty() {
            return;
        }
        self.take_off_shelf(|p| {
            delete
                .iter()
                .any(|d| *photo_engine::copies::source_file(p) == **d)
        });
        // What Undo would put back no longer exists.
        self.removed_from_shelf.retain(|(_, it)| {
            !delete
                .iter()
                .any(|d| *photo_engine::copies::source_file(&it.path) == **d)
        });
        let view = self.storage.get_or_insert_with(Default::default);
        view.entries = None;
        view.selected.clear();
        view.confirming = None;
        self.loader
            .file_job(loader::FileJob::Library { dir, delete });
    }

    fn take_library(
        &mut self,
        entries: Vec<crate::dam::storage::LibraryEntry>,
        deleted: usize,
        failed: Vec<String>,
    ) {
        if let Some(first) = failed.first() {
            log::warn!("could not delete from the library: {failed:?}");
            self.set_status(format!("Could not delete {first}"));
        } else if deleted > 0 {
            self.set_status(if deleted == 1 {
                "Deleted 1 copy from the library".to_string()
            } else {
                format!("Deleted {deleted} copies from the library")
            });
        }
        if let Some(view) = &mut self.storage {
            view.selected
                .retain(|p| entries.iter().any(|e| &e.path == p));
            view.entries = Some(entries);
        }
    }

    pub fn undo_remove_from_shelf(&mut self) {
        let removed = std::mem::take(&mut self.removed_from_shelf);
        let n = removed.len();
        for (index, item) in removed {
            self.shelf.insert(index.min(self.shelf.len()), item);
        }
        self.bump_shelf_epoch();
        self.set_status(format!("Put {n} back on the shelf"));
    }

    /// Invalidate shelf caches when items are added, removed, or modified.
    pub fn bump_shelf_epoch(&mut self) {
        self.shelf_epoch = self.shelf_epoch.wrapping_add(1);
    }
}
