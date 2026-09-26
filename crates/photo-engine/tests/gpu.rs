//! Real-GPU pixel tests: shaders run, pixels come back, and assertions are made on them.
//!
//! Everything else in the workspace runs on wgpu's no-op adapter, where no shader ever
//! executes — so until these existed, nothing automated could tell a correct render from a
//! black one. CI runs them on lavapipe (Mesa's software Vulkan):
//!
//!     VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
//!         DUCKFOOT_REQUIRE_GPU=1 cargo test --release -p photo-engine --test gpu
//!
//! Without an adapter each test prints a notice and passes, so a machine with no Vulkan
//! still runs the rest of the suite. With `DUCKFOOT_REQUIRE_GPU=1` a missing adapter fails
//! instead, which is what CI sets: a silently skipped GPU suite proves nothing.
//!
//! Inputs are synthesised (`photo_engine::testkit`) so every expected value is known in
//! advance, and small so a software rasteriser gets through them in seconds.

use std::sync::OnceLock;

use photo_engine::testkit::{self, read_texture};
use photo_engine::{Crop, Engine, PhotoStack};

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// One device for the whole suite; each test builds its own `Engine` on it.
fn gpu() -> Option<&'static Gpu> {
    static GPU: OnceLock<Option<Gpu>> = OnceLock::new();
    let gpu = GPU.get_or_init(|| {
        // Honours WGPU_BACKEND and WGPU_ADAPTER_NAME, so CI can pin llvmpipe.
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(
            &instance, None,
        ))
        .ok()?;
        let info = adapter.get_info();
        eprintln!("gpu tests: {} ({:?})", info.name, info.backend);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu tests"),
            // What the app asks for, so a test cannot pass on a limit the app never gets.
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .ok()?;
        Some(Gpu { device, queue })
    });
    if gpu.is_none() {
        let required = std::env::var_os("DUCKFOOT_REQUIRE_GPU").is_some_and(|v| v != "0");
        assert!(
            !required,
            "DUCKFOOT_REQUIRE_GPU is set but no GPU adapter was found \
             (on Linux: install mesa-vulkan-drivers and set VK_ICD_FILENAMES to lavapipe)"
        );
        eprintln!(
            "notice: no GPU adapter, skipping this GPU test \
             (set DUCKFOOT_REQUIRE_GPU=1 to make that a failure)"
        );
    }
    gpu.as_ref()
}

/// The GPU, or return from the test with the notice already printed.
macro_rules! gpu_or_skip {
    () => {
        match gpu() {
            Some(gpu) => gpu,
            None => return,
        }
    };
}

/// Render the preview at `level` and read it back: (width, height, RGBA8).
fn preview(gpu: &Gpu, engine: &mut Engine, stack: &PhotoStack, level: u32) -> (u32, u32, Vec<u8>) {
    engine.set_preview_level(&gpu.device, level);
    engine.set_geometry(&gpu.device, stack.geometry);
    engine.render(&gpu.device, &gpu.queue, stack, None);
    let image = engine.image().expect("an image is loaded");
    let (w, h) = image.preview_size();
    let px = read_texture(&gpu.device, &gpu.queue, image.output_texture()).expect("readback");
    (w, h, px)
}

/// A stack with most of the chain switched on, so a comparison covers more than a no-op.
fn busy_stack() -> PhotoStack {
    let mut stack = PhotoStack::default();
    stack.exposure.params.ev = 0.4;
    stack.clarity.enabled = true;
    stack.clarity.params.clarity = 40.0;
    stack.clarity.params.texture = 30.0;
    stack.sharpen.enabled = true;
    stack.sharpen.params.amount = 0.5;
    stack.vignette.enabled = true;
    stack.vignette.params.amount = -0.4;
    stack.vibrance.enabled = true;
    stack.vibrance.params.vibrance = 0.3;
    stack
}

#[test]
fn export_equals_preview_at_level_0() {
    let gpu = gpu_or_skip!();
    let (w, h) = (96, 64);
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(
            &gpu.device,
            &gpu.queue,
            testkit::cfa_from_fn(w, h, testkit::smooth_scene(w, h)),
        )
        .unwrap();

    for turns in [0u8, 1] {
        let mut stack = busy_stack();
        stack.geometry.quarter_turns = turns;
        let (pw, ph, shown) = preview(gpu, &mut engine, &stack, 0);
        let exported = engine
            .export(&gpu.device, &gpu.queue, &stack, Crop::default())
            .unwrap();
        assert_eq!((exported.width, exported.height), (pw, ph));
        // Same shader, same uniforms, same texture: not close, identical (invariant 2).
        assert!(
            exported.rgba == shown,
            "export and preview differ with {turns} quarter turns"
        );
    }
}

#[test]
fn a_flat_field_fills_exactly_one_histogram_bin() {
    let gpu = gpu_or_skip!();
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(64, 48, 0.25))
        .unwrap();
    let stack = PhotoStack::default();
    // The readback is asynchronous by design, so drive frames the way the app does.
    for _ in 0..64 {
        engine.render(&gpu.device, &gpu.queue, &stack, None);
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        engine.poll_histogram(&gpu.device);
        if engine.histogram().peak > 0 {
            break;
        }
    }
    let hist = engine.histogram().clone();
    let value = engine
        .export(&gpu.device, &gpu.queue, &stack, Crop::default())
        .unwrap()
        .rgba[1] as usize;

    for (name, bins) in [
        ("red", &hist.red),
        ("green", &hist.green),
        ("blue", &hist.blue),
        ("luma", &hist.luma),
    ] {
        let populated: Vec<usize> = (0..bins.len()).filter(|&i| bins[i] > 0).collect();
        assert_eq!(
            populated,
            vec![value],
            "{name}: a uniform frame of {value} filled bins {populated:?}"
        );
    }
}
