//! Barrel 1's UI. Knows about egui and the engine; knows nothing about windowing.
//!
//! The desktop and Android bootstraps both drive this same type.

pub mod batch;
pub mod compact;
pub mod dam;
pub mod gpu_errors;
pub mod inspector_map_3d;
mod loader;
pub mod platform;
pub mod presets;
pub mod settings;
pub mod theme;
pub mod tonemap_3d;
pub mod widgets;

mod curve_editor;
mod desktop;
mod export_flow;
mod icons;
mod inspector;
mod keys;
mod modules;
mod parts;
mod requests;
mod session;
mod view;

use curve_editor::*;
use icons::*;
use inspector::summary::*;
use modules::*;
use parts::*;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use std::sync::RwLock;

use egui::{pos2, vec2, Align2, Color32, FontId, Margin, Rect, RichText, Sense, Stroke};
use photo_engine::{
    find_sidecar, identity_nodes, load_sidecar, Crop, CurveNode, Engine, Geometry, PhotoStack,
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

    /// The camera orientation became known after edits were already recorded against a
    /// guess. Every state still carrying the guess gets the real one, so undoing past
    /// the moment it arrived does not turn the photo.
    fn rebase_geometry(&mut self, from: Geometry, to: Geometry) {
        for stack in self.past.iter_mut().chain(self.future.iter_mut()) {
            if stack.geometry == from {
                stack.geometry = to;
            }
        }
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
    /// The photo being edited: what the inspector, autosave, ratings and hotkeys act on.
    /// It moves the moment another photo is chosen, before that photo has decoded, so the
    /// controls never show one photo's values while another is on the stage.
    loaded_path: Option<PathBuf>,
    /// The photo whose pixels are on the GPU. Trails `loaded_path` while a newly chosen
    /// photo decodes.
    engine_path: Option<PathBuf>,
    /// Nothing has read the open photo's camera orientation yet, so `baseline` has none.
    orientation_pending: bool,
    /// The open photo failed to decode: there is nothing to edit, and nothing is saved.
    decode_failed: bool,
    /// The open photo's sidecar could not be read, or was written by a newer build: it is
    /// shown as found and never written over.
    sidecar_locked: bool,
    /// The stack as autosave last saw it, and when it is due to be written.
    autosave_seen: PhotoStack,
    autosave_due: Option<Instant>,
    /// Every shelf sidecar as it was before the last paste to all, to take the paste back.
    bulk_paste_undo: Option<Vec<(PathBuf, Option<Vec<u8>>)>>,
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
    /// The open photo's named snapshots, kept in its sidecar.
    pub snapshots: Vec<(String, PhotoStack)>,
    /// Undo history for photos stepped away from, most recent last, so returning to one
    /// brings its Ctrl+Z back. Capped: see `session::HISTORY_KEPT`.
    parked_histories: Vec<(PathBuf, History)>,
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
    /// A plain click adds to or removes from the selection, as Ctrl-click does: how a
    /// touch screen multi-selects.
    pub select_mode: bool,
    /// Sliders move ten times finer, as holding Shift does.
    pub fine_adjust: bool,
    /// Input has come from a touch screen: controls are sized for fingers.
    touch_seen: bool,
    /// The phone layout's open sheet; `None` gives the stage the whole screen.
    pub compact_tab: Option<compact::Tab>,
    /// The module the phone layout's Edit sheet shows.
    pub compact_module: InspectorSectionId,
    /// The phone layout's Library shows its rating, flag and search filters.
    pub library_filters_open: bool,
    /// A photo was asked for from outside the Library grid (a picker, a share); when it
    /// lands the phone layout goes straight to Edit, where the photo is worked on.
    edit_when_opened: bool,
    /// The Edit sheet shows the chosen module's controls; off, only the module strip.
    pub compact_controls_open: bool,
    /// The height the portrait sheet's content wanted last frame; it gets that, up to a cap.
    compact_sheet_fit: Option<f32>,
    /// User-adjusted width for the landscape side sheet.
    pub compact_sheet_width: Option<f32>,
    /// Where a swipe across the stage began, to step photos when it ends.
    swipe_from: Option<egui::Pos2>,
    /// A photo step asked for where there was no `Gpu` to do it with.
    pending_step: Option<isize>,
    /// Which arrangement this frame is laid out in; decided by the screen's size.
    layout: compact::Layout,
    /// Where settings are kept, and what they say.
    settings_dir: Option<PathBuf>,
    settings: settings::Settings,
    /// The phone's modules: which show in the Edit strip, in what order. Separate from
    /// the desktop's, because a phone wants fewer.
    pub compact_sections: Vec<InspectorSectionConfig>,
    /// The look last applied from Presets, and what it was applied over.
    preset: Option<compact::PresetState>,
    /// The phone tour's current step, while it is showing.
    tour_step: Option<usize>,
    /// Where the phone layout's parts were last frame, for the tour to point at.
    compact_rects: compact::Rects,
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
        let settings_dir = platform.settings_dir();
        let settings = settings::Settings::load(settings_dir.as_deref());
        let compact_sections = settings
            .compact_sections
            .clone()
            .map(settings::merge_sections)
            .unwrap_or_else(settings::default_compact_sections);
        // A marker left by the last run means it ended without `shutdown` — a crash, a
        // kill, a power cut. Autosave will have kept the edits; say so.
        let lock = session_lock_path(settings_dir.as_deref());
        let unclean = lock.as_ref().is_some_and(|l| l.is_file());
        if let Some(lock) = &lock {
            if let Some(dir) = lock.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(lock, std::process::id().to_string());
        }
        let mut app = Self {
            engine,
            stack: PhotoStack::default(),
            history: History::new(),
            texture_id: None,
            needs_render: false,
            error: None,
            loaded_path: None,
            engine_path: None,
            orientation_pending: false,
            decode_failed: false,
            sidecar_locked: false,
            autosave_seen: PhotoStack::default(),
            autosave_due: None,
            bulk_paste_undo: None,
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
            parked_histories: Vec::new(),
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
            select_mode: false,
            fine_adjust: false,
            touch_seen: false,
            compact_tab: Some(compact::Tab::Library),
            compact_module: InspectorSectionId::Exposure,
            library_filters_open: false,
            edit_when_opened: false,
            compact_controls_open: true,
            compact_sheet_width: None,
            compact_sheet_fit: None,
            swipe_from: None,
            pending_step: None,
            layout: compact::Layout::Desktop,
            settings_dir,
            settings,
            compact_sections,
            preset: None,
            tour_step: None,
            compact_rects: compact::Rects::default(),
        };
        if unclean {
            app.set_status("Duckfoot did not close cleanly last time; edits up to then were saved");
        }
        app
    }

    pub fn is_stack_edited(&self) -> bool {
        self.stack != self.baseline
    }

    /// The edit on the stage.
    pub fn stack(&self) -> &PhotoStack {
        &self.stack
    }

    /// A photo is chosen and editable: from the moment it is picked, before it decodes,
    /// and not once its decode has failed.
    pub fn can_edit(&self) -> bool {
        self.loaded_path.is_some() && !self.decode_failed
    }

    /// Change the open photo's stack as one undo step. Nothing happens with no photo open.
    pub fn edit(&mut self, change: impl FnOnce(&mut PhotoStack)) {
        if !self.can_edit() {
            return;
        }
        let before = self.stack.clone();
        change(&mut self.stack);
        if self.stack != before {
            self.history.checkpoint(&before);
            self.needs_render = true;
        }
    }

    /// Step back one edit, as Ctrl+Z does.
    pub fn undo(&mut self) -> bool {
        let moved = self.history.undo(&mut self.stack);
        self.needs_render |= moved;
        moved
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
    /// Show the phone tour from its first step.
    pub fn start_tour(&mut self) {
        self.tour_step = Some(0);
    }

    /// Show the phone tour at `step` (from 0), as its "next" would have.
    pub fn show_tour_step(&mut self, step: usize) {
        self.tour_step = Some(step);
    }

    /// The phone tour's step while it is showing.
    pub fn tour_step(&self) -> Option<usize> {
        self.tour_step
    }

    fn save_settings(&self) {
        self.settings.save(self.settings_dir.as_deref());
    }

    /// The size the photo's stage got last frame.
    pub fn stage_size(&self) -> egui::Vec2 {
        self.last_panel
    }

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
        // The engine still holds the last photo while the chosen one decodes; rendering
        // it with the new photo's stack would draw one photo with another's edits.
        if self.ready_image().is_none() {
            return;
        }
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
        // Framing a crop shows the whole frame with no divider, so no split either.
        let split_x = if self.split_active && !self.cropping {
            Some(self.split_pos)
        } else {
            None
        };
        self.engine
            .render(gpu.device, gpu.queue, &self.stack, split_x);
        self.render_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.needs_render = false;

        if self.texture_id.is_none() {
            if let Some(image) = self.ready_image() {
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
        // Phones, and tablets held upright, get the navigation-bar layout (`compact`): the
        // desktop's side panels need a screen both wide and tall.
        let screen = ui.max_rect().size();
        self.layout = compact::Layout::for_size(screen);
        self.touch_seen |= ctx.input(|i| {
            i.raw
                .events
                .iter()
                .any(|e| matches!(e, egui::Event::Touch { .. }))
        });
        widgets::set_touch(&ctx, self.touch_seen || self.layout.is_compact());
        widgets::set_fine(&ctx, self.fine_adjust);
        self.loader.set_context(&ctx);
        self.poll_platform(gpu);
        self.poll_loader(&ctx, gpu);
        self.handle_keys(&ctx, gpu);
        self.poll_export(&ctx);
        self.poll_batch_export(&ctx);
        if let Some(e) = gpu_errors::take() {
            self.error = Some(format!("GPU error: {e}"));
        }
        self.engine.poll_histogram(gpu.device, gpu.queue);
        if self.engine.histogram_busy() {
            // The readback's callback does not wake the event loop, and on a touch screen
            // nothing else may; come back for it.
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        self.sync(gpu, ctx.pixels_per_point());

        if self.layout.is_compact() {
            self.compact_layout(ui, gpu, screen);
        } else {
            // Panel order is layout order: full-width bars first, then the side panels,
            // then the filmstrip and stage toolbar, which only span the space left between.
            self.top_bar(ui);
            self.status_bar(ui);
            self.shelf(ui, gpu);
            self.inspector(ui);
            self.filmstrip(ui, gpu);
            self.stage_toolbar(ui);
            self.workspace(ui);
            self.batch_export_dialog(ui);
        }
        if let Some(dir) = self.pending_step.take() {
            self.step_photo(gpu, dir);
        }
        self.autosave(&ctx);
    }

    /// Write the edits a second after they stop changing. A drag changes the stack every
    /// frame and pushes the deadline on, so a gesture is saved once, when it ends.
    fn autosave(&mut self, ctx: &egui::Context) {
        const SETTLE: Duration = Duration::from_secs(1);
        if self.stack != self.autosave_seen {
            self.autosave_seen = self.stack.clone();
            self.autosave_due = Some(Instant::now() + SETTLE);
        }
        let Some(due) = self.autosave_due else {
            return;
        };
        let now = Instant::now();
        if now >= due {
            self.flush();
        } else {
            ctx.request_repaint_after(due - now);
        }
    }
}

// ── Summaries ───────────────────────────────────────────────────────────────────────────

fn renderer_write<'a>(gpu: &Gpu<'a>) -> std::sync::RwLockWriteGuard<'a, egui_wgpu::Renderer> {
    gpu.renderer
        .write()
        .expect("egui renderer lock poisoned by a panic in another thread")
}

/// Linear light to its sRGB-encoded display value, 0..=1.
fn encode_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Present while a desktop session runs; left behind, it means the last one did not end
/// cleanly. Android kills backgrounded apps as a matter of course, so it has no marker.
fn session_lock_path(settings_dir: Option<&Path>) -> Option<PathBuf> {
    if cfg!(target_os = "android") {
        return None;
    }
    settings_dir.map(|d| d.join("session.lock"))
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string()
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

    /// The curve is 400 × 220 at the origin: tap adds a node on the curve, a drag moves
    /// the node it starts on, a double-click removes it, and the ends keep their x.
    #[test]
    fn curve_nodes_are_added_moved_and_removed() {
        let ctx = egui::Context::default();
        let mut nodes = identity_nodes();
        let t = std::cell::Cell::new(0.0);
        let frame = |nodes: &mut Vec<CurveNode>, events: Vec<egui::Event>| {
            t.set(t.get() + 0.05);
            let input = egui::RawInput {
                time: Some(t.get()),
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                curve_editor(ui, nodes);
            });
            out.textures_delta.clear();
        };
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let at = |x: f32, y: f32| pos2(x * 400.0, 220.0 - y * 220.0);

        frame(&mut nodes, vec![]);
        // A tap well away from both ends adds a node where the curve already is.
        let p = at(0.5, 0.9);
        frame(
            &mut nodes,
            vec![egui::Event::PointerMoved(p), press(p, true)],
        );
        frame(&mut nodes, vec![press(p, false)]);
        assert_eq!(nodes.len(), 3);
        assert!((nodes[1].x - 0.5).abs() < 1e-3 && (nodes[1].y - 0.5).abs() < 1e-2);

        // Drag it up: grabbed a little off-centre, it follows without jumping.
        let from = at(0.5, 0.5) + vec2(3.0, 2.0);
        frame(
            &mut nodes,
            vec![egui::Event::PointerMoved(from), press(from, true)],
        );
        for i in 1..=6 {
            let p = from - vec2(0.0, 44.0 * i as f32 / 6.0);
            frame(&mut nodes, vec![egui::Event::PointerMoved(p)]);
        }
        frame(&mut nodes, vec![press(from - vec2(0.0, 44.0), false)]);
        assert_eq!(nodes.len(), 3);
        assert!((nodes[1].x - 0.5).abs() < 1e-3, "x moved: {:?}", nodes[1]);
        assert!((nodes[1].y - 0.7).abs() < 1e-2, "y: {:?}", nodes[1]);

        // An end node moves only up and down.
        let from = at(0.0, 0.0);
        frame(
            &mut nodes,
            vec![egui::Event::PointerMoved(from), press(from, true)],
        );
        for i in 1..=6 {
            let p = from + vec2(40.0, -40.0) * (i as f32 / 6.0);
            frame(&mut nodes, vec![egui::Event::PointerMoved(p)]);
        }
        frame(&mut nodes, vec![press(from + vec2(40.0, -40.0), false)]);
        assert_eq!(nodes[0].x, 0.0);
        assert!((nodes[0].y - 40.0 / 220.0).abs() < 1e-2, "{:?}", nodes[0]);

        // Double-clicking the middle node removes it; the ends cannot be removed.
        t.set(t.get() + 1.0);
        let p = at(0.5, 0.7);
        frame(&mut nodes, vec![egui::Event::PointerMoved(p)]);
        for pressed in [true, false, true, false] {
            frame(&mut nodes, vec![press(p, pressed)]);
        }
        assert_eq!(nodes.len(), 2);
        assert_eq!((nodes[0].x, nodes[1].x), (0.0, 1.0));
    }
}
