//! Desktop bootstrap. The only platform-specific crate in the workspace.
//!
//! An Android build is a sibling of this file: the same `PhotoApp` and the same `Gpu`
//! context, entered through `android-activity` instead of `EventLoop::run_app`. winit's
//! `ApplicationHandler` already models Android's lifecycle — `resumed` creates the
//! surface and `suspended` drops it — so that port is a bootstrap, not a rewrite.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, RwLock};

use photo_app::{Gpu, PhotoApp};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Everything that only exists while there is a live surface. On Android this whole
/// struct is dropped on suspend and rebuilt on resume; the `PhotoApp` outlives it.
struct Surface {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui_ctx: egui::Context,
    egui_winit: egui_winit::State,
    renderer: RwLock<egui_wgpu::Renderer>,
}

impl Surface {
    fn new(event_loop: &ActiveEventLoop) -> anyhow::Result<Self> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Duckfoot — Photo")
                    .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0))
                    .with_min_inner_size(winit::dpi::LogicalSize::new(900.0, 600.0)),
            )?,
        );

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
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

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        // egui writes gamma-encoded colour and wants a non-sRGB surface so the hardware
        // does not encode it a second time. The photo pipeline applies its own transfer
        // function for the same reason — see photo_engine::OUTPUT_FORMAT.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow::anyhow!("this adapter cannot present to the window surface"))?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let egui_ctx = egui::Context::default();
        photo_app::theme::apply(&egui_ctx);

        let egui_winit = egui_winit::State::new(
            egui_ctx.clone(),
            egui_ctx.viewport_id(),
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        let renderer = RwLock::new(egui_wgpu::Renderer::new(
            &device,
            format,
            egui_wgpu::RendererOptions::default(),
        ));

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            egui_ctx,
            egui_winit,
            renderer,
        })
    }

    fn gpu(&self) -> Gpu<'_> {
        Gpu {
            device: &self.device,
            queue: &self.queue,
            renderer: &self.renderer,
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn redraw(&mut self, app: &mut PhotoApp) {
        let input = self.egui_winit.take_egui_input(&self.window);

        let egui_ctx = self.egui_ctx.clone();
        let output = egui_ctx.run_ui(input, |ui| {
            app.update(ui, &self.gpu());
        });

        self.egui_winit
            .handle_platform_output(&self.window, output.platform_output);

        let pixels_per_point = self.egui_ctx.pixels_per_point();
        let primitives = self.egui_ctx.tessellate(output.shapes, pixels_per_point);

        use wgpu::CurrentSurfaceTexture as Acquired;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(f) => f,
            // Suboptimal still presents; reconfiguring next frame is enough.
            Acquired::Suboptimal(f) => f,
            Acquired::Outdated | Acquired::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            // Minimised or behind another window: skip the frame, do no GPU work.
            Acquired::Timeout | Acquired::Occluded => return,
            other => {
                log::error!("could not acquire a surface texture: {other:?}");
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("egui encoder"),
            });

        let desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };

        {
            let mut renderer = self
                .renderer
                .write()
                .expect("egui renderer lock poisoned by a panic in another thread");

            // One id can carry several deltas in a single frame (a partial font atlas
            // update followed by a full one, for instance).
            for (id, deltas) in &output.textures_delta.set {
                for delta in deltas {
                    renderer.update_texture(&self.device, &self.queue, *id, delta);
                }
            }
            renderer.update_buffers(&self.device, &self.queue, &mut encoder, &primitives, &desc);

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

            for id in &output.textures_delta.free {
                renderer.free_texture(id);
            }
        }

        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);

        if output
            .viewport_output
            .get(&self.egui_ctx.viewport_id())
            .map(|v| v.repaint_delay.is_zero())
            .unwrap_or(false)
        {
            self.window.request_redraw();
        }
    }
}

#[derive(Default)]
struct Duckfoot {
    surface: Option<Surface>,
    app: Option<PhotoApp>,
    pending_open: Option<std::path::PathBuf>,
}

impl ApplicationHandler for Duckfoot {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_some() {
            return;
        }
        match Surface::new(event_loop) {
            Ok(surface) => {
                // The app is rebuilt with the new device: GPU resources cannot outlive a
                // surface loss, and on Android that is a routine event, not an error.
                let mut app = PhotoApp::new(&surface.gpu());
                if let Some(path) = self.pending_open.clone() {
                    app.open_path(&surface.gpu(), path);
                }
                self.app = Some(app);
                self.surface = Some(surface);
            }
            Err(e) => {
                log::error!("could not create a window or GPU device: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // Android destroys the surface when the app backgrounds. Drop everything tied to
        // it; `resumed` rebuilds. Not doing this is the classic Android wgpu crash.
        self.app = None;
        self.surface = None;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(surface), Some(app)) = (self.surface.as_mut(), self.app.as_mut()) else {
            return;
        };

        let response = surface.egui_winit.on_window_event(&surface.window, &event);
        if response.repaint {
            surface.window.request_redraw();
        }
        if response.consumed {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                surface.resize(size.width, size.height);
                surface.window.request_redraw();
            }
            WindowEvent::DroppedFile(path) => {
                app.open_path(&surface.gpu(), path);
                surface.window.request_redraw();
            }
            WindowEvent::RedrawRequested => surface.redraw(app),
            _ => {}
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let event_loop = EventLoop::new()?;
    // Redraw only when something asks for it. A photo editor at rest should use no GPU.
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut duckfoot = Duckfoot {
        pending_open: std::env::args().nth(1).map(Into::into),
        ..Default::default()
    };
    event_loop.run_app(&mut duckfoot)?;
    Ok(())
}
