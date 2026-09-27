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
    /// One image to open on the stage. `from` is where the picker starts, if known: the
    /// folder the last one was chosen from, remembered between runs.
    OpenImage { from: Option<PathBuf> },
    /// Any number of images to add to the shelf.
    AddImages { from: Option<PathBuf> },
    /// A folder to browse in the shelf's folder tree.
    BrowseFolder { from: Option<PathBuf> },
    /// Where batch export writes.
    BatchOutputDir { from: Option<PathBuf> },
    /// Where the single-image export writes. `dir` and `name` are suggestions.
    ExportImage { dir: PathBuf, name: String },
    /// A `.duckfoot-preset` file to add to the user's presets.
    ImportPreset { from: Option<PathBuf> },
    /// Where to write a preset for another machine. `name` is a suggestion.
    ExportPreset { name: String },
    /// Hand an exported photo to another app (the Android share sheet). Only asked for
    /// when [`Platform::can_share`] says it can be.
    Share { path: PathBuf },
    /// The monitor's ICC profile (W-14).
    DisplayProfile { from: Option<PathBuf> },
}

/// The user's answer to a [`Request`]. A cancelled picker produces nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    OpenImage(PathBuf),
    AddImages(Vec<PathBuf>),
    BrowseFolder(PathBuf),
    BatchOutputDir(PathBuf),
    ExportImage(PathBuf),
    ImportPreset(PathBuf),
    ExportPreset(PathBuf),
    DisplayProfile(PathBuf),
    /// The system is short of memory: give back what can be made again.
    LowMemory,
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

    /// A private directory the app may keep its settings in. `None` remembers nothing.
    fn settings_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Where photos picked on this platform are copied to, if it copies them (Android's
    /// pickers hand out documents, not paths). The Library screen lists and deletes them.
    fn library_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Called when an image has been successfully written to disk. Used on mobile
    /// platforms to request system indexing into the media library/gallery.
    fn export_completed(&mut self, _path: &std::path::Path) {}

    /// Whether [`Request::Share`] does anything here.
    fn can_share(&self) -> bool {
        false
    }

    /// A dialog is open whose answer has not come back: the app keeps polling for it.
    fn busy(&self) -> bool {
        false
    }
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

/// Native dialogs through `rfd`. A dialog blocks until it is answered, so it runs on a
/// thread of its own and the window keeps drawing (WP5.6), except on macOS, whose dialogs
/// belong to the main thread.
#[cfg(not(target_os = "android"))]
#[derive(Debug, Default)]
pub struct DesktopPlatform {
    answered: Vec<Response>,
    open: Option<std::sync::mpsc::Receiver<Option<Response>>>,
}

/// The directory the desktop keeps settings (and its log) in: %APPDATA% on Windows,
/// ~/Library/Application Support on macOS, $XDG_CONFIG_HOME or ~/.config elsewhere.
#[cfg(not(target_os = "android"))]
pub fn desktop_settings_dir() -> Option<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))
    };
    base.map(|b| b.join("duckfoot"))
}

/// One native dialog, to its answer (`None` when cancelled).
#[cfg(not(target_os = "android"))]
fn ask(request: Request) -> Option<Response> {
    let start = |from: Option<PathBuf>| {
        let dialog = rfd::FileDialog::new();
        match from.filter(|d| d.is_dir()) {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        }
    };
    let images = |from| {
        start(from)
            .add_filter(
                "All Supported Images",
                &photo_engine::all_image_dialog_extensions(),
            )
            .add_filter("Camera RAW", &photo_engine::raw_dialog_extensions())
            .add_filter("Standard Images", &photo_engine::raster_dialog_extensions())
            .add_filter("All files", &["*"])
    };
    match request {
        Request::OpenImage { from } => images(from).pick_file().map(Response::OpenImage),
        Request::AddImages { from } => images(from).pick_files().map(Response::AddImages),
        Request::BrowseFolder { from } => start(from).pick_folder().map(Response::BrowseFolder),
        Request::BatchOutputDir { from } => start(from).pick_folder().map(Response::BatchOutputDir),
        Request::ExportImage { dir, name } => rfd::FileDialog::new()
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("PNG", &["png"])
            .add_filter("TIFF", &["tif", "tiff"])
            .add_filter("WebP", &["webp"])
            .set_directory(dir)
            .set_file_name(name)
            .save_file()
            .map(Response::ExportImage),
        Request::ImportPreset { from } => start(from)
            .add_filter("Duckfoot preset", &[crate::user_presets::EXTENSION])
            .pick_file()
            .map(Response::ImportPreset),
        Request::ExportPreset { name } => rfd::FileDialog::new()
            .add_filter("Duckfoot preset", &[crate::user_presets::EXTENSION])
            .set_file_name(name)
            .save_file()
            .map(Response::ExportPreset),
        // A desktop has no share sheet; can_share says so, so this is not asked.
        Request::Share { .. } => None,
        Request::DisplayProfile { from } => start(from)
            .add_filter("ICC profile", &["icc", "icm"])
            .pick_file()
            .map(Response::DisplayProfile),
    }
}

#[cfg(not(target_os = "android"))]
impl Platform for DesktopPlatform {
    fn request(&mut self, request: Request) {
        // One dialog at a time, as the OS shows them.
        if self.open.is_some() {
            return;
        }
        if cfg!(target_os = "macos") {
            self.answered.extend(ask(request));
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let started = std::thread::Builder::new()
            .name("duckfoot-dialog".into())
            .spawn(move || {
                let _ = tx.send(ask(request));
            });
        if started.is_ok() {
            self.open = Some(rx);
        }
    }

    fn poll(&mut self) -> Vec<Response> {
        if let Some(rx) = &self.open {
            match rx.try_recv() {
                Ok(answer) => {
                    self.answered.extend(answer);
                    self.open = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.open = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        std::mem::take(&mut self.answered)
    }

    fn busy(&self) -> bool {
        self.open.is_some()
    }

    fn settings_dir(&self) -> Option<PathBuf> {
        desktop_settings_dir()
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
