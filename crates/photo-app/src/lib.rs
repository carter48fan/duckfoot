//! Barrel 1's UI. Knows about egui and the engine; knows nothing about windowing.
//!
//! The desktop and Android bootstraps both drive this same type.

pub mod batch;
pub mod categories;
pub mod compact;
pub mod dam;
pub mod gesture;
pub mod gpu_errors;
pub mod inspector_map_3d;
pub mod knobs;
mod loader;
mod on_photo;
pub mod platform;
pub mod presets;
pub mod settings;
pub mod theme;
pub mod tonemap_3d;
pub mod widgets;

pub mod crop_tool;
mod curve_editor;
pub mod cutout_studio;
mod desktop;
mod display;
mod export_flow;
mod export_ui;
mod help;
mod i18n;
mod icons;
mod info;
mod inspector;
mod keymap;
mod keys;
mod labels;
pub mod mask_tool;
mod modules;
mod parts;
pub mod paste;
mod preferences;
mod requests;
mod session;
pub mod tat;
pub mod user_presets;
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

/// The brush for painting a brush mask.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrushTool {
    /// Radius, percent of the photo's long side.
    pub size: f32,
    /// How much of the radius fades, percent.
    pub feather: f32,
    /// Take the mask away instead of adding to it.
    pub erase: bool,
}

impl Default for BrushTool {
    fn default() -> Self {
        Self {
            size: 6.0,
            feather: 60.0,
            erase: false,
        }
    }
}

/// Sections available in the Inspector sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum InspectorSectionId {
    Scope,
    Geometry,
    Lens,
    Denoise,
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
    Local,
    Heal,
    Look,
}

impl InspectorSectionId {
    pub fn all() -> &'static [Self] {
        &[
            Self::Scope,
            Self::Geometry,
            Self::Lens,
            Self::Denoise,
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
            Self::Local,
            Self::Heal,
            Self::Look,
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
    /// Not on disk when it was shelved: a session restored with its drive unplugged. It
    /// stays, greyed, so plugging the drive back in does not mean rebuilding the shelf.
    pub missing: bool,
    /// What the file says about how it was taken; `None` until the loader has read it,
    /// or if the file says nothing.
    pub info: Option<photo_engine::PhotoInfo>,
    /// The name it sorts by, a virtual copy's photo's, and which copy it is (1 for the
    /// photo itself).
    pub sort_name: String,
    pub copy: u32,
    /// Its name lowercased once, for the search box to match every frame.
    pub search_name: String,
    /// Its sidecar and file not read yet (WP5.3): shelved at once, filled in when the
    /// loader has read them.
    pub pending: bool,
}

/// What a shelved photo's sidecar and file say: read on the loader's thread.
#[derive(Debug, Clone)]
pub struct ShelfFacts {
    pub path: PathBuf,
    pub metadata: photo_engine::AssetMetadata,
    pub has_sidecar: bool,
    pub bytes: u64,
    pub missing: bool,
}

impl ShelfFacts {
    pub fn read(path: &std::path::Path) -> Self {
        let (metadata, has_sidecar) = match photo_engine::load_sidecar_full(path) {
            Ok(Some(sc)) => (sc.metadata, true),
            // No sidecar of its own: the marks another app left (its .xmp sidecar, or the
            // file's own XMP) are where it starts. A sidecar that cannot be read is not
            // absent, so it is not seeded over.
            Ok(None) => (
                photo_engine::xmp::seed_metadata(&photo_engine::copies::source_file(path))
                    .unwrap_or_default(),
                false,
            ),
            Err(_) => (
                photo_engine::AssetMetadata::default(),
                find_sidecar(path).is_some(),
            ),
        };
        let file = photo_engine::copies::source_file(path).into_owned();
        Self {
            path: path.to_path_buf(),
            metadata,
            has_sidecar,
            bytes: std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0),
            missing: !file.exists(),
        }
    }
}

impl ShelfItem {
    /// Shelved with its facts read now, on this thread: for one photo at a time.
    fn new(path: PathBuf) -> Self {
        let mut item = Self::waiting(path);
        item.take(ShelfFacts::read(&item.path));
        item
    }

    /// Shelved with only its name, its facts to follow.
    fn waiting(path: PathBuf) -> Self {
        let (sort_name, copy) = match photo_engine::copies::parse(&path) {
            Some((source, n)) => (file_name(&source), n),
            None => (file_name(&path), 1),
        };
        let name = file_name(&path);
        Self {
            search_name: name.to_lowercase(),
            name,
            sort_name,
            copy,
            is_edited: false,
            has_sidecar: false,
            metadata: photo_engine::AssetMetadata::default(),
            bytes: 0,
            missing: false,
            info: None,
            path,
            pending: true,
        }
    }

    fn take(&mut self, facts: ShelfFacts) {
        self.metadata = facts.metadata;
        self.has_sidecar = facts.has_sidecar;
        self.bytes = facts.bytes;
        self.missing = facts.missing;
        self.pending = false;
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
    /// Started on an uncropped frame: draws a crop from this corner.
    Draw { anchor: egui::Pos2 },
    /// Started on the crop: moves it (inside) or one edge or corner (on a handle).
    Grip {
        grip: crop_tool::Grip,
        grab: egui::Pos2,
        start: Crop,
        /// Pixel width over height to keep, if the aspect is fixed or Shift held.
        ratio: Option<f32>,
    },
    /// Started outside the crop: turns the photo (straighten) about the crop's centre.
    Rotate {
        centre: egui::Pos2,
        from_angle: f32,
        from_deg: f32,
    },
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
        /// The photo exported, as it was when the export began.
        source: Option<PathBuf>,
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
    /// Oldest first. A deque, so dropping the oldest at the cap is not a shift of all 256.
    past: std::collections::VecDeque<PhotoStack>,
    future: Vec<PhotoStack>,
    /// The oldest state has been dropped to stay under the cap, so `past[0]` is no longer
    /// the stack the file opened with.
    truncated: bool,
    /// Changes whenever `past` or `future` does, unique across every history: what the
    /// History list's labels are cached against (WP5.4).
    revision: u64,
}

/// A number no history has had yet.
fn fresh_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl History {
    fn new() -> Self {
        Self::from_past(Vec::new(), false)
    }

    fn from_past(past: Vec<PhotoStack>, truncated: bool) -> Self {
        Self {
            past: past.into(),
            future: Vec::new(),
            truncated,
            revision: fresh_revision(),
        }
    }

    fn changed(&mut self) {
        self.revision = fresh_revision();
    }

    /// Call before mutating. Coalescing is the caller's job: a slider drag pushes once, on
    /// drag start, not once per frame.
    fn checkpoint(&mut self, current: &PhotoStack) {
        self.changed();
        self.past.push_back(current.clone());
        if self.past.len() > 256 {
            self.past.pop_front();
            self.truncated = true;
        }
        self.future.clear();
    }

    fn undo(&mut self, current: &mut PhotoStack) -> bool {
        let Some(prev) = self.past.pop_back() else {
            return false;
        };
        self.changed();
        self.future.push(std::mem::replace(current, prev));
        true
    }

    fn redo(&mut self, current: &mut PhotoStack) -> bool {
        let Some(next) = self.future.pop() else {
            return false;
        };
        self.changed();
        self.past.push_back(std::mem::replace(current, next));
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
        self.changed();
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

/// A one-shot read of the photo, asked for by the UI and done where the GPU is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Sample {
    /// The white-balance eyedropper at this point, 0..1 of the uncropped frame.
    Pick(egui::Pos2),
    AutoWb,
    AutoTone,
    Upright(photo_engine::keystone::Upright),
    Tat(egui::Pos2, tat::TatMode, egui::Pos2),
}

/// What a notice's Undo button takes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoticeAction {
    PasteToAll,
    Remove,
    DeleteSnapshot,
    /// Share the photo just exported.
    Share,
    /// Stop reading the folder being opened.
    CancelScan,
    /// Show the skipped tour again on the next launch.
    TourAgain,
    /// Draw an edit saved before the current colour math with it.
    UpdateRendering,
}

impl NoticeAction {
    /// What its button says.
    pub(crate) fn label(self) -> &'static str {
        match self {
            NoticeAction::Share => "share",
            NoticeAction::CancelScan => "cancel",
            NoticeAction::TourAgain => "next launch",
            NoticeAction::UpdateRendering => "update",
            _ => "undo",
        }
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

type ShelfDisplayCache = Option<(
    usize,
    dam::FilterState,
    Option<std::path::PathBuf>,
    bool,
    Vec<usize>,
)>;

/// Where the photo expected next has got to (`PhotoApp::prefetch_next`).
enum Prefetch {
    Decoding,
    /// Prepared in the engine; how the camera meant it to be viewed.
    Ready(photo_engine::Geometry),
    /// Not to be tried again: it opens the ordinary way.
    Failed,
}

pub struct PhotoApp {
    engine: Engine,
    stack: PhotoStack,
    history: History,
    texture_id: Option<egui::TextureId>,
    /// The photo textures let go of this frame, freed at the start of the next.
    retired_textures: Vec<egui::TextureId>,
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
    /// The crop's fixed shape, if any, and the guide drawn over it.
    pub crop_aspect: crop_tool::Aspect,
    pub crop_guide: crop_tool::Guide,
    /// Where the whole frame was drawn on the stage last frame.
    stage_frame: Option<egui::Rect>,
    /// The Shelf: assets in the project/session.
    pub shelf: Vec<ShelfItem>,
    /// The history panel at the left is open, not folded to its edge (desktop).
    pub shelf_open: bool,
    /// The shelf under the stage (desktop): hidden, a strip or a grid.
    pub(crate) strip: StripMode,
    /// Bumped whenever items are added/removed/rated in the shelf to invalidate caches.
    pub shelf_epoch: usize,
    /// Cached (epoch, filter, loaded_path, is_edited, indices) to eliminate per-frame O(N log N) sorting.
    shelf_display_cache: std::cell::RefCell<ShelfDisplayCache>,
    /// Before/after split view toggle and position (0.0..=1.0).
    pub split_active: bool,
    pub split_pos: f32,
    /// In-memory clipboard for copying/pasting adjustment stacks across photos.
    pub clipboard_stack: Option<PhotoStack>,
    /// What paste, same-as-previous and paste-to-all carry (W-03).
    pub paste_set: paste::PasteSet,
    /// The paste picker: a dialog on the desktop, inline on the phone.
    pub paste_picker_open: bool,
    /// What is typed in a keywords field, not yet added.
    keyword_draft: String,
    /// Filters kept under a name, and the name being typed for a new one.
    pub saved_filters: Vec<dam::SavedFilter>,
    saved_filter_draft: Option<String>,
    folder_watch: dam::watch::FolderWatch,
    /// The local mask being placed and adjusted (W-01): its handles are on the stage.
    pub selected_mask: Option<usize>,
    /// Show the selected mask as a red wash on the preview.
    pub mask_overlay: bool,
    /// A mask handle being dragged: which, from where, and the mask as it was.
    mask_drag: Option<(mask_tool::Handle, egui::Pos2, photo_engine::local::Mask)>,
    /// The mask the engine is washing red, to notice a change.
    shown_mask: Option<usize>,
    /// How a brush mask is painted.
    pub brush: BrushTool,
    /// A brush stroke in progress: where its last dab went (sensor, 0..1).
    painting: Option<[f32; 2]>,
    /// Spot heal (W-02): a tap on the photo heals a spot there.
    pub healing: bool,
    /// The spot whose source is shown and can be moved.
    pub selected_spot: Option<usize>,
    /// A new spot's radius, percent of the long side.
    pub spot_size: f32,
    /// The preview as its finest detail, so dust shows.
    pub dust_view: bool,
    /// A spot to add once its source is found: where, and how big (sensor, long side).
    pending_spot: Option<([f32; 2], f32)>,
    /// A spot circle being dragged: its source's (true) or its own, from where, as it was.
    spot_drag: Option<(bool, egui::Pos2, photo_engine::heal::Spot)>,
    /// What the engine is showing, to notice a change.
    shown_dust: bool,
    /// The History list's labels for the history revision they were made from; the two
    /// beside the live stack are made fresh each frame.
    history_labels: (u64, Vec<String>),
    /// The screen the settings name (W-14), why a profile file could not be used, and
    /// what the engine was last told.
    screen: photo_engine::gamut::Screen,
    screen_note: Option<String>,
    /// The monitor's own profile, baked, when the screen is a profile file that bakes.
    screen_profile: Option<display::ScreenProfile>,
    /// What the shell found the system has for the window's monitor, once it has looked:
    /// its name and bytes, none, or why it could not ask. `None` where nothing looks.
    system_display: Option<display::SystemDisplay>,
    /// Counts each change of screen, so what was baked or shown for one can tell.
    screen_rev: u64,
    shown_screen_rev: u64,
    /// Soft proofing against the export's colour space (W-10), with the gamut warning.
    pub proofing: bool,
    pub proof_warn: bool,
    shown_proof: Option<photo_engine::engine::Proof>,
    /// The printer profile the proof goes through, if one is chosen and reads, and why
    /// the chosen one could not be used.
    printer: Option<display::Printer>,
    printer_note: Option<String>,
    /// The user's own presets, as their files say (W-04).
    pub user_presets: Vec<(PathBuf, user_presets::UserPreset)>,
    /// The name being typed for a preset about to be saved.
    pub preset_draft: Option<String>,
    /// A preset waiting for the export picker's answer.
    exporting_preset: Option<user_presets::UserPreset>,
    /// The open photo's named snapshots, kept in its sidecar.
    pub snapshots: Vec<(String, PhotoStack)>,
    /// Undo history for photos stepped away from, most recent last, so returning to one
    /// brings its Ctrl+Z back. Capped: see `session::HISTORY_KEPT`.
    parked_histories: Vec<(PathBuf, History)>,
    /// Files dropped on the window since the last frame; they are shelved together.
    pending_drops: Vec<PathBuf>,
    /// The stack as it stood the last time nothing was being dragged: what Esc puts back
    /// mid-drag (DESIGN.md: a drag has a live preview and a cancel).
    stack_before_drag: PhotoStack,
    /// A drag was held at the end of the last frame. egui itself drops a drag when Esc is
    /// pressed, before the app sees the key, so this is how the app knows one was cut off.
    was_dragging: bool,
    pub new_snapshot_name: String,
    /// Brief notifications in the status bar with timestamp.
    pub status_notification: Option<(String, Instant)>,
    /// What the live notice's Undo button does, if it has one.
    status_action: Option<NoticeAction>,
    /// Photos last taken off the shelf, with where they sat, for the notice's Undo.
    removed_from_shelf: Vec<(usize, ShelfItem)>,
    pub tonemap_3d_state: tonemap_3d::Tonemap3DState,
    pub inspector_map_state: inspector_map_3d::InspectorMapState,
    pub inspector_sections: Vec<InspectorSectionConfig>,
    /// The Inspector is showing its Customize tab rather than the pipeline.
    pub customize_open: bool,
    /// The Preferences window is open (desktop).
    pub(crate) preferences_open: bool,
    /// The inspector sections folded shut (desktop).
    pub(crate) shut_groups: HashSet<modules::ModuleGroup>,
    /// The tool chosen on the rail beside the inspector (desktop).
    pub(crate) tool: Tool,
    /// The modules a section's switch turned off, so switching it on again brings back
    /// exactly those. Forgotten once any module in the section is on again.
    pub(crate) bypassed: HashMap<modules::ModuleGroup, Vec<InspectorSectionId>>,
    pub inspector_width: f32,
    pub inspector_force_width: Option<f32>,
    pub cutout_studio: cutout_studio::CutoutStudioState,
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
    /// Thumbnail requests the workers have not answered yet.
    thumbs_pending: HashSet<PathBuf>,
    /// Shelved photos whose facts the loader has not sent yet, and folder reads out.
    shelf_pending: HashSet<PathBuf>,
    scans_pending: usize,
    /// The folder being opened or dropped, while it is read, and how many such reads
    /// are under way.
    reading: Option<String>,
    reads: usize,
    /// When each thumbnail was last drawn (a frame count), to evict the stalest (WP5.2).
    thumb_used: HashMap<PathBuf, u64>,
    frame_no: u64,
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
    /// The phone's list of the copies in its library, while it is open.
    pub storage: Option<dam::storage::StorageView>,
    pub batch_renamer: batch::Renamer,
    pub export_recipes: Vec<batch::ExportRecipe>,
    pub export_queue: Option<batch::ExportQueue>,
    pub batch_dialog_open: bool,
    /// The `?` sheet of every shortcut.
    pub shortcuts_open: bool,
    /// The Help window, and the topic it shows.
    pub(crate) help_open: bool,
    pub(crate) help_topic: usize,
    /// The desktop tour's step, while it shows, and where the parts it points at were.
    pub(crate) desk_tour_step: Option<usize>,
    pub(crate) desk_spots: help::DeskSpots,
    /// The welcome card the desktop shows on its first run, until the tour is taken or
    /// skipped.
    pub(crate) welcome_open: bool,
    /// Blown and crushed pixels painted on the stage (J).
    pub clip_overlay: bool,
    /// The white-balance eyedropper is armed (W): the next click on the photo picks.
    pub picking_wb: bool,
    pub(crate) pending_sample: Option<Sample>,
    /// The desktop Export dialog.
    pub export_dialog_open: bool,
    /// The phone Export sheet's settings, unfolded.
    pub export_options_open: bool,
    /// The batch recipe being edited, by index.
    pub(crate) editing_recipe: Option<usize>,
    /// The photo open before this one, for "Same as previous".
    pub(crate) previous_photo: Option<PathBuf>,
    /// The clipping overlay is being held on (the scope's clip readout, pressed).
    clip_held: bool,
    /// The snapshot last deleted, the photo it belonged to and where it stood.
    pub(crate) deleted_snapshot: Option<(PathBuf, usize, (String, PhotoStack))>,
    /// Which list the phone's Presets sheet shows.
    pub(crate) presets_view: compact::PresetsView,
    /// The desktop inspector's Looks band is open.
    pub looks_open: bool,
    /// The desktop inspector's Info band is open.
    pub info_open: bool,
    pub batch_output_dir: Option<PathBuf>,
    /// How the single export writes: format, quality, size, depth, sharpening, metadata.
    pub export_settings: photo_engine::ExportSettings,
    pub color_eq_tab: ColorEqTab,
    /// Targeted Adjustment Tool (TAT) mode and active on-image drag state.
    pub tat_mode: Option<tat::TatMode>,
    pub tat_drag: Option<tat::TatDrag>,
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
    /// The phone's bottom bar: which group of chips the Edit sheet shows.
    pub compact_category: categories::Category,
    /// The chip last chosen in each category, to come back to.
    compact_memory: [Option<categories::Chip>; categories::Category::ALL.len()],
    /// The phone layout's Library shows its rating, flag and search filters.
    pub library_filters_open: bool,
    /// A photo was asked for from outside the Library grid (a picker, a share); when it
    /// lands the phone layout goes straight to Edit, where the photo is worked on.
    edit_when_opened: bool,
    /// The Edit sheet shows the chosen tool's own controls; off, the ruler under its
    /// category's chips.
    pub compact_controls_open: bool,
    /// The phone's top-bar menu (⋯) is open.
    compact_more_open: bool,
    /// The height the portrait sheet's content wanted last frame; it gets that, up to a cap.
    compact_sheet_fit: Option<f32>,
    /// User-adjusted width for the landscape side sheet.
    pub compact_sheet_width: Option<f32>,
    /// Where a swipe across the stage began, to step photos when it ends.
    /// A photo step asked for where there was no `Gpu` to do it with.
    pending_step: Option<isize>,
    /// The library deletion under way is the exported copies' sweep: no notice for it.
    quiet_library_delete: bool,
    /// The photo to open in place of one just taken off the shelf, which happens where
    /// there is no `Gpu` to open it with.
    pending_open: Option<PathBuf>,
    /// The photo expected next, being made ready while this one is looked at.
    prefetch: Option<(PathBuf, Prefetch)>,
    /// The way the last step went (+1 or -1): the side the next photo is expected on.
    step_dir: isize,
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
    pub(crate) tour_step: Option<usize>,
    /// Where the app stood when the tour's step began.
    tour_mark: Option<compact::TourMark>,
    /// The tour card and what it points at, as last drawn.
    pub(crate) tour_card: Option<(egui::Rect, egui::Rect)>,
    /// Where the phone layout's parts were last frame, for the tour to point at.
    pub(crate) compact_rects: compact::Rects,
    /// A touch on the photo, and what it is doing to it (G-02, G-03).
    pub(crate) photo_touch: on_photo::PhotoTouch,
    /// The gesture exclusions last given the platform, in physical pixels.
    gestures_excluded: Vec<[i32; 4]>,
}

impl PhotoApp {
    pub fn new(gpu: &Gpu) -> Self {
        Self::with_platform(gpu, platform::default_platform())
    }

    pub fn with_platform(gpu: &Gpu, platform: Box<dyn platform::Platform>) -> Self {
        let mut engine = Engine::new(gpu.device);
        // A photo opens on a quick half-size picture, and RCD lands a slice a frame later:
        // no frame waits on a whole demosaic.
        engine.set_progressive(true);
        let mut folder_tree = dam::FolderTree::default();
        let bookmarks = platform.bookmarks();
        if !bookmarks.is_empty() {
            folder_tree.bookmarks = bookmarks;
        }
        let loader =
            loader::Loader::new(gpu.device.clone(), gpu.queue.clone(), engine.upload_pool());
        let settings_dir = platform.settings_dir();
        let settings = settings::Settings::load(settings_dir.as_deref());
        // A skipped tour asked to be shown again: once, now.
        let tour_step = settings.tour_again.then_some(0);
        // The desktop's first run welcomes and offers the tour. Only where settings are
        // kept: without them (tests, the screenshot tool) every run would be the first.
        let welcome_open = settings_dir.is_some() && !settings.desk_tour_done;
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
            retired_textures: Vec::new(),
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
            crop_aspect: crop_tool::Aspect::Free,
            crop_guide: crop_tool::Guide::default(),
            stage_frame: None,
            shelf: Vec::new(),
            shelf_open: false,
            strip: StripMode::default(),
            split_active: false,
            split_pos: 0.5,
            clipboard_stack: None,
            paste_set: paste::PasteSet::default(),
            paste_picker_open: false,
            keyword_draft: String::new(),
            saved_filters: Vec::new(),
            saved_filter_draft: None,
            folder_watch: Default::default(),
            selected_mask: None,
            mask_overlay: false,
            mask_drag: None,
            shown_mask: None,
            brush: BrushTool::default(),
            painting: None,
            healing: false,
            selected_spot: None,
            spot_size: 1.5,
            dust_view: false,
            pending_spot: None,
            spot_drag: None,
            shown_dust: false,
            history_labels: (0, Vec::new()),
            screen: photo_engine::gamut::Screen::SRGB,
            screen_note: None,
            screen_profile: None,
            system_display: None,
            screen_rev: 0,
            shown_screen_rev: 0,
            proofing: false,
            proof_warn: true,
            shown_proof: None,
            printer: None,
            printer_note: None,
            user_presets: Vec::new(),
            preset_draft: None,
            exporting_preset: None,
            snapshots: Vec::new(),
            parked_histories: Vec::new(),
            pending_drops: Vec::new(),
            stack_before_drag: PhotoStack::default(),
            was_dragging: false,
            new_snapshot_name: String::new(),
            status_notification: None,
            status_action: None,
            removed_from_shelf: Vec::new(),
            tonemap_3d_state: tonemap_3d::Tonemap3DState::default(),
            inspector_map_state: inspector_map_3d::InspectorMapState::default(),
            inspector_sections: InspectorSectionConfig::default_sections(),
            customize_open: false,
            preferences_open: false,
            shut_groups: HashSet::new(),
            tool: Tool::default(),
            bypassed: HashMap::new(),
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
            thumbs_pending: HashSet::new(),
            shelf_pending: HashSet::new(),
            scans_pending: 0,
            reading: None,
            reads: 0,
            thumb_used: HashMap::new(),
            frame_no: 0,
            stage_preview: None,
            split_dragging: false,
            reveal_section: None,
            selection: dam::SelectionModel::new(),
            filter: dam::FilterState::default(),
            shelf_epoch: 0,
            shelf_display_cache: std::cell::RefCell::new(None),
            folder_tree,
            storage: None,
            batch_renamer: batch::Renamer::default(),
            export_recipes: batch::ExportRecipe::default_recipes(),
            export_queue: Some(batch::ExportQueue::new(
                gpu.device.clone(),
                gpu.queue.clone(),
            )),
            batch_dialog_open: false,
            shortcuts_open: false,
            help_open: false,
            help_topic: 0,
            desk_tour_step: None,
            desk_spots: help::DeskSpots::default(),
            welcome_open,
            clip_overlay: false,
            picking_wb: false,
            pending_sample: None,
            export_dialog_open: false,
            export_options_open: false,
            editing_recipe: None,
            previous_photo: None,
            clip_held: false,
            deleted_snapshot: None,
            presets_view: compact::PresetsView::default(),
            looks_open: false,
            info_open: false,
            batch_output_dir: None,
            export_settings: photo_engine::ExportSettings::new(photo_engine::ExportFormat::Jpeg),
            color_eq_tab: ColorEqTab::default(),
            tat_mode: None,
            tat_drag: None,
            platform,
            select_mode: false,
            fine_adjust: false,
            touch_seen: false,
            compact_tab: Some(compact::Tab::Library),
            compact_module: InspectorSectionId::Exposure,
            compact_category: categories::Category::Tune,
            compact_memory: [None; categories::Category::ALL.len()],
            library_filters_open: false,
            edit_when_opened: false,
            compact_controls_open: false,
            compact_more_open: false,
            compact_sheet_width: None,
            compact_sheet_fit: None,
            pending_step: None,
            quiet_library_delete: false,
            pending_open: None,
            prefetch: None,
            step_dir: 1,
            layout: compact::Layout::Desktop,
            settings_dir,
            settings,
            compact_sections,
            preset: None,
            tour_step,
            tour_mark: None,
            tour_card: None,
            compact_rects: compact::Rects::default(),
            photo_touch: Default::default(),
            gestures_excluded: Vec::new(),
            cutout_studio: cutout_studio::CutoutStudioState::default(),
        };
        if unclean {
            app.set_status("Duckfoot did not close cleanly last time; edits up to then were saved");
        }
        app.restore_session(gpu);
        app
    }

    /// Put back how the app was left: arrangement, recipes, the shelf and the open photo.
    /// Photos no longer where they were are skipped.
    fn restore_session(&mut self, gpu: &Gpu) {
        let s = self.settings.clone();
        self.reload_user_presets();
        if let Some(sections) = s.inspector_sections {
            self.inspector_sections =
                settings::merge_sections_with(sections, InspectorSectionConfig::default_sections());
        }
        if let Some(single) = s.single_open {
            self.single_open = single;
        }
        if let Some(width) = s.inspector_width {
            self.inspector_width = width.clamp(240.0, 640.0);
        }
        if let Some(strip) = s.strip {
            self.strip = strip;
        }
        if let Some(open) = s.shelf_open {
            self.shelf_open = open;
        }
        if let Some(recipes) = s.export_recipes {
            self.export_recipes = recipes;
        }
        if let Some(pattern) = s.rename_pattern {
            self.batch_renamer.pattern = pattern;
        }
        if let Some(export) = s.export {
            self.export_settings = export;
        }
        if let Some(filter) = s.filter {
            self.filter = filter;
        }
        if let Some(set) = s.paste {
            self.paste_set = set;
        }
        self.saved_filters = s.saved_filters;
        // Settings already hold the screen; the matrix is worked out from them once.
        self.refresh_screen();
        self.refresh_printer();
        // Read on the loader's thread; each is watched once its listing is back.
        for dir in s.watched_folders {
            self.scans_pending += 1;
            self.loader.file_job(loader::FileJob::Scan {
                folder: dir,
                purpose: loader::ScanFor::Watch,
                depth: 0,
                epoch: None,
            });
        }
        self.batch_output_dir = s.batch_output_dir.filter(|d| d.is_dir());
        // Missing photos are shelved too, and shown as missing. One photo at a time starts
        // each session empty: the photo is chosen anew, from the gallery it lives in.
        let shelf = if self.single_photo() {
            // What was exported last time and still open when the app closed goes now.
            self.sweep_exported_copies();
            Vec::new()
        } else {
            s.shelf
        };
        if !shelf.is_empty() {
            self.add_to_shelf(gpu, shelf);
            if let Some(last) = s
                .last_open
                .filter(|p| photo_engine::copies::source_file(p).is_file())
            {
                self.open_path(gpu, last);
            }
            // A restored session opens where it was left, not on the Edit sheet.
            self.edit_when_opened = false;
        }
    }

    /// Record the session for the next launch. Cheap (a list of paths), and called when
    /// edits are flushed, so it is as current as the edits are.
    fn persist_session(&mut self) {
        if self.settings_dir.is_none() {
            return;
        }
        let single = self.single_photo();
        let s = &mut self.settings;
        s.inspector_sections = Some(self.inspector_sections.clone());
        s.single_open = Some(self.single_open);
        s.inspector_width = Some(self.inspector_width);
        s.shelf_open = Some(self.shelf_open);
        s.strip = Some(self.strip);
        s.export_recipes = Some(self.export_recipes.clone());
        s.rename_pattern = Some(self.batch_renamer.pattern.clone());
        s.filter = Some(self.filter.clone());
        s.export = Some(self.export_settings);
        s.paste = Some(self.paste_set.clone());
        s.saved_filters = self.saved_filters.clone();
        s.watched_folders = self.folder_watch.folders();
        if single {
            // Nothing to come back to: the next launch starts empty.
            s.shelf = Vec::new();
            s.last_open = None;
        } else {
            s.shelf = self.shelf.iter().map(|it| it.path.clone()).collect();
            s.last_open = self.loaded_path.clone();
        }
        self.save_settings();
    }

    /// Set the size of the whole interface and remember it.
    pub fn set_ui_scale(&mut self, scale: f32) {
        self.settings.ui_scale = scale.clamp(0.85, 1.5);
        self.save_settings();
    }

    /// Whether straightening keeps the crop on photographed pixels; remembered.
    pub fn set_constrain_crop(&mut self, on: bool) {
        self.settings.constrain_crop = on;
        self.save_settings();
    }

    /// A straightened photo's corners swing out of the frame; unless told otherwise, the
    /// crop is kept to what was photographed, moved in and only then shrunk. Part of
    /// whatever edit turned it, so it undoes with that edit.
    fn constrain_crop(&mut self) {
        let deg = self.stack.geometry.straighten_deg;
        if !self.settings.constrain_crop || deg == 0.0 {
            return;
        }
        let Some(frame) = self.frame_pixels() else {
            return;
        };
        let crop = self.stack.geometry.crop.sanitised();
        let fitted = crop_tool::constrained(crop, deg, frame);
        if fitted != crop {
            self.stack.geometry.crop = fitted;
            self.needs_render = true;
        }
    }

    /// Carry out a one-shot read, as one undo step.
    fn take_sample(&mut self, gpu: &Gpu, sample: Sample) {
        let (device, queue) = (gpu.device, gpu.queue);
        let before = self.stack.clone();
        let outcome = match sample {
            Sample::Pick(p) => self
                .engine
                .neutral_at(device, queue, &self.stack, (p.x, p.y))
                .map(|wb| match wb {
                    Some(wb) => {
                        self.stack.white_balance.params = wb;
                        self.stack.white_balance.enabled = true;
                        Ok("White balance picked")
                    }
                    None => {
                        Err("Nothing to balance there: pick something grey, not blown or black")
                    }
                }),
            Sample::AutoWb => self
                .engine
                .auto_white_balance(device, queue)
                .map(|wb| match wb {
                    Some(wb) => {
                        self.stack.white_balance.params = wb;
                        self.stack.white_balance.enabled = true;
                        Ok("Auto white balance")
                    }
                    None => Err("Not enough of the photo to judge its light by"),
                }),
            Sample::AutoTone => self
                .engine
                .auto_tone(device, queue, &self.stack)
                .map(|tone| match tone {
                    Some(tone) => {
                        self.stack.exposure.params.ev = tone.ev;
                        self.stack.exposure.enabled = true;
                        self.stack.tonal.params = tone.tonal;
                        self.stack.tonal.enabled = tone.tonal != Default::default();
                        Ok("Auto tone")
                    }
                    None => Err("Not enough of the photo to judge its tones by"),
                }),
            Sample::Upright(mode) => self
                .engine
                .auto_upright(device, queue, &self.stack, mode)
                .map(|g| match g {
                    Some(g) => {
                        self.stack.geometry = g;
                        Ok(match mode {
                            photo_engine::keystone::Upright::Level => "Levelled",
                            photo_engine::keystone::Upright::Vertical => "Verticals stood up",
                            photo_engine::keystone::Upright::Full => "Upright",
                        })
                    }
                    None => Err("Not enough straight lines in the photo to go by"),
                }),
            Sample::Tat(p, mode, cursor) => {
                let res = self
                    .engine
                    .sample_color_at(device, queue, &self.stack, (p.x, p.y));
                match res {
                    Ok(Some(rgb)) => {
                        let (hue, _sat, _luma) = tat::rgb_to_hsl(rgb[0], rgb[1], rgb[2]);
                        let band_name = tat::hue_to_band(hue);
                        let initial_val = match mode {
                            tat::TatMode::ColorEq(ColorEqTab::Hue) => self
                                .stack
                                .color_eq
                                .params
                                .bands()
                                .into_iter()
                                .find(|(n, _)| *n == band_name)
                                .map(|(_, b)| b.hue)
                                .unwrap_or(0.0),
                            tat::TatMode::ColorEq(ColorEqTab::Saturation) => self
                                .stack
                                .color_eq
                                .params
                                .bands()
                                .into_iter()
                                .find(|(n, _)| *n == band_name)
                                .map(|(_, b)| b.saturation)
                                .unwrap_or(0.0),
                            tat::TatMode::ColorEq(ColorEqTab::Luminance) => self
                                .stack
                                .color_eq
                                .params
                                .bands()
                                .into_iter()
                                .find(|(n, _)| *n == band_name)
                                .map(|(_, b)| b.luminance)
                                .unwrap_or(0.0),
                        };
                        self.stack.color_eq.enabled = true;
                        self.tat_drag = Some(tat::TatDrag {
                            band_name,
                            initial_val,
                            start_pos: cursor,
                            current_delta: 0.0,
                        });
                        Ok(Ok("Targeted Color Band identified"))
                    }
                    Ok(None) => Ok(Err("Could not sample color there")),
                    Err(e) => Err(e),
                }
            }
        };
        match outcome {
            Ok(Ok(msg)) => {
                if self.stack != before {
                    self.history.checkpoint(&before);
                    self.needs_render = true;
                }
                self.set_status(msg);
            }
            Ok(Err(msg)) => self.set_status(msg),
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    /// Where the whole frame (uncropped while framing a crop) was on the stage last frame.
    pub fn stage_frame(&self) -> Option<egui::Rect> {
        self.stage_frame
    }

    pub fn is_stack_edited(&self) -> bool {
        self.stack != self.baseline
    }

    /// The edit on the stage.
    pub fn stack(&self) -> &PhotoStack {
        &self.stack
    }

    /// One photo at a time, as on a phone: see [`platform::Platform::single_photo`].
    pub fn single_photo(&self) -> bool {
        self.platform.single_photo()
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

    /// Step back one edit — Ctrl+Z, and every Undo button. While a crop is being framed,
    /// undo abandons that framing rather than the step before it.
    /// Undo steps behind the open photo's current state.
    pub fn history_len(&self) -> usize {
        self.history.past.len()
    }

    pub fn undo(&mut self) -> bool {
        if self.cropping {
            self.cancel_crop();
            return true;
        }
        let moved = self.history.undo(&mut self.stack);
        self.needs_render |= moved;
        moved
    }

    /// Step forward one undone edit — Ctrl+Shift+Z, and every Redo button.
    pub fn redo(&mut self) -> bool {
        if self.cropping {
            return false;
        }
        let moved = self.history.redo(&mut self.stack);
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

    /// The tour's card and the rectangle it points at, as last drawn.
    pub fn tour_card(&self) -> Option<(egui::Rect, egui::Rect)> {
        self.tour_step.and(self.tour_card)
    }

    /// Steps in the tour.
    pub fn tour_len(&self) -> usize {
        compact::TOUR_LEN
    }

    /// Open the desktop Help window on topic `topic` (from 0, in the guide's order).
    pub fn show_help(&mut self, topic: usize) {
        self.open_help();
        self.help_topic = topic;
    }

    /// Show the desktop tour at `step` (from 0).
    pub fn show_desk_tour_step(&mut self, step: usize) {
        self.desk_tour_step = Some(step);
    }

    /// What the desktop tour's step points at, as last drawn; `None` when it points at
    /// nothing, or no tour shows.
    pub fn desk_tour_target(&self) -> Option<egui::Rect> {
        self.desk_tour_step.and_then(|s| self.desk_spot(s))
    }

    /// The first run's welcome card shows.
    pub fn welcome_showing(&self) -> bool {
        self.welcome_open
    }

    /// The desktop tour's step, while it shows.
    pub fn desk_tour_showing(&self) -> Option<usize> {
        self.desk_tour_step
    }

    /// Steps in the desktop tour.
    pub fn desk_tour_len(&self) -> usize {
        help::DESK_TOUR_LEN
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

    /// No export is being written.
    pub fn export_finished(&self) -> bool {
        !matches!(self.export, Export::Writing { .. })
    }

    /// Something is still being read or decoded. That includes the next photo being got
    /// ready while this one is open: a test that waits for quiet and then steps on would
    /// otherwise race it.
    pub fn is_busy(&self) -> bool {
        self.loading.is_some()
            || matches!(self.prefetch, Some((_, Prefetch::Decoding)))
            || !self.thumbs_pending.is_empty()
            || !self.shelf_pending.is_empty()
            || self.scans_pending > 0
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
        self.status_action = None;
    }

    /// A notice with an Undo button: DESIGN.md makes destructive actions undoable rather
    /// than confirmed, and for the ones outside the stack's own history this is the undo.
    pub(crate) fn set_status_undoable(&mut self, msg: impl Into<String>, action: NoticeAction) {
        self.set_status(msg);
        self.status_action = Some(action);
    }

    /// The notice to show now, if any, and when it comes down. Undoable notices stay up
    /// long enough to reach for the button.
    pub(crate) fn live_notice(&self) -> Option<(&str, Option<NoticeAction>, Duration)> {
        let (msg, when) = self.status_notification.as_ref()?;
        let life = if self.status_action.is_some() {
            Duration::from_secs(8)
        } else {
            Duration::from_secs(4)
        };
        let left = life.checked_sub(when.elapsed())?;
        Some((msg.as_str(), self.status_action, left))
    }

    /// The photo the stage shows a stand-in for (its thumbnail or embedded preview), for
    /// tests.
    #[doc(hidden)]
    pub fn stand_in_for_tests(&self) -> Option<&std::path::Path> {
        self.stage_preview.as_ref().map(|(p, _)| p.as_path())
    }

    /// The stage would show its empty state, for tests.
    #[doc(hidden)]
    pub fn stage_empty_for_tests(&self) -> bool {
        !self.photo_on_stage()
            && self
                .stage_preview
                .as_ref()
                .is_none_or(|(p, _)| self.loaded_path.as_ref() != Some(p))
    }

    /// The notice up now and whether it has a button, for tests.
    #[doc(hidden)]
    pub fn notice_for_tests(&self) -> Option<(String, Option<&'static str>)> {
        self.live_notice()
            .map(|(m, a, _)| (m.to_string(), a.map(NoticeAction::label)))
    }

    /// The screen the preview is finished for, and why a profile could not be used.
    #[doc(hidden)]
    pub fn screen_for_tests(&self) -> (photo_engine::gamut::Screen, Option<String>) {
        (self.screen, self.screen_note.clone())
    }

    /// Follow the system's profile for the monitor, or stop, as the "system" chip does.
    #[doc(hidden)]
    pub fn follow_system_display_for_tests(&mut self, on: bool) {
        self.settings.display_system = on;
        self.refresh_screen();
    }

    /// Press the notice's button, for tests.
    #[doc(hidden)]
    pub fn press_notice_for_tests(&mut self) {
        if let Some((_, Some(action), _)) = self.live_notice() {
            self.run_notice_action(action);
        }
    }

    pub(crate) fn run_notice_action(&mut self, action: NoticeAction) {
        match action {
            NoticeAction::PasteToAll => self.undo_paste_to_all(),
            NoticeAction::Remove => self.undo_remove_from_shelf(),
            NoticeAction::DeleteSnapshot => self.undo_delete_snapshot(),
            NoticeAction::CancelScan => self.cancel_reading(),
            NoticeAction::UpdateRendering => self.update_rendering(),
            NoticeAction::TourAgain => {
                self.settings.tour_again = true;
                self.save_settings();
                self.set_status(crate::i18n::tr("tour-again"));
            }
            NoticeAction::Share => {
                if let Export::Done { path, .. } = &self.export {
                    let path = path.clone();
                    self.platform.request(platform::Request::Share { path });
                }
            }
        }
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
        // A photo taken off the shelf while open: its pixels go with it.
        if self.loaded_path.is_none() && self.engine.image().is_some() {
            self.release_texture(gpu);
        }
        // The engine still holds the last photo while the chosen one decodes; rendering
        // it with the new photo's stack would draw one photo with another's edits.
        if self.ready_image().is_none() {
            return;
        }
        if self.engine.advance_demosaic(gpu.device, gpu.queue) {
            self.needs_render = true;
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

        if let Some(sample) = self.pending_sample.take() {
            self.take_sample(gpu, sample);
        }
        self.constrain_crop();

        // The red wash over the selected mask: a uniform, so a change is a re-render.
        let masks = &self.stack.local.params.masks;
        if self.selected_mask.is_some_and(|i| i >= masks.len()) {
            self.selected_mask = None;
        }
        let shown = self
            .selected_mask
            .filter(|_| self.mask_overlay && self.stack.local.enabled);
        if shown != self.shown_mask {
            self.engine.set_mask_overlay(shown);
            self.shown_mask = shown;
            self.needs_render = true;
        }
        self.sync_display(gpu);
        if self.dust_view != self.shown_dust {
            self.engine.set_dust_view(self.dust_view);
            self.shown_dust = self.dust_view;
            self.needs_render = true;
        }
        if self
            .selected_spot
            .is_some_and(|i| i >= self.stack.heal.params.spots.len())
        {
            self.selected_spot = None;
        }
        // A tapped spot: its source found on the GPU's copy of the sensor, then one edit.
        if let Some((dst, radius)) = self.pending_spot.take() {
            match self.engine.heal_source(gpu.device, gpu.queue, dst, radius) {
                Ok(src) => {
                    self.history.checkpoint(&self.stack);
                    self.stack.heal.enabled = true;
                    self.stack
                        .heal
                        .params
                        .spots
                        .push(photo_engine::heal::Spot::new(dst, src, radius));
                    self.selected_spot = Some(self.stack.heal.params.spots.len() - 1);
                    self.needs_render = true;
                }
                Err(e) => self.error = Some(format!("Could not heal there: {e:#}")),
            }
        }

        // The clipping overlay, toggled or held: a texture of its own, registered in place
        // of the preview's while it is on.
        let clip = self.clip_overlay || self.clip_held;
        if self.engine.set_clip_overlay(gpu.device, clip) {
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
        // Held on a phone: the original, in the frame it is edited in.
        if self.photo_touch.holding {
            let original = PhotoStack {
                geometry: self.stack.geometry,
                ..self.baseline.clone()
            };
            self.engine.render(gpu.device, gpu.queue, &original, None);
        } else {
            self.engine
                .render(gpu.device, gpu.queue, &self.stack, split_x);
        }
        self.render_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.needs_render = false;

        if self.texture_id.is_none() && self.ready_image().is_some() {
            if let Some(view) = self.engine.display_view() {
                let id = renderer_write(gpu).register_native_texture(
                    gpu.device,
                    view,
                    wgpu::FilterMode::Linear,
                );
                self.texture_id = Some(id);
                // The photo replaces its stand-in from this frame.
                self.stage_preview = None;
            }
        }
    }

    pub fn update(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        let ctx = ui.ctx().clone();
        self.frame_no += 1;
        // The text-size setting, and only it: egui's own Ctrl+/- zoom would fight it.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let scale = self.settings.ui_scale.clamp(0.85, 1.5);
        if (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        // Phones, and tablets held upright, get the navigation-bar layout (`compact`): the
        // desktop's side panels need a screen both wide and tall.
        // Measured before the text-size setting's zoom, so a larger text size on a desktop
        // window does not tip it into the phone's layout (WP5.6).
        let screen = ui.max_rect().size() * ctx.zoom_factor();
        self.layout = compact::Layout::for_size(screen);
        // Controls size for the last input used: a laptop with a touch screen gets finger-
        // sized controls after a tap and desktop ones again once the mouse moves.
        let (touched, moused) = ctx.input(|i| {
            let touched = i
                .raw
                .events
                .iter()
                .any(|e| matches!(e, egui::Event::Touch { .. }));
            let moved = i
                .raw
                .events
                .iter()
                .any(|e| matches!(e, egui::Event::PointerMoved(_)));
            (touched, moved && !touched)
        });
        if touched {
            self.touch_seen = true;
        } else if moused {
            self.touch_seen = false;
        }
        widgets::set_touch(&ctx, self.touch_seen || self.layout.is_compact());
        widgets::set_fine(&ctx, self.fine_adjust);
        for id in self.retired_textures.drain(..) {
            renderer_write(gpu).free_texture(&id);
        }
        self.loader.set_context(&ctx);
        self.clip_held = widgets::take_clip_hold(&ctx);
        self.poll_platform(gpu);
        self.show_reading();
        // A dialog's answer arrives by polling; keep polling while one is open.
        if self.platform.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.take_drops(gpu);
        self.poll_loader(&ctx, gpu);
        self.poll_folders(&ctx, gpu);
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
        self.prefetch_next();
        if self.engine.demosaic_pending() || self.engine.prepare_pending() {
            // The next slice, whether or not anything else moves.
            ctx.request_repaint();
        }

        if self.layout.is_compact() {
            self.compact_layout(ui, gpu, screen);
        } else {
            // Room to breathe on the desktop (the phone keeps its own, finger-sized). Panels
            // inherit this; a few set their own rhythm inside.
            {
                let s = ui.spacing_mut();
                s.item_spacing = vec2(10.0, 8.0);
                s.button_padding = vec2(12.0, 6.0);
            }
            // Panel order is layout order: full-width bars first, then the side panels,
            // then the filmstrip and stage toolbar, which only span the space left between.
            self.top_bar(ui);
            self.status_bar(ui);
            self.history_panel(ui);
            self.tool_rail(ui);
            self.inspector(ui);
            self.filmstrip(ui, gpu);
            self.stage_toolbar(ui);
            self.desk_spots.stage = Some(ui.available_rect_before_wrap());
            self.workspace(ui, gpu);
            self.batch_export_dialog(ui);
            self.export_dialog(&ctx);
            self.paste_picker(&ctx);
            self.shortcuts_overlay(&ctx);
            self.preferences_window(&ctx);
            self.help_window(&ctx);
            self.welcome(&ctx);
            self.desk_tour(&ctx);
        }
        let ppp = ctx.pixels_per_point();
        let excluded: Vec<[i32; 4]> = self
            .gesture_strips()
            .iter()
            .map(|r| {
                [
                    (r.left() * ppp).floor() as i32,
                    (r.top() * ppp).floor() as i32,
                    (r.right() * ppp).ceil() as i32,
                    (r.bottom() * ppp).ceil() as i32,
                ]
            })
            .collect();
        if excluded != self.gestures_excluded {
            self.platform.exclude_gestures(&excluded);
            self.gestures_excluded = excluded;
        }
        if let Some(dir) = self.pending_step.take() {
            self.step_photo(gpu, dir);
        }
        if let Some(path) = self.pending_open.take() {
            self.open_path(gpu, path);
        }
        self.was_dragging = ctx.dragged_id().is_some();
        if !self.was_dragging && self.stack_before_drag != self.stack {
            self.stack_before_drag = self.stack.clone();
        }
        self.autosave(&ctx);
    }

    /// Esc during a drag — a slider, a curve node, a 3D handle — puts the stack back as it
    /// was before the drag began, and takes back the undo step the drag opened.
    pub(crate) fn cancel_drag(&mut self, ctx: &egui::Context) -> bool {
        if !(self.was_dragging || ctx.dragged_id().is_some()) || self.cropping {
            return false;
        }
        ctx.stop_dragging();
        if self.stack != self.stack_before_drag {
            if self.history.past.back() == Some(&self.stack_before_drag) {
                self.history.past.pop_back();
                self.history.changed();
            }
            self.stack = self.stack_before_drag.clone();
            self.needs_render = true;
        }
        true
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
pub(crate) const WB_KELVIN_MIN: f32 = 2000.0;
pub(crate) const WB_KELVIN_MAX: f32 = 50000.0;

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
