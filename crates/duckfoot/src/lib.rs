//! The winit shell shared by every bootstrap. `main.rs` (desktop) and the
//! `duckfoot-android` crate each build an event loop and a [`Platform`], then call [`run`].
//!
//! Lifecycle, which is what makes this work on Android: the window surface is the only
//! thing that dies on suspend. The wgpu device, the egui renderer and the [`PhotoApp`] —
//! the open photo, its stack, undo history, shelf and selection — all live for the whole
//! run. Android suspends the app on every app switch, on screen-off, and whenever the
//! system file picker opens, so rebuilding the app there would lose the user's work at
//! exactly the moment they chose a file.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use photo_app::platform::Platform;
use photo_app::{Gpu, PhotoApp};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

/// Wakes the event loop from another thread.
#[derive(Debug)]
pub enum UserEvent {
    /// egui wants a frame, now or after a delay.
    Repaint(Duration),
    /// Something outside egui changed (a picker answered, say); draw a frame.
    Wake,
}

/// What a bootstrap hands to [`run`].
pub struct Options {
    /// Opened once the first window exists.
    pub pending_open: Option<PathBuf>,
    /// Builds the platform for the app. Called once per app, which is normally once per
    /// run (see [`Shell::resumed`] for the exception).
    pub platform: Box<dyn FnMut() -> Box<dyn Platform>>,
}

/// Create the event loop type [`run`] expects.
pub fn event_loop() -> anyhow::Result<EventLoop<UserEvent>> {
    Ok(EventLoop::<UserEvent>::with_user_event().build()?)
}

/// Drive the app until the event loop exits.
pub fn run(event_loop: EventLoop<UserEvent>, options: Options) -> anyhow::Result<()> {
    // Redraw only when something asks for it. A photo editor at rest should use no GPU.
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = Shell::new(event_loop.create_proxy(), options);
    event_loop.run_app(&mut shell)?;
    Ok(())
}

/// The GPU device and everything bound to it. Outlives any one window surface.
struct GpuState {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The swapchain format the renderer's pipelines were built for.
    format: wgpu::TextureFormat,
    renderer: RwLock<egui_wgpu::Renderer>,
}

impl GpuState {
    fn new(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>) -> anyhow::Result<Self> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))?;
        log::info!("adapter: {:?}", adapter.get_info());

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("duckfoot"),
                required_features: wgpu::Features::empty(),
                // Downlevel defaults, so the same engine runs on mobile-class GPUs unchanged.
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
                ..Default::default()
            }))?;

        let format = surface_format(surface, &adapter)?;
        let renderer = RwLock::new(new_renderer(&device, format));
        Ok(Self {
            adapter,
            device,
            queue,
            format,
            renderer,
        })
    }

    fn renderer_write(&self) -> std::sync::RwLockWriteGuard<'_, egui_wgpu::Renderer> {
        self.renderer
            .write()
            .expect("egui renderer lock poisoned by a panic in another thread")
    }

    fn free_textures<'a>(&self, ids: impl IntoIterator<Item = &'a egui::TextureId>) {
        let mut renderer = self.renderer_write();
        for id in ids {
            renderer.free_texture(id);
        }
    }

    fn gpu(&self) -> Gpu<'_> {
        Gpu {
            device: &self.device,
            queue: &self.queue,
            renderer: &self.renderer,
        }
    }
}

fn new_renderer(device: &wgpu::Device, format: wgpu::TextureFormat) -> egui_wgpu::Renderer {
    egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default())
}

/// egui writes gamma-encoded colour and wants a non-sRGB surface so the hardware does
/// not encode it a second time. The photo pipeline applies its own transfer function for
/// the same reason — see photo_engine::OUTPUT_FORMAT.
fn surface_format(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
) -> anyhow::Result<wgpu::TextureFormat> {
    let caps = surface.get_capabilities(adapter);
    caps.formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or(caps.formats.first().copied())
        .ok_or_else(|| anyhow::anyhow!("this adapter cannot present to the window surface"))
}

/// Everything that only exists while there is a live window. On Android this is dropped
/// on suspend and rebuilt on resume; nothing else is.
struct WindowState {
    // Declared before `window` so it drops first; it borrows the window's handle.
    surface: wgpu::Surface<'static>,
    window: Arc<Window>,
    config: wgpu::SurfaceConfiguration,
    egui_winit: egui_winit::State,
}

impl WindowState {
    fn resize(&mut self, gpu: &GpuState, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&gpu.device, &self.config);
    }

    fn redraw(&mut self, gpu: &GpuState, egui_ctx: &egui::Context, app: &mut PhotoApp) {
        let input = self.egui_winit.take_egui_input(&self.window);

        let mut output = egui_ctx.run_ui(input, |ui| {
            app.update(ui, &gpu.gpu());
        });

        self.egui_winit
            .handle_platform_output(&self.window, output.platform_output);

        let pixels_per_point = egui_ctx.pixels_per_point();
        let primitives = egui_ctx.tessellate(output.shapes, pixels_per_point);

        // `TexturesDelta` panics (debug_assert) if it is dropped with entries still in
        // it, so it is taken out of `output` and emptied here rather than borrowed.
        // Uploads happen before the surface is acquired so that an early return below
        // cannot strand them — the font atlas arrives on the very first frame, and
        // losing it leaves egui drawing against a texture that was never uploaded.
        let mut textures_delta = std::mem::take(&mut output.textures_delta);
        {
            let mut renderer = gpu.renderer_write();
            // One id can carry several deltas in a single frame (a partial font atlas
            // update followed by a full one, for instance).
            for (id, deltas) in &textures_delta.set {
                for delta in deltas {
                    renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
                }
            }
        }
        // Frees are deferred until after the render pass: a texture released this frame
        // may still be referenced by this frame's primitives.
        let to_free = std::mem::take(&mut textures_delta.free);
        textures_delta.clear();

        use wgpu::CurrentSurfaceTexture as Acquired;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(f) => f,
            // Suboptimal still presents; reconfiguring next frame is enough.
            Acquired::Suboptimal(f) => f,
            Acquired::Outdated | Acquired::Lost => {
                self.surface.configure(&gpu.device, &self.config);
                gpu.free_textures(&to_free);
                // Without this the window can sit blank forever under ControlFlow::Wait,
                // because nothing else is guaranteed to ask for another frame.
                self.window.request_redraw();
                return;
            }
            // Minimised or behind another window: skip the frame, do no GPU work, and
            // do NOT ask for another — that would spin while the window is hidden.
            Acquired::Timeout | Acquired::Occluded => {
                gpu.free_textures(&to_free);
                return;
            }
            other => {
                log::error!("could not acquire a surface texture: {other:?}");
                gpu.free_textures(&to_free);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("egui encoder"),
            });

        let desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };

        {
            let mut renderer = gpu.renderer_write();

            renderer.update_buffers(&gpu.device, &gpu.queue, &mut encoder, &primitives, &desc);

            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.043,
                                g: 0.043,
                                b: 0.047,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();

            renderer.render(&mut pass, &primitives, &desc);
            drop(pass);

            for id in &to_free {
                renderer.free_texture(id);
            }
        }

        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);

        if output
            .viewport_output
            .get(&egui_ctx.viewport_id())
            .map(|v| v.repaint_delay.is_zero())
            .unwrap_or(false)
        {
            self.window.request_redraw();
        }
    }
}

/// egui's context, with the repaint callback that wakes the event loop.
fn new_egui_context(proxy: EventLoopProxy<UserEvent>) -> egui::Context {
    let egui_ctx = egui::Context::default();
    photo_app::theme::apply(&egui_ctx);
    // Under `ControlFlow::Wait` the loop sleeps until the OS has an event for it. A
    // decode or thumbnail finishing on a worker thread is not one: without this its
    // `request_repaint` is recorded and nothing wakes up, so the photo appears only
    // when the mouse next moves. Delayed repaints (tooltips, animations) likewise
    // need the loop told when to wake.
    egui_ctx.set_request_repaint_callback(move |info| {
        let _ = proxy.send_event(UserEvent::Repaint(info.delay));
    });
    egui_ctx
}

struct Shell {
    instance: wgpu::Instance,
    gpu: Option<GpuState>,
    window: Option<WindowState>,
    egui_ctx: egui::Context,
    app: Option<PhotoApp>,
    options: Options,
    proxy: EventLoopProxy<UserEvent>,
    /// The earliest delayed repaint egui has asked for, if any.
    repaint_at: Option<Instant>,
}

impl Shell {
    fn new(proxy: EventLoopProxy<UserEvent>, options: Options) -> Self {
        Self {
            instance: wgpu::Instance::default(),
            gpu: None,
            window: None,
            egui_ctx: new_egui_context(proxy.clone()),
            app: None,
            options,
            proxy,
            repaint_at: None,
        }
    }

    fn schedule_repaint(&mut self, event_loop: &ActiveEventLoop, delay: Duration) {
        if delay.is_zero() {
            self.request_redraw();
            return;
        }
        // egui uses Duration::MAX for "never"; that does not fit in an Instant.
        let Some(when) = Instant::now().checked_add(delay) else {
            return;
        };
        if self.repaint_at.is_none_or(|t| when < t) {
            self.repaint_at = Some(when);
            event_loop.set_control_flow(ControlFlow::WaitUntil(when));
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.window.request_redraw();
        }
    }

    /// Create the window and surface, and — on the first resume only — the device and
    /// the app.
    fn open_window(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Duckfoot — Photo")
                    .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0))
                    .with_min_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0)),
            )?,
        );
        let surface = self.instance.create_surface(window.clone())?;

        if self
            .gpu
            .as_ref()
            .is_some_and(|gpu| !gpu.adapter.is_surface_supported(&surface))
        {
            log::warn!("the new surface is not presentable by the old adapter; starting over");
            self.discard_device();
        }
        if self.gpu.is_none() {
            self.gpu = Some(GpuState::new(&self.instance, &surface)?);
        }
        let gpu = self.gpu.as_mut().expect("created above");

        let format = surface_format(&surface, &gpu.adapter)?;
        if format != gpu.format {
            // The renderer's pipelines are built for one format. Replacing the renderer
            // also forgets every texture egui uploaded to it, so the context and the app
            // (whose photo is registered there too) have to go with it.
            log::warn!("surface format changed {:?} → {format:?}", gpu.format);
            gpu.renderer = RwLock::new(new_renderer(&gpu.device, format));
            gpu.format = format;
            self.egui_ctx = new_egui_context(self.proxy.clone());
            self.app = None;
        }

        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow::anyhow!("this adapter cannot present to the window surface"))?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &config);

        let egui_winit = egui_winit::State::new(
            self.egui_ctx.clone(),
            self.egui_ctx.viewport_id(),
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        if self.app.is_none() {
            let mut app = PhotoApp::with_platform(&gpu.gpu(), (self.options.platform)());
            if let Some(path) = self.options.pending_open.take() {
                app.open_path(&gpu.gpu(), path);
            }
            self.app = Some(app);
        }

        window.request_redraw();
        self.window = Some(WindowState {
            surface,
            window,
            config,
            egui_winit,
        });
        Ok(())
    }

    /// Drop the device and everything built on it. The app's textures live on the device,
    /// so the app goes too; this is the fallback, not the normal path.
    fn discard_device(&mut self) {
        self.app = None;
        self.gpu = None;
        self.egui_ctx = new_egui_context(self.proxy.clone());
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            self.repaint_at = None;
            event_loop.set_control_flow(ControlFlow::Wait);
            self.request_redraw();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Repaint(delay) => self.schedule_repaint(event_loop, delay),
            UserEvent::Wake => self.request_redraw(),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(e) = self.open_window(event_loop) {
            log::error!("could not create a window or GPU device: {e:#}");
            event_loop.exit();
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // Android destroys the native window when the app backgrounds; using a surface
        // built on it afterwards is the classic Android wgpu crash. Only the window side
        // goes — the device and the app, with the user's work, stay.
        self.window = None;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(window), Some(gpu), Some(app)) =
            (self.window.as_mut(), self.gpu.as_ref(), self.app.as_mut())
        else {
            return;
        };

        let response = window.egui_winit.on_window_event(&window.window, &event);
        if response.repaint {
            window.window.request_redraw();
        }
        if response.consumed {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                window.resize(gpu, size.width, size.height);
                window.window.request_redraw();
            }
            WindowEvent::DroppedFile(path) => {
                app.open_path(&gpu.gpu(), path);
                window.window.request_redraw();
            }
            WindowEvent::RedrawRequested => window.redraw(gpu, &self.egui_ctx, app),
            _ => {}
        }
    }
}
