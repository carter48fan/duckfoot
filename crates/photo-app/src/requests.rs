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
                    self.open_path(gpu, path);
                }
                platform::Response::AddImages(paths) => {
                    self.remember_import_dir(paths.first().and_then(|p| p.parent()));
                    self.add_to_shelf(gpu, paths);
                }
                platform::Response::BrowseFolder(dir) => {
                    self.remember_import_dir(Some(&dir));
                    self.folder_tree.current_folder = Some(dir);
                    self.folder_browser_open = true;
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
