//! What the app needs from the operating system beyond a GPU: file pickers, and where
//! things live.
//!
//! Pickers are request/response rather than a blocking call returning a path, because
//! on Android they are separate activities whose answer arrives some frames later — and
//! the app is suspended while one is open. The desktop implementation answers at once.

use std::path::PathBuf;

use crate::dam::FolderBookmark;

/// Something the app wants the user to choose.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// One image to open on the stage.
    OpenImage,
    /// Any number of images to add to the shelf.
    AddImages,
    /// A folder to browse in the shelf's folder tree.
    BrowseFolder,
    /// Where batch export writes.
    BatchOutputDir,
    /// Where the single-image export writes. `dir` and `name` are suggestions.
    ExportImage { dir: PathBuf, name: String },
}

/// The user's answer to a [`Request`]. A cancelled picker produces nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    OpenImage(PathBuf),
    AddImages(Vec<PathBuf>),
    BrowseFolder(PathBuf),
    BatchOutputDir(PathBuf),
    ExportImage(PathBuf),
    /// Progress or a problem to show in the status bar.
    Status(String),
}

/// Supplied by the bootstrap, like [`crate::Gpu`], so the app never learns which OS it
/// is on.
pub trait Platform: Send {
    /// Start a picker. The answer, if any, comes back through [`Platform::poll`].
    fn request(&mut self, request: Request);

    /// Answers that have arrived since the last call.
    fn poll(&mut self) -> Vec<Response>;

    /// Folders to offer in the shelf's bookmark list. Empty keeps the defaults.
    fn bookmarks(&self) -> Vec<FolderBookmark> {
        Vec::new()
    }

    /// Where exports go when the user has not chosen, if this platform has an opinion.
    /// `None` means beside the source photo.
    fn default_output_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Back was pressed with nothing left in the app to close. On Android this sends the
    /// app to the background; a desktop has no such key.
    fn leave(&mut self) {}
}

/// Answers nothing. For shells with no pickers, and for tests.
#[derive(Debug, Default)]
pub struct NoPlatform;

impl Platform for NoPlatform {
    fn request(&mut self, _request: Request) {}

    fn poll(&mut self) -> Vec<Response> {
        Vec::new()
    }
}

/// Native dialogs through `rfd`, which block until the user answers.
#[cfg(not(target_os = "android"))]
#[derive(Debug, Default)]
pub struct DesktopPlatform {
    answered: Vec<Response>,
}

#[cfg(not(target_os = "android"))]
impl Platform for DesktopPlatform {
    fn request(&mut self, request: Request) {
        let images = || {
            rfd::FileDialog::new()
                .add_filter(
                    "All Supported Images",
                    &photo_engine::all_image_dialog_extensions(),
                )
                .add_filter("Camera RAW", &photo_engine::raw_dialog_extensions())
                .add_filter("Standard Images", &photo_engine::raster_dialog_extensions())
                .add_filter("All files", &["*"])
        };
        let answer = match request {
            Request::OpenImage => images().pick_file().map(Response::OpenImage),
            Request::AddImages => images().pick_files().map(Response::AddImages),
            Request::BrowseFolder => rfd::FileDialog::new()
                .pick_folder()
                .map(Response::BrowseFolder),
            Request::BatchOutputDir => rfd::FileDialog::new()
                .pick_folder()
                .map(Response::BatchOutputDir),
            Request::ExportImage { dir, name } => rfd::FileDialog::new()
                .add_filter("PNG", &["png"])
                .add_filter("JPEG", &["jpg", "jpeg"])
                .set_directory(dir)
                .set_file_name(name)
                .save_file()
                .map(Response::ExportImage),
        };
        self.answered.extend(answer);
    }

    fn poll(&mut self) -> Vec<Response> {
        std::mem::take(&mut self.answered)
    }
}

/// The platform `PhotoApp::new` uses when the bootstrap does not supply one.
pub fn default_platform() -> Box<dyn Platform> {
    #[cfg(not(target_os = "android"))]
    {
        Box::new(DesktopPlatform::default())
    }
    #[cfg(target_os = "android")]
    {
        Box::new(NoPlatform)
    }
}
