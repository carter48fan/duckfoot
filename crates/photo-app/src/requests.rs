//! Asking the platform for things (pickers, places) and taking its answers.

use crate::*;

impl PhotoApp {
    pub(crate) fn pick_and_open(&mut self, ctx: &egui::Context) {
        let from = self.settings.import_dir.clone();
        self.request(ctx, platform::Request::OpenImage { from });
    }

    pub(crate) fn pick_and_shelve(&mut self, ctx: &egui::Context) {
        let from = self.settings.import_dir.clone();
        self.request(ctx, platform::Request::AddImages { from });
    }

    /// Pick a folder; its photos, and its folders' photos, are added to the shelf.
    pub(crate) fn pick_folder_and_shelve(&mut self, ctx: &egui::Context) {
        let from = self.settings.import_dir.clone();
        self.request(ctx, platform::Request::BrowseFolder { from });
    }

    pub(crate) fn pick_and_export(&mut self, ctx: &egui::Context) {
        let (dir, name) = self.export_suggestion();
        self.request(ctx, platform::Request::ExportImage { dir, name });
    }

    /// Pickers for photos start next time in the folder this one came from.
    fn remember_import_dir(&mut self, dir: Option<&Path>) {
        if let Some(dir) = dir {
            self.settings.import_dir = Some(dir.to_path_buf());
            self.save_settings();
        }
    }

    /// Ask the platform for a picker. Its answer is handled at the top of a later frame,
    /// which this makes sure happens even when the answer was immediate.
    pub(crate) fn request(&mut self, ctx: &egui::Context, request: platform::Request) {
        self.platform.request(request);
        ctx.request_repaint();
    }

    pub(crate) fn poll_platform(&mut self, gpu: &Gpu) {
        for response in self.platform.poll() {
            match response {
                platform::Response::OpenImage(path) => {
                    self.remember_import_dir(path.parent());
                    self.edit_when_opened = true;
                    if self.single_photo() {
                        self.open_alone(gpu, path);
                    } else {
                        self.open_path(gpu, path);
                    }
                }
                platform::Response::AddImages(paths) if self.single_photo() => {
                    // One at a time: the first of them is the photo.
                    if let Some(first) = paths.into_iter().next() {
                        self.remember_import_dir(first.parent());
                        self.edit_when_opened = true;
                        self.open_alone(gpu, first);
                    }
                }
                platform::Response::AddImages(paths) => {
                    self.remember_import_dir(paths.first().and_then(|p| p.parent()));
                    let first = paths.first().cloned();
                    self.add_to_shelf(gpu, paths);
                    // Switch to what was just added, as a drop does. `open_path` is a no-op
                    // when `add_to_shelf` already opened it (nothing was open before).
                    if let Some(first) = first {
                        self.open_path(gpu, first);
                    }
                }
                // A picked folder goes straight onto the shelf, its folders with it.
                platform::Response::BrowseFolder(dir) => {
                    self.remember_import_dir(Some(&dir));
                    self.open_session_folder(gpu, dir);
                }
                platform::Response::BatchOutputDir(dir) => {
                    self.settings.batch_output_dir = Some(dir.clone());
                    self.batch_output_dir = Some(dir);
                    self.save_settings();
                }
                platform::Response::ExportImage(path) => {
                    self.settings.export_dir = path.parent().map(Path::to_path_buf);
                    if let Ok(format) = photo_engine::ExportFormat::from_path(&path) {
                        self.export_settings.format = format;
                    }
                    self.settings.export = Some(self.export_settings);
                    self.save_settings();
                    self.export_to(gpu, path);
                }
                platform::Response::ImportPreset(path) => self.import_preset(&path),
                platform::Response::DisplayProfile(path) => self.use_display_profile(path),
                platform::Response::PrinterProfile(path) => self.use_printer_profile(path),
                platform::Response::LookFile(path) => self.use_look_file(path),
                platform::Response::LowMemory => self.release_memory(),
                platform::Response::ExportPreset(path) => {
                    if let Some(preset) = self.exporting_preset.take() {
                        match photo_engine::fsio::atomic_write_bytes(
                            &path,
                            preset.to_json().as_bytes(),
                        ) {
                            Ok(()) => self.set_status(format!("Exported {}", file_name(&path))),
                            Err(e) => self.error = Some(format!("{}: {e:#}", path.display())),
                        }
                    }
                }
                platform::Response::Status(msg) => self.set_status(msg),
            }
        }
    }
}
