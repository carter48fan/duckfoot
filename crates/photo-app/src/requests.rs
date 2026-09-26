//! Asking the platform for things (pickers, places) and taking its answers.

use crate::*;

impl PhotoApp {
    pub(crate) fn pick_and_open(&mut self, ctx: &egui::Context) {
        self.request(ctx, platform::Request::OpenImage);
    }

    pub(crate) fn pick_and_shelve(&mut self, ctx: &egui::Context) {
        self.request(ctx, platform::Request::AddImages);
    }

    pub(crate) fn pick_and_export(&mut self, ctx: &egui::Context) {
        let (dir, name) = self.export_suggestion();
        self.request(ctx, platform::Request::ExportImage { dir, name });
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
                    self.edit_when_opened = true;
                    self.open_path(gpu, path);
                }
                platform::Response::AddImages(paths) => self.add_to_shelf(gpu, paths),
                platform::Response::BrowseFolder(dir) => {
                    self.folder_tree.current_folder = Some(dir);
                    self.folder_browser_open = true;
                }
                platform::Response::BatchOutputDir(dir) => self.batch_output_dir = Some(dir),
                platform::Response::ExportImage(path) => self.export_to(gpu, path),
                platform::Response::Status(msg) => self.set_status(msg),
            }
        }
    }
}
