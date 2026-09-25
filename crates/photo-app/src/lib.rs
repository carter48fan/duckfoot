//! Barrel 1's UI. Knows about egui and the engine; knows nothing about windowing.
//!
//! The desktop and Android bootstraps both drive this same type.

pub mod batch;
pub mod dam;
pub mod inspector_map_3d;
mod loader;
pub mod platform;
pub mod theme;
pub mod tonemap_3d;
pub mod widgets;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use std::sync::RwLock;

use egui::{pos2, vec2, Align2, Color32, FontId, Margin, Rect, RichText, Sense, Stroke};
use photo_engine::params::Module;
use photo_engine::{
    find_sidecar, identity_nodes, load_sidecar, save_sidecar, Crop, CurveNode, Engine, PhotoStack,
    WhiteScale,
};
use widgets::Kind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorEqTab {
    #[default]
    Hue,
    Saturation,
    Luminance,
}

/// Sections available in the Inspector sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum InspectorSectionId {
    Scope,
    Geometry,
    WhiteBalance,
    Exposure,
    Tonal,
    ToneCurve,
    Clarity,
    Dehaze,
    ColorEq,
    Filmic3D,
    ColorBalance,
    Vibrance,
    Sharpen,
    Vignette,
}

impl InspectorSectionId {
    pub fn all() -> &'static [Self] {
        &[
            Self::Scope,
            Self::Geometry,
            Self::WhiteBalance,
            Self::Exposure,
            Self::Tonal,
            Self::ToneCurve,
            Self::Clarity,
            Self::Dehaze,
            Self::ColorEq,
            Self::Filmic3D,
            Self::ColorBalance,
            Self::Vibrance,
            Self::Sharpen,
            Self::Vignette,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Scope => "scope",
            Self::Geometry => "crop & rotate",
            Self::WhiteBalance => "white balance",
            Self::Exposure => "exposure",
            Self::Tonal => "shadows & highlights",
            Self::ToneCurve => "tone curve",
            Self::Clarity => "clarity & texture",
            Self::Dehaze => "dehaze",
            Self::ColorEq => "color eq (8-band)",
            Self::Filmic3D => "filmic rgb",
            Self::ColorBalance => "colour balance",
            Self::Vibrance => "vibrance",
            Self::Sharpen => "sharpen",
            Self::Vignette => "vignette",
        }
    }
}

/// Persistent configuration for an Inspector section.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InspectorSectionConfig {
    pub id: InspectorSectionId,
    pub visible: bool,
    pub collapsed: bool,
}

impl InspectorSectionConfig {
    pub fn default_sections() -> Vec<Self> {
        InspectorSectionId::all()
            .iter()
            .map(|&id| Self {
                id,
                visible: true,
                // Open what nearly every edit touches; the rest show their value in the
                // header and open on click.
                collapsed: !matches!(
                    id,
                    InspectorSectionId::Scope
                        | InspectorSectionId::WhiteBalance
                        | InspectorSectionId::Exposure
                        | InspectorSectionId::ToneCurve
                ),
            })
            .collect()
    }
}

/// An asset represented in the Shelf.
#[derive(Debug, Clone, PartialEq)]
pub struct ShelfItem {
    pub path: PathBuf,
    pub name: String,
    pub is_edited: bool,
    pub has_sidecar: bool,
    pub metadata: photo_engine::AssetMetadata,
    /// Size on disk, read once when the item is shelved.
    pub bytes: u64,
}

impl ShelfItem {
    fn new(path: PathBuf) -> Self {
        let (metadata, has_sidecar) = match photo_engine::load_sidecar_full(&path) {
            Ok(Some(sc)) => (sc.metadata, true),
            _ => (
                photo_engine::AssetMetadata::default(),
                find_sidecar(&path).is_some(),
            ),
        };
        Self {
            name: file_name(&path),
            is_edited: false,
            has_sidecar,
            metadata,
            bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            path,
        }
    }
}

/// Everything the app needs from the GPU, supplied by whichever shell is running.
///
/// Deliberately not eframe's `RenderState`: the app must not know which bootstrap
/// created the device, or the Android build becomes a fork instead of a new `main`.
pub struct Gpu<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub renderer: &'a RwLock<egui_wgpu::Renderer>,
}

/// A drag in crop mode, in normalised coordinates of the full frame.
#[derive(Debug, Clone, Copy)]
enum CropDrag {
    /// Started outside the crop: draws a new one from this corner.
    Draw { anchor: egui::Pos2 },
    /// Started inside it: slides the whole crop, keeping its size.
    Move { grab: egui::Pos2, start: Crop },
}

/// Zoom and pan over the image.
///
/// `zoom` is relative to fit-to-window, so 1.0 always means "the whole frame is visible"
/// no matter how the window is resized. `pan` is in **image pixels**, not screen pixels,
/// for the same reason: resizing the window must not slide the image around.
struct View {
    zoom: f32,
    pan: egui::Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
        }
    }
}

/// Above this the image is bigger than the window and panning becomes meaningful.
const MAX_ZOOM_SCALE: f32 = 16.0;

/// Where an export has got to.
///
/// The GPU readback is fast enough to do inline, but encoding 24 MP of PNG is seconds of
/// CPU work — doing that on the UI thread would freeze the window with no way to tell the
/// user why. DESIGN.md: long operations show real progress.
enum Export {
    Idle,
    Writing {
        path: PathBuf,
        started: Instant,
        result: Receiver<Result<(), String>>,
    },
    Done {
        path: PathBuf,
        seconds: f32,
    },
    Failed(String),
}

/// Snapshot undo. The whole stack is a few hundred bytes, so there is no reason to build
/// a command-diff system for it — DESIGN.md invariant 3 only demands that *every* mutation
/// is undoable, not that it is clever.
struct History {
    past: Vec<PhotoStack>,
    future: Vec<PhotoStack>,
    /// The oldest state has been dropped to stay under the cap, so `past[0]` is no longer
    /// the stack the file opened with.
    truncated: bool,
}

impl History {
    fn new() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            truncated: false,
        }
    }

    /// Call before mutating. Coalescing is the caller's job: a slider drag pushes once, on
    /// drag start, not once per frame.
    fn checkpoint(&mut self, current: &PhotoStack) {
        self.past.push(current.clone());
        if self.past.len() > 256 {
            self.past.remove(0);
            self.truncated = true;
        }
        self.future.clear();
    }

    fn undo(&mut self, current: &mut PhotoStack) -> bool {
        let Some(prev) = self.past.pop() else {
            return false;
        };
        self.future.push(std::mem::replace(current, prev));
        true
    }

    fn redo(&mut self, current: &mut PhotoStack) -> bool {
        let Some(next) = self.future.pop() else {
            return false;
        };
        self.past.push(std::mem::replace(current, next));
        true
    }

    /// Every state, oldest first: the undo stack, then `current`, then the redo stack.
    /// `current` sits at index `past.len()`.
    fn timeline<'a>(&'a self, current: &'a PhotoStack) -> Vec<&'a PhotoStack> {
        self.past
            .iter()
            .chain(std::iter::once(current))
            .chain(self.future.iter().rev())
            .collect()
    }

    /// Walk the stack to a timeline index, as clicking a row in the History list does.
    fn jump_to(&mut self, current: &mut PhotoStack, target: usize) -> bool {
        let mut moved = false;
        while self.past.len() > target && self.undo(current) {
            moved = true;
        }
        while self.past.len() < target && self.redo(current) {
            moved = true;
        }
        moved
    }
}

/// Where the current edit stands relative to the sidecar on disk.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SidecarState {
    NoImage,
    /// Default stack, nothing saved.
    Pristine,
    /// Edited, never saved.
    Unsaved,
    /// Saved, and the stack still matches what was saved.
    Synced,
    /// Saved, then edited since.
    Modified,
}

impl SidecarState {
    fn describe(self) -> (&'static str, Color32) {
        match self {
            Self::NoImage => ("no image", theme::TEXT_FAINT),
            Self::Pristine => ("no edits", theme::TEXT_FAINT),
            Self::Unsaved => ("unsaved edits", theme::ACCENT),
            Self::Synced => ("sidecar synced", theme::LIME),
            Self::Modified => ("sidecar modified", theme::ACCENT),
        }
    }
}

pub struct PhotoApp {
    engine: Engine,
    stack: PhotoStack,
    history: History,
    texture_id: Option<egui::TextureId>,
    /// Set whenever the stack changes; cleared once the GPU has re-rendered.
    needs_render: bool,
    error: Option<String>,
    loaded_path: Option<PathBuf>,
    /// The stack as last written to (or read from) the sidecar, so "modified" means
    /// modified relative to disk rather than relative to defaults.
    saved_stack: Option<PhotoStack>,
    decode_ms: f32,
    render_ms: f32,
    export: Export,
    view: View,
    /// Size the image panel got last frame, so "100%" can be resolved outside of it.
    last_panel: egui::Vec2,
    /// While true the whole frame is on screen and dragging on it frames a crop. Outside
    /// crop mode only the cropped picture is shown, so editing carries on in the framing
    /// that will be exported.
    cropping: bool,
    /// The crop when crop mode was entered, which Cancel puts back.
    crop_before: Crop,
    /// The crop drag in progress.
    crop_drag: Option<CropDrag>,
    /// The Shelf: assets in the project/session.
    pub shelf: Vec<ShelfItem>,
    pub shelf_open: bool,
    /// Before/after split view toggle and position (0.0..=1.0).
    pub split_active: bool,
    pub split_pos: f32,
    /// In-memory clipboard for copying/pasting adjustment stacks across photos.
    pub clipboard_stack: Option<PhotoStack>,
    /// Named snapshot history checkpoints.
    pub snapshots: Vec<(String, PhotoStack)>,
    pub new_snapshot_name: String,
    /// Brief notifications in the status bar with timestamp.
    pub status_notification: Option<(String, Instant)>,
    pub tonemap_3d_state: tonemap_3d::Tonemap3DState,
    pub inspector_map_state: inspector_map_3d::InspectorMapState,
    pub inspector_sections: Vec<InspectorSectionConfig>,
    /// The Inspector is showing its Customize tab rather than the pipeline.
    pub customize_open: bool,
    pub inspector_width: f32,
    pub inspector_force_width: Option<f32>,
    /// The module card the user last touched; its border goes amber.
    focused_section: Option<InspectorSectionId>,
    /// Opening a module closes the others, so the inspector stays one screen tall.
    /// Shift-click opens one alongside.
    pub single_open: bool,
    /// What "no edits" means for the open photo: default modules, oriented the way the
    /// camera said. Reset returns here, and a stack equal to it is unedited.
    baseline: PhotoStack,
    loader: loader::Loader,
    /// Bumped per open request, so a slow decode that finishes after a newer request is
    /// recognised as stale and dropped.
    open_generation: u64,
    /// A photo being decoded in the background, and since when.
    loading: Option<(PathBuf, Instant)>,
    thumbs: HashMap<PathBuf, egui::TextureHandle>,
    thumbs_requested: HashSet<PathBuf>,
    /// Thumbnail requests the workers have answered, including those that failed.
    thumbs_answered: usize,
    /// The camera's embedded preview of the photo being opened, shown on the stage until
    /// its decode lands.
    stage_preview: Option<(PathBuf, egui::TextureHandle)>,
    /// The before/after divider is being dragged; the image must not pan meanwhile.
    split_dragging: bool,
    /// A module just opened while the others closed; scroll it into view next frame,
    /// once the layout has settled.
    reveal_section: Option<InspectorSectionId>,
    pub selection: dam::SelectionModel,
    pub filter: dam::FilterState,
    pub folder_tree: dam::FolderTree,
    pub folder_browser_open: bool,
    pub batch_renamer: batch::Renamer,
    pub export_recipes: Vec<batch::ExportRecipe>,
    pub export_queue: Option<batch::ExportQueue>,
    pub batch_dialog_open: bool,
    pub batch_output_dir: Option<PathBuf>,
    pub color_eq_tab: ColorEqTab,
    /// File pickers and storage locations, supplied by the bootstrap.
    platform: Box<dyn platform::Platform>,
}

impl PhotoApp {
    pub fn new(gpu: &Gpu) -> Self {
        Self::with_platform(gpu, platform::default_platform())
    }

    pub fn with_platform(gpu: &Gpu, platform: Box<dyn platform::Platform>) -> Self {
        let engine = Engine::new(gpu.device);
        let mut folder_tree = dam::FolderTree::default();
        let bookmarks = platform.bookmarks();
        if !bookmarks.is_empty() {
            folder_tree.bookmarks = bookmarks;
        }
        let loader =
            loader::Loader::new(gpu.device.clone(), gpu.queue.clone(), engine.upload_pool());
        Self {
            engine,
            stack: PhotoStack::default(),
            history: History::new(),
            texture_id: None,
            needs_render: false,
            error: None,
            loaded_path: None,
            saved_stack: None,
            decode_ms: 0.0,
            render_ms: 0.0,
            export: Export::Idle,
            view: View::default(),
            last_panel: egui::Vec2::ZERO,
            cropping: false,
            crop_before: Crop::default(),
            crop_drag: None,
            shelf: Vec::new(),
            shelf_open: true,
            split_active: false,
            split_pos: 0.5,
            clipboard_stack: None,
            snapshots: Vec::new(),
            new_snapshot_name: String::new(),
            status_notification: None,
            tonemap_3d_state: tonemap_3d::Tonemap3DState::default(),
            inspector_map_state: inspector_map_3d::InspectorMapState::default(),
            inspector_sections: InspectorSectionConfig::default_sections(),
            customize_open: false,
            inspector_width: 300.0,
            inspector_force_width: None,
            focused_section: None,
            single_open: true,
            baseline: PhotoStack::default(),
            loader,
            open_generation: 0,
            loading: None,
            thumbs: HashMap::new(),
            thumbs_requested: HashSet::new(),
            thumbs_answered: 0,
            stage_preview: None,
            split_dragging: false,
            reveal_section: None,
            selection: dam::SelectionModel::new(),
            filter: dam::FilterState::default(),
            folder_tree,
            folder_browser_open: false,
            batch_renamer: batch::Renamer::default(),
            export_recipes: batch::ExportRecipe::default_recipes(),
            export_queue: Some(batch::ExportQueue::new(
                gpu.device.clone(),
                gpu.queue.clone(),
            )),
            batch_dialog_open: false,
            batch_output_dir: None,
            color_eq_tab: ColorEqTab::default(),
            platform,
        }
    }

    /// Read the current frame back and write it, encoding off the UI thread.
    fn export_to(&mut self, gpu: &Gpu, path: PathBuf) {
        // Renders the current stack at full resolution, whatever the preview is at, and
        // never with the split.
        let exported =
            match self
                .engine
                .export(gpu.device, gpu.queue, &self.stack, self.stack.geometry.crop)
            {
                Ok(e) => e,
                Err(e) => {
                    self.export = Export::Failed(format!("{e:#}"));
                    return;
                }
            };

        let (tx, rx) = mpsc::channel();
        let target = path.clone();
        std::thread::spawn(move || {
            let outcome = exported.write(&target).map_err(|e| format!("{e:#}"));
            // The receiver is gone only if the app is shutting down; nothing to report to.
            let _ = tx.send(outcome);
        });

        self.export = Export::Writing {
            path,
            started: Instant::now(),
            result: rx,
        };
        // The export overwrote the preview's uniforms; put the preview back.
        self.needs_render = true;
    }

    /// Default filename beside the source: DSC08450.ARW -> DSC08450.png
    fn export_suggestion(&self) -> (PathBuf, String) {
        match &self.loaded_path {
            Some(src) => {
                let stem = src
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("untitled");
                let dir = src.parent().map(PathBuf::from).unwrap_or_default();
                (dir, format!("{stem}.png"))
            }
            None => (PathBuf::new(), "untitled.png".to_string()),
        }
    }

    fn poll_export(&mut self, ctx: &egui::Context) {
        let Export::Writing {
            path,
            started,
            result,
        } = &self.export
        else {
            return;
        };
        match result.try_recv() {
            Ok(Ok(())) => {
                self.export = Export::Done {
                    path: path.clone(),
                    seconds: started.elapsed().as_secs_f32(),
                };
            }
            Ok(Err(e)) => self.export = Export::Failed(e),
            Err(mpsc::TryRecvError::Empty) => {
                // Keep animating the "writing…" state rather than waiting for input.
                ctx.request_repaint();
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.export = Export::Failed("export thread stopped unexpectedly".into());
            }
        }
    }

    fn poll_batch_export(&mut self) {
        if let Some(queue) = &mut self.export_queue {
            let status = queue.poll().clone();
            if let batch::ExportStatus::Finished {
                total,
                success,
                errors,
            } = status
            {
                if errors.is_empty() {
                    self.set_status(format!(
                        "Batch export complete: {success}/{total} photos rendered."
                    ));
                } else {
                    self.error = Some(format!(
                        "Batch export finished with {} errors out of {total} jobs",
                        errors.len()
                    ));
                }
            }
        }
    }

    fn batch_export_dialog(&mut self, ui: &mut egui::Ui) {
        if !self.batch_dialog_open {
            return;
        }

        let mut open = self.batch_dialog_open;
        let mut close_dialog = false;
        let mut start_export = false;

        egui::Window::new("Batch Multi-Export & Recipes")
            .open(&mut open)
            .resizable(true)
            .default_size(vec2(520.0, 480.0))
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
                        self.request(ui.ctx(), platform::Request::BatchOutputDir);
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

                for recipe in &mut self.export_recipes {
                    ui.horizontal(|ui| {
                        widgets::checkbox(ui, &mut recipe.enabled);
                        let fmt_tag = match recipe.format {
                            batch::ExportImageFormat::Jpeg => "JPEG",
                            batch::ExportImageFormat::Png => "PNG",
                            batch::ExportImageFormat::Tiff => "TIFF",
                            batch::ExportImageFormat::Webp => "WEBP",
                        };
                        ui.label(
                            RichText::new(format!("[{fmt_tag}]"))
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
                    });
                }

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

                let sample_ctx = batch::RenameContext {
                    source_path: &sample_path,
                    sequence_index: 0,
                    camera_model: Some("Camera"),
                    rating: 5,
                    custom_text: "",
                    target_extension: "jpg",
                    date_override: None,
                };
                let preview_name = self.batch_renamer.format_filename(&sample_ctx);
                ui.horizontal(|ui| {
                    ui.label(theme::tag("Preview:").color(theme::TEXT_FAINT));
                    ui.label(
                        RichText::new(preview_name)
                            .family(theme::mono_bold())
                            .color(theme::LIME),
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
                    if widgets::button(ui, Kind::Secondary, "Cancel Export", 120.0).clicked() {
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

    fn execute_batch_export(&mut self) {
        let Some(out_dir) = self.batch_output_dir.clone() else {
            return;
        };

        let selected_paths: Vec<PathBuf> = if !self.selection.is_empty() {
            self.selection.selected.iter().cloned().collect()
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

        for (seq_idx, src_path) in selected_paths.iter().enumerate() {
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
                    _ => (
                        PhotoStack::default(),
                        photo_engine::AssetMetadata::default(),
                    ),
                };
                let crop = stack.geometry.crop;
                (stack, crop, meta.rating)
            };

            for recipe in &active_recipes {
                let target_dir = if let Some(sub) = &recipe.subfolder {
                    out_dir.join(sub)
                } else {
                    out_dir.clone()
                };

                let ctx = batch::RenameContext {
                    source_path: src_path,
                    sequence_index: seq_idx,
                    camera_model: None,
                    rating,
                    custom_text: "",
                    target_extension: recipe.format.extension(),
                    date_override: None,
                };

                if let Some(target_path) = self.batch_renamer.resolve_target_path(&target_dir, &ctx)
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

    fn pick_and_open(&mut self, ctx: &egui::Context) {
        self.request(ctx, platform::Request::OpenImage);
    }

    fn pick_and_shelve(&mut self, ctx: &egui::Context) {
        self.request(ctx, platform::Request::AddImages);
    }

    fn pick_and_export(&mut self, ctx: &egui::Context) {
        let (dir, name) = self.export_suggestion();
        self.request(ctx, platform::Request::ExportImage { dir, name });
    }

    /// Ask the platform for a picker. Its answer is handled at the top of a later frame,
    /// which this makes sure happens even when the answer was immediate.
    fn request(&mut self, ctx: &egui::Context, request: platform::Request) {
        self.platform.request(request);
        ctx.request_repaint();
    }

    fn poll_platform(&mut self, gpu: &Gpu) {
        for response in self.platform.poll() {
            match response {
                platform::Response::OpenImage(path) => self.open_path(gpu, path),
                platform::Response::AddImages(paths) => self.add_to_shelf(gpu, paths),
                platform::Response::BrowseFolder(dir) => {
                    self.folder_tree.current_folder = Some(dir);
                }
                platform::Response::BatchOutputDir(dir) => self.batch_output_dir = Some(dir),
                platform::Response::ExportImage(path) => self.export_to(gpu, path),
                platform::Response::Status(msg) => self.set_status(msg),
            }
        }
    }

    pub fn is_stack_edited(&self) -> bool {
        self.stack != self.baseline
    }

    /// The edit on the stage.
    pub fn stack(&self) -> &PhotoStack {
        &self.stack
    }

    /// Framing a crop: the whole frame is on screen instead of the cropped picture.
    pub fn is_cropping(&self) -> bool {
        self.cropping
    }

    /// The photo on the stage, once its decode has landed.
    pub fn loaded_path(&self) -> Option<&std::path::Path> {
        self.loaded_path.as_deref()
    }

    /// Why the last open failed, as shown on the stage.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// A decode is running, or thumbnails are still being built.
    pub fn is_busy(&self) -> bool {
        self.loading.is_some() || self.thumbs_requested.len() > self.thumbs_answered
    }

    fn sidecar_state(&self) -> SidecarState {
        if self.loaded_path.is_none() {
            return SidecarState::NoImage;
        }
        match &self.saved_stack {
            Some(saved) if *saved == self.stack => SidecarState::Synced,
            Some(_) => SidecarState::Modified,
            None if self.is_stack_edited() => SidecarState::Unsaved,
            None => SidecarState::Pristine,
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_notification = Some((msg.into(), Instant::now()));
    }

    pub fn save_current_sidecar(&mut self) {
        let Some(path) = &self.loaded_path else {
            return;
        };
        let metadata = self
            .shelf
            .iter()
            .find(|it| &it.path == path)
            .map(|it| it.metadata.clone())
            .unwrap_or_default();
        match photo_engine::save_sidecar_full(path, &self.stack, &metadata) {
            Ok(p) => {
                if let Some(item) = self.shelf.iter_mut().find(|it| &it.path == path) {
                    item.has_sidecar = true;
                    item.is_edited = false;
                }
                self.saved_stack = Some(self.stack.clone());
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("sidecar");
                self.set_status(format!("Saved sidecar: {name}"));
            }
            Err(e) => {
                self.error = Some(format!("Failed to save sidecar: {e:#}"));
            }
        }
    }

    pub fn set_rating_for_path(&mut self, p: &Path, rating: u8) {
        if let Some(item) = self.shelf.iter_mut().find(|it| it.path == p) {
            item.metadata.rating = rating;
            item.has_sidecar = true;
            let stack = if Some(p) == self.loaded_path.as_deref() {
                self.stack.clone()
            } else {
                photo_engine::load_sidecar(p)
                    .unwrap_or(None)
                    .unwrap_or_default()
            };
            let _ = photo_engine::save_sidecar_full(p, &stack, &item.metadata);
        }
    }

    pub fn set_flag_for_path(&mut self, p: &Path, flag: photo_engine::Flag) {
        if let Some(item) = self.shelf.iter_mut().find(|it| it.path == p) {
            item.metadata.flag = flag;
            item.has_sidecar = true;
            let stack = if Some(p) == self.loaded_path.as_deref() {
                self.stack.clone()
            } else {
                photo_engine::load_sidecar(p)
                    .unwrap_or(None)
                    .unwrap_or_default()
            };
            let _ = photo_engine::save_sidecar_full(p, &stack, &item.metadata);
        }
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
        let Some(path) = &self.loaded_path else {
            return;
        };
        match load_sidecar(path) {
            Ok(Some(saved)) => {
                self.history.checkpoint(&self.stack);
                self.saved_stack = Some(saved.clone());
                self.stack = saved;
                self.needs_render = true;
                if let Some(item) = self.shelf.iter_mut().find(|it| &it.path == path) {
                    item.is_edited = false;
                }
                self.set_status("Reverted to saved sidecar");
            }
            Ok(None) => {
                self.set_status("No sidecar file found to revert to");
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
        self.clipboard_stack = Some(self.stack.clone());
        self.set_status("Copied adjustments to clipboard");
    }

    pub fn paste_adjustments(&mut self) {
        let Some(copied) = &self.clipboard_stack else {
            self.set_status("Clipboard is empty");
            return;
        };
        self.history.checkpoint(&self.stack);
        self.stack = copied.clone();
        self.needs_render = true;
        if let Some(path) = &self.loaded_path {
            if let Some(item) = self.shelf.iter_mut().find(|it| &it.path == path) {
                item.is_edited = true;
            }
        }
        self.set_status("Pasted adjustments from clipboard");
    }

    pub fn paste_to_all_in_shelf(&mut self) {
        let Some(copied) = &self.clipboard_stack else {
            self.set_status("Clipboard is empty");
            return;
        };
        let mut count = 0;
        for item in &mut self.shelf {
            if save_sidecar(&item.path, copied).is_ok() {
                item.has_sidecar = true;
                count += 1;
                if self.loaded_path.as_ref() == Some(&item.path) {
                    self.saved_stack = Some(copied.clone());
                }
            }
        }
        self.set_status(format!("Applied adjustments to {count} photos in shelf"));
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
            self.open_path(gpu, first);
        }
    }

    fn request_thumbnail(&mut self, path: &std::path::Path) {
        if self.thumbs_requested.insert(path.to_path_buf()) {
            self.loader.thumbnail(path.to_path_buf());
        }
    }

    /// Step to the previous (-1) or next (+1) photo on the shelf.
    fn step_photo(&mut self, gpu: &Gpu, dir: isize) {
        if self.shelf.is_empty() {
            return;
        }
        let current = self
            .loading
            .as_ref()
            .map(|(p, _)| p)
            .or(self.loaded_path.as_ref())
            .and_then(|p| self.shelf.iter().position(|it| &it.path == p));
        let n = self.shelf.len() as isize;
        let next = match current {
            Some(i) => (i as isize + dir).rem_euclid(n) as usize,
            None => 0,
        };
        let path = self.shelf[next].path.clone();
        self.open_path(gpu, path);
    }

    /// Start opening a photo. The decode runs in the background (see `loader`); the
    /// current photo stays on screen and editable until the new one is ready.
    pub fn open_path(&mut self, _gpu: &Gpu, path: PathBuf) {
        if self.loaded_path.as_ref() == Some(&path) && self.loading.is_none() {
            return;
        }
        self.open_generation += 1;
        self.loading = Some((path.clone(), Instant::now()));
        self.loader.open(path, self.open_generation);
    }

    /// Collect whatever the workers have finished.
    fn poll_loader(&mut self, ctx: &egui::Context, gpu: &Gpu) {
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
                        Err(e) => self.error = Some(e),
                    }
                }
                // Superseded by a newer request while it was decoding.
                loader::Done::Opened { .. } => {}
                loader::Done::Preview {
                    path,
                    generation,
                    image,
                } => {
                    // Only while its own decode is still running; a preview that loses the
                    // race to the real image has nothing left to stand in for.
                    let current = self.loading.as_ref().is_some_and(|(p, _)| *p == path);
                    if generation == self.open_generation && current {
                        let texture =
                            ctx.load_texture("stage preview", image, egui::TextureOptions::LINEAR);
                        self.stage_preview = Some((path, texture));
                    }
                }
                loader::Done::Thumbnail { path, image } => {
                    self.thumbs_answered += 1;
                    if let Some(image) = image {
                        let name = format!("thumb:{}", path.display());
                        let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
                        self.thumbs.insert(path, texture);
                    }
                }
            }
        }
    }

    fn finish_open(
        &mut self,
        gpu: &Gpu,
        path: PathBuf,
        uploaded: Box<photo_engine::UploadedImage>,
    ) {
        // Save the photo being left, including anything edited while this one decoded.
        if let Some(current) = &self.loaded_path {
            if self.is_stack_edited() && save_sidecar(current, &self.stack).is_ok() {
                if let Some(item) = self.shelf.iter_mut().find(|it| &it.path == current) {
                    item.has_sidecar = true;
                }
            }
        }

        self.error = None;
        let orientation = uploaded.orientation;
        self.release_texture(gpu);

        // The pixels are already on the GPU (the loader uploaded them); what is left here
        // is recording the demosaic.
        let load_res = self.engine.load_uploaded(gpu.device, gpu.queue, *uploaded);

        if let Err(e) = load_res {
            // DESIGN.md: failures surface in the UI, with the filename.
            self.error = Some(format!("{}: {e:#}", path.display()));
            self.loaded_path = None;
            return;
        }

        let (saved, metadata) = match photo_engine::load_sidecar_full(&path) {
            Ok(Some(sc)) => {
                let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                self.set_status(format!("Loaded sidecar for {fname}"));
                (Some(sc.stack), sc.metadata)
            }
            _ => (None, photo_engine::AssetMetadata::default()),
        };

        let mut item = ShelfItem::new(path.clone());
        item.metadata = metadata;
        if let Some(existing) = self.shelf.iter_mut().find(|it| it.path == path) {
            existing.has_sidecar = item.has_sidecar;
            existing.is_edited = false;
            existing.metadata = item.metadata.clone();
        } else {
            self.request_thumbnail(&path);
            self.shelf.push(item);
        }

        self.selection.select_single(path.clone());
        self.baseline = PhotoStack {
            geometry: orientation,
            ..PhotoStack::default()
        };
        self.loaded_path = Some(path);
        self.stack = saved.clone().unwrap_or_else(|| self.baseline.clone());
        self.saved_stack = saved;
        self.history = History::new();
        self.view = View::default();
        self.cropping = false;
        self.crop_drag = None;
        self.needs_render = true;
    }

    fn release_texture(&mut self, gpu: &Gpu) {
        if let Some(id) = self.texture_id.take() {
            renderer_write(gpu).free_texture(&id);
        }
        self.engine.unload();
    }

    /// The coarsest preview level still at least as dense as the screen: each level
    /// halves the resolution, so level L is right while one image pixel covers less than
    /// 1/2^L of a screen pixel.
    fn wanted_preview_level(&self, pixels_per_point: f32) -> u32 {
        let Some(fit) = self.fit_scale() else {
            return 0;
        };
        let density = fit * self.view.zoom * pixels_per_point;
        if density >= 1.0 || density <= 0.0 {
            return 0;
        }
        ((1.0 / density).log2().floor() as u32).min(photo_engine::MAX_PREVIEW_LEVEL)
    }

    /// Runs the GPU work for this frame, if anything changed.
    fn sync(&mut self, gpu: &Gpu, pixels_per_point: f32) {
        // Zooming changes how much resolution the screen can show; the preview follows,
        // and a new preview texture needs registering with egui like a turned one does.
        let level = self.wanted_preview_level(pixels_per_point);
        if self.engine.set_preview_level(gpu.device, level) {
            if let Some(id) = self.texture_id.take() {
                renderer_write(gpu).free_texture(&id);
            }
            self.needs_render = true;
        }

        // A right-angle turn replaces the output texture, so the id egui holds becomes
        // stale and has to be re-registered. Straighten, mirror and crop never resize.
        if self.engine.set_geometry(gpu.device, self.stack.geometry) {
            if let Some(id) = self.texture_id.take() {
                renderer_write(gpu).free_texture(&id);
            }
            self.needs_render = true;
        }

        if !self.needs_render {
            return;
        }
        let started = Instant::now();
        let split_x = if self.split_active {
            Some(self.split_pos)
        } else {
            None
        };
        self.engine
            .render(gpu.device, gpu.queue, &self.stack, split_x);
        self.render_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.needs_render = false;

        if self.texture_id.is_none() {
            if let Some(image) = self.engine.image() {
                let id = renderer_write(gpu).register_native_texture(
                    gpu.device,
                    &image.output_view,
                    wgpu::FilterMode::Linear,
                );
                self.texture_id = Some(id);
            }
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        let ctx = ui.ctx().clone();
        self.loader.set_context(&ctx);
        self.poll_platform(gpu);
        self.poll_loader(&ctx, gpu);
        self.handle_keys(&ctx, gpu);
        self.poll_export(&ctx);
        self.poll_batch_export();
        self.engine.poll_histogram(gpu.device);
        self.sync(gpu, ctx.pixels_per_point());

        // Panel order is layout order: full-width bars first, then the side panels, then
        // the filmstrip and stage toolbar, which only span the space left between them.
        self.top_bar(ui);
        self.status_bar(ui);
        self.shelf(ui, gpu);
        self.inspector(ui);
        self.filmstrip(ui, gpu);
        self.stage_toolbar(ui);
        self.workspace(ui);
        self.batch_export_dialog(ui);
    }

    fn handle_keys(&mut self, ctx: &egui::Context, gpu: &Gpu) {
        // Android's back gesture: close one thing per press, innermost first, and leave
        // the app only once there is nothing left to close.
        if ctx.input(|i| i.key_pressed(egui::Key::BrowserBack)) {
            if let Some(focused) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(focused));
            } else if self.cropping {
                // As Esc: the framing in progress is thrown away.
                self.cancel_crop();
            } else if self.batch_dialog_open {
                self.batch_dialog_open = false;
            } else if self.customize_open {
                self.customize_open = false;
            } else if self.folder_browser_open {
                self.folder_browser_open = false;
            } else {
                self.platform.leave();
            }
            return;
        }
        // DESIGN.md: hotkeys never fire while a text field has focus.
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let (prev, next) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        if prev {
            self.step_photo(gpu, -1);
        }
        if next {
            self.step_photo(gpu, 1);
        }
        let (undo, redo) = ctx.input(|i| {
            (
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Z),
            )
        });
        if undo && self.history.undo(&mut self.stack) {
            self.needs_render = true;
        }
        if redo && self.history.redo(&mut self.stack) {
            self.needs_render = true;
        }

        let (save, copy, paste, split_toggle, ctrl_a, batch_export) = ctx.input(|i| {
            (
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::S),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::C),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::V),
                i.key_pressed(egui::Key::Backslash),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::A),
                i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::E),
            )
        });
        if save {
            self.save_current_sidecar();
        }
        if copy {
            self.copy_adjustments();
        }
        if paste {
            self.paste_adjustments();
        }
        if split_toggle {
            self.split_active = !self.split_active;
            self.needs_render = true;
        }
        if ctrl_a {
            let all_paths: Vec<PathBuf> = self.shelf.iter().map(|it| it.path.clone()).collect();
            self.selection.select_all(&all_paths);
        }
        if batch_export {
            self.batch_dialog_open = true;
        }

        let (pick, reject, unflag) = ctx.input(|i| {
            (
                !i.modifiers.command && i.key_pressed(egui::Key::P),
                !i.modifiers.command && i.key_pressed(egui::Key::X),
                !i.modifiers.command && i.key_pressed(egui::Key::U),
            )
        });
        if pick {
            self.set_flag_for_selection(photo_engine::Flag::Pick);
        }
        if reject {
            self.set_flag_for_selection(photo_engine::Flag::Reject);
        }
        if unflag {
            self.set_flag_for_selection(photo_engine::Flag::Unflagged);
        }

        let rating_cmd = ctx.input(|i| {
            if !i.modifiers.command {
                if i.key_pressed(egui::Key::Num1) {
                    Some(1)
                } else if i.key_pressed(egui::Key::Num2) {
                    Some(2)
                } else if i.key_pressed(egui::Key::Num3) {
                    Some(3)
                } else if i.key_pressed(egui::Key::Num4) {
                    Some(4)
                } else if i.key_pressed(egui::Key::Num5) {
                    Some(5)
                } else if i.key_pressed(egui::Key::Num0) {
                    Some(0)
                } else {
                    None
                }
            } else {
                None
            }
        });
        if let Some(r) = rating_cmd {
            self.set_rating_for_selection(r);
        }

        let (fit, actual, zoom_toggle) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::Num0),
                i.modifiers.command && i.key_pressed(egui::Key::Num1),
                !i.modifiers.command && i.key_pressed(egui::Key::Z),
            )
        });
        if fit {
            self.view = View::default();
        }
        if actual {
            self.zoom_actual();
        }
        if zoom_toggle {
            if (self.view.zoom - 1.0).abs() < 0.05 {
                self.zoom_actual();
            } else {
                self.view = View::default();
            }
        }

        let (rot_left, rot_right, flip_h, flip_v, crop_toggle, crop_esc, crop_enter) =
            ctx.input(|i| {
                (
                    !i.modifiers.command && i.key_pressed(egui::Key::OpenBracket),
                    !i.modifiers.command && i.key_pressed(egui::Key::CloseBracket),
                    !i.modifiers.command && i.key_pressed(egui::Key::H),
                    !i.modifiers.command && i.key_pressed(egui::Key::V),
                    !i.modifiers.command && i.key_pressed(egui::Key::C),
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::Enter),
                )
            });
        if rot_left {
            let before = self.stack.clone();
            self.stack.geometry.rotate_left();
            self.history.checkpoint(&before);
            self.rebase_crop();
            self.needs_render = true;
        }
        if rot_right {
            let before = self.stack.clone();
            self.stack.geometry.rotate_right();
            self.history.checkpoint(&before);
            self.rebase_crop();
            self.needs_render = true;
        }
        if flip_h {
            let before = self.stack.clone();
            self.stack.geometry.flip_h();
            self.history.checkpoint(&before);
            self.rebase_crop();
            self.needs_render = true;
        }
        if flip_v {
            let before = self.stack.clone();
            self.stack.geometry.flip_v();
            self.history.checkpoint(&before);
            self.rebase_crop();
            self.needs_render = true;
        }
        if crop_toggle {
            self.toggle_crop();
        }
        if crop_esc {
            self.cancel_crop();
        }
        if crop_enter {
            self.apply_crop();
        }
    }

    // ── Crop mode ───────────────────────────────────────────────────────────────────────

    /// Show the whole frame again, with the crop drawn over it, to reframe.
    fn begin_crop(&mut self) {
        if self.cropping || self.engine.image().is_none() {
            return;
        }
        self.cropping = true;
        self.crop_before = self.stack.geometry.crop;
        self.crop_drag = None;
        // The shown region just changed size, so a zoom into the old one means nothing.
        self.view = View::default();
    }

    /// Keep the new framing and go back to editing inside it. The whole session is one
    /// undo step, however many drags it took.
    fn apply_crop(&mut self) {
        if !self.cropping {
            return;
        }
        self.cropping = false;
        self.crop_drag = None;
        if self.stack.geometry.crop != self.crop_before {
            let mut before = self.stack.clone();
            before.geometry.crop = self.crop_before;
            self.history.checkpoint(&before);
        }
        self.view = View::default();
        self.needs_render = true;
    }

    /// Leave crop mode with the crop as it was on entering.
    fn cancel_crop(&mut self) {
        if !self.cropping {
            return;
        }
        self.cropping = false;
        self.crop_drag = None;
        self.stack.geometry.crop = self.crop_before;
        self.view = View::default();
        self.needs_render = true;
    }

    fn toggle_crop(&mut self) {
        if self.cropping {
            self.apply_crop();
        } else {
            self.begin_crop();
        }
    }

    /// A turn or flip mid-session moves the crop into a new frame, and is its own undo
    /// step; Cancel returns to the crop as it stands after it, not to one in the old frame.
    fn rebase_crop(&mut self) {
        if self.cropping {
            self.crop_before = self.stack.geometry.crop;
        }
    }

    /// The part of the frame on screen: all of it while framing, otherwise the crop.
    fn shown_region(&self) -> Crop {
        if self.cropping {
            Crop::default()
        } else {
            self.stack.geometry.crop.sanitised()
        }
    }

    /// Size of the shown region, in full-resolution output pixels.
    fn shown_size(&self) -> Option<egui::Vec2> {
        let image = self.engine.image()?;
        let region = self.shown_region();
        Some(egui::vec2(
            image.width as f32 * region.width,
            image.height as f32 * region.height,
        ))
    }

    /// Kelvin and tint for the open photo's camera.
    fn white_scale(&self) -> WhiteScale {
        self.engine
            .image()
            .map_or_else(WhiteScale::srgb, |image| image.white_scale())
    }

    /// Scale at which the whole frame fits the panel it had last frame.
    fn fit_scale(&self) -> Option<f32> {
        let img = self.shown_size()?;
        let panel = self.last_panel;
        if panel.x < 1.0 || panel.y < 1.0 {
            return None;
        }
        Some((panel.x / img.x).min(panel.y / img.y))
    }

    fn zoom_by(&mut self, factor: f32) {
        let max_zoom = self
            .fit_scale()
            .map_or(1.0, |fit| (MAX_ZOOM_SCALE / fit).max(1.0));
        self.view.zoom = (self.view.zoom * factor).clamp(1.0, max_zoom);
    }

    fn zoom_actual(&mut self) {
        // 1:1 is expressed relative to fit, which depends on the panel size, so it is
        // resolved against the screen rect the workspace actually got last frame.
        if let Some(percent) = self.zoom_percent(self.last_panel) {
            if percent > 0.0 {
                self.view.zoom *= 100.0 / percent;
                self.view.zoom = self.view.zoom.max(1.0);
            }
        }
    }

    // ── Top bar ─────────────────────────────────────────────────────────────────────────

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(14, 9)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::brand_mark(ui, 26.0);
                    ui.add_space(2.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(
                            RichText::new("DUCKFOOT")
                                .family(theme::display())
                                .size(15.0)
                                .extra_letter_spacing(1.4)
                                .color(theme::TEXT_HI),
                        );
                        ui.label(
                            RichText::new("THREE BARRELS, ONE STOCK")
                                .monospace()
                                .size(9.0)
                                .extra_letter_spacing(0.6)
                                .color(Color32::from_gray(0xcc)),
                        );
                    });
                    ui.add_space(12.0);

                    // The other two barrels exist in the design, not in the code. Showing
                    // them disabled is honest; showing them clickable would not be.
                    pod(ui, |ui| {
                        let _ = widgets::chip(ui, true, "photo");
                        ui.add_enabled_ui(false, |ui| {
                            widgets::chip(ui, false, "audio")
                                .on_disabled_hover_text("Barrel 2 is design only — see DESIGN.md");
                            widgets::chip(ui, false, "video")
                                .on_disabled_hover_text("Barrel 3 is design only — see DESIGN.md");
                        });
                    });

                    if ui.available_width() > 720.0 {
                        ui.add_space(10.0);
                        self.telemetry_pill(ui);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let busy = matches!(self.export, Export::Writing { .. });
                        let ready = self.engine.image().is_some() && !busy;
                        let export = ui
                            .add_enabled_ui(ready, |ui| {
                                widgets::button(ui, Kind::Primary, "Export", 84.0)
                                    .on_hover_text("Write the full-resolution image as PNG or JPEG")
                            })
                            .inner;
                        if export.clicked() {
                            self.pick_and_export(ui.ctx());
                        }
                        if widgets::button(ui, Kind::Secondary, "Open RAW…", 0.0).clicked() {
                            self.pick_and_open(ui.ctx());
                        }
                        ui.add_space(4.0);

                        // Right-to-left, so these read shelf · split · inspector on screen.
                        pod(ui, |ui| {
                            let wide = self.inspector_width >= 400.0;
                            if widgets::icon(
                                ui,
                                "◨",
                                wide,
                                if wide {
                                    "Narrow the inspector (300px)"
                                } else {
                                    "Widen the inspector for the 3D views (460px)"
                                },
                            )
                            .clicked()
                            {
                                let target = if wide { 300.0 } else { 460.0 };
                                self.inspector_width = target;
                                self.inspector_force_width = Some(target);
                            }
                            if widgets::icon(
                                ui,
                                "◫",
                                self.split_active,
                                "Before / after split (\\)",
                            )
                            .clicked()
                            {
                                self.split_active = !self.split_active;
                                self.needs_render = true;
                            }
                            if widgets::icon(ui, "◧", self.shelf_open, "Show or hide the shelf")
                                .clicked()
                            {
                                self.shelf_open = !self.shelf_open;
                            }
                        });
                    });
                });
            });
    }

    /// The session readout: camera, dimensions, undo depth. Real values only.
    fn telemetry_pill(&self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .stroke(Stroke::new(1.0, Color32::from_gray(0x2a)))
            .corner_radius(theme::RADIUS)
            .inner_margin(Margin::symmetric(12, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let sep = |ui: &mut egui::Ui| {
                        ui.label(
                            RichText::new("|")
                                .monospace()
                                .color(Color32::from_gray(0x33)),
                        );
                    };
                    match self.engine.image() {
                        Some(image) => {
                            widgets::dot(ui, theme::ACCENT);
                            let camera = format!("{} {}", image.make, image.model);
                            ui.label(
                                RichText::new(camera.trim())
                                    .family(theme::mono_bold())
                                    .size(11.5)
                                    .color(theme::ACCENT),
                            );
                            sep(ui);
                            ui.label(
                                theme::num(format!("{} × {}", image.width, image.height))
                                    .size(11.5),
                            );
                            sep(ui);
                            ui.label(
                                theme::num(format!("{:.1} MP", image.megapixels()))
                                    .size(11.5)
                                    .color(Color32::from_gray(0xcc)),
                            );
                            sep(ui);
                            ui.label(
                                theme::num(format!("undo ({})", self.history.past.len()))
                                    .size(11.5)
                                    .color(theme::TEXT_DIM),
                            );
                        }
                        None => {
                            widgets::dot(ui, theme::TEXT_FAINT);
                            ui.label(
                                theme::num("no image loaded")
                                    .size(11.5)
                                    .color(theme::TEXT_DIM),
                            );
                        }
                    }
                });
            });
    }

    // ── Shelf bin (left) ────────────────────────────────────────────────────────────────

    fn shelf(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        if !self.shelf_open {
            return;
        }
        egui::Panel::left("shelf")
            .frame(egui::Frame::new().fill(theme::BG_950))
            .resizable(true)
            .default_size(272.0)
            .min_size(220.0)
            .max_size(400.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;

                // Footer first, so it pins to the bottom of the panel.
                egui::Panel::bottom("shelf_footer")
                    .frame(
                        egui::Frame::new()
                            .fill(theme::BG_925)
                            .inner_margin(Margin::symmetric(12, 8)),
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(theme::tag("shelf total").color(Color32::from_gray(0xcc)));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let total: u64 = self.shelf.iter().map(|it| it.bytes).sum();
                                    ui.label(
                                        RichText::new(format_bytes(total))
                                            .family(theme::mono_bold())
                                            .size(12.0)
                                            .color(theme::ACCENT),
                                    );
                                },
                            );
                        });
                    });

                widgets::band(ui, |ui| {
                    ui.label(RichText::new("🗁").size(15.0).color(theme::ACCENT));
                    ui.label(theme::head("Shelf bin"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon(ui, "✚", false, "Add photos to the shelf").clicked() {
                            self.pick_and_shelve(ui.ctx());
                        }
                    });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let total_shelf = self.shelf.len();
                    if widgets::chip(
                        ui,
                        !self.folder_browser_open,
                        &format!("Stills ({total_shelf})"),
                    )
                    .clicked()
                    {
                        self.folder_browser_open = false;
                    }
                    if widgets::chip(ui, self.folder_browser_open, "Folders").clicked() {
                        self.folder_browser_open = true;
                    }
                });
                ui.add_space(4.0);

                let mut to_open = None;
                let mut to_remove = None;

                if self.folder_browser_open {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                        )
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(Margin {
                                    left: 10,
                                    right: 10,
                                    top: 4,
                                    bottom: 10,
                                })
                                .show(ui, |ui| {
                                    self.shelf_folders(ui, gpu);
                                });
                        });
                } else {
                    // Drop target. Dropping works anywhere in the window; this is where it's
                    // advertised, and clicking it is the same as the + above.
                    egui::Frame::new()
                        .inner_margin(Margin {
                            left: 10,
                            right: 10,
                            top: 6,
                            bottom: 6,
                        })
                        .show(ui, |ui| {
                            let (rect, response) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 34.0),
                                Sense::click(),
                            );
                            let dragging_in = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
                            let hot = response.hovered() || dragging_in;
                            widgets::dashed_rect(
                                ui.painter(),
                                rect,
                                if hot {
                                    theme::ACCENT
                                } else {
                                    Color32::from_gray(0x2a)
                                },
                            );
                            let painter = ui.painter();
                            let label = painter.layout_no_wrap(
                                "Drop Photos (RAW / Raster)".to_owned(),
                                FontId::proportional(12.5),
                                if hot {
                                    theme::TEXT_HI
                                } else {
                                    Color32::from_gray(0xcc)
                                },
                            );
                            let left = rect.center().x - (label.size().x + 20.0) / 2.0;
                            painter.text(
                                pos2(left + 6.0, rect.center().y),
                                Align2::CENTER_CENTER,
                                "⬆",
                                FontId::proportional(14.0),
                                theme::ACCENT,
                            );
                            painter.galley(
                                pos2(left + 20.0, rect.center().y - label.size().y / 2.0),
                                label,
                                theme::TEXT,
                            );
                            if hot {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if response.clicked() {
                                self.pick_and_shelve(ui.ctx());
                            }
                        });

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                        )
                        .show(ui, |ui| {
                            egui::Frame::new()
                                .inner_margin(Margin {
                                    left: 10,
                                    right: 10,
                                    top: 4,
                                    bottom: 10,
                                })
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 10.0;
                                    self.shelf_group(ui, &mut to_open, &mut to_remove);
                                    self.snapshots_group(ui);
                                    self.history_group(ui);
                                });
                        });
                }

                if let Some(path) = to_open {
                    self.open_path(gpu, path);
                }
                if let Some(idx) = to_remove {
                    self.shelf.remove(idx);
                }
            });
    }

    fn shelf_folders(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        let mut load_folder = None;
        let mut change_folder = None;

        group(
            ui,
            "Bookmarks",
            |_ui| {},
            |ui| {
                for bm in &self.folder_tree.bookmarks {
                    let is_cur = self.folder_tree.current_folder.as_ref() == Some(&bm.path);
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    let painter = ui.painter();
                    if resp.hovered() {
                        painter.rect_filled(rect, 3.0, Color32::from_gray(0x18));
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let col = if is_cur { theme::ACCENT } else { theme::TEXT };
                    painter.text(
                        pos2(rect.left() + 8.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        format!("📁 {}", bm.name),
                        FontId::new(12.0, theme::sans_strong()),
                        col,
                    );
                    if resp.clicked() {
                        change_folder = Some(bm.path.clone());
                    }
                }

                ui.add_space(4.0);
                if widgets::button(ui, Kind::Secondary, "Browse Folder…", 0.0).clicked() {
                    self.request(ui.ctx(), platform::Request::BrowseFolder);
                }
            },
        );

        if let Some(cur) = &self.folder_tree.current_folder.clone() {
            ui.add_space(8.0);
            let folder_name = cur.file_name().and_then(|n| n.to_str()).unwrap_or("Folder");
            let title = format!("Folder: {folder_name}");
            group(
                ui,
                &title,
                |_ui| {},
                |ui| {
                    ui.label(
                        RichText::new(cur.display().to_string())
                            .family(egui::FontFamily::Monospace)
                            .size(10.5)
                            .color(theme::TEXT_DIM),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if widgets::button(ui, Kind::Primary, "Scan & Load", 0.0)
                            .on_hover_text(
                                "Load all supported images from this folder into the shelf",
                            )
                            .clicked()
                        {
                            load_folder = Some(cur.clone());
                        }
                        if let Some(parent) = cur.parent() {
                            if widgets::button(ui, Kind::Secondary, "⬆ Parent", 0.0).clicked() {
                                change_folder = Some(parent.to_path_buf());
                            }
                        }
                    });

                    ui.add_space(6.0);
                    ui.label(theme::dim("Subfolders:"));
                    let subs = dam::FolderTree::list_subfolders(cur);
                    if subs.is_empty() {
                        ui.label(theme::dim("No subdirectories").size(11.0));
                    } else {
                        for sub in subs {
                            let name = sub.file_name().and_then(|n| n.to_str()).unwrap_or("dir");
                            let (rect, resp) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 24.0),
                                Sense::click(),
                            );
                            let painter = ui.painter();
                            if resp.hovered() {
                                painter.rect_filled(rect, 2.0, Color32::from_gray(0x18));
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            painter.text(
                                pos2(rect.left() + 8.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                format!("📁 {name}"),
                                FontId::proportional(11.5),
                                theme::TEXT,
                            );
                            if resp.clicked() {
                                change_folder = Some(sub);
                            }
                        }
                    }
                },
            );
        } else {
            ui.add_space(8.0);
            ui.label(theme::dim(
                "Select a bookmark or browse to a session folder to scan photos.",
            ));
        }

        if let Some(cf) = change_folder {
            self.folder_tree.current_folder = Some(cf);
        }
        if let Some(lf) = load_folder {
            self.open_session_folder(gpu, lf);
        }
    }

    fn shelf_group(
        &mut self,
        ui: &mut egui::Ui,
        to_open: &mut Option<PathBuf>,
        to_remove: &mut Option<usize>,
    ) {
        let total: u64 = self.shelf.iter().map(|it| it.bytes).sum();
        let title = format!("Photos ({})", self.shelf.len());
        let mut row_shift = None;
        let mut row_ctrl = None;
        let mut row_single = None;
        let mut row_rating = None;
        let mut row_flag = None;

        group(
            ui,
            &title,
            |ui| {
                ui.label(
                    theme::num(format_bytes(total))
                        .size(11.0)
                        .color(theme::TEXT_DIM),
                );
            },
            |ui| {
                if self.shelf.is_empty() {
                    ui.add_space(6.0);
                    ui.label(theme::dim("Nothing shelved yet.").size(12.0));
                    ui.add_space(6.0);
                    return;
                }
                let mut any_visible = false;
                for (i, item) in self.shelf.iter().enumerate() {
                    let active = self.loaded_path.as_ref() == Some(&item.path);
                    let is_edited = if active {
                        self.is_stack_edited()
                    } else {
                        item.is_edited
                    };
                    if !self
                        .filter
                        .matches(&item.path, &item.metadata, is_edited, item.has_sidecar)
                    {
                        continue;
                    }
                    any_visible = true;
                    // Rows are all one height, so a row scrolled out of view only has to
                    // hold its space: the same allocation `shelf_row` makes, so the scroll
                    // extent and every later row's id are unchanged. Laying out and
                    // painting all of a 2000-photo roll every frame is what this avoids.
                    let row = Rect::from_min_size(
                        ui.cursor().min,
                        vec2(ui.available_width(), SHELF_ROW_HEIGHT),
                    );
                    if !ui.is_rect_visible(row) {
                        ui.allocate_space(row.size());
                        continue;
                    }
                    let selected = self.selection.is_selected(&item.path);
                    let status = if active {
                        match self.sidecar_state() {
                            SidecarState::Synced => Some(theme::LIME),
                            SidecarState::Unsaved | SidecarState::Modified => Some(theme::ACCENT),
                            _ => None,
                        }
                    } else if item.has_sidecar {
                        Some(theme::LIME)
                    } else if item.is_edited {
                        Some(theme::ACCENT)
                    } else {
                        None
                    };
                    let (resp, remove) = shelf_row(ui, item, active, selected, status);
                    if resp.clicked() && !remove {
                        let (ctrl, shift) = ui.input(|i| {
                            (i.modifiers.command || i.modifiers.ctrl, i.modifiers.shift)
                        });
                        if shift {
                            row_shift = Some(item.path.clone());
                        } else if ctrl {
                            row_ctrl = Some(item.path.clone());
                        } else {
                            row_single = Some((item.path.clone(), active));
                        }
                    }
                    if remove {
                        *to_remove = Some(i);
                    }
                    resp.context_menu(|ui| {
                        ui.label(
                            RichText::new(&item.name)
                                .family(theme::mono_bold())
                                .color(theme::ACCENT),
                        );
                        ui.separator();
                        ui.label(theme::dim("Flags:"));
                        ui.horizontal(|ui| {
                            if ui.button("Pick (P)").clicked() {
                                row_flag = Some((item.path.clone(), photo_engine::Flag::Pick));
                                ui.close();
                            }
                            if ui.button("Reject (X)").clicked() {
                                row_flag = Some((item.path.clone(), photo_engine::Flag::Reject));
                                ui.close();
                            }
                            if ui.button("Clear (U)").clicked() {
                                row_flag = Some((item.path.clone(), photo_engine::Flag::Unflagged));
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
                                    row_rating = Some((item.path.clone(), r));
                                    ui.close();
                                }
                            }
                        });
                    });
                }

                if !any_visible {
                    ui.add_space(6.0);
                    ui.label(theme::dim("No photos match filters.").size(12.0));
                    ui.add_space(6.0);
                }
            },
        );

        if let Some(target) = row_shift {
            let all_paths: Vec<PathBuf> = self.shelf.iter().map(|it| it.path.clone()).collect();
            self.selection.select_range(target, &all_paths);
        }
        if let Some(target) = row_ctrl {
            self.selection.toggle_select(target);
        }
        if let Some((target, active)) = row_single {
            self.selection.select_single(target.clone());
            if !active {
                *to_open = Some(target);
            }
        }
        if let Some((path, rating)) = row_rating {
            if self.selection.is_selected(&path) {
                self.set_rating_for_selection(rating);
            } else {
                self.set_rating_for_path(&path, rating);
            }
        }
        if let Some((path, flag)) = row_flag {
            if self.selection.is_selected(&path) {
                self.set_flag_for_selection(flag);
            } else {
                self.set_flag_for_path(&path, flag);
            }
        }
    }

    fn snapshots_group(&mut self, ui: &mut egui::Ui) {
        let count = self.snapshots.len();
        let mut restore = None;
        let mut delete = None;
        let mut add = false;
        group(
            ui,
            "Snapshots",
            |ui| {
                ui.label(theme::tag(count.to_string()).color(theme::TEXT_DIM));
            },
            |ui| {
                list_row(ui, "current", RowStyle::Now, Some(("LIVE", theme::LIME)));
                for (i, (name, stack)) in self.snapshots.iter().enumerate() {
                    let key = snapshot_key(i);
                    let style = if *stack == self.stack {
                        RowStyle::Match
                    } else {
                        RowStyle::Normal
                    };
                    let row = list_row(ui, name, style, Some((&key, theme::TEXT_DIM)));
                    if row.clicked() {
                        restore = Some(i);
                    }
                    row.context_menu(|ui| {
                        if ui.button("Delete snapshot").clicked() {
                            delete = Some(i);
                            ui.close();
                        }
                    });
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    // `desired_width` is the text area alone; the field's own margin and
                    // the gap to the button come on top of it.
                    let field_w = ui.available_width()
                        - 52.0
                        - ui.spacing().item_spacing.x
                        - 2.0 * ui.spacing().button_padding.x;
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.new_snapshot_name)
                            .hint_text("name this state…")
                            .font(egui::TextStyle::Monospace)
                            .desired_width(field_w.max(40.0)),
                    );
                    let submitted =
                        edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if widgets::button(ui, Kind::Secondary, "+ save", 52.0)
                        .on_hover_text("Save the current stack as a snapshot")
                        .clicked()
                        || submitted
                    {
                        add = true;
                    }
                });
                ui.add_space(2.0);
            },
        );

        if add {
            let name = if self.new_snapshot_name.trim().is_empty() {
                format!("snapshot {}", self.snapshots.len() + 1)
            } else {
                self.new_snapshot_name.trim().to_string()
            };
            self.snapshots.push((name, self.stack.clone()));
            self.new_snapshot_name.clear();
        }
        if let Some(i) = restore {
            self.history.checkpoint(&self.stack);
            self.stack = self.snapshots[i].1.clone();
            self.needs_render = true;
        }
        if let Some(i) = delete {
            self.snapshots.remove(i);
        }
    }

    fn history_group(&mut self, ui: &mut egui::Ui) {
        let now = self.history.past.len();
        let mut jump = None;
        {
            let timeline = self.history.timeline(&self.stack);
            let steps = timeline.len() - 1;
            group(
                ui,
                "History",
                |ui| {
                    ui.label(theme::tag(format!("{steps} steps")).color(theme::ACCENT));
                },
                |ui| {
                    // Newest first. Rows above NOW are redo steps: not applied, so faint.
                    for k in (0..timeline.len()).rev() {
                        let label = if k == 0 {
                            if self.history.truncated {
                                "oldest kept"
                            } else {
                                "opened"
                            }
                            .to_string()
                        } else {
                            describe_change(timeline[k - 1], timeline[k])
                        };
                        let (style, right) = if k == now {
                            (RowStyle::Now, "NOW".to_string())
                        } else if k > now {
                            (RowStyle::Faint, k.to_string())
                        } else {
                            (RowStyle::Normal, k.to_string())
                        };
                        let colour = if k == now {
                            theme::ACCENT
                        } else {
                            theme::TEXT_FAINT
                        };
                        if list_row(ui, &label, style, Some((&right, colour))).clicked() && k != now
                        {
                            jump = Some(k);
                        }
                    }
                },
            );
        }
        if let Some(k) = jump {
            if self.history.jump_to(&mut self.stack, k) {
                self.needs_render = true;
            }
        }
    }

    // ── Stage toolbar, canvas and filmstrip (centre) ────────────────────────────────────

    /// CROP to start framing; APPLY and CANCEL while framing.
    fn crop_chips(&mut self, ui: &mut egui::Ui) {
        if self.cropping {
            if widgets::chip(ui, true, "✓ apply crop")
                .on_hover_text("Keep this framing and carry on editing inside it (Enter or C)")
                .clicked()
            {
                self.apply_crop();
            }
            if widgets::chip(ui, false, "✕ cancel")
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

    fn stage_toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("stage_toolbar")
            .frame(egui::Frame::new().fill(theme::BG_950).inner_margin(Margin::symmetric(14, 7)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let loaded = self.engine.image().is_some();
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
                                .on_hover_text("Actual pixels (1)")
                                .clicked()
                            {
                                self.zoom_actual();
                            }
                            if widgets::button(ui, Kind::Secondary, "FIT", 0.0)
                                .on_hover_text("Fit to window (0)")
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

    fn filmstrip(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        let mut to_open = None;
        let mut action_shift = None;
        let mut action_ctrl = None;
        let mut action_single = None;
        let mut action_rating = None;
        let mut action_flag = None;
        let mut action_export = false;

        let is_stack_edited = self.is_stack_edited();
        let loaded_path = self.loaded_path.clone();

        // Indices into the shelf, not copies of it: this runs every frame, and cloning
        // every item (metadata strings and all) grew with the size of the folder.
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

                // Filter & Sort bar
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

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

                    ui.separator();

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
                        if widgets::chip(ui, f == dam::FlagFilter::HideRejects, "Hide Rej").clicked()
                        {
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

                    ui.separator();

                    // Search box
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter.search_query)
                            .hint_text("Search…")
                            .desired_width(75.0)
                            .font(egui::TextStyle::Monospace),
                    );
                    if !self.filter.search_query.is_empty()
                        && ui.button("✕").on_hover_text("Clear search").clicked()
                    {
                        self.filter.search_query.clear();
                    }

                    // Sort & Batch Actions (right aligned)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let arrow = if self.filter.sort_descending { "↓" } else { "↑" };
                        if widgets::button(ui, Kind::Secondary, arrow, 22.0)
                            .on_hover_text(if self.filter.sort_descending {
                                "Descending order"
                            } else {
                                "Ascending order"
                            })
                            .clicked()
                        {
                            self.filter.sort_descending = !self.filter.sort_descending;
                        }

                        pod(ui, |ui| {
                            let s = self.filter.sort_by;
                            if widgets::chip(ui, s == dam::SortBy::FileSize, "Size").clicked() {
                                self.filter.sort_by = dam::SortBy::FileSize;
                            }
                            if widgets::chip(ui, s == dam::SortBy::Rating, "Rating").clicked() {
                                self.filter.sort_by = dam::SortBy::Rating;
                            }
                            if widgets::chip(ui, s == dam::SortBy::FileName, "Name").clicked() {
                                self.filter.sort_by = dam::SortBy::FileName;
                            }
                        });
                        ui.label(theme::dim("Sort:"));

                        if !self.selection.is_empty() {
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
                        }
                    });
                });
                ui.add_space(6.0);

                egui::ScrollArea::horizontal()
                    .auto_shrink([false, true])
                    .show_viewport(ui, |ui, viewport| {
                        if self.shelf.is_empty() {
                            let (rect, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 70.0),
                                Sense::hover(),
                            );
                            ui.painter().text(
                                rect.left_center() + vec2(4.0, 0.0),
                                Align2::LEFT_CENTER,
                                "Drop photos anywhere in the window to start a roll.",
                                FontId::proportional(12.0),
                                theme::TEXT_DIM,
                            );
                            return;
                        }

                        if displayed.is_empty() {
                            let (rect, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 70.0),
                                Sense::hover(),
                            );
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
                        ui.set_min_size(vec2(
                            total as f32 * FILM_STRIDE - FILM_GAP,
                            FILM_TILE.y,
                        ));
                        let visible = visible_tiles(viewport.x_range(), total);

                        let loading = self.loading.as_ref().map(|(p, _)| p);
                        for (slot, &index) in
                            displayed.iter().enumerate().take(visible.end).skip(visible.start)
                        {
                            let item = &self.shelf[index];
                            let active = self.loaded_path.as_ref() == Some(&item.path);
                            let selected = self.selection.is_selected(&item.path);
                            let picture = if active {
                                self.texture_id.zip(
                                    self.engine
                                        .image()
                                        .map(|img| img.width as f32 / img.height.max(1) as f32),
                                )
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
                                        action_flag = Some((
                                            item.path.clone(),
                                            photo_engine::Flag::Pick,
                                        ));
                                        ui.close();
                                    }
                                    if ui.button("Reject (X)").clicked() {
                                        action_flag = Some((
                                            item.path.clone(),
                                            photo_engine::Flag::Reject,
                                        ));
                                        ui.close();
                                    }
                                    if ui.button("Clear (U)").clicked() {
                                        action_flag = Some((
                                            item.path.clone(),
                                            photo_engine::Flag::Unflagged,
                                        ));
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

    /// Undo · Redo | Reset all · Copy stack · Paste · Paste to all. `right_to_left` callers
    /// get them reversed, so they read the same way on screen either way.
    fn filmstrip_actions(&mut self, ui: &mut egui::Ui, right_to_left: bool) {
        #[derive(Clone, Copy)]
        enum Action {
            Undo,
            Redo,
            Sep,
            Reset,
            Copy,
            Paste,
            PasteAll,
        }
        let mut order = [
            Action::Undo,
            Action::Redo,
            Action::Sep,
            Action::Reset,
            Action::Copy,
            Action::Paste,
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
                        && self.history.undo(&mut self.stack)
                    {
                        self.needs_render = true;
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
                        && self.history.redo(&mut self.stack)
                    {
                        self.needs_render = true;
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
                Action::PasteAll => {
                    if button(
                        ui,
                        has_clip && n > 0,
                        Kind::Accent,
                        &format!("Paste to all {n}"),
                        0.0,
                    )
                    .on_hover_text("Write the copied stack to the sidecar of every shelf item")
                    .clicked()
                    {
                        self.paste_to_all_in_shelf();
                    }
                }
            }
        }
    }

    fn workspace(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::same(16)),
            )
            .show(ui, |ui| {
                if let Some((path, preview)) = &self.stage_preview {
                    if self.loading.as_ref().is_some_and(|(p, _)| p == path) {
                        stage_preview(ui, preview, path);
                        return;
                    }
                }
                if self.texture_id.is_none() || self.engine.image().is_none() {
                    self.empty_stage(ui);
                    return;
                }
                let (Some(id), Some(image)) = (self.texture_id, self.engine.image()) else {
                    return;
                };

                // Only the crop is on screen outside crop mode: `img` is the size of what is
                // shown, and `frame_of` finds where the whole frame would sit around it.
                let full = egui::vec2(image.width as f32, image.height as f32);
                let region = self.shown_region();
                let img = full * egui::vec2(region.width, region.height);
                let frame_of = |shown: egui::Rect| {
                    let size = shown.size() / egui::vec2(region.width, region.height);
                    egui::Rect::from_min_size(
                        shown.min - size * egui::vec2(region.x, region.y),
                        size,
                    )
                };
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                self.last_panel = rect.size();
                if rect.width() < 1.0 || rect.height() < 1.0 {
                    return;
                }

                // Scale at which the whole frame fits. Everything is expressed relative to
                // this so that resizing the window never changes what you are looking at.
                let fit = (rect.width() / img.x).min(rect.height() / img.y);
                let max_zoom = (MAX_ZOOM_SCALE / fit).max(1.0);

                let scale_now = fit * self.view.zoom;
                let dest_now = egui::Rect::from_center_size(
                    rect.center() - self.view.pan * scale_now,
                    img * scale_now,
                );
                // At fit the whole frame is visible and there is nothing to pan to.
                let at_fit = self.view.zoom <= 1.0 + 1e-3;

                // The before/after divider is its own widget, registered after the image so
                // it sits on top: grabbing it can never start a pan. The hit strip is wider
                // than the line it draws.
                // The split is a fraction of the whole frame (that is what the shader tests),
                // so it is placed against the frame, not the crop.
                let frame_now = frame_of(dest_now);
                let divider = (self.split_active && !self.cropping).then(|| {
                    let x = (frame_now.left() + self.split_pos * frame_now.width())
                        .clamp(dest_now.left(), dest_now.right());
                    let strip = Rect::from_x_y_ranges(x - 12.0..=x + 12.0, dest_now.y_range())
                        .intersect(rect);
                    ui.interact(
                        strip,
                        ui.id().with("split_divider"),
                        Sense::click_and_drag(),
                    )
                });
                let mut move_divider_to = None;
                if let Some(div) = &divider {
                    if div.hovered() || div.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if div.double_clicked() {
                        self.split_pos = 0.5;
                        self.needs_render = true;
                    } else if div.is_pointer_button_down_on() {
                        move_divider_to = div.interact_pointer_pos();
                    }
                }
                // With nothing to pan at fit, a drag anywhere on the image moves the divider.
                if self.split_active && !self.cropping && at_fit && response.dragged() {
                    move_divider_to = response.interact_pointer_pos();
                }
                self.split_dragging = move_divider_to.is_some();
                if let Some(p) = move_divider_to {
                    let x = p.x.clamp(dest_now.left(), dest_now.right());
                    let fraction = ((x - frame_now.left()) / frame_now.width()).clamp(0.0, 1.0);
                    if (fraction - self.split_pos).abs() > 1e-4 {
                        self.split_pos = fraction;
                        self.needs_render = true;
                    }
                }

                if self.cropping {
                    // In crop mode a drag frames the picture instead of moving it: from
                    // outside the crop it draws a new one, from inside it slides this one.
                    // Nothing is checkpointed per drag — Apply makes the session one step.
                    let to_norm = |p: egui::Pos2| {
                        egui::pos2(
                            ((p.x - frame_now.left()) / frame_now.width()).clamp(0.0, 1.0),
                            ((p.y - frame_now.top()) / frame_now.height()).clamp(0.0, 1.0),
                        )
                    };
                    let current = self.stack.geometry.crop.sanitised();
                    let inside = |p: egui::Pos2| {
                        !current.is_full_frame()
                            && (current.x..=current.x + current.width).contains(&p.x)
                            && (current.y..=current.y + current.height).contains(&p.y)
                    };
                    if response.drag_started() {
                        self.crop_drag = response.interact_pointer_pos().map(to_norm).map(|p| {
                            if inside(p) {
                                CropDrag::Move {
                                    grab: p,
                                    start: current,
                                }
                            } else {
                                CropDrag::Draw { anchor: p }
                            }
                        });
                    }
                    if let (Some(drag), Some(now)) =
                        (self.crop_drag, response.interact_pointer_pos().map(to_norm))
                    {
                        if response.dragged() || response.drag_stopped() {
                            let crop = match drag {
                                CropDrag::Draw { anchor } => Crop {
                                    x: anchor.x.min(now.x),
                                    y: anchor.y.min(now.y),
                                    width: (now.x - anchor.x).abs(),
                                    height: (now.y - anchor.y).abs(),
                                },
                                CropDrag::Move { grab, start } => Crop {
                                    x: start.x + now.x - grab.x,
                                    y: start.y + now.y - grab.y,
                                    ..start
                                },
                            }
                            .sanitised();
                            if crop != self.stack.geometry.crop {
                                self.stack.geometry.crop = crop;
                                self.needs_render = true;
                            }
                        }
                    }
                    if response.drag_stopped() {
                        self.crop_drag = None;
                    }
                    if let Some(p) = response.hover_pos() {
                        let moving = matches!(self.crop_drag, Some(CropDrag::Move { .. }));
                        if moving || (self.crop_drag.is_none() && inside(to_norm(p))) {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                        }
                    }
                } else if response.dragged() && !at_fit && !self.split_dragging {
                    self.view.pan -= response.drag_delta() / scale_now;
                }

                if let Some(pointer) = response.hover_pos() {
                    // Wheel, plus pinch / Ctrl+wheel, which egui reports as a zoom factor.
                    let (scroll, pinch) = ui
                        .ctx()
                        .input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
                    let factor = (scroll * 0.004).exp() * pinch;
                    if factor != 1.0 {
                        let before = fit * self.view.zoom;
                        let centre = rect.center() - self.view.pan * before;
                        // The image point under the cursor, in image pixels from centre.
                        let anchor = (pointer - centre) / before;

                        self.view.zoom = (self.view.zoom * factor).clamp(1.0, max_zoom);

                        // Keep that point under the cursor rather than zooming to centre.
                        let after = fit * self.view.zoom;
                        self.view.pan = (rect.center() - pointer) / after + anchor;
                    }
                }

                let scale = fit * self.view.zoom;
                // Pan only as far as the image still covers the view: at fit that is
                // nowhere, so the frame cannot be knocked off-centre by a stray drag.
                let slack = ((img * scale - rect.size()) * 0.5).max(egui::Vec2::ZERO) / scale;
                self.view.pan = self.view.pan.clamp(-slack, slack);

                let dest = egui::Rect::from_center_size(
                    rect.center() - self.view.pan * scale,
                    img * scale,
                );
                let frame = frame_of(dest);
                let painter = ui.painter().with_clip_rect(rect);
                painter.image(
                    id,
                    dest,
                    egui::Rect::from_min_size(
                        egui::pos2(region.x, region.y),
                        egui::vec2(region.width, region.height),
                    ),
                    egui::Color32::WHITE,
                );
                // The frame the image sits in: a hairline, not a shadow.
                let framed_view = dest.intersect(rect);
                painter.rect_stroke(
                    framed_view,
                    0.0,
                    Stroke::new(1.0, theme::RAIL),
                    egui::StrokeKind::Outside,
                );

                // While framing, the whole picture is shown and the crop drawn over it:
                // everything outside is dimmed, and thirds are marked.
                let crop = self.stack.geometry.crop.sanitised();
                if self.cropping {
                    let framed = egui::Rect::from_min_size(
                        dest.min + egui::vec2(crop.x * dest.width(), crop.y * dest.height()),
                        egui::vec2(crop.width * dest.width(), crop.height * dest.height()),
                    );
                    let veil = egui::Color32::from_black_alpha(170);
                    // Four bands rather than a stencil: cheap, exact, and no extra pass.
                    painter.rect_filled(
                        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), framed.top())),
                        0.0,
                        veil,
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(rect.left(), framed.bottom()),
                            rect.max,
                        ),
                        0.0,
                        veil,
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(rect.left(), framed.top()),
                            egui::pos2(framed.left(), framed.bottom()),
                        ),
                        0.0,
                        veil,
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(framed.right(), framed.top()),
                            egui::pos2(rect.right(), framed.bottom()),
                        ),
                        0.0,
                        veil,
                    );
                    painter.rect_stroke(
                        framed,
                        0.0,
                        egui::Stroke::new(1.0, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                    let thin = egui::Stroke::new(1.0, theme::TEXT.linear_multiply(0.25));
                    for i in 1..3 {
                        let t = i as f32 / 3.0;
                        let x = framed.left() + framed.width() * t;
                        let y = framed.top() + framed.height() * t;
                        painter.line_segment(
                            [egui::pos2(x, framed.top()), egui::pos2(x, framed.bottom())],
                            thin,
                        );
                        painter.line_segment(
                            [egui::pos2(framed.left(), y), egui::pos2(framed.right(), y)],
                            thin,
                        );
                    }
                    hud_badge(
                        &painter,
                        pos2(framed_view.center().x, framed_view.top() + 12.0),
                        Align2::CENTER_TOP,
                        "CROP · drag to frame · drag inside to move · Enter apply · Esc cancel",
                        theme::ACCENT,
                        theme::ACCENT,
                    );
                }

                if self.split_active {
                    let split_screen_x = (frame.left() + self.split_pos * frame.width())
                        .clamp(dest.left(), dest.right());
                    let (top, bottom) = (framed_view.top(), framed_view.bottom());
                    painter.vline(
                        split_screen_x,
                        top..=bottom,
                        Stroke::new(2.0, theme::ACCENT),
                    );

                    let badge_y = top + 14.0;
                    if self.split_pos > 0.08 {
                        hud_badge(
                            &painter,
                            pos2(split_screen_x - 8.0, badge_y),
                            Align2::RIGHT_TOP,
                            "BEFORE",
                            theme::TEXT_HI,
                            Color32::from_gray(0x2a),
                        );
                    }
                    if self.split_pos < 0.92 {
                        hud_badge(
                            &painter,
                            pos2(split_screen_x + 8.0, badge_y),
                            Align2::LEFT_TOP,
                            "AFTER",
                            theme::ACCENT,
                            theme::ACCENT,
                        );
                    }

                    let handle = pos2(split_screen_x, (top + bottom) / 2.0);
                    painter.circle(handle, 15.0, theme::BG_950, Stroke::new(2.0, theme::ACCENT));
                    painter.text(
                        handle,
                        Align2::CENTER_CENTER,
                        "⇆",
                        FontId::proportional(15.0),
                        theme::ACCENT,
                    );
                }

                // Corner readouts. What exports is exactly this texture, cropped; only the
                // display scale differs.
                if framed_view.width() > 420.0 {
                    let (_, _, w, h) = crop.to_pixels(image.width, image.height);
                    hud_badge(
                        &painter,
                        framed_view.left_bottom() + vec2(12.0, -12.0),
                        Align2::LEFT_BOTTOM,
                        &format!(
                            "exports these pixels · {w} × {h} · shown at {:.0}%",
                            scale * 100.0
                        ),
                        Color32::from_gray(0xcc),
                        theme::RAIL,
                    );
                }
                if let Some((path, _)) = &self.loading {
                    hud_badge(
                        &painter,
                        framed_view.left_top() + vec2(12.0, 12.0),
                        Align2::LEFT_TOP,
                        &format!("opening {}…", file_name(path)),
                        theme::ACCENT,
                        theme::ACCENT,
                    );
                }
                let hist = self.engine.histogram();
                if hist.peak > 0 {
                    let clipped = hist.clipped_high().max(hist.clipped_low()) * 100.0;
                    let (text, colour) = if clipped >= 0.1 {
                        (format!("●  clipped {clipped:.1}%"), theme::DANGER)
                    } else {
                        (format!("clipped {clipped:.1}%"), theme::TEXT_DIM)
                    };
                    hud_badge(
                        &painter,
                        framed_view.right_bottom() + vec2(-12.0, -12.0),
                        Align2::RIGHT_BOTTOM,
                        &text,
                        colour,
                        theme::RAIL,
                    );
                }

                if self.cropping {
                    if ui.ctx().output(|o| o.cursor_icon) != egui::CursorIcon::Move {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    }
                } else if self.split_dragging {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                } else if !at_fit && response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if !at_fit && response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
            });
    }

    /// No image yet: a drop target that is also the Open button, and the reason the last
    /// file failed, in place (DESIGN.md: failures surface in the UI, with the filename).
    fn empty_stage(&mut self, ui: &mut egui::Ui) {
        let area = ui.available_rect_before_wrap();
        let zone = Rect::from_center_size(
            area.center(),
            vec2((area.width() - 32.0).clamp(160.0, 440.0), 180.0),
        );
        let response = ui.interact(zone, ui.id().with("drop_zone"), Sense::click());
        let dragging_in = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let hot = response.hovered() || dragging_in;

        let painter = ui.painter();
        widgets::dashed_rect(
            painter,
            zone,
            if hot {
                theme::ACCENT
            } else {
                Color32::from_gray(0x2a)
            },
        );
        if let Some((path, _)) = &self.loading {
            painter.text(
                zone.center(),
                Align2::CENTER_CENTER,
                format!("decoding {}…", file_name(path)),
                FontId::new(13.0, theme::mono_bold()),
                theme::ACCENT,
            );
            return;
        }
        let c = zone.center();
        painter.text(
            c - vec2(0.0, 44.0),
            Align2::CENTER_CENTER,
            "⬆",
            FontId::proportional(22.0),
            theme::ACCENT,
        );
        painter.text(
            c - vec2(0.0, 12.0),
            Align2::CENTER_CENTER,
            "Drop a camera RAW file",
            FontId::new(15.0, theme::sans_strong()),
            theme::TEXT_HI,
        );
        painter.text(
            c + vec2(0.0, 10.0),
            Align2::CENTER_CENTER,
            "or click to open one",
            FontId::proportional(12.5),
            theme::TEXT_DIM,
        );
        painter.text(
            c + vec2(0.0, 40.0),
            Align2::CENTER_CENTER,
            "ARW · CR2 · CR3 · NEF · RAF · ORF · RW2 · DNG",
            FontId::monospace(10.5),
            theme::TEXT_FAINT,
        );
        if let Some(err) = &self.error {
            let galley = painter.layout(
                err.clone(),
                FontId::monospace(11.5),
                theme::DANGER,
                zone.width(),
            );
            painter.galley(
                pos2(
                    zone.center().x - galley.size().x / 2.0,
                    zone.bottom() + 16.0,
                ),
                galley,
                theme::DANGER,
            );
        }
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            self.pick_and_open(ui.ctx());
        }
    }

    // ── Status bar ──────────────────────────────────────────────────────────────────────

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(14, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(path) = &self.loaded_path {
                            ui.label(
                                theme::num(file_name(path))
                                    .size(11.5)
                                    .color(Color32::from_gray(0xcc)),
                            );
                            let (state, colour) = self.sidecar_state().describe();
                            ui.label(theme::tag(state).color(colour));
                            widgets::dot(ui, colour);
                            vsep(ui);
                        }

                        match &self.export {
                            Export::Idle => {}
                            Export::Writing { path, started, .. } => {
                                readout(
                                    ui,
                                    "writing",
                                    &format!(
                                        "{}  {:.1}s",
                                        file_name(path),
                                        started.elapsed().as_secs_f32()
                                    ),
                                    theme::ACCENT,
                                );
                            }
                            Export::Done { path, seconds } => {
                                readout(
                                    ui,
                                    "exported",
                                    &format!("{}  {seconds:.1}s", file_name(path)),
                                    theme::LIME,
                                );
                            }
                            Export::Failed(e) => {
                                ui.label(theme::num(e).size(11.5).color(theme::DANGER));
                            }
                        }

                        if self.engine.image().is_some() {
                            if let Some(percent) = self.zoom_percent(self.last_panel) {
                                readout(ui, "zoom", &format!("{percent:.0}%"), theme::TEXT_HI);
                            }
                            readout(
                                ui,
                                "submit",
                                &format!("{:.2} ms", self.render_ms),
                                theme::TEXT_HI,
                            );
                            readout(
                                ui,
                                "decode",
                                &format!("{:.0} ms", self.decode_ms),
                                theme::TEXT_HI,
                            );
                        }

                        // Whatever width is left, from the left edge: errors win, then the
                        // latest notification.
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if let Some(err) = &self.error {
                                widgets::dot(ui, theme::DANGER);
                                ui.add(
                                    egui::Label::new(
                                        theme::num(err).size(11.5).color(theme::DANGER),
                                    )
                                    .truncate(),
                                );
                            } else if let Some((msg, when)) = &self.status_notification {
                                if when.elapsed().as_secs_f32() < 4.0 {
                                    widgets::dot(ui, theme::ACCENT);
                                    ui.add(
                                        egui::Label::new(
                                            theme::num(msg).size(11.5).color(theme::ACCENT),
                                        )
                                        .truncate(),
                                    );
                                } else {
                                    ui.label(theme::tag("ready").color(theme::TEXT_FAINT));
                                }
                            } else {
                                ui.label(theme::tag("ready").color(theme::TEXT_FAINT));
                            }
                        });
                    });
                });
            });
    }

    /// Display scale as a percentage of 1:1, for the status bar.
    fn zoom_percent(&self, panel: egui::Vec2) -> Option<f32> {
        let img = self.shown_size()?;
        if panel.x < 1.0 || panel.y < 1.0 {
            return None;
        }
        let fit = (panel.x / img.x).min(panel.y / img.y);
        Some(fit * self.view.zoom * 100.0)
    }

    // ── Inspector (right) ───────────────────────────────────────────────────────────────

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let mut panel = egui::Panel::right("inspector")
            .frame(egui::Frame::new().fill(theme::BG_950))
            .resizable(true)
            .default_size(self.inspector_width)
            .min_size(270.0)
            .max_size(850.0);

        if let Some(target_w) = self.inspector_force_width.take() {
            panel = panel.exact_size(target_w);
        }

        let inner = panel.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            self.inspector_footer(ui);

            let loaded = self.engine.image().is_some();
            if loaded {
                self.scope_block(ui);
            }

            // One slim header: what the list below is, and the two things you do to it.
            widgets::band(ui, |ui| {
                let title = if self.customize_open {
                    "Modules"
                } else {
                    "Pipeline"
                };
                ui.label(theme::head(title));
                if !self.customize_open && loaded {
                    let on = InspectorSectionId::all()
                        .iter()
                        .filter(|id| {
                            // Scope and geometry have no on/off; everything else does.
                            !matches!(id, InspectorSectionId::Scope | InspectorSectionId::Geometry)
                                && module_on(&self.stack, **id)
                        })
                        .count();
                    ui.label(theme::tag(format!("{on} on")).color(theme::TEXT_DIM));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let tip = if self.customize_open {
                        "Back to the pipeline"
                    } else {
                        "Choose which modules show, and their order"
                    };
                    if widgets::icon(ui, "⚙", self.customize_open, tip).clicked() {
                        self.customize_open = !self.customize_open;
                    }
                    if !self.customize_open
                        && widgets::icon(ui, "↕", false, "Expand or collapse every module")
                            .clicked()
                    {
                        let any_collapsed = self
                            .inspector_sections
                            .iter()
                            .any(|s| s.collapsed && s.id != InspectorSectionId::Scope);
                        for s in &mut self.inspector_sections {
                            if s.id != InspectorSectionId::Scope {
                                s.collapsed = !any_collapsed;
                            }
                        }
                    }
                });
            });

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(Margin::symmetric(10, 10))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            if self.customize_open {
                                self.customize_body(ui);
                            } else if loaded {
                                self.pipeline_body(ui);
                            } else {
                                ui.add_space(8.0);
                                ui.label(theme::dim("Open a RAW to see its pipeline.").size(12.0));
                            }
                        });
                });
        });

        self.inspector_width = inner.response.rect.width();
    }

    fn section_mut(&mut self, id: InspectorSectionId) -> Option<&mut InspectorSectionConfig> {
        self.inspector_sections.iter_mut().find(|s| s.id == id)
    }

    /// The scope, pinned above the tabs as in every darkroom: it is what you watch while
    /// you drag everything below it. Its visibility still comes from Customize.
    fn scope_block(&mut self, ui: &mut egui::Ui) {
        let Some(cfg) = self.section_mut(InspectorSectionId::Scope) else {
            return;
        };
        if !cfg.visible {
            return;
        }
        let mut collapsed = cfg.collapsed;

        widgets::band(ui, |ui| {
            let title = ui
                .add(egui::Label::new(theme::head("Scope")).sense(Sense::click()))
                .on_hover_text(if collapsed {
                    "Show the scope"
                } else {
                    "Hide the scope"
                });
            if title.clicked() {
                collapsed = !collapsed;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (mode, label, tip) in inspector_map_3d::MODES.iter().rev() {
                    let on = self.inspector_map_state.mode == *mode;
                    if widgets::chip(ui, on, label).on_hover_text(*tip).clicked() {
                        self.inspector_map_state.mode = *mode;
                        collapsed = false;
                    }
                }
            });
        });
        if !collapsed {
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 12,
                    right: 12,
                    top: 10,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    inspector_map_3d::render_inspector_map(
                        ui,
                        &mut self.inspector_map_state,
                        self.engine.histogram(),
                    );
                });
            let r = ui.min_rect();
            ui.painter()
                .hline(r.x_range(), r.bottom(), Stroke::new(1.0, theme::BORDER));
        }
        if let Some(cfg) = self.section_mut(InspectorSectionId::Scope) {
            cfg.collapsed = collapsed;
        }
    }

    fn pipeline_body(&mut self, ui: &mut egui::Ui) {
        // Everything that happens this frame is checkpointed against this one snapshot.
        let snapshot = self.stack.clone();
        // "Modified" means changed from how this photo opened — so the camera's own
        // orientation is not an edit.
        let defaults = self.baseline.clone();
        let white = self.white_scale();
        let mut changed = false;
        let mut started = false;
        let mut close_others = None;

        for idx in 0..self.inspector_sections.len() {
            let InspectorSectionConfig {
                id,
                visible,
                mut collapsed,
            } = self.inspector_sections[idx].clone();
            if !visible || id == InspectorSectionId::Scope {
                continue;
            }

            let (summary, modified) = summarise(id, &self.stack, &defaults, &white);
            let on = module_on(&self.stack, id);
            let summary_colour = if !on {
                theme::TEXT_FAINT
            } else if modified {
                theme::ACCENT
            } else {
                theme::TEXT_DIM
            };
            let focused = self.focused_section == Some(id);

            let mut card_changed = false;
            let mut card_started = false;
            let card = widgets::card(ui, focused, |ui| {
                let head = widgets::module_header(
                    ui,
                    id.title(),
                    module_enabled(&mut self.stack, id),
                    &summary,
                    summary_colour,
                    focused,
                );
                if head.toggled {
                    // A toggle is a mutation like any other: DESIGN.md invariant 3.
                    card_changed = true;
                    card_started = true;
                }
                if head.clicked {
                    let opening = collapsed;
                    collapsed = !collapsed;
                    // One open at a time keeps the list one screen tall; Shift opens this
                    // one alongside whatever is already open.
                    if opening && self.single_open && !ui.input(|i| i.modifiers.shift) {
                        close_others = Some(id);
                    }
                }
                if !collapsed {
                    // Controls show whether or not the module is on, and touching one turns
                    // it on — no need to tick the box before you can try it.
                    ui.add_space(2.0);
                    let mut body_changed = false;
                    self.module_body(ui, id, &mut body_changed, &mut card_started);
                    if body_changed {
                        if let Some(on) = module_enabled(&mut self.stack, id) {
                            *on = true;
                        }
                    }
                    card_changed |= body_changed;
                    ui.add_space(4.0);
                }
                head.clicked || head.toggled
            });
            let touched = card.inner;

            if touched || card_changed {
                self.focused_section = Some(id);
            }
            if self.reveal_section == Some(id) {
                // The cards above it just collapsed; bring this one fully into view.
                card.response.scroll_to_me(None);
                self.reveal_section = None;
            }
            changed |= card_changed;
            started |= card_started;
            self.inspector_sections[idx].collapsed = collapsed;
        }

        if let Some(open) = close_others {
            for s in &mut self.inspector_sections {
                if s.id != open && s.id != InspectorSectionId::Scope {
                    s.collapsed = true;
                }
            }
            self.reveal_section = Some(open);
        }

        if started {
            self.history.checkpoint(&snapshot);
        }
        if changed {
            self.needs_render = true;
        }
    }

    /// The controls inside one module card.
    fn module_body(
        &mut self,
        ui: &mut egui::Ui,
        id: InspectorSectionId,
        changed: &mut bool,
        started: &mut bool,
    ) {
        macro_rules! track {
            ($r:expr) => {{
                let r: egui::Response = $r;
                if r.drag_started() || (r.changed() && !r.dragged()) {
                    *started = true;
                }
                if r.changed() {
                    *changed = true;
                }
            }};
        }
        // A click, not a drag: checkpoint and re-render in one go.
        macro_rules! discrete {
            () => {{
                *started = true;
                *changed = true;
            }};
        }

        // Where each control returns to on double-click.
        let d = PhotoStack::default();

        match id {
            InspectorSectionId::Scope => {}
            InspectorSectionId::Geometry => {
                let g = &mut self.stack.geometry;
                let mut edited = false;
                let mut turned = false;
                ui.horizontal(|ui| {
                    if geometry_icon_button(
                        ui,
                        GeometryIcon::RotateLeft,
                        false,
                        "Rotate 90° CCW ([)",
                    )
                    .clicked()
                    {
                        g.rotate_left();
                        edited = true;
                        turned = true;
                    }
                    if geometry_icon_button(
                        ui,
                        GeometryIcon::RotateRight,
                        false,
                        "Rotate 90° CW (])",
                    )
                    .clicked()
                    {
                        g.rotate_right();
                        edited = true;
                        turned = true;
                    }
                    if geometry_icon_button(
                        ui,
                        GeometryIcon::MirrorH,
                        g.mirror_h,
                        "Flip horizontally (H)",
                    )
                    .clicked()
                    {
                        g.flip_h();
                        edited = true;
                        turned = true;
                    }
                    if geometry_icon_button(
                        ui,
                        GeometryIcon::MirrorV,
                        g.mirror_v,
                        "Flip vertically (V)",
                    )
                    .clicked()
                    {
                        g.flip_v();
                        edited = true;
                        turned = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_enabled_ui(!g.crop.is_full_frame(), |ui| {
                            if widgets::chip(ui, false, "reset crop").clicked() {
                                g.crop = Crop::default();
                                edited = true;
                            }
                        });
                        if self.cropping {
                            ui.label(theme::tag("framing").color(theme::ACCENT));
                        }
                    });
                });
                if edited {
                    discrete!();
                }
                if turned {
                    self.rebase_crop();
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                    self.crop_chips(ui);
                });
                track!(widgets::param_slider(
                    ui,
                    "straighten",
                    &mut self.stack.geometry.straighten_deg,
                    -15.0..=15.0,
                    2,
                    d.geometry.straighten_deg,
                ));
            }
            InspectorSectionId::WhiteBalance => {
                // Kelvin and tint through this camera's own colour matrix, so the numbers
                // name real lights and the as-shot white reads as what the camera metered.
                let scale = self.white_scale();
                let p = &mut self.stack.white_balance.params;
                let dp = d.white_balance.params;
                let (mut kelvin, mut tint) = scale.temperature(p);
                let (home_k, home_t) = scale.temperature(&dp);
                let (k_lo, k_hi) = (WB_KELVIN_MIN, WB_KELVIN_MAX);
                // Evenly spaced in mired, as Lightroom's is: a step from 3000 to 3500 K is
                // as visible as one from 7000 to 10000 K.
                let (m_lo, m_hi) = (1e6 / k_lo, 1e6 / k_hi);
                let k = widgets::scaled_slider(
                    ui,
                    "temp (K)",
                    &mut kelvin,
                    k_lo..=k_hi,
                    0,
                    home_k,
                    widgets::SliderScale {
                        to_rail: &|k| (m_lo - 1e6 / k) / (m_lo - m_hi),
                        from_rail: &|t| 1e6 / (m_lo - t * (m_lo - m_hi)),
                        speed: 10.0,
                        gradient: Some([theme::WB_COOL, theme::WB_WARM]),
                    },
                )
                .on_hover_text("The light the photo was taken under. Higher is warmer.");
                let t = widgets::scaled_slider(
                    ui,
                    "tint",
                    &mut tint,
                    -150.0..=150.0,
                    0,
                    home_t,
                    widgets::SliderScale {
                        to_rail: &|v| (v + 150.0) / 300.0,
                        from_rail: &|t| t * 300.0 - 150.0,
                        speed: 0.5,
                        gradient: Some([theme::WB_GREEN, theme::WB_MAGENTA]),
                    },
                )
                .on_hover_text("Negative is greener, positive more magenta.");
                let both = k | t;
                if both.changed() {
                    // A light the camera cannot neutralise with positive gains is simply
                    // out of reach: the slider stops rather than jumping somewhere else.
                    if let Some(wb) = scale.white_balance(kelvin, tint) {
                        *p = wb;
                    }
                }
                track!(both);

                // Presets as chips; the one matching the current setting is lit. Adobe's
                // standard lights, so a preset means the same here as in Lightroom.
                let mut preset = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                    let as_shot = (p.tint_r - 1.0).abs() < 1e-4 && (p.tint_b - 1.0).abs() < 1e-4;
                    if widgets::chip(ui, as_shot, "as shot")
                        .on_hover_text(format!("What the camera metered: {home_k:.0} K"))
                        .clicked()
                    {
                        preset = Some(dp);
                    }
                    for (label, tip, pk, pt) in [
                        ("daylight", "5500 K, tint +10", 5500.0, 10.0),
                        ("cloudy", "6500 K, tint +10", 6500.0, 10.0),
                        ("shade", "7500 K, tint +10", 7500.0, 10.0),
                        ("tungsten", "2850 K incandescent", 2850.0, 0.0),
                        ("fluoro", "3800 K, tint +21", 3800.0, 21.0),
                        ("flash", "5500 K", 5500.0, 0.0),
                    ] {
                        let lit = !as_shot && (kelvin - pk).abs() < 5.0 && (tint - pt).abs() < 0.5;
                        let wb = scale.white_balance(pk, pt);
                        let chip = ui.add_enabled_ui(wb.is_some(), |ui| {
                            widgets::chip(ui, lit, label).on_hover_text(tip)
                        });
                        if chip.inner.clicked() {
                            preset = wb;
                        }
                    }
                });
                if let Some(wb) = preset {
                    *p = wb;
                    discrete!();
                }
            }
            InspectorSectionId::Exposure => {
                let p = &mut self.stack.exposure.params;
                let dp = d.exposure.params;

                ui.horizontal(|ui| {
                    ui.label(theme::tag("mode").color(theme::TEXT_DIM));
                    let modes = [
                        (
                            photo_engine::params::ExposureMode::ClassicLinear,
                            "Classic",
                            "Pure linear gain without highlight compression. Matches Lightroom Classic's punchy, direct sensor response.",
                        ),
                        (
                            photo_engine::params::ExposureMode::ModernSoft,
                            "Modern",
                            "Soft highlight shoulder (similar to Lightroom 2026). Compresses extreme highlights to preserve detail in blown areas.",
                        ),
                        (
                            photo_engine::params::ExposureMode::FilmicShoulder,
                            "Filmic",
                            "Cinematic filmic shoulder. Gentle logarithmic knee simulating analog film highlight latitude.",
                        ),
                        (
                            photo_engine::params::ExposureMode::Custom,
                            "Custom",
                            "Fine-tune the compression shoulder rolloff manually.",
                        ),
                    ];
                    for (m, label, tip) in modes {
                        let sel = p.mode == m;
                        if widgets::chip(ui, sel, label).on_hover_text(tip).clicked() {
                            p.mode = m;
                            match m {
                                photo_engine::params::ExposureMode::ClassicLinear => p.rolloff = 0.0,
                                photo_engine::params::ExposureMode::ModernSoft => p.rolloff = 0.25,
                                photo_engine::params::ExposureMode::FilmicShoulder => p.rolloff = 0.50,
                                photo_engine::params::ExposureMode::Custom => {}
                            }
                            discrete!();
                        }
                    }
                });

                ui.add_space(4.0);

                let max_ev = if p.extended_range { 8.0 } else { 4.0 };
                track!(widgets::param_slider(
                    ui,
                    "EV",
                    &mut p.ev,
                    -max_ev..=max_ev,
                    2,
                    dp.ev
                ));
                track!(widgets::param_slider(
                    ui,
                    "black",
                    &mut p.black,
                    -0.1..=0.1,
                    4,
                    dp.black
                ));

                if p.mode == photo_engine::params::ExposureMode::Custom {
                    track!(widgets::param_slider(
                        ui,
                        "rolloff",
                        &mut p.rolloff,
                        0.0..=1.0,
                        2,
                        0.0
                    ));
                }

                ui.horizontal(|ui| {
                    if widgets::chip(
                        ui,
                        p.extended_range,
                        if p.extended_range {
                            "±8 EV (Extended)"
                        } else {
                            "±4 EV (Standard)"
                        },
                    )
                    .on_hover_text(
                        "Toggle between standard ±4 EV range and extended ±8 EV extreme range",
                    )
                    .clicked()
                    {
                        p.extended_range = !p.extended_range;
                        discrete!();
                    }
                });

                ui.add_space(4.0);
                widgets::mini_knee_curve(ui, p.ev, p.rolloff);
            }
            InspectorSectionId::Tonal => {
                let p = &mut self.stack.tonal.params;
                let dp = d.tonal.params;
                track!(widgets::param_slider(
                    ui,
                    "highlights",
                    &mut p.highlights,
                    -1.0..=1.0,
                    2,
                    dp.highlights
                ));
                track!(widgets::param_slider(
                    ui,
                    "shadows",
                    &mut p.shadows,
                    -1.0..=1.0,
                    2,
                    dp.shadows
                ));
                track!(widgets::param_slider(
                    ui,
                    "whites",
                    &mut p.whites,
                    -1.0..=1.0,
                    2,
                    dp.whites
                ));
                track!(widgets::param_slider(
                    ui,
                    "blacks",
                    &mut p.blacks,
                    -1.0..=1.0,
                    2,
                    dp.blacks
                ));
            }
            InspectorSectionId::ToneCurve => {
                track!(curve_editor(ui, &mut self.stack.tone_curve.params));
                let is_linear = self.stack.tone_curve.params == identity_nodes();
                ui.horizontal(|ui| {
                    ui.label(theme::tag("drag a node").color(theme::TEXT_FAINT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_enabled_ui(!is_linear, |ui| {
                            if widgets::chip(ui, false, "reset").clicked() {
                                self.stack.tone_curve.params = identity_nodes();
                                discrete!();
                            }
                        });
                    });
                });
            }
            InspectorSectionId::Clarity => {
                let p = &mut self.stack.clarity.params;
                let dp = d.clarity.params;
                track!(widgets::param_slider(
                    ui,
                    "clarity",
                    &mut p.clarity,
                    -100.0..=100.0,
                    1,
                    dp.clarity
                ));
                track!(widgets::param_slider(
                    ui,
                    "texture",
                    &mut p.texture,
                    -100.0..=100.0,
                    1,
                    dp.texture
                ));
            }
            InspectorSectionId::Dehaze => {
                let p = &mut self.stack.dehaze.params;
                let dp = d.dehaze.params;
                track!(widgets::param_slider(
                    ui,
                    "amount",
                    &mut p.amount,
                    -100.0..=100.0,
                    1,
                    dp.amount
                ));
            }
            InspectorSectionId::ColorEq => {
                let p = &mut self.stack.color_eq.params;
                let dp = d.color_eq.params;

                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (ColorEqTab::Hue, "Hue"),
                        (ColorEqTab::Saturation, "Saturation"),
                        (ColorEqTab::Luminance, "Luminance"),
                    ] {
                        let sel = self.color_eq_tab == tab;
                        if widgets::chip(ui, sel, label).clicked() {
                            self.color_eq_tab = tab;
                        }
                    }
                });
                ui.add_space(4.0);

                for ((name, band), (_, home_band)) in p.bands_mut().into_iter().zip(dp.bands()) {
                    match self.color_eq_tab {
                        ColorEqTab::Hue => {
                            track!(widgets::param_slider(
                                ui,
                                name,
                                &mut band.hue,
                                -30.0..=30.0,
                                1,
                                home_band.hue
                            ));
                        }
                        ColorEqTab::Saturation => {
                            track!(widgets::param_slider(
                                ui,
                                name,
                                &mut band.saturation,
                                -100.0..=100.0,
                                1,
                                home_band.saturation
                            ));
                        }
                        ColorEqTab::Luminance => {
                            track!(widgets::param_slider(
                                ui,
                                name,
                                &mut band.luminance,
                                -100.0..=100.0,
                                1,
                                home_band.luminance
                            ));
                        }
                    }
                }
            }
            InspectorSectionId::Filmic3D => {
                let p = &mut self.stack.filmic.params;
                let dp = d.filmic.params;
                tonemap_3d::render_tonemap_3d(ui, &mut self.tonemap_3d_state, p, changed, started);
                ui.add_space(4.0);
                track!(widgets::param_slider(
                    ui,
                    "white EV",
                    &mut p.white_ev,
                    0.0..=8.0,
                    2,
                    dp.white_ev
                ));
                track!(widgets::param_slider(
                    ui,
                    "black EV",
                    &mut p.black_ev,
                    -16.0..=-1.0,
                    2,
                    dp.black_ev
                ));
                track!(widgets::param_slider(
                    ui,
                    "latitude %",
                    &mut p.latitude,
                    5.0..=80.0,
                    1,
                    dp.latitude
                ));
                track!(widgets::param_slider(
                    ui,
                    "contrast",
                    &mut p.contrast,
                    0.1..=3.0,
                    2,
                    dp.contrast
                ));
            }
            InspectorSectionId::ColorBalance => {
                let p = &mut self.stack.color_balance.params;
                let dp = d.color_balance.params;
                for (label, rgb, home) in [
                    ("lift", &mut p.lift, dp.lift),
                    ("gamma", &mut p.gamma, dp.gamma),
                    ("gain", &mut p.gain, dp.gain),
                    ("offset", &mut p.offset, dp.offset),
                ] {
                    ui.add_space(2.0);
                    ui.label(theme::tag(label).color(theme::TEXT_DIM));
                    let range = -0.5..=0.5;
                    track!(widgets::param_slider(
                        ui,
                        "red",
                        &mut rgb.r,
                        range.clone(),
                        3,
                        home.r
                    ));
                    track!(widgets::param_slider(
                        ui,
                        "green",
                        &mut rgb.g,
                        range.clone(),
                        3,
                        home.g
                    ));
                    track!(widgets::param_slider(
                        ui, "blue", &mut rgb.b, range, 3, home.b
                    ));
                }
            }
            InspectorSectionId::Vibrance => {
                let p = &mut self.stack.vibrance.params;
                let dp = d.vibrance.params;
                track!(widgets::param_slider(
                    ui,
                    "vibrance",
                    &mut p.vibrance,
                    -1.0..=1.0,
                    2,
                    dp.vibrance
                ));
                track!(widgets::param_slider(
                    ui,
                    "saturation",
                    &mut p.saturation,
                    -1.0..=1.0,
                    2,
                    dp.saturation
                ));
            }
            InspectorSectionId::Sharpen => {
                let p = &mut self.stack.sharpen.params;
                let dp = d.sharpen.params;
                track!(widgets::param_slider(
                    ui,
                    "amount",
                    &mut p.amount,
                    0.0..=2.0,
                    2,
                    dp.amount
                ));
            }
            InspectorSectionId::Vignette => {
                let p = &mut self.stack.vignette.params;
                let dp = d.vignette.params;
                track!(widgets::param_slider(
                    ui,
                    "amount",
                    &mut p.amount,
                    -1.0..=1.0,
                    2,
                    dp.amount
                ));
                track!(widgets::param_slider(
                    ui,
                    "radius",
                    &mut p.radius,
                    0.1..=1.5,
                    2,
                    dp.radius
                ));
            }
        }
    }

    fn customize_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if widgets::button(ui, Kind::Secondary, "Reset order", 0.0)
                .on_hover_text("Restore the default pipeline order")
                .clicked()
            {
                self.inspector_sections = InspectorSectionConfig::default_sections();
            }
            if widgets::button(ui, Kind::Secondary, "Show all", 0.0).clicked() {
                for s in &mut self.inspector_sections {
                    s.visible = true;
                }
            }
            if widgets::button(ui, Kind::Secondary, "Essentials", 0.0)
                .on_hover_text("Scope, crop, white balance, exposure and filmic only")
                .clicked()
            {
                for s in &mut self.inspector_sections {
                    s.visible = matches!(
                        s.id,
                        InspectorSectionId::Scope
                            | InspectorSectionId::Geometry
                            | InspectorSectionId::WhiteBalance
                            | InspectorSectionId::Exposure
                            | InspectorSectionId::Filmic3D
                    );
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            widgets::checkbox(ui, &mut self.single_open);
            ui.label(
                RichText::new("One module open at a time")
                    .size(12.5)
                    .color(theme::TEXT),
            );
        })
        .response
        .on_hover_text("Opening a module closes the others. Shift-click to open one alongside.");
        ui.add_space(2.0);
        ui.label(
            theme::tag("order is display only · the pipeline itself is fixed")
                .color(theme::TEXT_FAINT),
        );
        ui.add_space(2.0);

        let mut move_act = None;
        let total = self.inspector_sections.len();
        for (i, sec) in self.inspector_sections.iter_mut().enumerate() {
            widgets::card(ui, false, |ui| {
                ui.horizontal(|ui| {
                    widgets::checkbox(ui, &mut sec.visible);
                    let colour = if sec.visible {
                        theme::TEXT_HI
                    } else {
                        theme::TEXT_DIM
                    };
                    ui.label(
                        RichText::new(sec.id.title())
                            .family(theme::sans_strong())
                            .size(13.0)
                            .color(colour),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if sec.id == InspectorSectionId::Scope {
                            ui.label(theme::tag("pinned").color(theme::TEXT_FAINT));
                            return;
                        }
                        ui.add_enabled_ui(i + 1 < total, |ui| {
                            if widgets::icon(ui, "↓", false, "Move down").clicked() {
                                move_act = Some((i, 1));
                            }
                        });
                        ui.add_enabled_ui(i > 1, |ui| {
                            if widgets::icon(ui, "↑", false, "Move up").clicked() {
                                move_act = Some((i, -1));
                            }
                        });
                    });
                });
            });
        }

        if let Some((idx, dir)) = move_act {
            let new_idx = (idx as isize + dir) as usize;
            if new_idx < self.inspector_sections.len() {
                self.inspector_sections.swap(idx, new_idx);
            }
        }
    }

    fn inspector_footer(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("inspector_footer")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(12, 10)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.horizontal(|ui| {
                    // Not `theme::tag`: that uppercases, and it is sRGB, not SRGB.
                    ui.label(
                        RichText::new("PNG / JPEG · 8-BIT · sRGB")
                            .family(theme::mono_bold())
                            .size(10.5)
                            .extra_letter_spacing(0.3)
                            .color(theme::TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(image) = self.engine.image() {
                            let (_, _, w, h) = self
                                .stack
                                .geometry
                                .crop
                                .sanitised()
                                .to_pixels(image.width, image.height);
                            ui.label(
                                RichText::new(format!("{w} × {h}"))
                                    .family(theme::mono_bold())
                                    .size(11.5)
                                    .color(theme::ACCENT),
                            );
                        }
                    });
                });
                ui.horizontal(|ui| {
                    let loaded = self.loaded_path.is_some();
                    let has_sidecar = self.saved_stack.is_some();
                    let w = ui.available_width() - 76.0 - ui.spacing().item_spacing.x;
                    ui.add_enabled_ui(loaded, |ui| {
                        if widgets::button(ui, Kind::Accent, "SAVE SIDECAR  →", w)
                            .on_hover_text("Write this stack beside the RAW (Ctrl+S)")
                            .clicked()
                        {
                            self.save_current_sidecar();
                        }
                    });
                    ui.add_enabled_ui(loaded && has_sidecar, |ui| {
                        if widgets::button(ui, Kind::Secondary, "REVERT", 76.0)
                            .on_hover_text("Reload the saved sidecar (undoable)")
                            .clicked()
                        {
                            self.revert_to_sidecar();
                        }
                    });
                });
                ui.label(
                    RichText::new("edits are non-destructive · the RAW is never written")
                        .monospace()
                        .size(10.0)
                        .color(theme::TEXT_DIM),
                );
            });
    }
}

// ── Summaries ───────────────────────────────────────────────────────────────────────────

fn module_enabled(stack: &mut PhotoStack, id: InspectorSectionId) -> Option<&mut bool> {
    use InspectorSectionId as S;
    match id {
        S::Scope | S::Geometry => None,
        S::WhiteBalance => Some(&mut stack.white_balance.enabled),
        S::Exposure => Some(&mut stack.exposure.enabled),
        S::Tonal => Some(&mut stack.tonal.enabled),
        S::ToneCurve => Some(&mut stack.tone_curve.enabled),
        S::Clarity => Some(&mut stack.clarity.enabled),
        S::Dehaze => Some(&mut stack.dehaze.enabled),
        S::ColorEq => Some(&mut stack.color_eq.enabled),
        S::Filmic3D => Some(&mut stack.filmic.enabled),
        S::ColorBalance => Some(&mut stack.color_balance.enabled),
        S::Vibrance => Some(&mut stack.vibrance.enabled),
        S::Sharpen => Some(&mut stack.sharpen.enabled),
        S::Vignette => Some(&mut stack.vignette.enabled),
    }
}

fn module_on(stack: &PhotoStack, id: InspectorSectionId) -> bool {
    use InspectorSectionId as S;
    match id {
        S::Scope | S::Geometry => true,
        S::WhiteBalance => stack.white_balance.enabled,
        S::Exposure => stack.exposure.enabled,
        S::Tonal => stack.tonal.enabled,
        S::ToneCurve => stack.tone_curve.enabled,
        S::Clarity => stack.clarity.enabled,
        S::Dehaze => stack.dehaze.enabled,
        S::ColorEq => stack.color_eq.enabled,
        S::Filmic3D => stack.filmic.enabled,
        S::ColorBalance => stack.color_balance.enabled,
        S::Vibrance => stack.vibrance.enabled,
        S::Sharpen => stack.sharpen.enabled,
        S::Vignette => stack.vignette.enabled,
    }
}

/// A module's current value, for its collapsed header, and whether it differs from default.
/// DESIGN.md: every control that changes the image shows its current numeric value.
fn summarise(
    id: InspectorSectionId,
    s: &PhotoStack,
    d: &PhotoStack,
    white: &WhiteScale,
) -> (String, bool) {
    use InspectorSectionId as S;
    let signed = |label: &str, v: f32| format!("{label} {v:+.2}");
    match id {
        S::Scope => (String::new(), false),
        S::Geometry => {
            let g = &s.geometry;
            let mut parts = Vec::new();
            if g.quarter_turns != 0 {
                parts.push(format!("{}°", u32::from(g.quarter_turns) * 90));
            }
            if g.mirror_h {
                parts.push("flip h".to_string());
            }
            if g.mirror_v {
                parts.push("flip v".to_string());
            }
            if !g.crop.is_full_frame() {
                let c = g.crop.sanitised();
                parts.push(format!(
                    "crop {:.0}×{:.0}%",
                    c.width * 100.0,
                    c.height * 100.0
                ));
            }
            if g.straighten_deg != 0.0 {
                parts.push(format!("{:+.1}°", g.straighten_deg));
            }
            let text = if parts.is_empty() {
                "none".to_string()
            } else {
                parts.join(" · ")
            };
            (text, *g != d.geometry)
        }
        S::WhiteBalance => {
            let p = &s.white_balance.params;
            let modified = *p != d.white_balance.params;
            if !modified {
                return ("as shot".to_string(), false);
            }
            let (k, t) = white.temperature(p);
            (format!("{k:.0} K · {t:+.0} tint"), true)
        }
        S::Exposure => {
            let p = &s.exposure.params;
            let mode_tag = match p.mode {
                photo_engine::params::ExposureMode::ClassicLinear => "",
                photo_engine::params::ExposureMode::ModernSoft => " · modern",
                photo_engine::params::ExposureMode::FilmicShoulder => " · filmic",
                photo_engine::params::ExposureMode::Custom => " · custom",
            };
            (
                format!("{:+.2} EV{mode_tag}", p.ev),
                *p != d.exposure.params,
            )
        }
        S::Tonal => {
            let p = &s.tonal.params;
            let set: Vec<String> = [
                ("hl", p.highlights),
                ("sh", p.shadows),
                ("wh", p.whites),
                ("bl", p.blacks),
            ]
            .into_iter()
            .filter(|(_, v)| *v != 0.0)
            .map(|(l, v)| signed(l, v))
            .collect();
            let text = if set.is_empty() {
                "neutral".to_string()
            } else {
                set.join(" · ")
            };
            (text, *p != d.tonal.params)
        }
        S::ToneCurve => {
            let nodes = &s.tone_curve.params;
            let modified = *nodes != d.tone_curve.params;
            let text = if modified {
                format!("{} pts", nodes.len())
            } else {
                "linear".to_string()
            };
            (text, modified)
        }
        S::Clarity => {
            let p = &s.clarity.params;
            let mut parts = Vec::new();
            if p.clarity != 0.0 {
                parts.push(format!("clr {:+.0}", p.clarity));
            }
            if p.texture != 0.0 {
                parts.push(format!("tex {:+.0}", p.texture));
            }
            let text = if parts.is_empty() {
                "neutral".to_string()
            } else {
                parts.join(" · ")
            };
            (text, *p != d.clarity.params)
        }
        S::Dehaze => {
            let p = &s.dehaze.params;
            let text = if p.amount != 0.0 {
                format!("{:+.0}", p.amount)
            } else {
                "neutral".to_string()
            };
            (text, *p != d.dehaze.params)
        }
        S::ColorEq => {
            let active_bands = s
                .color_eq
                .params
                .bands()
                .iter()
                .filter(|(_, b)| b.hue != 0.0 || b.saturation != 0.0 || b.luminance != 0.0)
                .count();
            let text = if active_bands > 0 {
                format!("{active_bands} active")
            } else {
                "neutral".to_string()
            };
            (text, s.color_eq.params != d.color_eq.params)
        }
        S::Filmic3D => {
            let p = &s.filmic.params;
            (
                format!("latitude {:.0}%", p.latitude),
                *p != d.filmic.params,
            )
        }
        S::ColorBalance => {
            let p = &s.color_balance.params;
            let zero = |c: &photo_engine::params::Rgb| c.r == 0.0 && c.g == 0.0 && c.b == 0.0;
            let set: Vec<&str> = [
                ("lift", &p.lift),
                ("gamma", &p.gamma),
                ("gain", &p.gain),
                ("offset", &p.offset),
            ]
            .into_iter()
            .filter(|(_, c)| !zero(c))
            .map(|(l, _)| l)
            .collect();
            let text = if set.is_empty() {
                "neutral".to_string()
            } else {
                set.join(" · ")
            };
            (text, *p != d.color_balance.params)
        }
        S::Vibrance => {
            let p = &s.vibrance.params;
            (
                format!(
                    "{} · {}",
                    signed("vib", p.vibrance),
                    signed("sat", p.saturation)
                ),
                *p != d.vibrance.params,
            )
        }
        S::Sharpen => {
            let p = &s.sharpen.params;
            (format!("amount {:.2}", p.amount), *p != d.sharpen.params)
        }
        S::Vignette => {
            let p = &s.vignette.params;
            (
                format!("{:+.2} · r {:.2}", p.amount, p.radius),
                *p != d.vignette.params,
            )
        }
    }
}

/// What changed between two adjacent undo states, for the History list.
fn describe_change(a: &PhotoStack, b: &PhotoStack) -> String {
    let mut names: Vec<String> = Vec::new();
    let (ga, gb) = (&a.geometry, &b.geometry);
    if ga.quarter_turns != gb.quarter_turns {
        names.push("rotate".into());
    }
    if ga.mirror_h != gb.mirror_h || ga.mirror_v != gb.mirror_v {
        names.push("flip".into());
    }
    if ga.straighten_deg != gb.straighten_deg {
        names.push("straighten".into());
    }
    if ga.crop != gb.crop {
        names.push("crop".into());
    }

    fn module<T: PartialEq>(names: &mut Vec<String>, name: &str, a: &Module<T>, b: &Module<T>) {
        if a.enabled != b.enabled {
            names.push(format!("{name} {}", if b.enabled { "on" } else { "off" }));
        } else if a.params != b.params {
            names.push(name.to_string());
        }
    }
    module(
        &mut names,
        "white balance",
        &a.white_balance,
        &b.white_balance,
    );
    module(&mut names, "exposure", &a.exposure, &b.exposure);
    module(&mut names, "shadows & highlights", &a.tonal, &b.tonal);
    module(&mut names, "tone curve", &a.tone_curve, &b.tone_curve);
    module(&mut names, "filmic rgb", &a.filmic, &b.filmic);
    module(
        &mut names,
        "colour balance",
        &a.color_balance,
        &b.color_balance,
    );
    module(&mut names, "vibrance", &a.vibrance, &b.vibrance);
    module(&mut names, "sharpen", &a.sharpen, &b.sharpen);
    module(&mut names, "vignette", &a.vignette, &b.vignette);

    match names.len() {
        0 => "no change".to_string(),
        1 => names.remove(0),
        2 => names.join(" + "),
        n => format!("{n} modules"),
    }
}

// ── Small pieces of layout used above ───────────────────────────────────────────────────

/// A hairline-bordered pod that groups a few controls, as in the top bar.
fn pod<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, Color32::from_gray(0x2a)))
        .corner_radius(5)
        .inner_margin(Margin::same(2))
        .show(ui, |ui| ui.horizontal(add).inner)
        .inner
}

/// A short vertical hairline between groups in a row.
fn vsep(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 16.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, theme::RAIL),
    );
}

/// `LABEL value` in the status bar.
fn readout(ui: &mut egui::Ui, label: &str, value: &str, colour: Color32) {
    // Right-to-left callers, so the value goes in first.
    ui.label(
        RichText::new(value)
            .family(theme::mono_bold())
            .size(11.5)
            .color(colour),
    );
    ui.label(theme::tag(label).color(theme::TEXT_DIM));
    ui.add_space(8.0);
}

/// A bordered card with a heading band, for the Shelf bin's groups.
fn group(
    ui: &mut egui::Ui,
    title: &str,
    right: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(theme::BG_925)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            let head = egui::Frame::new()
                .fill(theme::BG_900)
                .corner_radius(egui::CornerRadius {
                    nw: theme::RADIUS - 1,
                    ne: theme::RADIUS - 1,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    widgets::card_title(ui, title, right);
                });
            let r = head.response.rect;
            ui.painter().hline(
                r.x_range(),
                r.bottom(),
                Stroke::new(1.0, Color32::from_gray(0x1f)),
            );
            egui::Frame::new()
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 3.0;
                    body(ui);
                });
        });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowStyle {
    Normal,
    /// Redo steps: not applied.
    Faint,
    /// The live state.
    Now,
    /// A snapshot identical to the live state.
    Match,
}

/// One row of a Shelf-bin list: mono label on the left, a short tag on the right.
fn list_row(
    ui: &mut egui::Ui,
    label: &str,
    style: RowStyle,
    right: Option<(&str, Color32)>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    let hovered = response.hovered() && style != RowStyle::Now;
    let painter = ui.painter();
    match style {
        RowStyle::Now => {
            painter.rect(
                rect,
                3.0,
                theme::BG_950,
                Stroke::new(1.0, theme::ACCENT.linear_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(
                Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
                egui::CornerRadius {
                    nw: 3,
                    sw: 3,
                    ne: 0,
                    se: 0,
                },
                theme::ACCENT,
            );
        }
        RowStyle::Match => {
            painter.rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0, theme::LIME.linear_multiply(0.5)),
                egui::StrokeKind::Inside,
            );
        }
        _ if hovered => {
            painter.rect_filled(rect, 3.0, theme::BG_850);
        }
        _ => {}
    }
    let (family, colour) = match style {
        RowStyle::Now => (theme::mono_bold(), theme::ACCENT),
        RowStyle::Faint => (egui::FontFamily::Monospace, theme::TEXT_FAINT),
        _ if hovered => (egui::FontFamily::Monospace, theme::TEXT_HI),
        _ => (egui::FontFamily::Monospace, Color32::from_gray(0xcc)),
    };
    painter.text(
        rect.left_center() + vec2(9.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::new(11.5, family),
        colour,
    );
    if let Some((tag, tag_colour)) = right {
        painter.text(
            rect.right_center() - vec2(8.0, 0.0),
            Align2::RIGHT_CENTER,
            tag,
            FontId::new(10.0, theme::mono_bold()),
            tag_colour,
        );
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A shelved file: icon, name, size badge, and a status dot. The size badge turns into a
/// remove button under the pointer. Returns `(response, removed)`.
const SHELF_ROW_HEIGHT: f32 = 30.0;

fn shelf_row(
    ui: &mut egui::Ui,
    item: &ShelfItem,
    active: bool,
    selected: bool,
    status: Option<Color32>,
) -> (egui::Response, bool) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), SHELF_ROW_HEIGHT), Sense::click());
    let hovered = ui.rect_contains_pointer(rect);

    let badge_text = format_bytes(item.bytes);
    let badge_font = FontId::monospace(10.5);
    let badge_galley = ui
        .painter()
        .layout_no_wrap(badge_text, badge_font, Color32::PLACEHOLDER);
    let badge = Rect::from_min_size(
        pos2(
            rect.right() - 6.0 - badge_galley.size().x - 10.0,
            rect.center().y - 9.0,
        ),
        vec2(badge_galley.size().x + 10.0, 18.0),
    );
    let remove = ui.interact(badge, response.id.with("remove"), Sense::click());

    let painter = ui.painter();
    let border = if active {
        theme::ACCENT
    } else if selected {
        theme::ACCENT.linear_multiply(0.8)
    } else if hovered {
        theme::BORDER_HI
    } else {
        Color32::from_gray(0x20)
    };
    let fill = if active {
        theme::BG_900
    } else if selected {
        theme::ACCENT.linear_multiply(0.12)
    } else if hovered {
        Color32::from_gray(0x0c)
    } else {
        theme::BG_950
    };
    painter.rect(
        rect,
        3.0,
        fill,
        Stroke::new(if active || selected { 1.5 } else { 1.0 }, border),
        egui::StrokeKind::Inside,
    );

    let is_raster = photo_engine::is_raster_path(&item.path);
    painter.text(
        rect.left_center() + vec2(14.0, 0.0),
        Align2::CENTER_CENTER,
        if is_raster { "🖼" } else { "📷" },
        FontId::proportional(13.0),
        if active {
            theme::ACCENT
        } else {
            Color32::from_gray(0x88)
        },
    );

    let mut right_cursor = badge.left() - 6.0;

    if let Some(colour) = status {
        painter.circle_filled(pos2(right_cursor - 4.0, rect.center().y), 3.0, colour);
        right_cursor -= 12.0;
    }

    if item.metadata.rating > 0 {
        let r_text = format!("★{}", item.metadata.rating);
        let r_galley =
            painter.layout_no_wrap(r_text, FontId::new(10.5, theme::mono_bold()), theme::ACCENT);
        painter.galley(
            pos2(
                right_cursor - r_galley.size().x,
                rect.center().y - r_galley.size().y / 2.0,
            ),
            r_galley.clone(),
            theme::ACCENT,
        );
        right_cursor -= r_galley.size().x + 6.0;
    }

    match item.metadata.flag {
        photo_engine::Flag::Pick => {
            let f_rect =
                Rect::from_center_size(pos2(right_cursor - 7.0, rect.center().y), vec2(14.0, 14.0));
            painter.rect(
                f_rect,
                2.0,
                theme::BG_850,
                Stroke::new(1.0, theme::LIME),
                egui::StrokeKind::Inside,
            );
            painter.text(
                f_rect.center(),
                Align2::CENTER_CENTER,
                "P",
                FontId::new(9.0, theme::mono_bold()),
                theme::LIME,
            );
            right_cursor -= 18.0;
        }
        photo_engine::Flag::Reject => {
            let f_rect =
                Rect::from_center_size(pos2(right_cursor - 7.0, rect.center().y), vec2(14.0, 14.0));
            painter.rect(
                f_rect,
                2.0,
                theme::BG_850,
                Stroke::new(1.0, theme::DANGER),
                egui::StrokeKind::Inside,
            );
            painter.text(
                f_rect.center(),
                Align2::CENTER_CENTER,
                "X",
                FontId::new(9.0, theme::mono_bold()),
                theme::DANGER,
            );
            right_cursor -= 18.0;
        }
        photo_engine::Flag::Unflagged => {}
    }

    let (family, colour) = if active {
        (theme::mono_bold(), theme::ACCENT)
    } else {
        (egui::FontFamily::Monospace, theme::TEXT)
    };
    let mut job = egui::text::LayoutJob::single_section(
        item.name.clone(),
        egui::TextFormat::simple(FontId::new(12.0, family), colour),
    );
    let name_left = rect.left() + 30.0;
    job.wrap = egui::text::TextWrapping::truncate_at_width((right_cursor - name_left).max(10.0));
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(name_left, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );

    if hovered {
        let hot = remove.hovered();
        painter.rect(
            badge,
            3.0,
            if hot {
                theme::DANGER.linear_multiply(0.2)
            } else {
                theme::BG_850
            },
            Stroke::new(1.0, if hot { theme::DANGER } else { theme::RAIL }),
            egui::StrokeKind::Inside,
        );
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            "✕ remove",
            FontId::monospace(10.0),
            if hot { theme::DANGER } else { theme::TEXT_DIM },
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else {
        painter.rect(
            badge,
            3.0,
            theme::BG_850,
            Stroke::new(1.0, theme::RAIL),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            badge.center() - badge_galley.size() / 2.0,
            badge_galley,
            if active {
                theme::TEXT
            } else {
                Color32::from_gray(0x88)
            },
        );
    }

    (response, remove.clicked())
}

/// The camera's own rendering of the photo being opened, fitted to the stage, until the
/// real decode replaces it. Not interactive: there is nothing to edit yet.
fn stage_preview(ui: &mut egui::Ui, preview: &egui::TextureHandle, path: &std::path::Path) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let [w, h] = preview.size();
    if rect.width() < 1.0 || rect.height() < 1.0 || w == 0 || h == 0 {
        return;
    }
    let img = egui::vec2(w as f32, h as f32);
    let fit = (rect.width() / img.x).min(rect.height() / img.y);
    let dest = egui::Rect::from_center_size(rect.center(), img * fit);
    let painter = ui.painter().with_clip_rect(rect);
    painter.image(
        preview.id(),
        dest,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    painter.rect_stroke(
        dest,
        0.0,
        Stroke::new(1.0, theme::RAIL),
        egui::StrokeKind::Outside,
    );
    hud_badge(
        &painter,
        dest.left_top() + vec2(12.0, 12.0),
        Align2::LEFT_TOP,
        &format!("opening {}…", file_name(path)),
        theme::ACCENT,
        theme::ACCENT,
    );
}

/// A filmstrip tile, and the distance from one tile's left edge to the next.
const FILM_TILE: egui::Vec2 = vec2(112.0, 70.0);
const FILM_GAP: f32 = 8.0;
const FILM_STRIDE: f32 = FILM_TILE.x + FILM_GAP;

/// Which of `total` tiles overlap the horizontal span `x` of the strip, measured from its
/// left edge. A tile only partly in view is included.
fn visible_tiles(x: egui::Rangef, total: usize) -> std::ops::Range<usize> {
    let first = (x.min / FILM_STRIDE).floor().max(0.0) as usize;
    let last = (x.max / FILM_STRIDE).ceil().max(0.0) as usize;
    first.min(total)..last.min(total)
}

/// A filmstrip frame with thumbnail preview, rating stars, flag badge, and selection border.
fn film_tile(
    ui: &mut egui::Ui,
    item: &ShelfItem,
    active: bool,
    selected: bool,
    pending: bool,
    picture: Option<(egui::TextureId, f32)>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(FILM_TILE, Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::BG_900);

    let inner = rect.shrink(if active { 3.0 } else { 1.0 });
    match picture {
        Some((id, aspect)) => {
            // Cover-fit: crop the texture's UVs to the tile's aspect.
            let tile_aspect = inner.width() / inner.height();
            let uv = if aspect > tile_aspect {
                let w = tile_aspect / aspect;
                Rect::from_min_max(pos2(0.5 - w / 2.0, 0.0), pos2(0.5 + w / 2.0, 1.0))
            } else {
                let h = aspect / tile_aspect;
                Rect::from_min_max(pos2(0.0, 0.5 - h / 2.0), pos2(1.0, 0.5 + h / 2.0))
            };
            painter.image(id, inner, uv, Color32::from_white_alpha(220));
        }
        None => {
            let is_raster = photo_engine::is_raster_path(&item.path);
            painter.text(
                inner.center() - vec2(0.0, 6.0),
                Align2::CENTER_CENTER,
                if is_raster { "🖼" } else { "📷" },
                FontId::proportional(16.0),
                theme::RAIL,
            );
        }
    }

    if selected && !active {
        painter.rect_filled(inner, 0.0, theme::ACCENT.linear_multiply(0.12));
    }

    // File stem on a black strip, bottom-left, as the frame number.
    let stem = item
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&item.name)
        .to_string();
    let mut job = egui::text::LayoutJob::single_section(
        stem,
        egui::TextFormat::simple(
            FontId::new(
                10.5,
                if active {
                    theme::mono_bold()
                } else {
                    egui::FontFamily::Monospace
                },
            ),
            if active {
                theme::TEXT_HI
            } else {
                Color32::from_gray(0xcc)
            },
        ),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(inner.width() - 14.0);
    let galley = painter.layout_job(job);
    let strip = Rect::from_min_max(
        pos2(inner.left(), inner.bottom() - galley.size().y - 6.0),
        inner.right_bottom(),
    );
    painter.rect_filled(strip, 0.0, Color32::from_black_alpha(200));
    painter.galley(
        pos2(strip.left() + 6.0, strip.top() + 3.0),
        galley,
        theme::TEXT,
    );

    // Rating badge (top-right)
    if item.metadata.rating > 0 {
        let stars_text = format!("★{}", item.metadata.rating);
        let star_galley = painter.layout_no_wrap(
            stars_text,
            FontId::new(9.5, theme::mono_bold()),
            theme::ACCENT,
        );
        let star_rect = Rect::from_min_size(
            inner.right_top() + vec2(-star_galley.size().x - 6.0, 3.0),
            star_galley.size() + vec2(4.0, 2.0),
        );
        painter.rect_filled(star_rect, 2.0, Color32::from_black_alpha(200));
        painter.galley(
            star_rect.center() - star_galley.size() / 2.0,
            star_galley,
            theme::ACCENT,
        );
    }

    // Flag badge (top-left)
    match item.metadata.flag {
        photo_engine::Flag::Pick => {
            let p_rect = Rect::from_min_size(inner.left_top() + vec2(3.0, 3.0), vec2(14.0, 14.0));
            painter.rect_filled(p_rect, 2.0, theme::LIME.linear_multiply(0.85));
            painter.text(
                p_rect.center(),
                Align2::CENTER_CENTER,
                "P",
                FontId::new(9.0, theme::mono_bold()),
                Color32::BLACK,
            );
        }
        photo_engine::Flag::Reject => {
            let x_rect = Rect::from_min_size(inner.left_top() + vec2(3.0, 3.0), vec2(14.0, 14.0));
            painter.rect_filled(x_rect, 2.0, theme::DANGER.linear_multiply(0.85));
            painter.text(
                x_rect.center(),
                Align2::CENTER_CENTER,
                "X",
                FontId::new(9.0, theme::mono_bold()),
                Color32::WHITE,
            );
        }
        photo_engine::Flag::Unflagged => {}
    }

    let status = if item.has_sidecar {
        Some(theme::LIME)
    } else if item.is_edited {
        Some(theme::ACCENT)
    } else {
        None
    };
    if let Some(colour) = status {
        let dot_pos = if item.metadata.rating > 0 {
            inner.right_top() + vec2(-6.0, 18.0)
        } else {
            inner.right_top() + vec2(-6.0, 6.0)
        };
        painter.circle_filled(dot_pos, 3.0, colour);
    }

    if pending {
        painter.rect_filled(inner, 0.0, Color32::from_black_alpha(140));
        painter.text(
            inner.center(),
            Align2::CENTER_CENTER,
            "loading…",
            FontId::new(10.5, theme::mono_bold()),
            theme::ACCENT,
        );
    }
    let border = if active {
        Stroke::new(2.0, theme::ACCENT)
    } else if selected {
        Stroke::new(2.0, Color32::from_rgb(220, 220, 220))
    } else if pending {
        Stroke::new(2.0, theme::ACCENT.linear_multiply(0.5))
    } else if hovered {
        Stroke::new(1.0, theme::BORDER_HI)
    } else {
        Stroke::new(1.0, theme::BORDER)
    };
    painter.rect_stroke(rect, 3.0, border, egui::StrokeKind::Inside);
    if hovered && !active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.on_hover_text(&item.name)
}

/// Text on a black pill with a hairline, for readouts drawn over the image.
fn hud_badge(
    painter: &egui::Painter,
    anchor: egui::Pos2,
    align: Align2,
    text: &str,
    colour: Color32,
    border: Color32,
) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        FontId::new(11.0, theme::mono_bold()),
        colour,
    );
    let size = galley.size() + vec2(16.0, 8.0);
    let rect = align.anchor_size(anchor, size);
    painter.rect(
        rect,
        3.0,
        Color32::from_black_alpha(235),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(8.0, 4.0), galley, colour);
}

/// A, B, … Z, then 27, 28, … — a short handle for each snapshot.
fn snapshot_key(i: usize) -> String {
    if i < 26 {
        char::from(b'A' + i as u8).to_string()
    } else {
        (i + 1).to_string()
    }
}

fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// A minimal draggable curve editor. Endpoints are locked in x so the curve always spans
/// the full range.
fn curve_editor(ui: &mut egui::Ui, nodes: &mut [CurveNode]) -> egui::Response {
    let width = ui.available_width();
    let size = egui::vec2(width, (width * 0.62).clamp(120.0, 220.0));
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 3.0, theme::BG_950);
    // Quarter reticle and the identity diagonal, so a move reads against neutral.
    let grid = egui::Stroke::new(1.0, egui::Color32::from_gray(0x1a));
    for i in 1..4 {
        let t = i as f32 / 4.0;
        painter.vline(rect.left() + t * rect.width(), rect.y_range(), grid);
        painter.hline(rect.x_range(), rect.top() + t * rect.height(), grid);
    }
    painter.extend(egui::Shape::dashed_line(
        &[rect.left_bottom(), rect.right_top()],
        egui::Stroke::new(1.0, theme::BORDER_HI),
        4.0,
        4.0,
    ));

    let to_screen = |n: &CurveNode| {
        egui::pos2(
            rect.left() + n.x.clamp(0.0, 1.0) * rect.width(),
            rect.bottom() - n.y.clamp(0.0, 1.0) * rect.height(),
        )
    };

    if let Some(pointer) = response.interact_pointer_pos() {
        if response.dragged() {
            let x = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let y = ((rect.bottom() - pointer.y) / rect.height()).clamp(0.0, 1.0);

            // Grab the nearest node in x and move it. Endpoints keep their x.
            let mut best = 0usize;
            let mut best_d = f32::MAX;
            for (i, n) in nodes.iter().enumerate() {
                let d = (n.x - x).abs();
                if d < best_d {
                    best_d = d;
                    best = i;
                }
            }
            let last = nodes.len().saturating_sub(1);
            if best != 0 && best != last {
                nodes[best].x = x;
            }
            nodes[best].y = y;
            nodes.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            response.mark_changed();
        }
    }

    let lut = photo_engine::curve::build_lut(nodes);
    let points: Vec<egui::Pos2> = (0..96)
        .map(|i| {
            let t = i as f32 / 95.0;
            let v = lut[(t * (lut.len() - 1) as f32) as usize];
            egui::pos2(
                rect.left() + t * rect.width(),
                rect.bottom() - v.clamp(0.0, 1.0) * rect.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(1.75, theme::ACCENT),
    ));

    for n in nodes.iter() {
        painter.circle(
            to_screen(n),
            3.5,
            theme::TEXT_HI,
            egui::Stroke::new(1.5, theme::ACCENT),
        );
    }
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    response
}

fn renderer_write<'a>(gpu: &Gpu<'a>) -> std::sync::RwLockWriteGuard<'a, egui_wgpu::Renderer> {
    gpu.renderer
        .write()
        .expect("egui renderer lock poisoned by a panic in another thread")
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string()
}

/// RGB + luminance histogram of what is actually on screen.
///
/// Drawn as three additive channel outlines over a filled luminance body, which is the
/// convention every photographer already reads. The vertical scale ignores pure black and
/// pure white, so a large flat sky or shadow does not squash everything else into a line.
#[allow(dead_code)]
fn histogram_plot(ui: &mut egui::Ui, hist: &photo_engine::Histogram) {
    let size = egui::vec2(ui.available_width().min(252.0), 90.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 2.0, theme::BG_950);

    if hist.peak > 0 {
        let bins = hist.luma.len().max(1);
        let step = rect.width() / bins as f32;
        let peak = hist.peak as f32;
        let height = |v: u32| (v as f32 / peak).min(1.0) * rect.height();

        // Luminance first, as a solid body behind the channels.
        for (i, v) in hist.luma.iter().enumerate() {
            let h = height(*v);
            if h <= 0.0 {
                continue;
            }
            let x = rect.left() + i as f32 * step;
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x, rect.bottom() - h),
                    egui::pos2(x + step.max(1.0), rect.bottom()),
                ),
                0.0,
                theme::TEXT_DIM.linear_multiply(0.30),
            );
        }

        let channels: [(&Vec<u32>, egui::Color32); 3] = [
            (&hist.red, egui::Color32::from_rgb(0xd0, 0x5a, 0x50)),
            (&hist.green, egui::Color32::from_rgb(0x6a, 0xb0, 0x6a)),
            (&hist.blue, egui::Color32::from_rgb(0x5a, 0x86, 0xd0)),
        ];
        for (data, colour) in channels {
            let points: Vec<egui::Pos2> = data
                .iter()
                .enumerate()
                .map(|(i, v)| egui::pos2(rect.left() + i as f32 * step, rect.bottom() - height(*v)))
                .collect();
            painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, colour)));
        }
    }

    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );

    // Clipping is the one number worth calling out; it is what you actually lose.
    let low = hist.clipped_low() * 100.0;
    let high = hist.clipped_high() * 100.0;
    ui.horizontal(|ui| {
        ui.label(theme::dim("clipped"));
        let warn = |v: f32| {
            if v >= 0.1 {
                theme::num(format!("{v:.2}%")).color(theme::DANGER)
            } else {
                theme::num(format!("{v:.2}%"))
            }
        };
        ui.label(theme::dim("lo"));
        ui.label(warn(low));
        ui.label(theme::dim("hi"));
        ui.label(warn(high));
    });
}

#[derive(Clone, Copy)]
enum GeometryIcon {
    RotateLeft,
    RotateRight,
    MirrorH,
    MirrorV,
}

fn geometry_icon_button(
    ui: &mut egui::Ui,
    icon: GeometryIcon,
    active: bool,
    tooltip: &str,
) -> egui::Response {
    let size = egui::vec2(28.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let bg_color = if active {
            theme::ACCENT.linear_multiply(0.25)
        } else if response.hovered() {
            theme::BG_850
        } else {
            theme::BG_900
        };

        let stroke_color = if active {
            theme::ACCENT
        } else if response.hovered() {
            theme::TEXT_DIM
        } else {
            theme::BORDER
        };

        let icon_color = if active {
            theme::ACCENT
        } else if response.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };

        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(2), bg_color);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(2),
            egui::Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );

        let center = rect.center();
        let stroke = egui::Stroke::new(1.5, icon_color);

        match icon {
            GeometryIcon::RotateLeft => {
                let r = 5.5;
                let mut points = Vec::new();
                for i in 0..=12 {
                    let angle =
                        std::f32::consts::PI * 0.2 + (i as f32 / 12.0) * std::f32::consts::PI * 1.4;
                    points.push(center + egui::vec2(angle.cos() * r, -angle.sin() * r));
                }
                for pair in points.windows(2) {
                    ui.painter().line_segment([pair[0], pair[1]], stroke);
                }
                let tip = points[points.len() - 1];
                ui.painter()
                    .line_segment([tip, tip + egui::vec2(3.0, 0.0)], stroke);
                ui.painter()
                    .line_segment([tip, tip + egui::vec2(0.5, 3.0)], stroke);
            }
            GeometryIcon::RotateRight => {
                let r = 5.5;
                let mut points = Vec::new();
                for i in 0..=12 {
                    let angle =
                        std::f32::consts::PI * 0.8 - (i as f32 / 12.0) * std::f32::consts::PI * 1.4;
                    points.push(center + egui::vec2(angle.cos() * r, -angle.sin() * r));
                }
                for pair in points.windows(2) {
                    ui.painter().line_segment([pair[0], pair[1]], stroke);
                }
                let tip = points[points.len() - 1];
                ui.painter()
                    .line_segment([tip, tip - egui::vec2(3.0, 0.0)], stroke);
                ui.painter()
                    .line_segment([tip, tip + egui::vec2(-0.5, 3.0)], stroke);
            }
            GeometryIcon::MirrorH => {
                ui.painter().line_segment(
                    [
                        center + egui::vec2(0.0, -7.0),
                        center + egui::vec2(0.0, 7.0),
                    ],
                    egui::Stroke::new(1.0, theme::BORDER),
                );
                let t_left = [
                    center + egui::vec2(-2.0, -4.5),
                    center + egui::vec2(-7.0, 0.0),
                    center + egui::vec2(-2.0, 4.5),
                ];
                ui.painter().add(egui::Shape::convex_polygon(
                    t_left.to_vec(),
                    if active {
                        theme::ACCENT.linear_multiply(0.4)
                    } else {
                        egui::Color32::TRANSPARENT
                    },
                    stroke,
                ));
                let t_right = [
                    center + egui::vec2(2.0, -4.5),
                    center + egui::vec2(7.0, 0.0),
                    center + egui::vec2(2.0, 4.5),
                ];
                ui.painter().add(egui::Shape::convex_polygon(
                    t_right.to_vec(),
                    if active {
                        theme::ACCENT.linear_multiply(0.4)
                    } else {
                        egui::Color32::TRANSPARENT
                    },
                    stroke,
                ));
            }
            GeometryIcon::MirrorV => {
                ui.painter().line_segment(
                    [
                        center + egui::vec2(-7.0, 0.0),
                        center + egui::vec2(7.0, 0.0),
                    ],
                    egui::Stroke::new(1.0, theme::BORDER),
                );
                let t_top = [
                    center + egui::vec2(-4.5, -2.0),
                    center + egui::vec2(0.0, -7.0),
                    center + egui::vec2(4.5, -2.0),
                ];
                ui.painter().add(egui::Shape::convex_polygon(
                    t_top.to_vec(),
                    if active {
                        theme::ACCENT.linear_multiply(0.4)
                    } else {
                        egui::Color32::TRANSPARENT
                    },
                    stroke,
                ));
                let t_bottom = [
                    center + egui::vec2(-4.5, 2.0),
                    center + egui::vec2(0.0, 7.0),
                    center + egui::vec2(4.5, 2.0),
                ];
                ui.painter().add(egui::Shape::convex_polygon(
                    t_bottom.to_vec(),
                    if active {
                        theme::ACCENT.linear_multiply(0.4)
                    } else {
                        egui::Color32::TRANSPARENT
                    },
                    stroke,
                ));
            }
        }
    }

    response.on_hover_text(tooltip)
}

/// The white balance temperature slider's span, as Lightroom's.
const WB_KELVIN_MIN: f32 = 2000.0;
const WB_KELVIN_MAX: f32 = 50000.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_tiles_in_view_are_drawn() {
        let span = |a: f32, b: f32| egui::Rangef::new(a, b);
        // Unscrolled, a strip wide enough for two and a bit tiles.
        assert_eq!(visible_tiles(span(0.0, 2.5 * FILM_STRIDE), 1000), 0..3);
        // Scrolled so tile 10 is cut off on the left and tile 14 on the right.
        let v = visible_tiles(span(10.5 * FILM_STRIDE, 14.2 * FILM_STRIDE), 1000);
        assert_eq!(v, 10..15);
        // Never past the end, even when the viewport is wider than the strip.
        assert_eq!(visible_tiles(span(0.0, 50.0 * FILM_STRIDE), 3), 0..3);
        assert_eq!(visible_tiles(span(-5.0, 1.0), 0), 0..0);
    }
}
