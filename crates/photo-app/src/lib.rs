//! Barrel 1's UI. Knows about egui and the engine; knows nothing about windowing.
//!
//! The desktop bootstrap and the eventual Android bootstrap both drive this same type.

pub mod theme;
pub mod tonemap_3d;
pub mod inspector_map_3d;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use std::sync::RwLock;

use photo_engine::{
    find_sidecar, identity_nodes, load_sidecar, save_sidecar, Crop, CurveNode, Engine, PhotoStack,
    RawDecoder, RawloaderBackend,
};

/// Sections available in the Inspector sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum InspectorSectionId {
    Scope,
    Geometry,
    Snapshots,
    WhiteBalance,
    Exposure,
    Tonal,
    ToneCurve,
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
            Self::Snapshots,
            Self::WhiteBalance,
            Self::Exposure,
            Self::Tonal,
            Self::ToneCurve,
            Self::Filmic3D,
            Self::ColorBalance,
            Self::Vibrance,
            Self::Sharpen,
            Self::Vignette,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Scope => "inspector map & scope",
            Self::Geometry => "geometry",
            Self::Snapshots => "snapshots",
            Self::WhiteBalance => "white balance",
            Self::Exposure => "exposure",
            Self::Tonal => "highlights & shadows",
            Self::ToneCurve => "tone curve",
            Self::Filmic3D => "filmic rgb / 3d tonemap",
            Self::ColorBalance => "colour balance",
            Self::Vibrance => "vibrance & saturation",
            Self::Sharpen => "capture sharpen",
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
                collapsed: id == InspectorSectionId::Snapshots,
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
}

impl History {
    fn new() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
        }
    }

    /// Call before mutating. Coalescing is the caller's job: a slider drag pushes once, on
    /// drag start, not once per frame.
    fn checkpoint(&mut self, current: &PhotoStack) {
        self.past.push(current.clone());
        if self.past.len() > 256 {
            self.past.remove(0);
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
    decode_ms: f32,
    render_ms: f32,
    export: Export,
    view: View,
    /// Size the image panel got last frame, so "100%" can be resolved outside of it.
    last_panel: egui::Vec2,
    /// While true, dragging on the image defines a crop instead of panning.
    cropping: bool,
    /// Where a crop drag started, in normalised image coordinates.
    crop_anchor: Option<egui::Pos2>,
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
    pub customize_open: bool,
    pub inspector_width: f32,
    pub inspector_force_width: Option<f32>,
}

impl PhotoApp {
    pub fn new(gpu: &Gpu) -> Self {
        Self {
            engine: Engine::new(gpu.device),
            stack: PhotoStack::default(),
            history: History::new(),
            texture_id: None,
            needs_render: false,
            error: None,
            loaded_path: None,
            decode_ms: 0.0,
            render_ms: 0.0,
            export: Export::Idle,
            view: View::default(),
            last_panel: egui::Vec2::ZERO,
            cropping: false,
            crop_anchor: None,
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
            inspector_width: 288.0,
            inspector_force_width: None,
        }
    }

    /// Read the current frame back and write it, encoding off the UI thread.
    fn export_to(&mut self, gpu: &Gpu, path: PathBuf) {
        // Make sure what we are about to copy reflects the current stack (export never splits).
        self.engine.render(gpu.device, gpu.queue, &self.stack, None);

        let exported = match self
            .engine
            .export(gpu.device, gpu.queue, self.stack.geometry.crop)
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

    pub fn is_stack_edited(&self) -> bool {
        self.stack != PhotoStack::default()
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_notification = Some((msg.into(), Instant::now()));
    }

    pub fn save_current_sidecar(&mut self) {
        let Some(path) = &self.loaded_path else {
            return;
        };
        match save_sidecar(path, &self.stack) {
            Ok(p) => {
                if let Some(item) = self.shelf.iter_mut().find(|it| &it.path == path) {
                    item.has_sidecar = true;
                    item.is_edited = false;
                }
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("sidecar");
                self.set_status(format!("Saved sidecar: {name}"));
            }
            Err(e) => {
                self.error = Some(format!("Failed to save sidecar: {e:#}"));
            }
        }
    }

    pub fn revert_to_sidecar(&mut self) {
        let Some(path) = &self.loaded_path else {
            return;
        };
        match load_sidecar(path) {
            Ok(Some(saved)) => {
                self.history.checkpoint(&self.stack);
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
        self.stack = PhotoStack::default();
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
            }
        }
        self.set_status(format!("Applied adjustments to {count} photos in shelf"));
    }

    pub fn add_to_shelf(&mut self, gpu: &Gpu, paths: Vec<PathBuf>) {
        let open_first = self.loaded_path.is_none();
        let mut to_open = None;
        for path in paths {
            let has_sidecar = find_sidecar(&path).is_some();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("RAW")
                .to_string();
            if !self.shelf.iter().any(|it| it.path == path) {
                if open_first && to_open.is_none() {
                    to_open = Some(path.clone());
                }
                self.shelf.push(ShelfItem {
                    path,
                    name,
                    is_edited: false,
                    has_sidecar,
                });
            }
        }
        if let Some(first) = to_open {
            self.open_path(gpu, first);
        }
    }

    pub fn open_path(&mut self, gpu: &Gpu, path: PathBuf) {
        // Auto-save sidecar for previous photo if it was edited
        if let Some(current) = &self.loaded_path {
            if self.is_stack_edited() {
                let _ = save_sidecar(current, &self.stack);
            }
        }

        self.error = None;
        let started = Instant::now();

        match RawloaderBackend.decode_file(&path) {
            Ok(cfa) => {
                self.decode_ms = started.elapsed().as_secs_f32() * 1000.0;
                self.release_texture(gpu);

                if let Err(e) = self.engine.load(gpu.device, gpu.queue, cfa) {
                    // DESIGN.md: failures surface in the UI, with the filename.
                    self.error = Some(format!("{}: {e:#}", path.display()));
                    self.loaded_path = None;
                    return;
                }

                let has_sidecar = find_sidecar(&path).is_some();
                let initial_stack = match load_sidecar(&path) {
                    Ok(Some(s)) => {
                        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
                        self.set_status(format!("Loaded sidecar for {fname}"));
                        s
                    }
                    _ => PhotoStack::default(),
                };

                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("RAW")
                    .to_string();
                if let Some(existing) = self.shelf.iter_mut().find(|it| it.path == path) {
                    existing.has_sidecar = has_sidecar;
                    existing.is_edited = false;
                } else {
                    self.shelf.push(ShelfItem {
                        path: path.clone(),
                        name,
                        is_edited: false,
                        has_sidecar,
                    });
                }

                self.loaded_path = Some(path);
                self.stack = initial_stack;
                self.history = History::new();
                self.view = View::default();
                self.needs_render = true;
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                self.loaded_path = None;
            }
        }
    }

    fn release_texture(&mut self, gpu: &Gpu) {
        if let Some(id) = self.texture_id.take() {
            renderer_write(gpu).free_texture(&id);
        }
        self.engine.unload();
    }

    /// Runs the GPU work for this frame, if anything changed.
    fn sync(&mut self, gpu: &Gpu) {
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
        self.handle_keys(ui.ctx());
        self.poll_export(ui.ctx());
        self.engine.poll_histogram(gpu.device);
        self.sync(gpu);

        self.top_bar(ui, gpu);
        self.shelf(ui, gpu);
        self.inspector(ui);
        self.status_bar(ui);
        self.workspace(ui);
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // DESIGN.md: hotkeys never fire while a text field has focus.
        if ctx.memory(|m| m.focused().is_some()) {
            return;
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

        let (save, copy, paste, split_toggle) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.modifiers.command && i.key_pressed(egui::Key::C),
                i.modifiers.command && i.key_pressed(egui::Key::V),
                i.key_pressed(egui::Key::Backslash),
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

        let (fit, actual) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Num0),
                i.key_pressed(egui::Key::Num1),
            )
        });
        if fit {
            self.view = View::default();
        }
        if actual {
            // 1:1 is expressed relative to fit, which depends on the panel size, so it is
            // resolved against the screen rect the workspace actually got last frame.
            if let Some(percent) = self.zoom_percent(self.last_panel) {
                if percent > 0.0 {
                    self.view.zoom *= 100.0 / percent;
                    self.view.zoom = self.view.zoom.max(1.0);
                }
            }
        }

        let (rot_left, rot_right, flip_h, flip_v, crop_toggle, crop_esc) = ctx.input(|i| {
            (
                !i.modifiers.command && i.key_pressed(egui::Key::OpenBracket),
                !i.modifiers.command && i.key_pressed(egui::Key::CloseBracket),
                !i.modifiers.command && i.key_pressed(egui::Key::H),
                !i.modifiers.command && i.key_pressed(egui::Key::V),
                !i.modifiers.command && i.key_pressed(egui::Key::C),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if rot_left {
            let before = self.stack.clone();
            self.stack.geometry.rotate_left();
            self.history.checkpoint(&before);
            self.needs_render = true;
        }
        if rot_right {
            let before = self.stack.clone();
            self.stack.geometry.rotate_right();
            self.history.checkpoint(&before);
            self.needs_render = true;
        }
        if flip_h {
            let before = self.stack.clone();
            self.stack.geometry.flip_h();
            self.history.checkpoint(&before);
            self.needs_render = true;
        }
        if flip_v {
            let before = self.stack.clone();
            self.stack.geometry.flip_v();
            self.history.checkpoint(&before);
            self.needs_render = true;
        }
        if crop_toggle {
            self.cropping = !self.cropping;
        }
        if crop_esc && self.cropping {
            self.cropping = false;
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        egui::Panel::top("top")
            .frame(bare_frame(theme::BG_900))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("DUCKFOOT")
                            .monospace()
                            .color(theme::ACCENT)
                            .strong(),
                    );
                    ui.add_space(12.0);

                    // The other two barrels exist in the design, not in the code. Showing
                    // them disabled is honest; showing them clickable would not be.
                    let _ = ui.selectable_label(true, "Photo");
                    ui.add_enabled(false, egui::Button::new(theme::dim("Audio")));
                    ui.add_enabled(false, egui::Button::new(theme::dim("Video")));

                    ui.separator();
                    if ui.selectable_label(self.shelf_open, "Shelf").clicked() {
                        self.shelf_open = !self.shelf_open;
                    }

                    if ui.button("Open RAW…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Camera RAW", &photo_engine::raw_dialog_extensions())
                            // A fallback the filter cannot lock the user out of: the
                            // extension list will never cover every camera.
                            .add_filter("All files", &["*"])
                            .pick_file()
                        {
                            self.open_path(gpu, path);
                        }
                    }

                    let busy = matches!(self.export, Export::Writing { .. });
                    let ready = self.engine.image().is_some() && !busy;
                    if ui
                        .add_enabled(ready, egui::Button::new("Export…"))
                        .clicked()
                    {
                        let (dir, name) = self.export_suggestion();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("PNG", &["png"])
                            .add_filter("JPEG", &["jpg", "jpeg"])
                            .set_directory(dir)
                            .set_file_name(name)
                            .save_file()
                        {
                            self.export_to(gpu, path);
                        }
                    }

                    ui.separator();

                    // Before / After split view toggle and control
                    if ui
                        .selectable_label(self.split_active, "Split (\\)")
                        .on_hover_text("Toggle Before/After split view (\\)")
                        .clicked()
                    {
                        self.split_active = !self.split_active;
                        self.needs_render = true;
                    }

                    if self.split_active {
                        let slider = ui.add(
                            egui::Slider::new(&mut self.split_pos, 0.0..=1.0)
                                .text("divider")
                                .show_value(false),
                        );
                        if slider.changed() {
                            self.needs_render = true;
                        }
                        if ui
                            .small_button("50%")
                            .on_hover_text("Reset split to center")
                            .clicked()
                        {
                            self.split_pos = 0.5;
                            self.needs_render = true;
                        }
                    }

                    if ui
                        .add_enabled(
                            self.loaded_path.is_some(),
                            egui::Button::new("Save Sidecar"),
                        )
                        .on_hover_text("Save adjustments to companion sidecar file (Ctrl+S)")
                        .clicked()
                    {
                        self.save_current_sidecar();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(image) = self.engine.image() {
                            ui.label(theme::dim(format!("{} {}", image.make, image.model)));
                        }
                    });
                });
            });
    }

    fn shelf(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        if !self.shelf_open {
            return;
        }
        egui::Panel::left("shelf")
            .frame(bare_frame(theme::BG_950))
            .resizable(true)
            .default_size(216.0)
            .min_size(160.0)
            .max_size(360.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(theme::head("SHELF"));
                    ui.label(theme::dim(format!("({})", self.shelf.len())));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("+ Add…")
                            .on_hover_text("Add RAW photos to shelf")
                            .clicked()
                        {
                            if let Some(paths) = rfd::FileDialog::new()
                                .add_filter("Camera RAW", &photo_engine::raw_dialog_extensions())
                                .add_filter("All files", &["*"])
                                .pick_files()
                            {
                                self.add_to_shelf(gpu, paths);
                            }
                        }
                    });
                });
                ui.separator();

                let mut to_open = None;
                let mut to_remove = None;

                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 70.0)
                    .show(ui, |ui| {
                        for (i, item) in self.shelf.iter().enumerate() {
                            let is_active = self.loaded_path.as_ref() == Some(&item.path);
                            ui.horizontal(|ui| {
                                let text_color = if is_active {
                                    theme::ACCENT
                                } else {
                                    theme::TEXT
                                };
                                let label_text = egui::RichText::new(&item.name).color(text_color);
                                let btn = ui.selectable_label(is_active, label_text);
                                if btn.clicked() && !is_active {
                                    to_open = Some(item.path.clone());
                                }

                                if item.has_sidecar {
                                    ui.label(
                                        egui::RichText::new("●").color(theme::ACCENT).size(9.0),
                                    )
                                    .on_hover_text("Sidecar saved on disk");
                                } else if item.is_edited {
                                    ui.label(
                                        egui::RichText::new("●").color(theme::TEXT_DIM).size(9.0),
                                    )
                                    .on_hover_text("Edits in memory");
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .small_button("×")
                                            .on_hover_text("Remove from shelf")
                                            .clicked()
                                        {
                                            to_remove = Some(i);
                                        }
                                    },
                                );
                            });
                        }

                        if self.shelf.is_empty() {
                            ui.add_space(20.0);
                            ui.label(theme::dim(
                                "Shelf is empty.\nClick '+ Add…' or drag\nRAW files onto window.",
                            ));
                        }
                    });

                if let Some(path) = to_open {
                    self.open_path(gpu, path);
                }
                if let Some(idx) = to_remove {
                    self.shelf.remove(idx);
                }

                ui.separator();
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button("Copy Stack")
                            .on_hover_text("Copy current adjustments (Ctrl+C)")
                            .clicked()
                        {
                            self.copy_adjustments();
                        }
                        let has_clip = self.clipboard_stack.is_some();
                        if ui
                            .add_enabled(has_clip, egui::Button::new("Paste Stack"))
                            .on_hover_text("Paste adjustments (Ctrl+V)")
                            .clicked()
                        {
                            self.paste_adjustments();
                        }
                    });
                    if self.clipboard_stack.is_some()
                        && !self.shelf.is_empty()
                        && ui
                            .button("Apply to All in Shelf")
                            .on_hover_text("Save copied adjustments to sidecar for all shelf items")
                            .clicked()
                    {
                        self.paste_to_all_in_shelf();
                    }
                });
            });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status")
            .frame(bare_frame(theme::BG_900))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(err) = &self.error {
                        ui.colored_label(theme::DANGER, err);
                        return;
                    }
                    match self.engine.image() {
                        Some(image) => {
                            ui.label(theme::num(format!("{}×{}", image.width, image.height)));
                            ui.separator();
                            ui.label(theme::num(format!("{:.1} MP", image.megapixels())));
                            ui.separator();
                            ui.label(theme::dim("decode"));
                            ui.label(theme::num(format!("{:.0} ms", self.decode_ms)));
                            ui.separator();
                            ui.label(theme::dim("adjust submit"));
                            ui.label(theme::num(format!("{:.2} ms", self.render_ms)));
                            if let Some(percent) = self.zoom_percent(self.last_panel) {
                                ui.separator();
                                ui.label(theme::dim("zoom"));
                                ui.label(theme::num(format!("{percent:.0}%")));
                            }

                            match &self.export {
                                Export::Idle => {}
                                Export::Writing { path, started, .. } => {
                                    ui.separator();
                                    ui.label(theme::dim("writing"));
                                    ui.label(theme::num(format!(
                                        "{}  {:.1}s",
                                        file_name(path),
                                        started.elapsed().as_secs_f32()
                                    )));
                                }
                                Export::Done { path, seconds } => {
                                    ui.separator();
                                    ui.label(theme::dim("exported"));
                                    ui.label(theme::num(format!(
                                        "{}  {seconds:.1}s",
                                        file_name(path)
                                    )));
                                }
                                Export::Failed(e) => {
                                    ui.separator();
                                    ui.colored_label(theme::DANGER, e);
                                }
                            }

                            if let Some((msg, when)) = &self.status_notification {
                                if when.elapsed().as_secs_f32() < 4.0 {
                                    ui.separator();
                                    ui.colored_label(theme::ACCENT, msg);
                                }
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if let Some(path) = &self.loaded_path {
                                        let sidecar_label = if find_sidecar(path).is_some() {
                                            if self.is_stack_edited() {
                                                theme::dim("● sidecar (modified)")
                                            } else {
                                                theme::dim("● sidecar (synced)")
                                            }
                                        } else if self.is_stack_edited() {
                                            theme::dim("○ unsaved edits")
                                        } else {
                                            theme::dim("○ default")
                                        };
                                        ui.label(sidecar_label);
                                        ui.separator();
                                        ui.label(theme::num(file_name(path)));
                                    }
                                },
                            );
                        }
                        None => {
                            ui.label(theme::dim("no image — Open RAW to begin"));
                        }
                    }
                });
            });
    }

    fn workspace(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(bare_frame(theme::BG_950))
            .show(ui, |ui| {
                let (Some(id), Some(image)) = (self.texture_id, self.engine.image()) else {
                    ui.centered_and_justified(|ui| {
                        ui.label(theme::dim("Open a camera RAW file"));
                    });
                    return;
                };

                let img = egui::vec2(image.width as f32, image.height as f32);
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

                if self.cropping {
                    // In crop mode a drag frames the picture instead of moving it.
                    let to_norm = |p: egui::Pos2| {
                        egui::pos2(
                            ((p.x - dest_now.left()) / dest_now.width()).clamp(0.0, 1.0),
                            ((p.y - dest_now.top()) / dest_now.height()).clamp(0.0, 1.0),
                        )
                    };
                    if response.drag_started() {
                        self.crop_anchor = response.interact_pointer_pos().map(to_norm);
                    }
                    if let (Some(anchor), Some(now)) = (
                        self.crop_anchor,
                        response.interact_pointer_pos().map(to_norm),
                    ) {
                        if response.dragged() || response.drag_stopped() {
                            let crop = Crop {
                                x: anchor.x.min(now.x),
                                y: anchor.y.min(now.y),
                                width: (now.x - anchor.x).abs(),
                                height: (now.y - anchor.y).abs(),
                            }
                            .sanitised();
                            if crop != self.stack.geometry.crop {
                                if response.drag_started() {
                                    self.history.checkpoint(&self.stack);
                                }
                                self.stack.geometry.crop = crop;
                                self.needs_render = true;
                            }
                        }
                    }
                    if response.drag_stopped() {
                        self.crop_anchor = None;
                    }
                } else if response.dragged() {
                    self.view.pan -= response.drag_delta() / scale_now;
                }

                if let Some(pointer) = response.hover_pos() {
                    let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        let before = fit * self.view.zoom;
                        let centre = rect.center() - self.view.pan * before;
                        // The image point under the cursor, in image pixels from centre.
                        let anchor = (pointer - centre) / before;

                        self.view.zoom =
                            (self.view.zoom * (scroll * 0.004).exp()).clamp(1.0, max_zoom);

                        // Keep that point under the cursor rather than zooming to centre.
                        let after = fit * self.view.zoom;
                        self.view.pan = (rect.center() - pointer) / after + anchor;
                    }
                }

                let scale = fit * self.view.zoom;
                // Never let the image be dragged entirely out of view.
                let limit = img * 0.5;
                self.view.pan = self.view.pan.clamp(-limit, limit);

                let dest = egui::Rect::from_center_size(
                    rect.center() - self.view.pan * scale,
                    img * scale,
                );
                ui.painter().with_clip_rect(rect).image(
                    id,
                    dest,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );

                // The crop is not baked into the texture, so it is drawn: everything
                // outside it is dimmed, and thirds are marked while framing.
                let crop = self.stack.geometry.crop.sanitised();
                if self.cropping || !crop.is_full_frame() {
                    let framed = egui::Rect::from_min_size(
                        dest.min + egui::vec2(crop.x * dest.width(), crop.y * dest.height()),
                        egui::vec2(crop.width * dest.width(), crop.height * dest.height()),
                    );
                    let painter = ui.painter().with_clip_rect(rect);
                    let veil = egui::Color32::from_black_alpha(150);
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
                    if self.cropping {
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
                    }
                }

                if self.split_active {
                    let painter = ui.painter().with_clip_rect(rect);
                    let font = egui::FontId::monospace(11.0);
                    if self.split_pos > 0.12 {
                        painter.text(
                            dest.left_top() + egui::vec2(12.0, 12.0),
                            egui::Align2::LEFT_TOP,
                            "BEFORE (RAW)",
                            font.clone(),
                            theme::TEXT_DIM,
                        );
                    }
                    if self.split_pos < 0.88 {
                        painter.text(
                            dest.right_top() + egui::vec2(-12.0, 12.0),
                            egui::Align2::RIGHT_TOP,
                            "AFTER (ADJUSTED)",
                            font,
                            theme::ACCENT,
                        );
                    }

                    let split_screen_x = dest.left() + self.split_pos * dest.width();
                    if let Some(pointer) = response.hover_pos() {
                        if (pointer.x - split_screen_x).abs() < 14.0
                            && dest.contains(pointer)
                            && !self.cropping
                        {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            if ui.input(|i| i.pointer.primary_down()) {
                                let new_fraction =
                                    ((pointer.x - dest.left()) / dest.width()).clamp(0.0, 1.0);
                                if (new_fraction - self.split_pos).abs() > 0.001 {
                                    self.split_pos = new_fraction;
                                    self.needs_render = true;
                                }
                            }
                        }
                    }
                }

                if self.cropping {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                } else if response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if self.view.zoom > 1.0 && response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
            });
    }

    /// Display scale as a percentage of 1:1, for the status bar.
    fn zoom_percent(&self, panel: egui::Vec2) -> Option<f32> {
        let image = self.engine.image()?;
        let img = egui::vec2(image.width as f32, image.height as f32);
        if panel.x < 1.0 || panel.y < 1.0 {
            return None;
        }
        let fit = (panel.x / img.x).min(panel.y / img.y);
        Some(fit * self.view.zoom * 100.0)
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let mut panel = egui::Panel::right("inspector")
            .frame(bare_frame(theme::BG_900))
            .resizable(true)
            .default_size(self.inspector_width)
            .min_size(240.0)
            .max_size(850.0);

        if let Some(target_w) = self.inspector_force_width.take() {
            panel = panel.exact_size(target_w);
        }

        let inner = panel.show(ui, |ui| {
            // Header pinned at top
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(theme::head("INSPECTOR"));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cust_label = if self.customize_open { "Done" } else { "⚙ Custom" };
                    if ui
                        .selectable_label(self.customize_open, cust_label)
                        .on_hover_text("Customize visible modules and change ordering")
                        .clicked()
                    {
                        self.customize_open = !self.customize_open;
                    }

                    if ui
                        .small_button("↕ All")
                        .on_hover_text("Expand all or collapse all sidebar sections")
                        .clicked()
                    {
                        let any_collapsed = self.inspector_sections.iter().any(|s| s.collapsed);
                        for s in &mut self.inspector_sections {
                            s.collapsed = !any_collapsed;
                        }
                    }

                    let is_wide = self.inspector_width >= 400.0;
                    let width_label = if is_wide { "⤹ Narrow" } else { "⤢ Expand" };
                    if ui
                        .small_button(width_label)
                        .on_hover_text(if is_wide {
                            "Compact sidebar width (288px)"
                        } else {
                            "Expand sidebar for large 3D views & precise control (460px)"
                        })
                        .clicked()
                    {
                        let target = if is_wide { 288.0 } else { 460.0 };
                        self.inspector_width = target;
                        self.inspector_force_width = Some(target);
                    }
                });
            });
            ui.add_space(4.0);

            // Customize drawer when open
            if self.customize_open {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("CUSTOMIZE SIDEBAR").size(10.5).color(theme::ACCENT));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Reset Order").on_hover_text("Restore default pipeline ordering").clicked() {
                                self.inspector_sections = InspectorSectionConfig::default_sections();
                            }
                            if ui.small_button("Show All").on_hover_text("Show all modules").clicked() {
                                for s in &mut self.inspector_sections {
                                    s.visible = true;
                                }
                            }
                            if ui.small_button("Essentials").on_hover_text("Show only essential modules").clicked() {
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
                    });
                    ui.separator();

                    let mut move_act = None;
                    let total = self.inspector_sections.len();
                    for (i, sec) in self.inspector_sections.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut sec.visible, "");
                            let title_text = if sec.visible {
                                egui::RichText::new(sec.id.title()).color(theme::TEXT)
                            } else {
                                theme::dim(sec.id.title())
                            };
                            ui.label(title_text);

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add_enabled(i + 1 < total, egui::Button::new("▼").small()).clicked() {
                                    move_act = Some((i, 1));
                                }
                                if ui.add_enabled(i > 0, egui::Button::new("▲").small()).clicked() {
                                    move_act = Some((i, -1));
                                }
                            });
                        });
                    }

                    if let Some((idx, dir)) = move_act {
                        let new_idx = (idx as isize + dir) as usize;
                        if new_idx < self.inspector_sections.len() {
                            self.inspector_sections.swap(idx, new_idx);
                        }
                    }
                });
                ui.add_space(4.0);
            }

            ui.separator();

            if self.engine.image().is_none() {
                ui.label(theme::dim("no image"));
                return;
            }

            // ScrollArea for the entire inspector sidebar:
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut move_action: Option<(usize, isize)> = None;
                    let total = self.inspector_sections.len();

                    let mut before: Option<PhotoStack> = None;
                    let mut changed = false;
                    let mut started = false;

                    macro_rules! track {
                        ($r:expr) => {{
                            let r = $r;
                            if r.drag_started() || (r.changed() && !r.dragged()) {
                                started = true;
                            }
                            if r.changed() {
                                changed = true;
                            }
                        }};
                    }

                    before.get_or_insert_with(|| self.stack.clone());
                    let snapshot = before.take().unwrap();
                    let before_geometry = self.stack.clone();
                    let mut geometry_changed = false;

                    let section_metas: Vec<(usize, InspectorSectionId, bool, bool)> = self
                        .inspector_sections
                        .iter()
                        .enumerate()
                        .map(|(i, s)| (i, s.id, s.visible, s.collapsed))
                        .collect();

                    for (idx, id, visible, mut collapsed) in section_metas {
                        if !visible {
                            continue;
                        }

                        let can_up = idx > 0;
                        let can_down = idx + 1 < total;

                        match id {
                            InspectorSectionId::Scope => {
                                let (up, down) = section_header_row(ui, id.title(), &mut collapsed, None, can_up, can_down);
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed {
                                    ui.add_space(2.0);
                                    inspector_map_3d::render_inspector_map(
                                        ui,
                                        &mut self.inspector_map_state,
                                        self.engine.histogram(),
                                    );
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::Geometry => {
                                let (up, down) = section_header_row(ui, id.title(), &mut collapsed, None, can_up, can_down);
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed {
                                    ui.add_space(2.0);
                                    let g = &mut self.stack.geometry;
                                    ui.horizontal(|ui| {
                                        if geometry_icon_button(ui, GeometryIcon::RotateLeft, false, "Rotate 90° CCW ([)").clicked() {
                                            g.rotate_left();
                                            geometry_changed = true;
                                        }
                                        if geometry_icon_button(ui, GeometryIcon::RotateRight, false, "Rotate 90° CW (])").clicked() {
                                            g.rotate_right();
                                            geometry_changed = true;
                                        }
                                        if geometry_icon_button(ui, GeometryIcon::MirrorH, g.mirror_h, "Flip horizontally (H)").clicked() {
                                            g.flip_h();
                                            geometry_changed = true;
                                        }
                                        if geometry_icon_button(ui, GeometryIcon::MirrorV, g.mirror_v, "Flip vertically (V)").clicked() {
                                            g.flip_v();
                                            geometry_changed = true;
                                        }
                                    });

                                    let straighten = ui.add(
                                        egui::Slider::new(&mut g.straighten_deg, -15.0..=15.0)
                                            .text("straighten°")
                                            .fixed_decimals(2),
                                    );
                                    if straighten.drag_started() {
                                        self.history.checkpoint(&before_geometry);
                                    }
                                    if straighten.changed() {
                                        self.needs_render = true;
                                    }

                                    ui.horizontal(|ui| {
                                        let cropping = self.cropping;
                                        if ui.selectable_label(cropping, "crop").clicked() {
                                            self.cropping = !cropping;
                                        }
                                        let g = &mut self.stack.geometry;
                                        if ui
                                            .add_enabled(!g.crop.is_full_frame(), egui::Button::new("reset"))
                                            .clicked()
                                        {
                                            g.crop = Crop::default();
                                            geometry_changed = true;
                                        }
                                    });
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::Snapshots => {
                                let (up, down) = section_header_row(ui, id.title(), &mut collapsed, None, can_up, can_down);
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed {
                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        ui.text_edit_singleline(&mut self.new_snapshot_name);
                                        if ui
                                            .button("+ Add")
                                            .on_hover_text("Save current stack as a snapshot")
                                            .clicked()
                                        {
                                            let name = if self.new_snapshot_name.trim().is_empty() {
                                                format!("Snapshot {}", self.snapshots.len() + 1)
                                            } else {
                                                self.new_snapshot_name.trim().to_string()
                                            };
                                            self.snapshots.push((name, self.stack.clone()));
                                            self.new_snapshot_name.clear();
                                        }
                                    });
                                    let mut restore_idx = None;
                                    let mut delete_idx = None;
                                    for (s_i, (name, _)) in self.snapshots.iter().enumerate() {
                                        ui.horizontal(|ui| {
                                            if ui
                                                .button(name)
                                                .on_hover_text("Restore this snapshot")
                                                .clicked()
                                            {
                                                restore_idx = Some(s_i);
                                            }
                                            if ui
                                                .small_button("×")
                                                .on_hover_text("Delete snapshot")
                                                .clicked()
                                            {
                                                delete_idx = Some(s_i);
                                            }
                                        });
                                    }
                                    if let Some(s_i) = restore_idx {
                                        self.history.checkpoint(&self.stack);
                                        self.stack = self.snapshots[s_i].1.clone();
                                        self.needs_render = true;
                                    }
                                    if let Some(s_i) = delete_idx {
                                        self.snapshots.remove(s_i);
                                    }
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::WhiteBalance => {
                                let (up, down) = section_header_row(
                                    ui,
                                    id.title(),
                                    &mut collapsed,
                                    Some((&mut self.stack.white_balance.enabled, &mut changed)),
                                    can_up,
                                    can_down,
                                );
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed && self.stack.white_balance.enabled {
                                    ui.add_space(2.0);
                                    let p = &mut self.stack.white_balance.params;
                                    let (mut kelvin, mut tint) = rgb_to_kelvin_tint(p.tint_r, p.tint_b);
                                    let mut kt_changed = false;
                                    let k_slider = ui.add(
                                        egui::Slider::new(&mut kelvin, 2000.0..=12000.0)
                                            .text("temp (K)")
                                            .fixed_decimals(0),
                                    );
                                    if k_slider.drag_started() {
                                        started = true;
                                    }
                                    if k_slider.changed() {
                                        kt_changed = true;
                                    }

                                    let t_slider = ui.add(
                                        egui::Slider::new(&mut tint, -50.0..=50.0)
                                            .text("tint")
                                            .fixed_decimals(1),
                                    );
                                    if t_slider.drag_started() {
                                        started = true;
                                    }
                                    if t_slider.changed() {
                                        kt_changed = true;
                                    }

                                    if kt_changed {
                                        let (r, b) = kelvin_tint_to_rgb(kelvin, tint);
                                        p.tint_r = r;
                                        p.tint_b = b;
                                        changed = true;
                                    }

                                    ui.horizontal_wrapped(|ui| {
                                        if ui.small_button("As-Shot").clicked() {
                                            p.tint_r = 1.0;
                                            p.tint_b = 1.0;
                                            changed = true;
                                            started = true;
                                        }
                                        if ui
                                            .small_button("Daylight")
                                            .on_hover_text("5500K daylight")
                                            .clicked()
                                        {
                                            p.tint_r = 1.05;
                                            p.tint_b = 0.95;
                                            changed = true;
                                            started = true;
                                        }
                                        if ui
                                            .small_button("Cloudy")
                                            .on_hover_text("6500K overcast")
                                            .clicked()
                                        {
                                            p.tint_r = 1.15;
                                            p.tint_b = 0.88;
                                            changed = true;
                                            started = true;
                                        }
                                        if ui
                                            .small_button("Shade")
                                            .on_hover_text("7500K deep shade")
                                            .clicked()
                                        {
                                            p.tint_r = 1.28;
                                            p.tint_b = 0.80;
                                            changed = true;
                                            started = true;
                                        }
                                        if ui
                                            .small_button("Tungsten")
                                            .on_hover_text("3200K incandescent")
                                            .clicked()
                                        {
                                            p.tint_r = 0.75;
                                            p.tint_b = 1.35;
                                            changed = true;
                                            started = true;
                                        }
                                        if ui
                                            .small_button("Fluorescent")
                                            .on_hover_text("4000K fluorescent")
                                            .clicked()
                                        {
                                            p.tint_r = 0.88;
                                            p.tint_b = 1.12;
                                            changed = true;
                                            started = true;
                                        }
                                    });
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::Exposure => {
                                let (up, down) = section_header_row(
                                    ui,
                                    id.title(),
                                    &mut collapsed,
                                    Some((&mut self.stack.exposure.enabled, &mut changed)),
                                    can_up,
                                    can_down,
                                );
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed && self.stack.exposure.enabled {
                                    ui.add_space(2.0);
                                    let p = &mut self.stack.exposure.params;
                                    track!(ui.add(egui::Slider::new(&mut p.ev, -4.0..=4.0).text("EV")));
                                    track!(
                                        ui.add(
                                            egui::Slider::new(&mut p.black, -0.1..=0.1)
                                                .text("black")
                                                .fixed_decimals(4)
                                        )
                                    );
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::Tonal => {
                                let (up, down) = section_header_row(
                                    ui,
                                    id.title(),
                                    &mut collapsed,
                                    Some((&mut self.stack.tonal.enabled, &mut changed)),
                                    can_up,
                                    can_down,
                                );
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed && self.stack.tonal.enabled {
                                    ui.add_space(2.0);
                                    let p = &mut self.stack.tonal.params;
                                    track!(ui.add(egui::Slider::new(&mut p.highlights, -1.0..=1.0).text("highlights")));
                                    track!(ui.add(egui::Slider::new(&mut p.shadows, -1.0..=1.0).text("shadows")));
                                    track!(ui.add(egui::Slider::new(&mut p.whites, -1.0..=1.0).text("whites")));
                                    track!(ui.add(egui::Slider::new(&mut p.blacks, -1.0..=1.0).text("blacks")));
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::ToneCurve => {
                                let (up, down) = section_header_row(
                                    ui,
                                    id.title(),
                                    &mut collapsed,
                                    Some((&mut self.stack.tone_curve.enabled, &mut changed)),
                                    can_up,
                                    can_down,
                                );
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed && self.stack.tone_curve.enabled {
                                    ui.add_space(2.0);
                                    let curve_response = curve_editor(ui, &mut self.stack.tone_curve.params);
                                    if curve_response.drag_started() {
                                        started = true;
                                    }
                                    if curve_response.changed() {
                                        changed = true;
                                    }
                                    if ui
                                        .add_enabled(
                                            self.stack.tone_curve.params != identity_nodes(),
                                            egui::Button::new("reset"),
                                        )
                                        .clicked()
                                    {
                                        started = true;
                                        self.stack.tone_curve.params = identity_nodes();
                                        changed = true;
                                    }
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::Filmic3D => {
                                let (up, down) = section_header_row(
                                    ui,
                                    id.title(),
                                    &mut collapsed,
                                    Some((&mut self.stack.filmic.enabled, &mut changed)),
                                    can_up,
                                    can_down,
                                );
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed && self.stack.filmic.enabled {
                                    ui.add_space(2.0);
                                    let p = &mut self.stack.filmic.params;
                                    tonemap_3d::render_tonemap_3d(
                                        ui,
                                        &mut self.tonemap_3d_state,
                                        p,
                                        &mut changed,
                                        &mut started,
                                    );
                                    ui.add_space(4.0);
                                    track!(ui.add(egui::Slider::new(&mut p.white_ev, 0.0..=8.0).text("white EV")));
                                    track!(
                                        ui.add(egui::Slider::new(&mut p.black_ev, -16.0..=-1.0).text("black EV"))
                                    );
                                    track!(
                                        ui.add(egui::Slider::new(&mut p.latitude, 5.0..=80.0).text("latitude %"))
                                    );
                                    track!(ui.add(egui::Slider::new(&mut p.contrast, 0.1..=3.0).text("contrast")));
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::ColorBalance => {
                                let (up, down) = section_header_row(
                                    ui,
                                    id.title(),
                                    &mut collapsed,
                                    Some((&mut self.stack.color_balance.enabled, &mut changed)),
                                    can_up,
                                    can_down,
                                );
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed && self.stack.color_balance.enabled {
                                    ui.add_space(2.0);
                                    let p = &mut self.stack.color_balance.params;
                                    for (label, rgb) in [
                                        ("lift", &mut p.lift),
                                        ("gamma", &mut p.gamma),
                                        ("gain", &mut p.gain),
                                        ("offset", &mut p.offset),
                                    ] {
                                        ui.label(theme::dim(label));
                                        track!(ui.add(egui::Slider::new(&mut rgb.r, -0.5..=0.5).text("r")));
                                        track!(ui.add(egui::Slider::new(&mut rgb.g, -0.5..=0.5).text("g")));
                                        track!(ui.add(egui::Slider::new(&mut rgb.b, -0.5..=0.5).text("b")));
                                    }
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::Vibrance => {
                                let (up, down) = section_header_row(
                                    ui,
                                    id.title(),
                                    &mut collapsed,
                                    Some((&mut self.stack.vibrance.enabled, &mut changed)),
                                    can_up,
                                    can_down,
                                );
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed && self.stack.vibrance.enabled {
                                    ui.add_space(2.0);
                                    let p = &mut self.stack.vibrance.params;
                                    track!(ui.add(
                                        egui::Slider::new(&mut p.vibrance, -1.0..=1.0)
                                            .text("vibrance")
                                            .fixed_decimals(2)
                                    ));
                                    track!(ui.add(
                                        egui::Slider::new(&mut p.saturation, -1.0..=1.0)
                                            .text("saturation")
                                            .fixed_decimals(2)
                                    ));
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::Sharpen => {
                                let (up, down) = section_header_row(
                                    ui,
                                    id.title(),
                                    &mut collapsed,
                                    Some((&mut self.stack.sharpen.enabled, &mut changed)),
                                    can_up,
                                    can_down,
                                );
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed && self.stack.sharpen.enabled {
                                    ui.add_space(2.0);
                                    let p = &mut self.stack.sharpen.params;
                                    track!(ui.add(
                                        egui::Slider::new(&mut p.amount, 0.0..=2.0)
                                            .text("amount")
                                            .fixed_decimals(2)
                                    ));
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                            InspectorSectionId::Vignette => {
                                let (up, down) = section_header_row(
                                    ui,
                                    id.title(),
                                    &mut collapsed,
                                    Some((&mut self.stack.vignette.enabled, &mut changed)),
                                    can_up,
                                    can_down,
                                );
                                if up { move_action = Some((idx, -1)); }
                                if down { move_action = Some((idx, 1)); }
                                if !collapsed && self.stack.vignette.enabled {
                                    ui.add_space(2.0);
                                    let p = &mut self.stack.vignette.params;
                                    track!(ui.add(
                                        egui::Slider::new(&mut p.amount, -1.0..=1.0)
                                            .text("amount")
                                            .fixed_decimals(2)
                                    ));
                                    track!(ui.add(
                                        egui::Slider::new(&mut p.radius, 0.1..=1.5)
                                            .text("radius")
                                            .fixed_decimals(2)
                                    ));
                                    ui.add_space(4.0);
                                }
                                ui.separator();
                            }
                        }

                        self.inspector_sections[idx].collapsed = collapsed;
                    }

                    if let Some((idx, dir)) = move_action {
                        let new_idx = (idx as isize + dir) as usize;
                        if new_idx < self.inspector_sections.len() {
                            self.inspector_sections.swap(idx, new_idx);
                        }
                    }

                    if started {
                        self.history.checkpoint(&snapshot);
                    }
                    if changed {
                        self.needs_render = true;
                    }
                    if geometry_changed {
                        self.history.checkpoint(&before_geometry);
                        self.needs_render = true;
                    }

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("undo").clicked() && self.history.undo(&mut self.stack) {
                            self.needs_render = true;
                        }
                        if ui.button("redo").clicked() && self.history.redo(&mut self.stack) {
                            self.needs_render = true;
                        }
                    });
                });
        });

        self.inspector_width = inner.response.rect.width();
    }
}

fn section_header_row(
    ui: &mut egui::Ui,
    title: &str,
    collapsed: &mut bool,
    mut enabled: Option<(&mut bool, &mut bool)>,
    can_up: bool,
    can_down: bool,
) -> (bool, bool) {
    let mut move_up = false;
    let mut move_down = false;

    ui.horizontal(|ui| {
        let arrow = if *collapsed { "▶" } else { "▼" };
        if ui
            .small_button(arrow)
            .on_hover_text(if *collapsed { "Expand section" } else { "Collapse section" })
            .clicked()
        {
            *collapsed = !*collapsed;
        }

        let is_active = match enabled {
            Some((ref mut en, ref mut chg)) => {
                if ui.checkbox(en, "").changed() {
                    **chg = true;
                }
                **en
            }
            None => true,
        };

        let label_text = if is_active {
            egui::RichText::new(title).color(theme::TEXT)
        } else {
            theme::dim(title)
        };

        if ui
            .selectable_label(false, label_text)
            .on_hover_text("Click to collapse/expand")
            .clicked()
        {
            *collapsed = !*collapsed;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(can_down, egui::Button::new("▼").small())
                .on_hover_text("Move section down in sidebar")
                .clicked()
            {
                move_down = true;
            }
            if ui
                .add_enabled(can_up, egui::Button::new("▲").small())
                .on_hover_text("Move section up in sidebar")
                .clicked()
            {
                move_up = true;
            }
        });
    });

    (move_up, move_down)
}

#[allow(dead_code)]
fn module_header(ui: &mut egui::Ui, label: &str, enabled: &mut bool, changed: &mut bool) {
    ui.horizontal(|ui| {
        if ui.checkbox(enabled, "").changed() {
            *changed = true;
        }
        let text = if *enabled {
            egui::RichText::new(label).color(theme::TEXT)
        } else {
            theme::dim(label)
        };
        ui.label(text);
    });
}

/// A minimal draggable curve editor. Endpoints are locked in x so the curve always spans
/// the full range.
fn curve_editor(ui: &mut egui::Ui, nodes: &mut [CurveNode]) -> egui::Response {
    let size = egui::vec2(ui.available_width().min(240.0), 160.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 2.0, theme::BG_950);
    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );

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
    let points: Vec<egui::Pos2> = (0..64)
        .map(|i| {
            let t = i as f32 / 63.0;
            let v = lut[(t * (lut.len() - 1) as f32) as usize];
            egui::pos2(
                rect.left() + t * rect.width(),
                rect.bottom() - v.clamp(0.0, 1.0) * rect.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(1.5, theme::ACCENT),
    ));

    for n in nodes.iter() {
        painter.circle_filled(to_screen(n), 3.0, theme::TEXT);
    }
    response
}

fn bare_frame(fill: egui::Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(10, 6))
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

fn kelvin_tint_to_rgb(kelvin: f32, tint: f32) -> (f32, f32) {
    let k_factor = (kelvin - 5500.0) / 4500.0;
    let r = 1.0 + k_factor * 0.45 - tint * 0.005;
    let b = 1.0 - k_factor * 0.40 - tint * 0.005;
    (r.clamp(0.2, 4.0), b.clamp(0.2, 4.0))
}

fn rgb_to_kelvin_tint(tint_r: f32, tint_b: f32) -> (f32, f32) {
    let diff = (tint_r - tint_b) * 0.5;
    let kelvin = (5500.0 + diff * 4500.0).clamp(2000.0, 12000.0);
    let avg = (tint_r + tint_b) * 0.5;
    let tint = ((1.0 - avg) * 100.0).clamp(-50.0, 50.0);
    (kelvin, tint)
}
