//! Invariant 6: a slider drag writes a uniform buffer and makes nothing on the GPU. The
//! engine counts every bind group, texture and buffer it makes, and sixty frames of a drag
//! must make none; wgpu's own counters (the `counters` feature, for this test only) must
//! also find no more alive at the end than at the start. Its own binary, with a device of
//! its own, so no other test's objects are counted.
//!
//!     VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
//!         DUCKFOOT_REQUIRE_GPU=1 cargo test -p photo-engine --test drag

use photo_engine::curve::CurveNode;
use photo_engine::testkit;
use photo_engine::{Engine, PhotoStack};

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(
        &instance, None,
    ))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("drag test"),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .ok()
}

/// Live bind groups, textures, views and buffers.
fn alive(device: &wgpu::Device) -> [isize; 4] {
    let c = device.get_internal_counters().hal;
    [
        c.bind_groups.read(),
        c.textures.read(),
        c.texture_views.read(),
        c.buffers.read(),
    ]
}

#[test]
fn a_slider_drag_makes_nothing_on_the_gpu() {
    let Some((device, queue)) = device() else {
        let required = std::env::var_os("DUCKFOOT_REQUIRE_GPU").is_some_and(|v| v != "0");
        assert!(
            !required,
            "DUCKFOOT_REQUIRE_GPU is set but no GPU adapter was found"
        );
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = (96, 64);
    let mut engine = Engine::new(&device);
    engine
        .load(
            &device,
            &queue,
            testkit::cfa_from_fn(w, h, testkit::smooth_scene(w, h)),
        )
        .unwrap();
    // The modules a drag goes through, the tone curve and the histogram among them.
    let mut stack = PhotoStack::default();
    stack.tone_curve.enabled = true;
    stack.vibrance.enabled = true;
    // Denoise renders a pre-pass of its own, which a drag must redraw, not rebuild.
    stack.denoise.enabled = true;
    stack.denoise.params.luminance = 0.3;
    let frame = |engine: &mut Engine, stack: &PhotoStack| {
        engine.render(&device, &queue, stack, Some(0.5));
        engine.poll_histogram(&device, &queue);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    };
    // Settled: whatever the first frames make once (the histogram's bind group, the
    // curve's LUT) is made.
    for _ in 0..3 {
        frame(&mut engine, &stack);
    }
    let before = alive(&device);
    assert!(before[0] > 0, "wgpu's counters are off: {before:?}");
    for i in 0..60 {
        let made = photo_engine::gpu_objects_made();
        stack.exposure.params.ev = i as f32 / 30.0 - 1.0;
        stack.vibrance.params.vibrance = i as f32 / 120.0;
        stack.denoise.params.luminance = 0.3 + i as f32 / 200.0;
        // A curve node dragged too: its LUT is written, not made again.
        stack.tone_curve.params = vec![
            CurveNode::new(0.0, 0.0),
            CurveNode::new(0.5, 0.4 + i as f32 / 300.0),
            CurveNode::new(1.0, 1.0),
        ];
        frame(&mut engine, &stack);
        assert_eq!(
            photo_engine::gpu_objects_made(),
            made,
            "frame {i} of the drag made a bind group, texture or buffer"
        );
        let now = alive(&device);
        assert_eq!(
            now, before,
            "frame {i} of the drag changed what is alive on the GPU \
             ([bind groups, textures, views, buffers])"
        );
    }
}
