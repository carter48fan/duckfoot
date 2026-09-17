//! Barrel 1's UI. Knows about egui and the engine; knows nothing about windowing.
//!
//! The desktop bootstrap and the eventual Android bootstrap both drive this same type.

pub mod theme;

use std::path::PathBuf;
use std::time::Instant;

use std::sync::RwLock;

use photo_engine::{identity_nodes, CurveNode, Engine, PhotoStack, RawDecoder, RawloaderBackend};

/// Everything the app needs from the GPU, supplied by whichever shell is running.
///
/// Deliberately not eframe's `RenderState`: the app must not know which bootstrap
/// created the device, or the Android build becomes a fork instead of a new `main`.
pub struct Gpu<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub renderer: &'a RwLock<egui_wgpu::Renderer>,
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
        }
    }

    pub fn open_path(&mut self, gpu: &Gpu, path: PathBuf) {
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
                self.loaded_path = Some(path);
                self.stack = PhotoStack::default();
                self.history = History::new();
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
        if !self.needs_render {
            return;
        }
        let started = Instant::now();
        self.engine.render(gpu.device, gpu.queue, &self.stack);
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
        self.sync(gpu);

        self.top_bar(ui, gpu);
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

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(image) = self.engine.image() {
                            ui.label(theme::dim(format!("{} {}", image.make, image.model)));
                        }
                    });
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
            .show(ui, |ui| match (self.texture_id, self.engine.image()) {
                (Some(id), Some(image)) => {
                    let available = ui.available_size();
                    let aspect = image.width as f32 / image.height as f32;
                    let mut size = egui::vec2(available.x, available.x / aspect);
                    if size.y > available.y {
                        size = egui::vec2(available.y * aspect, available.y);
                    }
                    ui.centered_and_justified(|ui| {
                        ui.add(
                            egui::Image::new(egui::load::SizedTexture::new(id, size))
                                .fit_to_exact_size(size),
                        );
                    });
                }
                _ => {
                    ui.centered_and_justified(|ui| {
                        ui.label(theme::dim("Open a camera RAW file"));
                    });
                }
            });
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .frame(bare_frame(theme::BG_900))
            .default_size(272.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.label(theme::dim("INSPECTOR"));
                ui.add_space(6.0);

                if self.engine.image().is_none() {
                    ui.label(theme::dim("no image"));
                    return;
                }

                // One checkpoint per gesture, taken on drag start — not per frame, or the
                // undo stack fills with a hundred identical steps per slider drag.
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

                module_header(
                    ui,
                    "white balance",
                    &mut self.stack.white_balance.enabled,
                    &mut changed,
                );
                if self.stack.white_balance.enabled {
                    let p = &mut self.stack.white_balance.params;
                    track!(ui.add(
                        egui::Slider::new(&mut p.tint_r, 0.2..=4.0)
                            .text("red")
                            .fixed_decimals(3)
                    ));
                    track!(ui.add(
                        egui::Slider::new(&mut p.tint_b, 0.2..=4.0)
                            .text("blue")
                            .fixed_decimals(3)
                    ));
                }
                ui.separator();

                module_header(
                    ui,
                    "exposure",
                    &mut self.stack.exposure.enabled,
                    &mut changed,
                );
                if self.stack.exposure.enabled {
                    let p = &mut self.stack.exposure.params;
                    track!(ui.add(
                        egui::Slider::new(&mut p.ev, -5.0..=5.0)
                            .text("EV")
                            .fixed_decimals(2)
                    ));
                    track!(ui.add(
                        egui::Slider::new(&mut p.black, -0.1..=0.1)
                            .text("black")
                            .fixed_decimals(4)
                    ));
                }
                ui.separator();

                module_header(
                    ui,
                    "tone curve",
                    &mut self.stack.tone_curve.enabled,
                    &mut changed,
                );
                if self.stack.tone_curve.enabled {
                    track!(curve_editor(ui, &mut self.stack.tone_curve.params));
                    if ui.button("reset curve").clicked() {
                        self.stack.tone_curve.params = identity_nodes();
                        changed = true;
                        started = true;
                    }
                }
                ui.separator();

                module_header(
                    ui,
                    "filmic rgb",
                    &mut self.stack.filmic.enabled,
                    &mut changed,
                );
                if self.stack.filmic.enabled {
                    let p = &mut self.stack.filmic.params;
                    track!(ui.add(egui::Slider::new(&mut p.white_ev, 0.0..=8.0).text("white EV")));
                    track!(
                        ui.add(egui::Slider::new(&mut p.black_ev, -16.0..=-1.0).text("black EV"))
                    );
                    track!(
                        ui.add(egui::Slider::new(&mut p.latitude, 10.0..=600.0).text("latitude"))
                    );
                    track!(ui.add(egui::Slider::new(&mut p.contrast, 0.1..=3.0).text("contrast")));
                }
                ui.separator();

                module_header(
                    ui,
                    "colour balance",
                    &mut self.stack.color_balance.enabled,
                    &mut changed,
                );
                if self.stack.color_balance.enabled {
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
                }

                if started {
                    self.history.checkpoint(&snapshot);
                }
                if changed {
                    self.needs_render = true;
                }

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("undo").clicked() && self.history.undo(&mut self.stack) {
                        self.needs_render = true;
                    }
                    if ui.button("redo").clicked() && self.history.redo(&mut self.stack) {
                        self.needs_render = true;
                    }
                });
            });
    }
}

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
