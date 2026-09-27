//! Render the whole app to a PNG, with no window: see a layout without the device it is
//! for — the phone layout on a desktop, say.
//!
//!     cargo run --release -p photo-app --example screenshot -- out.png 412x892 [photo] [tab] [module] [tour step]
//!
//! Sizes are in points and drawn at 2 pixels per point, as a phone would. `tab` is
//! library, presets, edit, export or none, and picks the phone layout's sheet;
//! `module` is a module's title, as in the Edit sheet ("exposure", "tone curve"…), or `-`;
//! `tour step` shows the phone tour at that step (from 0).
//! Needs a Vulkan driver; Mesa's software one (lavapipe) is enough.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use photo_app::compact::Tab;
use photo_app::platform::NoPlatform;
use photo_app::{Gpu, InspectorSectionId, PhotoApp};

const PIXELS_PER_POINT: f32 = 2.0;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(out), Some(size)) = (args.next(), args.next()) else {
        eprintln!("usage: screenshot <out.png> <width>x<height> [photo] [tab] [module]");
        std::process::exit(2);
    };
    let (w, h) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))
        .ok_or_else(|| anyhow::anyhow!("size must look like 412x892"))?;
    let photo = args.next().filter(|p| p != "-").map(PathBuf::from);
    // `Some(None)` is "none": the sheet closed.
    let tab = match args.next() {
        None => None,
        Some(t) if t == "none" => Some(None),
        Some(t) => Some(Some(
            Tab::ALL
                .into_iter()
                .find(|tab| tab.label() == t)
                .ok_or_else(|| anyhow::anyhow!("unknown tab {t}"))?,
        )),
    };
    let module = args.next().filter(|m| m != "-").map(|m| {
        InspectorSectionId::all()
            .iter()
            .copied()
            .find(|id| id.title() == m)
            .ok_or_else(|| anyhow::anyhow!("unknown module {m}"))
    });

    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    eprintln!("adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("screenshot"),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))?;
    let renderer = RwLock::new(egui_wgpu::Renderer::new(
        &device,
        FORMAT,
        egui_wgpu::RendererOptions::default(),
    ));
    let gpu = Gpu {
        device: &device,
        queue: &queue,
        renderer: &renderer,
    };
    let ctx = egui::Context::default();
    photo_app::theme::apply(&ctx);

    let mut app = PhotoApp::with_platform(&gpu, Box::new(NoPlatform));
    let screen = egui::vec2(w, h);
    let frame = |app: &mut PhotoApp| {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            max_texture_side: Some(device.limits().max_texture_dimension_2d as usize),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(PIXELS_PER_POINT);
        let mut output = ctx.run_ui(input, |ui| app.update(ui, &gpu));
        let mut delta = std::mem::take(&mut output.textures_delta);
        {
            let mut r = renderer.write().unwrap();
            for (id, deltas) in &delta.set {
                for d in deltas {
                    r.update_texture(&device, &queue, *id, d);
                }
            }
        }
        let _ = device.poll(wgpu::PollType::Poll);
        let freed = std::mem::take(&mut delta.free);
        delta.clear();
        (output, freed)
    };

    frame(&mut app);
    if let Some(photo) = photo {
        app.add_to_shelf(&gpu, vec![photo]);
    }
    let started = Instant::now();
    while app.is_busy() && started.elapsed() < Duration::from_secs(60) {
        frame(&mut app);
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(tab) = tab {
        app.compact_tab = tab;
    }
    if let Some(module) = module {
        app.compact_module = module?;
    }
    if let Some(step) = args.next() {
        app.show_tour_step(step.parse()?);
    }
    // DUCKFOOT_SHOT_SHORTCUTS=1: the `?` sheet over it all.
    app.shortcuts_open = std::env::var_os("DUCKFOOT_SHOT_SHORTCUTS").is_some();
    // DUCKFOOT_SHOT_LOOKS=1: the desktop inspector's Looks band open.
    app.looks_open = std::env::var_os("DUCKFOOT_SHOT_LOOKS").is_some();
    app.info_open = std::env::var_os("DUCKFOOT_SHOT_INFO").is_some();
    app.clip_overlay = std::env::var_os("DUCKFOOT_SHOT_CLIP").is_some();
    // DUCKFOOT_SHOT_LENS=1: lens corrections on, their card open on the desktop.
    if std::env::var_os("DUCKFOOT_SHOT_LENS").is_some() {
        app.edit(|s| {
            s.lens.enabled = true;
            s.lens.params.distortion = 40.0;
            s.lens.params.vignetting = 60.0;
        });
        for s in &mut app.inspector_sections {
            s.collapsed = s.id != photo_app::InspectorSectionId::Lens;
        }
    }
    // DUCKFOOT_SHOT_DENOISE=1: noise reduction on, its card open on the desktop.
    if std::env::var_os("DUCKFOOT_SHOT_DENOISE").is_some() {
        app.edit(|s| {
            s.denoise.enabled = true;
            s.denoise.params.luminance = 40.0;
        });
        for s in &mut app.inspector_sections {
            s.collapsed = s.id != photo_app::InspectorSectionId::Denoise;
        }
    }
    // DUCKFOOT_SHOT_PASTE=1: the paste picker open.
    app.paste_picker_open = std::env::var_os("DUCKFOOT_SHOT_PASTE").is_some();
    // DUCKFOOT_SHOT_PRESETS=1: a user preset beside the looks, and one being named.
    if std::env::var_os("DUCKFOOT_SHOT_PRESETS").is_some() {
        app.looks_open = true;
        let stack = app.stack().clone();
        app.user_presets.push((
            PathBuf::from("evening.duckfoot-preset"),
            photo_app::user_presets::UserPreset::new("Evening glow", Default::default(), &stack),
        ));
        app.preset_draft = Some("Portrait".into());
    }
    // DUCKFOOT_SHOT_CROP=1: framing a 4:5 crop.
    if std::env::var_os("DUCKFOOT_SHOT_CROP").is_some() {
        app.begin_crop();
        app.set_crop_aspect(photo_app::crop_tool::Aspect::Ratio(4, 5));
    }
    // DUCKFOOT_SHOT_EXPORT=1: the Export dialog (desktop) or its options (phone).
    if std::env::var_os("DUCKFOOT_SHOT_EXPORT").is_some() {
        app.export_dialog_open = true;
        app.export_options_open = true;
    }
    // Let the layout, the render and the histogram readback settle.
    for _ in 0..8 {
        frame(&mut app);
        std::thread::sleep(Duration::from_millis(20));
    }
    let (output, freed) = frame(&mut app);

    let size_px = [(w * PIXELS_PER_POINT) as u32, (h * PIXELS_PER_POINT) as u32];
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screenshot"),
        size: wgpu::Extent3d {
            width: size_px[0],
            height: size_px[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let primitives = ctx.tessellate(output.shapes, PIXELS_PER_POINT);
    let desc = egui_wgpu::ScreenDescriptor {
        size_in_pixels: size_px,
        pixels_per_point: PIXELS_PER_POINT,
    };
    let stride = (size_px[0] * 4).div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot readback"),
        size: stride as u64 * size_px[1] as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut r = renderer.write().unwrap();
        r.update_buffers(&device, &queue, &mut encoder, &primitives, &desc);
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screenshot pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            })
            .forget_lifetime();
        r.render(&mut pass, &primitives, &desc);
        drop(pass);
        for id in &freed {
            r.free_texture(id);
        }
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(size_px[1]),
            },
        },
        target.size(),
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    })?;
    let data = slice.get_mapped_range()?;
    let mut pixels = Vec::with_capacity((size_px[0] * size_px[1] * 4) as usize);
    for row in data.chunks(stride as usize) {
        pixels.extend_from_slice(&row[..(size_px[0] * 4) as usize]);
    }
    drop(data);
    image::RgbaImage::from_raw(size_px[0], size_px[1], pixels)
        .ok_or_else(|| anyhow::anyhow!("readback was the wrong size"))?
        .save(&out)?;
    eprintln!("wrote {out}");
    Ok(())
}
