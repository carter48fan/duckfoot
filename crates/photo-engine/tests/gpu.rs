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
    adapter: wgpu::Adapter,
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
        Some(Gpu {
            adapter,
            device,
            queue,
        })
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
        let exported = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
        assert_eq!((exported.width, exported.height), (pw, ph));
        // Same shader, same uniforms, same texture: not close, identical (invariant 2).
        assert!(
            exported.rgba == shown,
            "export and preview differ with {turns} quarter turns"
        );
    }
}

#[test]
fn a_flat_field_fills_the_histogram_bins_the_file_holds() {
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
        engine.poll_histogram(&gpu.device, &gpu.queue);
        if engine.histogram().peak > 0 {
            break;
        }
    }
    let hist = engine.histogram().clone();
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();

    // The 8-bit output is dithered, so a level between two codes is a mix of both: the
    // histogram shows exactly that mix, no more and no less.
    for (c, (name, bins)) in [
        ("red", &hist.red),
        ("green", &hist.green),
        ("blue", &hist.blue),
    ]
    .into_iter()
    .enumerate()
    {
        let mut want = vec![0u32; bins.len()];
        for p in out.rgba.chunks_exact(4) {
            want[p[c] as usize] += 1;
        }
        let populated: Vec<usize> = (0..bins.len()).filter(|&i| bins[i] > 0).collect();
        assert!(
            populated.len() <= 2 && populated.windows(2).all(|w| w[1] == w[0] + 1),
            "{name}: a uniform frame filled bins {populated:?}"
        );
        // The scope samples the preview, not every pixel of the file: the mix, not the
        // counts, is what must agree.
        let (total, file_total) = (
            bins.iter().sum::<u32>() as f32,
            want.iter().sum::<u32>() as f32,
        );
        for i in 0..bins.len() {
            let (a, b) = (bins[i] as f32 / total, want[i] as f32 / file_total);
            assert!(
                (a - b).abs() < 0.1,
                "{name}: bin {i} holds {a:.3} of the scope but {b:.3} of the file"
            );
        }
    }
    let luma: Vec<usize> = (0..hist.luma.len()).filter(|&i| hist.luma[i] > 0).collect();
    assert!(
        luma.len() <= 3,
        "luma of a uniform frame spread over {luma:?}"
    );
}

/// Rows `y0..y0+h`, columns `x0..x0+w` of a tightly packed RGBA8 image `width` wide.
fn sub_rect(px: &[u8], width: u32, (x0, y0, w, h): (u32, u32, u32, u32)) -> Vec<u8> {
    let row = width as usize * 4;
    (y0..y0 + h)
        .flat_map(|y| {
            let start = y as usize * row + x0 as usize * 4;
            px[start..start + w as usize * 4].iter().copied()
        })
        .collect()
}

#[test]
fn a_rotated_export_takes_its_size_and_crop_from_the_stack() {
    let gpu = gpu_or_skip!();
    let (w, h) = (64, 48);
    let scene = testkit::cfa_from_fn(w, h, testkit::smooth_scene(w, h));

    // What batch export does: load, then export a stack that turns the frame, with no
    // `set_geometry` in between. The engine still thinks the frame is landscape.
    let mut batch = Engine::new(&gpu.device);
    batch.load(&gpu.device, &gpu.queue, scene).unwrap();
    let mut stack = PhotoStack::default();
    stack.geometry.quarter_turns = 1;
    stack.geometry.crop = Crop {
        x: 0.25,
        y: 0.125,
        width: 0.5,
        height: 0.75,
    };
    let exported = batch.export(&gpu.device, &gpu.queue, &stack).unwrap();

    // Portrait, and cropped in the portrait frame.
    let rect = stack.geometry.crop.to_pixels(h, w);
    assert_eq!((exported.width, exported.height), (rect.2, rect.3));
    assert_eq!((rect.2, rect.3), (24, 48));

    // Pixel for pixel what the editor shows for that stack, where geometry was applied.
    let mut editor = Engine::new(&gpu.device);
    editor
        .load(
            &gpu.device,
            &gpu.queue,
            testkit::cfa_from_fn(w, h, testkit::smooth_scene(w, h)),
        )
        .unwrap();
    let (pw, ph, shown) = preview(gpu, &mut editor, &stack, 0);
    assert_eq!((pw, ph), (h, w));
    assert!(
        exported.rgba == sub_rect(&shown, pw, rect),
        "the batch export is not the frame the editor shows"
    );
    // No black bars: the smooth scene has no black anywhere.
    let black = exported
        .rgba
        .chunks_exact(4)
        .filter(|p| p[..3] == [0, 0, 0])
        .count();
    assert_eq!(black, 0, "{black} black pixels in a scene that has none");
}

#[test]
fn a_banded_export_equals_a_single_shot_one() {
    let gpu = gpu_or_skip!();
    let (w, h) = (100, 70);
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(
            &gpu.device,
            &gpu.queue,
            testkit::cfa_from_fn(w, h, testkit::smooth_scene(w, h)),
        )
        .unwrap();
    let mut stack = busy_stack();
    // An offset crop, so each band's source origin is exercised too.
    stack.geometry.crop = Crop {
        x: 0.13,
        y: 0.21,
        width: 0.6,
        height: 0.7,
    };
    let whole = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();

    // One padded row is 256 bytes at this width: bands of one row, then of three, then a
    // size that is not a whole number of rows.
    for band in [1, 256 * 3, 1000] {
        engine.set_export_band_bytes(Some(band));
        let banded = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
        assert_eq!((banded.width, banded.height), (whole.width, whole.height));
        assert!(
            banded.rgba == whole.rgba,
            "bands of {band} bytes changed the export"
        );
    }
}

#[test]
fn export_fits_under_a_small_device_buffer_limit() {
    let gpu = gpu_or_skip!();
    // The B7 crash in miniature: a device whose largest buffer is smaller than the frame.
    // Before banding this was one 512 KiB readback buffer, a validation error, and a panic.
    let limit = 256 << 10;
    let (device, queue) = pollster::block_on(gpu.adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("small buffers"),
        required_limits: wgpu::Limits {
            max_buffer_size: limit,
            ..wgpu::Limits::downlevel_defaults().using_resolution(gpu.adapter.limits())
        },
        ..Default::default()
    }))
    .unwrap();
    let (w, h) = (512, 256);
    let mut engine = Engine::new(&device);
    engine
        .load(
            &device,
            &queue,
            testkit::cfa_from_fn(w, h, testkit::smooth_scene(w, h)),
        )
        .unwrap();
    let stack = PhotoStack::default();
    let exported = engine.export(&device, &queue, &stack).unwrap();
    assert!(exported.rgba.len() as u64 > limit);
    assert_eq!((exported.width, exported.height), (w, h));

    engine.render(&device, &queue, &stack, None);
    let shown = read_texture(&device, &queue, engine.image().unwrap().output_texture()).unwrap();
    assert!(
        exported.rgba == shown,
        "the banded export is not the preview"
    );
}

/// Render patches of exact linear scene values and return each patch's centre pixel.
fn render_patches(gpu: &Gpu, stack: &PhotoStack, patches: &[[f32; 3]]) -> Vec<[u8; 3]> {
    const SIDE: u32 = 8;
    let n = patches.len() as u32;
    let raster = testkit::raster_from_fn(SIDE * n, SIDE, |x, _| patches[(x / SIDE) as usize]);
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let out = engine.export(&gpu.device, &gpu.queue, stack).unwrap();
    (0..n)
        .map(|i| {
            let at = ((SIDE / 2 * out.width + i * SIDE + SIDE / 2) * 4) as usize;
            [out.rgba[at], out.rgba[at + 1], out.rgba[at + 2]]
        })
        .collect()
}

/// Hue in degrees of a linear RGB triple.
fn hue(c: [f32; 3]) -> f32 {
    let [r, g, b] = c;
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

#[test]
fn filmic_maps_mid_grey_to_mid_grey_and_its_range_to_black_and_white() {
    let gpu = gpu_or_skip!();
    let mut stack = PhotoStack::default();
    stack.filmic.enabled = true;
    let f = stack.filmic.params;
    // The defaults this is all derived against.
    assert_eq!(
        (f.white_ev, f.black_ev, f.latitude, f.contrast),
        (3.5, -8.0, 20.0, 1.1)
    );

    let red = [0.5, 0.1, 0.05];
    let out = render_patches(
        gpu,
        &stack,
        &[
            [0.18; 3],
            [2f32.powf(f.black_ev); 3],
            [2f32.powf(f.white_ev); 3],
            red,
        ],
    );
    let [grey, black, white, shown_red] = [out[0], out[1], out[2], out[3]];
    println!("filmic: grey {grey:?}  black {black:?}  white {white:?}  red {shown_red:?}");

    // Filmic off shows 0.18 as 118. Encoding the sigmoid twice showed it as 184, and lifted
    // black_ev to 69 — the flat, grey look.
    assert!(
        grey.iter().all(|&v| v.abs_diff(118) <= 2),
        "mid grey came out {grey:?}, not 118"
    );
    assert!(black.iter().all(|&v| v <= 2), "black_ev came out {black:?}");
    assert!(
        white.iter().all(|&v| v >= 253),
        "white_ev came out {white:?}"
    );

    // Applied to the pixel's brightness, not to each channel: the hue survives.
    let shown = shown_red.map(testkit::srgb_to_linear);
    let drift = (hue(shown) - hue(red)).abs();
    assert!(
        drift <= 3.0,
        "a saturated red moved {drift:.1}° in hue ({red:?} -> {shown:?})"
    );
}

#[test]
fn saturated_colours_never_go_nan_through_local_contrast() {
    let gpu = gpu_or_skip!();
    // A pure camera-blue object on grey, through a real camera matrix (the Sony A7 III's, with its
    // negative coefficients). Camera blue lands outside sRGB with a negative red and green
    // and a luminance below zero, and its edges blend into grey whose luminance is not:
    // the log of their ratio is NaN, which survives every clamp after it and ends as a
    // black speck. Stage lights, LEDs and violet flowers are exactly this.
    //
    // WGSL leaves NaN through `clamp` to the implementation, and lavapipe's returns the
    // bound, so on CI this guards the whole chain rather than reproducing the original
    // bug; GPUs whose `clamp` propagates it are where the specks came from.
    let mut cfa = testkit::cfa_from_fn(96, 64, |x, y| {
        let (dx, dy) = (x as f32 - 48.0, y as f32 - 32.0);
        if dx * dx + dy * dy < 20.0 * 20.0 {
            [0.0, 0.0, 0.9]
        } else {
            [0.2, 0.2, 0.2]
        }
    });
    cfa.cam_to_srgb = testkit::SONY_A7III;
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    engine.set_nan_paint(true);
    let mut stack = busy_stack();
    stack.dehaze.enabled = true;
    stack.dehaze.params.amount = 0.5;
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    // The NaN paint is the only thing that writes alpha below 255.
    let bad = out.rgba.chunks_exact(4).filter(|p| p[3] != 255).count();
    assert_eq!(bad, 0, "{bad} pixels went NaN or infinite");
}

#[test]
fn the_nan_paint_sees_a_value_that_went_infinite() {
    let gpu = gpu_or_skip!();
    // Without this, a NaN test that passes might only mean the detector is blind.
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(32, 32, 0.2))
        .unwrap();
    engine.set_nan_paint(true);
    let mut stack = PhotoStack::default();
    stack.exposure.params.ev = 200.0; // a gain of 2^200: infinity in f32
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let bad = out.rgba.chunks_exact(4).filter(|p| p[3] != 255).count();
    assert!(bad > 0, "an infinite exposure gain was not flagged");
}

/// Box-downscale an sRGB RGBA8 image by `f` in linear light, as the preview's mips do.
fn downscale(px: &[u8], w: u32, h: u32, f: u32) -> Vec<[f32; 3]> {
    let (ow, oh) = (w / f, h / f);
    let mut out = Vec::with_capacity((ow * oh) as usize);
    for oy in 0..oh {
        for ox in 0..ow {
            let mut acc = [0.0f32; 3];
            for y in oy * f..(oy + 1) * f {
                for x in ox * f..(ox + 1) * f {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..3 {
                        acc[c] += testkit::srgb_to_linear(px[i + c]);
                    }
                }
            }
            out.push(acc.map(|v| v / (f * f) as f32));
        }
    }
    out
}

/// Mean absolute difference, in linear light × 255, between a preview at `level` and the
/// full-resolution export downscaled to the preview's size.
fn preview_vs_export(gpu: &Gpu, stack: &PhotoStack, level: u32) -> f32 {
    let (w, h) = (256u32, 192u32);
    let scene = testkit::smooth_scene(w, h);
    // Fine detail on top of the smooth scene: the band sharpen and texture act on.
    let cfa = testkit::cfa_from_fn(w, h, |x, y| {
        let base = scene(x, y);
        let stripe = if (x / 2 + y / 3) % 2 == 0 { 1.15 } else { 0.87 };
        base.map(|v| v * stripe)
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    let exported = engine.export(&gpu.device, &gpu.queue, stack).unwrap();
    let (pw, ph, shown) = preview(gpu, &mut engine, stack, level);
    let f = 1 << level;
    assert_eq!((pw, ph), (exported.width / f, exported.height / f));
    let small = downscale(&exported.rgba, exported.width, exported.height, f);
    let mut sum = 0.0;
    for (i, e) in small.iter().enumerate() {
        for c in 0..3 {
            sum += (testkit::srgb_to_linear(shown[i * 4 + c]) - e[c]).abs();
        }
    }
    sum / (small.len() * 3) as f32 * 255.0
}

#[test]
fn a_reduced_preview_shows_what_the_downscaled_export_will() {
    let gpu = gpu_or_skip!();
    // What the mip-based preview differs by with nothing scale-dependent on: the
    // tolerance the scale-dependent modules are measured against.
    let plain = PhotoStack::default();
    let base = preview_vs_export(gpu, &plain, 2);

    let mut sharp = PhotoStack::default();
    sharp.sharpen.enabled = true;
    sharp.sharpen.params.amount = 1.0;
    let sharpened = preview_vs_export(gpu, &sharp, 2);

    let mut punchy = PhotoStack::default();
    punchy.clarity.enabled = true;
    punchy.clarity.params.clarity = 60.0;
    let clarity = preview_vs_export(gpu, &punchy, 2);

    let mut textured = PhotoStack::default();
    textured.clarity.enabled = true;
    textured.clarity.params.texture = 100.0;
    let texture = preview_vs_export(gpu, &textured, 2);

    println!(
        "preview vs export at level 2: plain {base:.2}, sharpen {sharpened:.2}, \
         clarity {clarity:.2}, texture {texture:.2}"
    );
    // Sharpening in preview texels made fit view 11 over the export here; clarity is on
    // absolute levels. Both now match the export as closely as a plain render does.
    assert!(
        sharpened <= base + 1.0,
        "sharpen: {sharpened:.2} vs {base:.2} plain"
    );
    assert!(
        clarity <= base + 1.0,
        "clarity: {clarity:.2} vs {base:.2} plain"
    );
    // Texture's band is below a fit preview's resolution; it is approximated (see
    // `local_contrast`) and held to that approximation's measured accuracy.
    assert!(
        texture <= base + 2.5,
        "texture: {texture:.2} vs {base:.2} plain"
    );
}

#[test]
fn the_vignette_centres_on_the_crop() {
    let gpu = gpu_or_skip!();
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(128, 64, 0.3))
        .unwrap();
    let mut stack = PhotoStack::default();
    stack.vignette.enabled = true;
    stack.vignette.params.amount = -0.6;
    // The right half only: centred on the whole frame, its left edge would sit at the
    // frame's centre and stay bright while its right edge darkened.
    stack.geometry.crop = Crop {
        x: 0.5,
        y: 0.0,
        width: 0.5,
        height: 1.0,
    };
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let column = |x: u32| -> f32 {
        (0..out.height)
            .map(|y| out.rgba[((y * out.width + x) * 4) as usize] as f32)
            .sum::<f32>()
            / out.height as f32
    };
    let (left, right, middle) = (column(0), column(out.width - 1), column(out.width / 2));
    assert!(
        (left - right).abs() <= 1.0,
        "edges {left:.1} and {right:.1} should darken alike"
    );
    assert!(middle > left + 5.0, "no vignette: {left:.1} .. {middle:.1}");
}

#[test]
fn colour_eq_moves_every_hue_by_what_the_sliders_say() {
    let gpu = gpu_or_skip!();
    // Every band's saturation at -100 takes every hue to grey: under the old weights a hue
    // between two bands (90°, between yellow and green) kept half its colour, and one near
    // two of them (15°) was pushed past grey.
    let hue_patch = |h: f32| -> [f32; 3] {
        let x = 1.0 - ((h / 60.0) % 2.0 - 1.0).abs();
        let rgb = match (h / 60.0) as u32 {
            0 => [1.0, x, 0.0],
            1 => [x, 1.0, 0.0],
            2 => [0.0, 1.0, x],
            3 => [0.0, x, 1.0],
            4 => [x, 0.0, 1.0],
            _ => [1.0, 0.0, x],
        };
        rgb.map(|v| 0.05 + 0.4 * v)
    };
    let hues = [0.0, 15.0, 45.0, 90.0, 150.0, 210.0, 260.0, 300.0, 340.0];
    let patches: Vec<[f32; 3]> = hues.iter().map(|&h| hue_patch(h)).collect();
    let mut stack = PhotoStack::default();
    stack.color_eq.enabled = true;
    for (_, band) in stack.color_eq.params.bands_mut() {
        band.saturation = -100.0;
    }
    let out = render_patches(gpu, &stack, &patches);
    for (h, px) in hues.iter().zip(&out) {
        let spread = px.iter().max().unwrap() - px.iter().min().unwrap();
        assert!(spread <= 2, "hue {h}° kept colour: {px:?}");
    }
}

#[test]
fn the_before_half_of_the_split_is_the_file_as_shot() {
    let gpu = gpu_or_skip!();
    let (w, h) = (96u32, 64u32);
    let scene = testkit::smooth_scene(w, h);
    let render = |stack: &PhotoStack| {
        let mut engine = Engine::new(&gpu.device);
        engine
            .load(&gpu.device, &gpu.queue, testkit::cfa_from_fn(w, h, &scene))
            .unwrap();
        engine.set_preview_level(&gpu.device, 0);
        // The whole frame is "before".
        engine.render(&gpu.device, &gpu.queue, stack, Some(1.0));
        read_texture(
            &gpu.device,
            &gpu.queue,
            engine.image().unwrap().output_texture(),
        )
        .unwrap()
    };
    let mut sharpened = PhotoStack::default();
    sharpened.sharpen.enabled = true;
    sharpened.sharpen.params.amount = 1.0;
    assert!(
        render(&sharpened) == render(&PhotoStack::default()),
        "the before half was sharpened"
    );
}

#[test]
fn the_last_render_of_a_drag_is_the_one_the_histogram_shows() {
    let gpu = gpu_or_skip!();
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(64, 48, 0.1))
        .unwrap();
    // A drag: two renders with no collection between them. The first is measured; the
    // second finds the readback busy and is owed its measurement.
    let first = PhotoStack::default();
    let mut last = PhotoStack::default();
    last.exposure.params.ev = 2.0;
    engine.render(&gpu.device, &gpu.queue, &first, None);
    engine.render(&gpu.device, &gpu.queue, &last, None);
    // Then nothing changes: no render, only the polling an idle app does while
    // `histogram_busy` says to come back.
    let mut polls = 0;
    while engine.histogram_busy() && polls < 500 {
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        engine.poll_histogram(&gpu.device, &gpu.queue);
        polls += 1;
    }
    assert!(!engine.histogram_busy(), "the histogram never settled");
    let value = engine.export(&gpu.device, &gpu.queue, &last).unwrap().rgba[1] as usize;
    assert!(
        engine.histogram().luma[value] > 0,
        "the scopes show an earlier frame, not the last render's {value}"
    );
}

#[test]
fn a_curve_node_moves_the_display_value_it_sits_on() {
    let gpu = gpu_or_skip!();
    // 0.214 linear displays as 50% grey. A node at (0.5, 0.6) on the display-domain curve
    // must show it at 60%, and an identity curve must change nothing.
    let half = testkit::srgb_to_linear(128);
    let mut stack = PhotoStack::default();
    stack.tone_curve.enabled = true;
    let plain = render_patches(gpu, &stack, &[[half; 3]])[0];
    assert!(
        plain.iter().all(|&v| v.abs_diff(128) <= 1),
        "identity curve moved 50% grey to {plain:?}"
    );
    stack.tone_curve.params = vec![
        photo_engine::CurveNode::new(0.0, 0.0),
        photo_engine::CurveNode::new(0.5, 0.6),
        photo_engine::CurveNode::new(1.0, 1.0),
    ];
    let lifted = render_patches(gpu, &stack, &[[half; 3]])[0];
    assert!(
        lifted.iter().all(|&v| v.abs_diff(153) <= 2),
        "a node at 0.5 -> 0.6 showed 50% grey as {lifted:?}, not 153"
    );
}

#[test]
fn toggling_curves_renders_what_a_fresh_engine_does() {
    let gpu = gpu_or_skip!();
    // The curve LUT is cached on the engine, so every state is rendered on one engine and
    // compared with a fresh one, which has no cache to go stale.
    let half = testkit::srgb_to_linear(128);
    let raster = testkit::raster_from_fn(8, 8, |_, _| [half; 3]);
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let mut cached = |stack: &PhotoStack| {
        let out = engine.export(&gpu.device, &gpu.queue, stack).unwrap();
        let at = ((4 * out.width + 4) * 4) as usize;
        [out.rgba[at], out.rgba[at + 1], out.rgba[at + 2]]
    };

    let mut stack = PhotoStack::default();
    stack.tone_curve.params = vec![
        photo_engine::CurveNode::new(0.0, 0.0),
        photo_engine::CurveNode::new(0.5, 0.6),
        photo_engine::CurveNode::new(1.0, 1.0),
    ];
    stack.rgb_curves.params.red = vec![
        photo_engine::CurveNode::new(0.0, 0.0),
        photo_engine::CurveNode::new(0.5, 0.3),
        photo_engine::CurveNode::new(1.0, 1.0),
    ];

    // Master only, both, RGB turned back off, then master off with RGB still on.
    for (master, rgb) in [(true, false), (true, true), (true, false), (false, true)] {
        stack.tone_curve.enabled = master;
        stack.rgb_curves.enabled = rgb;
        let fresh = render_patches(gpu, &stack, &[[half; 3]])[0];
        assert_eq!(
            cached(&stack),
            fresh,
            "master {master}, rgb {rgb}: the cached LUT renders differently from a fresh one"
        );
    }
}

#[test]
fn a_uniform_colour_is_uniform_out_to_the_frame_edge() {
    let gpu = gpu_or_skip!();
    // One colour everywhere. Every pixel, edge and corner included, must come out as the
    // centre does: reading past the edge must keep the mosaic's colour phase, or the
    // outermost pixels are interpolated from the wrong channel and come out tinted.
    let cfa = testkit::cfa_from_fn(40, 30, |_, _| [0.45, 0.2, 0.08]);
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    let out = engine
        .export(&gpu.device, &gpu.queue, &PhotoStack::default())
        .unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * out.width + x) * 4) as usize;
        [out.rgba[i], out.rgba[i + 1], out.rgba[i + 2]]
    };
    let centre = at(out.width / 2, out.height / 2);
    for y in 0..out.height {
        for x in 0..out.width {
            let px = at(x, y);
            let off = (0..3).map(|c| px[c].abs_diff(centre[c])).max().unwrap();
            assert!(off <= 1, "({x}, {y}) is {px:?}, the centre {centre:?}");
        }
    }
}

#[test]
fn a_16_bit_export_keeps_the_levels_an_8_bit_one_rounds_away() {
    let gpu = gpu_or_skip!();
    // A slow ramp: 2048 px across a stop and a half of shadows and mid-tones.
    let (w, h) = (2048u32, 8u32);
    let cfa = testkit::cfa_from_fn(w, h, |x, _| {
        let v = 0.05 + 0.25 * x as f32 / w as f32;
        [v, v, v]
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    let stack = PhotoStack::default();
    let deep = engine.export_16(&gpu.device, &gpu.queue, &stack).unwrap();
    let eight = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    assert_eq!((deep.width, deep.height), (eight.width, eight.height));

    let row = h / 2;
    let levels = |px: &[u16]| {
        (0..w)
            .map(|x| px[((row * w + x) * 4 + 1) as usize])
            .collect::<std::collections::BTreeSet<u16>>()
            .len()
    };
    let deep_levels = levels(&deep.rgba);
    let eight_levels = (0..w)
        .map(|x| eight.rgba[((row * w + x) * 4 + 1) as usize])
        .collect::<std::collections::BTreeSet<u8>>()
        .len();
    assert!(
        deep_levels > 4 * eight_levels,
        "16-bit held {deep_levels} levels, 8-bit {eight_levels}"
    );
    // The same picture: the 8-bit one is the 16-bit one rounded, give or take the dither.
    let worst = deep
        .rgba
        .iter()
        .zip(&eight.rgba)
        .map(|(&d, &e)| (d as f32 / 257.0 - e as f32).abs())
        .fold(0.0f32, f32::max);
    // Half a level of dither, half of rounding, and the half-float's own step.
    assert!(worst <= 1.15, "8-bit strays {worst} levels from 16-bit");
}

#[test]
fn the_dither_shifts_no_tone() {
    let gpu = gpu_or_skip!();
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(256, 256, 0.21))
        .unwrap();
    let stack = PhotoStack::default();
    let eight = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let deep = engine.export_16(&gpu.device, &gpu.queue, &stack).unwrap();
    let mean8 = eight.rgba.chunks_exact(4).map(|p| p[1] as f64).sum::<f64>()
        / (eight.rgba.len() / 4) as f64;
    let mean16 = deep
        .rgba
        .chunks_exact(4)
        .map(|p| p[1] as f64 / 257.0)
        .sum::<f64>()
        / (deep.rgba.len() / 4) as f64;
    assert!(
        (mean8 - mean16).abs() < 0.1,
        "dithered mean {mean8:.3} against the true level {mean16:.3}"
    );
}

#[test]
fn the_clipping_overlay_paints_the_preview_and_never_the_export() {
    let gpu = gpu_or_skip!();
    // Left third black, middle grey, right third far past white.
    let (w, h) = (96u32, 16u32);
    let cfa = testkit::cfa_from_fn(w, h, |x, _| match x * 3 / w {
        0 => [0.0, 0.0, 0.0],
        1 => [0.18, 0.18, 0.18],
        _ => [0.95, 0.95, 0.95],
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    let mut stack = PhotoStack::default();
    stack.exposure.params.ev = 2.0;
    engine.render(&gpu.device, &gpu.queue, &stack, None);
    let clean_export = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let clean_preview = testkit::read_texture(
        &gpu.device,
        &gpu.queue,
        engine.image().unwrap().output_texture(),
    )
    .unwrap();

    assert!(
        engine.set_clip_overlay(&gpu.device, true),
        "the stage must re-register"
    );
    engine.render(&gpu.device, &gpu.queue, &stack, None);
    let shown =
        testkit::read_texture(&gpu.device, &gpu.queue, engine.display_texture().unwrap()).unwrap();
    let (pw, _) = engine.image().unwrap().preview_size();
    let at = |x: u32| {
        let i = ((h / 2 * pw + x) * 4) as usize;
        [shown[i], shown[i + 1], shown[i + 2]]
    };
    assert_eq!(at(pw / 6), [0, 89, 255], "crushed black shows blue");
    assert_eq!(at(pw * 5 / 6), [255, 0, 0], "blown white shows red");
    let mid = at(pw / 2);
    assert!(
        mid[0] == mid[1] && mid[1] == mid[2],
        "mid-grey shows as itself: {mid:?}"
    );

    // The preview under it, and the export, are exactly what they were.
    let preview = testkit::read_texture(
        &gpu.device,
        &gpu.queue,
        engine.image().unwrap().output_texture(),
    )
    .unwrap();
    assert!(
        preview == clean_preview,
        "the overlay leaked into the preview"
    );
    let export = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    assert!(
        export.rgba == clean_export.rgba,
        "the overlay leaked into the export"
    );

    assert!(engine.set_clip_overlay(&gpu.device, false));
    assert!(std::ptr::eq(
        engine.display_texture().unwrap(),
        engine.image().unwrap().output_texture()
    ));
}

/// Mean of each channel over a centred box of an export.
fn centre_mean(out: &photo_engine::ExportedImage, side: u32) -> [f32; 3] {
    let (cx, cy) = (out.width / 2, out.height / 2);
    let mut sum = [0.0f32; 3];
    let mut n = 0.0;
    for y in cy - side / 2..cy + side / 2 {
        for x in cx - side / 2..cx + side / 2 {
            let i = ((y * out.width + x) * 4) as usize;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += out.rgba[i + c] as f32;
            }
            n += 1.0;
        }
    }
    sum.map(|s| s / n)
}

#[test]
fn the_eyedropper_neutralises_a_grey_card_under_tungsten() {
    let gpu = gpu_or_skip!();
    // Grey under warm light, as the camera records it: red strong, blue weak. The right
    // half is a blue card, which must not be what the dropper reads.
    let (w, h) = (96u32, 64u32);
    let cfa = testkit::cfa_from_fn(w, h, |x, _| {
        if x < w / 2 {
            [0.30 * 1.7, 0.30, 0.30 * 0.55]
        } else {
            [0.05, 0.10, 0.40]
        }
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    let mut stack = PhotoStack::default();
    // Turned and mirrored, so the tap has to be mapped back to the sensor to land on grey:
    // one clockwise turn puts the sensor's left half at the top.
    stack.geometry.rotate_right();
    stack.geometry.flip_h();
    let wb = engine
        .neutral_at(&gpu.device, &gpu.queue, &stack, (0.5, 0.25))
        .unwrap()
        .expect("a grey patch");
    stack.white_balance.params = wb;
    stack.white_balance.enabled = true;
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    // The grey is now the top half of the turned frame.
    let i = (((out.height / 4) * out.width + out.width / 2) * 4) as usize;
    let px = [out.rgba[i], out.rgba[i + 1], out.rgba[i + 2]].map(|v| v as i32);
    let spread = px.iter().max().unwrap() - px.iter().min().unwrap();
    assert!(spread <= 3, "not neutral: {px:?}");

    // On the blue card it would have picked a very different balance.
    let blue = engine
        .neutral_at(&gpu.device, &gpu.queue, &stack, (0.5, 0.75))
        .unwrap()
        .unwrap();
    assert!(blue.tint_b < wb.tint_b * 0.5, "{blue:?} vs {wb:?}");
}

#[test]
fn auto_white_balance_and_auto_tone_fix_a_cast_dark_frame() {
    let gpu = gpu_or_skip!();
    // A dim, warm-cast scene with some texture.
    let scene = testkit::smooth_scene(128, 96);
    let cfa = testkit::cfa_from_fn(128, 96, |x, y| {
        let [r, g, b] = scene(x, y);
        let v = (r + g + b) / 3.0 * 0.12 + 0.01;
        [v * 1.6, v, v * 0.6]
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    let mut stack = PhotoStack::default();
    stack.white_balance.params = engine
        .auto_white_balance(&gpu.device, &gpu.queue)
        .unwrap()
        .expect("an estimate");
    stack.white_balance.enabled = true;
    let tone = engine
        .auto_tone(&gpu.device, &gpu.queue, &stack)
        .unwrap()
        .expect("an estimate");
    assert!(tone.ev > 1.0, "a dark frame is brightened: {tone:?}");
    stack.exposure.params.ev = tone.ev;
    stack.tonal.params = tone.tonal;
    stack.tonal.enabled = true;
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let mut all: Vec<u8> = out.rgba.chunks_exact(4).map(|p| p[1]).collect();
    all.sort_unstable();
    let median = all[all.len() / 2] as i32;
    assert!(
        (100..=140).contains(&median),
        "median {median}, want mid grey (~118)"
    );
    let [r, g, b] = centre_mean(&out, 16);
    assert!(
        (r - g).abs() < 6.0 && (b - g).abs() < 6.0,
        "cast left: {r:.1} {g:.1} {b:.1}"
    );
}

/// The red and green of one export row, as 0..255 values.
fn row_channels(out: &photo_engine::ExportedImage, y: u32) -> (Vec<f32>, Vec<f32>) {
    let at = |x: u32, c: u32| out.rgba[((y * out.width + x) * 4 + c) as usize] as f32;
    (
        (0..out.width).map(|x| at(x, 0)).collect(),
        (0..out.width).map(|x| at(x, 1)).collect(),
    )
}

/// Where a rising edge crosses half way, to a fraction of a pixel.
fn edge_at(row: &[f32]) -> f32 {
    let (lo, hi) = (row[0], row[row.len() - 1]);
    let mid = (lo + hi) * 0.5;
    let i = row
        .windows(2)
        .position(|w| w[0] < mid && w[1] >= mid)
        .unwrap();
    i as f32 + (mid - row[i]) / (row[i + 1] - row[i])
}

#[test]
fn distortion_moves_the_frame_where_the_eyedropper_thinks_it_does() {
    let gpu = gpu_or_skip!();
    // A bright spot on the sensor where, with barrel removed, the output pixel near the
    // corner should come from — as the CPU mirror of the mapping works it out.
    let (w, h) = (256u32, 128u32);
    let mut stack = PhotoStack::default();
    stack.lens.enabled = true;
    stack.lens.params.distortion = 80.0;
    let target = (230.0, 110.0);
    let (sx, sy) = photo_engine::sensor_point(&stack, None, (w, h), target);
    // Pulled toward the middle, as removing barrel does.
    assert!(sx < target.0 - 5.0 && sy < target.1 - 2.0, "({sx}, {sy})");
    let raster = testkit::raster_from_fn(w, h, |x, y| {
        let near = (x as f32 + 0.5 - sx).abs() < 3.0 && (y as f32 + 0.5 - sy).abs() < 3.0;
        [if near { 0.8 } else { 0.02 }; 3]
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let px = |out: &photo_engine::ExportedImage| {
        out.rgba[((target.1 as u32 * out.width + target.0 as u32) * 4) as usize]
    };
    let corrected = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    stack.lens.enabled = false;
    let plain = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    assert!(
        px(&corrected) > 200,
        "the spot is not there: {}",
        px(&corrected)
    );
    assert!(
        px(&plain) < 50,
        "the spot was there all along: {}",
        px(&plain)
    );
}

#[test]
fn fringe_correction_lines_red_up_with_green() {
    let gpu = gpu_or_skip!();
    // A lens that drew red 0.2% larger than green: at 900 px from the centre, the red
    // edge sits 1.8 px further out than the green one.
    let (w, h) = (2048u32, 16u32);
    let (c, edge, m) = (w as f32 * 0.5, 1900.0, 1.002);
    let red_edge = c + (edge - c) * m;
    let raster = testkit::raster_from_fn(w, h, |x, _| {
        let x = x as f32 + 0.5;
        let r = if x > red_edge { 0.6 } else { 0.05 };
        let g = if x > edge { 0.6 } else { 0.05 };
        [r, g, g]
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let mut stack = PhotoStack::default();
    let gap = |engine: &mut Engine, stack: &PhotoStack| {
        let out = engine.export(&gpu.device, &gpu.queue, stack).unwrap();
        let (red, green) = row_channels(&out, h / 2);
        edge_at(&red[1800..]) - edge_at(&green[1800..])
    };
    let before = gap(&mut engine, &stack);
    assert!((before - 1.8).abs() < 0.3, "the test scene: {before}");
    stack.lens.enabled = true;
    stack.lens.params.ca_red = 100.0;
    let after = gap(&mut engine, &stack);
    assert!(after.abs() < 0.3, "still {after} px apart");
}

#[test]
fn vignetting_correction_flattens_a_lens_falloff() {
    let gpu = gpu_or_skip!();
    // A flat field as a lens that loses 1.2 stops at the corner records it.
    let (w, h) = (192u32, 128u32);
    let half = (w as f32 * 0.5, h as f32 * 0.5);
    let raster = testkit::raster_from_fn(w, h, |x, y| {
        let (dx, dy) = (x as f32 + 0.5 - half.0, y as f32 + 0.5 - half.1);
        let r2 = (dx * dx + dy * dy) / (half.0 * half.0 + half.1 * half.1);
        [0.25 * (-1.2 * r2).exp2(); 3]
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let mut stack = PhotoStack::default();
    let spread = |engine: &mut Engine, stack: &PhotoStack| {
        let out = engine.export(&gpu.device, &gpu.queue, stack).unwrap();
        let at = |x: u32, y: u32| out.rgba[((y * out.width + x) * 4 + 1) as usize] as f32;
        at(w / 2, h / 2) - at(1, 1)
    };
    let before = spread(&mut engine, &stack);
    assert!(before > 40.0, "the test scene: {before}");
    stack.lens.enabled = true;
    // +100 is 1.5 stops at the corner; 1.2 is 80.
    stack.lens.params.vignetting = 80.0;
    let after = spread(&mut engine, &stack);
    assert!(after.abs() < 4.0, "corner still {after} below the centre");
}

/// A fixed pseudo-random value in -1..1 for each (x, y, channel): noise a test can repeat.
fn noise(x: u32, y: u32, c: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// A step from dark to light grey down the middle, clean and with high-ISO grain: shot
/// noise in each channel on its own, so both grain and colour noise.
fn noisy_step(w: u32, h: u32, grain: f32) -> photo_engine::RasterImage {
    testkit::raster_from_fn(w, h, move |x, y| {
        let v: f32 = if x < w / 2 { 0.12 } else { 0.4 };
        let spread = grain * v.sqrt();
        [0, 1, 2].map(|c| (v + spread * 1.7 * noise(x, y, c)).max(0.0))
    })
}

fn psnr(a: &[u8], b: &[u8]) -> f32 {
    let mse = a
        .chunks(4)
        .zip(b.chunks(4))
        .map(|(p, q)| {
            (0..3)
                .map(|c| (p[c] as f32 - q[c] as f32).powi(2))
                .sum::<f32>()
        })
        .sum::<f32>()
        / (a.len() / 4 * 3) as f32;
    10.0 * (255.0f32 * 255.0 / mse.max(1e-6)).log10()
}

fn denoising() -> PhotoStack {
    let mut stack = PhotoStack::default();
    stack.denoise.enabled = true;
    stack.denoise.params.luminance = 60.0;
    stack.denoise.params.color = 60.0;
    stack
}

#[test]
fn denoise_removes_grain_and_keeps_the_edge() {
    let gpu = gpu_or_skip!();
    let (w, h) = (192u32, 128u32);
    let export = |raster, stack: &PhotoStack| {
        let mut engine = Engine::new(&gpu.device);
        engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
        engine.export(&gpu.device, &gpu.queue, stack).unwrap()
    };
    let clean = export(noisy_step(w, h, 0.0), &PhotoStack::default());
    let noisy = export(noisy_step(w, h, 0.03), &PhotoStack::default());
    let denoised = export(noisy_step(w, h, 0.03), &denoising());
    let (before, after) = (
        psnr(&noisy.rgba, &clean.rgba),
        psnr(&denoised.rgba, &clean.rgba),
    );
    assert!(
        after >= before + 6.0,
        "PSNR {before:.1} dB -> {after:.1} dB"
    );

    // The step is still a step: the mean profile across it rises within the same couple
    // of pixels as the clean one.
    let profile = |out: &photo_engine::ExportedImage| -> Vec<f32> {
        (0..w)
            .map(|x| {
                (0..h)
                    .map(|y| out.rgba[((y * w + x) * 4 + 1) as usize] as f32)
                    .sum::<f32>()
                    / h as f32
            })
            .collect()
    };
    let rise = |p: &[f32]| {
        let (lo, hi) = (p[w as usize / 4], p[w as usize * 3 / 4]);
        let at = |f: f32| p.iter().position(|v| *v >= lo + (hi - lo) * f).unwrap();
        at(0.9) - at(0.1)
    };
    let (sharp, kept) = (rise(&profile(&clean)), rise(&profile(&denoised)));
    assert!(
        kept <= sharp + 1,
        "the edge spread from {sharp} to {kept} px"
    );
}

#[test]
fn colour_noise_reduction_takes_out_false_colour() {
    let gpu = gpu_or_skip!();
    let (w, h) = (128u32, 96u32);
    let chroma_spread = |stack: &PhotoStack| {
        let mut engine = Engine::new(&gpu.device);
        engine
            .load_raster(&gpu.device, &gpu.queue, noisy_step(w, h, 0.03))
            .unwrap();
        let out = engine.export(&gpu.device, &gpu.queue, stack).unwrap();
        // Red against green, across the flat light half away from the step.
        let d: Vec<f32> = (8..h - 8)
            .flat_map(|y| (w * 5 / 8..w - 8).map(move |x| (x, y)))
            .map(|(x, y)| {
                let i = ((y * w + x) * 4) as usize;
                out.rgba[i] as f32 - out.rgba[i + 1] as f32
            })
            .collect();
        let mean = d.iter().sum::<f32>() / d.len() as f32;
        (d.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / d.len() as f32).sqrt()
    };
    let mut colour_only = PhotoStack::default();
    colour_only.denoise.enabled = true;
    colour_only.denoise.params.luminance = 0.0;
    colour_only.denoise.params.color = 100.0;
    let (before, after) = (
        chroma_spread(&PhotoStack::default()),
        chroma_spread(&colour_only),
    );
    assert!(
        after < before * 0.5,
        "false colour {before:.2} -> {after:.2}"
    );
}

#[test]
fn a_denoised_preview_shows_what_the_denoised_export_will() {
    let gpu = gpu_or_skip!();
    let (w, h) = (256u32, 192u32);
    let level = 2;
    let gap = |stack: &PhotoStack| {
        let mut engine = Engine::new(&gpu.device);
        engine
            .load_raster(&gpu.device, &gpu.queue, noisy_step(w, h, 0.03))
            .unwrap();
        let exported = engine.export(&gpu.device, &gpu.queue, stack).unwrap();
        let (_, _, shown) = preview(gpu, &mut engine, stack, level);
        let f = 1 << level;
        let small = downscale(&exported.rgba, exported.width, exported.height, f);
        let mut sum = 0.0;
        for (i, e) in small.iter().enumerate() {
            for c in 0..3 {
                sum += (testkit::srgb_to_linear(shown[i * 4 + c]) - e[c]).abs();
            }
        }
        sum / (small.len() * 3) as f32 * 255.0
    };
    let (plain, denoised) = (gap(&PhotoStack::default()), gap(&denoising()));
    assert!(
        denoised <= plain + 1.0,
        "preview vs export: {denoised:.2} denoised, {plain:.2} without"
    );
}

#[test]
fn the_before_half_is_not_denoised() {
    let gpu = gpu_or_skip!();
    let (w, h) = (96u32, 64u32);
    let render = |stack: &PhotoStack| {
        let mut engine = Engine::new(&gpu.device);
        engine
            .load_raster(&gpu.device, &gpu.queue, noisy_step(w, h, 0.03))
            .unwrap();
        engine.set_preview_level(&gpu.device, 0);
        engine.render(&gpu.device, &gpu.queue, stack, Some(1.0));
        read_texture(
            &gpu.device,
            &gpu.queue,
            engine.image().unwrap().output_texture(),
        )
        .unwrap()
    };
    assert!(render(&denoising()) == render(&PhotoStack::default()));
}

/// Mean spread between the brightest and darkest channel, away from the frame's edge:
/// on a grey scene, all of it is false colour.
fn false_colour(out: &photo_engine::ExportedImage) -> f32 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in 4..out.height - 4 {
        for x in 4..out.width - 4 {
            let i = ((y * out.width + x) * 4) as usize;
            let px = &out.rgba[i..i + 3];
            sum += (*px.iter().max().unwrap() - *px.iter().min().unwrap()) as f32;
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn x_trans_demosaics_a_grey_scene_about_as_cleanly_as_bayer() {
    let gpu = gpu_or_skip!();
    let (w, h) = (192u32, 144u32);
    let smooth = testkit::smooth_scene(w, h);
    // Grey, with detail at several scales and a hard diagonal edge.
    let scene = move |x: u32, y: u32| {
        let v = smooth(x, y)[1] * if x + y / 2 > 120 { 1.6 } else { 0.7 };
        [v; 3]
    };
    let export = |cfa| {
        let mut engine = Engine::new(&gpu.device);
        engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
        engine
            .export(&gpu.device, &gpu.queue, &PhotoStack::default())
            .unwrap()
    };
    let bayer = false_colour(&export(testkit::cfa_from_fn(w, h, &scene)));
    let pattern = photo_engine::CfaPattern::xtrans(testkit::XTRANS).unwrap();
    let xtrans = false_colour(&export(testkit::mosaic_from_fn(pattern, w, h, &scene)));
    assert!(
        xtrans <= bayer + 1.0,
        "X-Trans {xtrans:.2} vs Bayer {bayer:.2}"
    );
}

#[test]
fn x_trans_keeps_each_patch_its_own_colour() {
    let gpu = gpu_or_skip!();
    let (w, h) = (96u32, 96u32);
    let patches = [
        [0.5, 0.1, 0.1],
        [0.1, 0.4, 0.1],
        [0.1, 0.15, 0.5],
        [0.3, 0.3, 0.3],
    ];
    let scene = move |x: u32, y: u32| patches[((y / 48) * 2 + x / 48) as usize];
    let pattern = photo_engine::CfaPattern::xtrans(testkit::XTRANS).unwrap();
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(
            &gpu.device,
            &gpu.queue,
            testkit::mosaic_from_fn(pattern, w, h, scene),
        )
        .unwrap();
    let out = engine
        .export(&gpu.device, &gpu.queue, &PhotoStack::default())
        .unwrap();
    for (i, want) in patches.iter().enumerate() {
        let (cx, cy) = ((i as u32 % 2) * 48 + 24, (i as u32 / 2) * 48 + 24);
        let at = ((cy * w + cx) * 4) as usize;
        let got: Vec<f32> = (0..3)
            .map(|c| testkit::srgb_to_linear(out.rgba[at + c]))
            .collect();
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() < 0.02,
                "patch {i}: {got:?} for {want:?}"
            );
        }
    }
}

#[test]
fn a_display_p3_jpeg_renders_in_its_own_colours() {
    use image::ImageEncoder;
    let gpu = gpu_or_skip!();
    // Flat patches, as a phone that shoots P3 would write them, with its profile.
    let (w, h) = (64u32, 32u32);
    let patches: [[u8; 3]; 4] = [
        [200, 120, 60],
        [60, 160, 90],
        [90, 100, 200],
        [128, 128, 128],
    ];
    let mut px = Vec::new();
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&patches[((y / 16) * 2 + x / 32) as usize]);
        }
    }
    let mut jpeg = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100);
    enc.set_icc_profile(photo_engine::icc::display_p3_profile().to_vec())
        .unwrap();
    enc.write_image(&px, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();

    let raster = photo_engine::decode_raster(&jpeg).unwrap();
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let out = engine
        .export(&gpu.device, &gpu.queue, &PhotoStack::default())
        .unwrap();

    // The published linear Display P3 → sRGB matrix, not this crate's own derivation.
    const P3_TO_SRGB: [[f32; 3]; 3] = [
        [1.2249, -0.2247, 0.0],
        [-0.0420, 1.0419, 0.0],
        [-0.0197, -0.0786, 1.0979],
    ];
    let lin = |v: u8| testkit::srgb_to_linear(v);
    let encode = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let e = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        e * 255.0
    };
    for (i, p) in patches.iter().enumerate() {
        let (cx, cy) = ((i as u32 % 2) * 32 + 16, (i as u32 / 2) * 16 + 8);
        let at = ((cy * w + cx) * 4) as usize;
        let l = p.map(lin);
        let srgb: [f32; 3] =
            std::array::from_fn(|c| P3_TO_SRGB[c].iter().zip(l).map(|(m, v)| m * v).sum());
        // A colour beyond sRGB (the P3 green) is brought in towards the grey of its own
        // luminance, keeping its hue, rather than having its negative channel cut off.
        let y = 0.2126 * srgb[0] + 0.7152 * srgb[1] + 0.0722 * srgb[2];
        let lo = srgb.iter().cloned().fold(f32::INFINITY, f32::min);
        let s = if lo < 0.0 { y / (y - lo) } else { 1.0 };
        for (c, v) in srgb.iter().enumerate() {
            let want = encode(y + (v - y) * s);
            let got = out.rgba[at + c] as f32;
            assert!(
                (got - want).abs() <= 2.5,
                "patch {i} channel {c}: {got} for {want:.1}"
            );
        }
    }
    // Not what reading it as sRGB would give: the orange is visibly more saturated.
    let at = ((8 * w + 16) * 4) as usize;
    assert!(
        out.rgba[at] as i32 - 200 >= 8,
        "P3 orange came out {:?}",
        &out.rgba[at..at + 3]
    );
}

#[test]
fn a_files_own_warp_moves_each_plane_where_it_says() {
    let gpu = gpu_or_skip!();
    // A barrel correction for green and blue, and a stronger one for red: the file's own
    // lateral colour fringe correction.
    let (w, h) = (256u32, 128u32);
    let warp = photo_engine::FileWarp {
        centre: [w as f32 * 0.5, h as f32 * 0.5],
        m: (w as f32 * 0.5).hypot(h as f32 * 0.5),
        planes: [
            [1.0, 0.0, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, -0.2, 0.0, 0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, -0.2, 0.0, 0.0, 0.0, 0.0, 0.0],
        ],
        tca: None,
        vignetting: None,
    };
    let target = (230.0, 110.0);
    let stack = PhotoStack::default();
    let (gx, gy) = photo_engine::sensor_point(&stack, Some(&warp), (w, h), target);
    let (rx, ry) = warp.source(0, target);
    assert!(
        gx < target.0 - 5.0 && (rx - gx).abs() > 3.0,
        "g ({gx}, {gy}) r ({rx}, {ry})"
    );
    // Green where green belongs, red where red does, and nothing elsewhere.
    let near = |x: u32, y: u32, (cx, cy): (f32, f32)| {
        (x as f32 + 0.5 - cx).abs() < 3.0 && (y as f32 + 0.5 - cy).abs() < 3.0
    };
    let mut cfa = testkit::cfa_from_fn(w, h, |x, y| {
        [
            if near(x, y, (rx, ry)) { 0.8 } else { 0.02 },
            if near(x, y, (gx, gy)) { 0.8 } else { 0.02 },
            0.02,
        ]
    });
    cfa.warp = Some(warp);
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let at = ((target.1 as u32 * out.width + target.0 as u32) * 4) as usize;
    let px = &out.rgba[at..at + 3];
    assert!(
        px[0] > 150 && px[1] > 150,
        "the planes did not meet: {px:?}"
    );
}

#[test]
fn a_lens_profile_applies_only_while_lens_asks_for_it() {
    let gpu = gpu_or_skip!();
    // A flat field as a lens that loses half its light at the corner records it.
    let (w, h) = (192u32, 128u32);
    let half_diag = (w as f32).hypot(h as f32) * 0.5;
    let cfa = testkit::cfa_from_fn(w, h, |x, y| {
        let (dx, dy) = (
            x as f32 + 0.5 - w as f32 * 0.5,
            y as f32 + 0.5 - h as f32 * 0.5,
        );
        let r2 = (dx * dx + dy * dy) / (half_diag * half_diag);
        [0.25 * (1.0 - 0.5 * r2); 3]
    });
    let profile = photo_engine::FileWarp {
        centre: [w as f32 * 0.5, h as f32 * 0.5],
        m: h as f32 * 0.5,
        planes: [[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]; 3],
        tca: None,
        vignetting: Some(([-0.5, 0.0, 0.0], half_diag)),
    };
    let pool = photo_engine::UploadPool::default();
    let mut up = photo_engine::upload_cfa(&gpu.device, &gpu.queue, &pool, cfa).unwrap();
    up.lens_profile = Some(("Acme 35mm f/2".into(), profile));
    let mut engine = Engine::new(&gpu.device);
    engine.load_uploaded(&gpu.device, &gpu.queue, up).unwrap();
    assert_eq!(
        engine.image().unwrap().lens_profile_name(),
        Some("Acme 35mm f/2")
    );
    let spread = |engine: &mut Engine, stack: &PhotoStack| {
        let out = engine.export(&gpu.device, &gpu.queue, stack).unwrap();
        let at = |x: u32, y: u32| out.rgba[((y * out.width + x) * 4 + 1) as usize] as f32;
        at(w / 2, h / 2) - at(2, 2)
    };
    let mut stack = PhotoStack::default();
    let off = spread(&mut engine, &stack);
    assert!(off > 25.0, "the test scene: {off}");
    stack.lens.enabled = true;
    let on = spread(&mut engine, &stack);
    assert!(on.abs() < 4.0, "corner still {on} below the centre");
    stack.lens.params.profile = false;
    assert!((spread(&mut engine, &stack) - off).abs() < 1.0);
}

fn grey_at(out: &photo_engine::ExportedImage, x: u32, y: u32) -> [f32; 3] {
    let i = ((y * out.width + x) * 4) as usize;
    [0, 1, 2].map(|c| testkit::srgb_to_linear(out.rgba[i + c]))
}

#[test]
fn a_radial_mask_lifts_its_middle_by_the_stops_it_asks_for() {
    use photo_engine::local::Mask;
    let gpu = gpu_or_skip!();
    let (w, h) = (192u32, 128u32);
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(w, h, 0.1))
        .unwrap();
    let mut stack = PhotoStack::default();
    let plain = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let mut mask = Mask::radial([0.25, 0.5], 0.15);
    mask.adjust.exposure = 1.0;
    stack.local.enabled = true;
    stack.local.params.masks.push(mask);
    let lit = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let (inside, outside) = ((48, 64), (170, 64));
    let ratio = grey_at(&lit, inside.0, inside.1)[1] / grey_at(&plain, inside.0, inside.1)[1];
    assert!(
        (ratio - 2.0).abs() < 0.1,
        "one stop is twice the light, got {ratio}"
    );
    assert_eq!(
        grey_at(&lit, outside.0, outside.1),
        grey_at(&plain, outside.0, outside.1)
    );
    // Inverted, it is the other way round.
    stack.local.params.masks[0].invert = true;
    let inv = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    assert_eq!(
        grey_at(&inv, inside.0, inside.1),
        grey_at(&plain, inside.0, inside.1)
    );
    assert!(
        grey_at(&inv, outside.0, outside.1)[1] > grey_at(&plain, outside.0, outside.1)[1] * 1.8
    );
}

#[test]
fn a_mask_stays_on_what_it_was_drawn_over_when_the_photo_turns() {
    use photo_engine::local::Mask;
    let gpu = gpu_or_skip!();
    let (w, h) = (192u32, 128u32);
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(w, h, 0.1))
        .unwrap();
    let mut stack = PhotoStack::default();
    let mut mask = Mask::radial([0.25, 0.5], 0.1);
    mask.adjust.exposure = 2.0;
    stack.local.enabled = true;
    stack.local.params.masks.push(mask);
    stack.geometry.rotate_right();
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    // The middle of what was lifted maps back to the mask's centre on the sensor.
    let base = grey_at(&out, 2, 2)[1];
    let (mut sx, mut sy, mut n) = (0.0f32, 0.0f32, 0.0f32);
    for y in 0..out.height {
        for x in 0..out.width {
            if grey_at(&out, x, y)[1] > base * 2.0 {
                let (px, py) = photo_engine::sensor_point(
                    &stack,
                    None,
                    (w, h),
                    (x as f32 + 0.5, y as f32 + 0.5),
                );
                sx += px;
                sy += py;
                n += 1.0;
            }
        }
    }
    assert!(n > 50.0, "nothing was lifted");
    let (sx, sy) = (sx / n, sy / n);
    assert!(
        (sx - 48.0).abs() < 1.5 && (sy - 64.0).abs() < 1.5,
        "lifted around sensor ({sx}, {sy})"
    );
}

#[test]
fn a_warm_gradient_warms_only_where_it_applies() {
    use photo_engine::local::Mask;
    let gpu = gpu_or_skip!();
    let (w, h) = (128u32, 128u32);
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(w, h, 0.2))
        .unwrap();
    let mut stack = PhotoStack::default();
    let mut mask = Mask::linear([0.5, 0.0], [0.5, 0.5]);
    mask.adjust.temperature = 100.0;
    stack.local.enabled = true;
    stack.local.params.masks.push(mask);
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let top = grey_at(&out, 64, 2);
    let bottom = grey_at(&out, 64, 120);
    assert!(top[0] > top[2] * 1.3, "not warm at the top: {top:?}");
    assert!(
        (bottom[0] - bottom[2]).abs() < 0.01,
        "warm at the bottom: {bottom:?}"
    );
}

#[test]
fn the_mask_overlay_shows_on_the_preview_and_never_in_an_export() {
    use photo_engine::local::Mask;
    let gpu = gpu_or_skip!();
    let (w, h) = (96u32, 64u32);
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(w, h, 0.2))
        .unwrap();
    let mut stack = PhotoStack::default();
    stack.local.enabled = true;
    stack.local.params.masks.push(Mask::radial([0.5, 0.5], 0.2));
    let plain = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    engine.set_mask_overlay(Some(0));
    let exported = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    assert_eq!(plain.rgba, exported.rgba, "the overlay reached an export");
    let (pw, _, shown) = preview(gpu, &mut engine, &stack, 0);
    let i = (((h / 2) * pw + w / 2) * 4) as usize;
    assert!(
        shown[i] > shown[i + 1] + 60,
        "no red wash: {:?}",
        &shown[i..i + 3]
    );
}

#[test]
fn a_brushed_mask_applies_where_it_was_painted_and_erasing_takes_it_back() {
    use photo_engine::local::{Dab, Mask, Shape};
    let gpu = gpu_or_skip!();
    let (w, h) = (192u32, 128u32);
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(w, h, 0.1))
        .unwrap();
    let mut stack = PhotoStack::default();
    let plain = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let stroke: Vec<Dab> = (0..8)
        .map(|i| Dab {
            at: [0.15 + i as f32 * 0.02, 0.5],
            radius: 0.06,
            hardness: 0.6,
            erase: false,
        })
        .collect();
    let mut mask = Mask::brush();
    mask.shape = Shape::Brush {
        dabs: stroke.clone().into(),
    };
    mask.adjust.exposure = 1.0;
    stack.local.enabled = true;
    stack.local.params.masks.push(mask);
    let painted = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let (on, off) = ((48, 64), (160, 64));
    let ratio = grey_at(&painted, on.0, on.1)[1] / grey_at(&plain, on.0, on.1)[1];
    assert!((ratio - 2.0).abs() < 0.12, "painted: {ratio}");
    assert_eq!(
        grey_at(&painted, off.0, off.1),
        grey_at(&plain, off.0, off.1)
    );

    // A stroke that grows adds to what is there; an eraser dab takes it away.
    let Shape::Brush { dabs } = &mut stack.local.params.masks[0].shape else {
        unreachable!()
    };
    dabs.push(Dab {
        at: [0.25, 0.5],
        radius: 0.05,
        hardness: 0.8,
        erase: true,
    });
    let erased = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    assert_eq!(grey_at(&erased, on.0, on.1), grey_at(&plain, on.0, on.1));
    assert!(grey_at(&erased, 34, 64)[1] > grey_at(&plain, 34, 64)[1] * 1.6);
}

#[test]
fn the_scopes_do_not_measure_the_mask_wash() {
    use photo_engine::local::Mask;
    let gpu = gpu_or_skip!();
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::flat(96, 64, 0.2))
        .unwrap();
    let mut stack = PhotoStack::default();
    stack.local.enabled = true;
    stack.local.params.masks.push(Mask::radial([0.5, 0.5], 0.4));
    let settle = |engine: &mut Engine, stack: &PhotoStack| {
        engine.render(&gpu.device, &gpu.queue, stack, None);
        for _ in 0..50 {
            gpu.device.poll(wgpu::PollType::wait_indefinitely()).ok();
            engine.poll_histogram(&gpu.device, &gpu.queue);
            if !engine.histogram_busy() {
                break;
            }
        }
        engine.histogram().clone()
    };
    let before = settle(&mut engine, &stack);
    engine.set_mask_overlay(Some(0));
    let washed = settle(&mut engine, &stack);
    assert_eq!(before, washed, "the red wash reached the scopes");
}

#[test]
fn a_linear_dng_opens_as_its_raw_would_neutral_and_in_order() {
    let gpu = gpu_or_skip!();
    let bytes =
        photo_engine::testkit::synth_linear_dng(96, 32, |x| 0.05 + 0.6 * x as f32 / 95.0).unwrap();
    use photo_engine::RawDecoder;
    let cfa = photo_engine::RawlerBackend.decode_bytes(&bytes).unwrap();
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    let out = engine
        .export(&gpu.device, &gpu.queue, &PhotoStack::default())
        .unwrap();
    assert_eq!((out.width, out.height), (96, 32));
    let mut last = -1.0f32;
    for x in [4u32, 30, 60, 90] {
        let [r, g, b] = grey_at(&out, x, 16);
        // The cast is the as-shot white's to undo: grey comes out grey.
        assert!(
            (r - g).abs() < 0.03 && (b - g).abs() < 0.03,
            "x {x}: {r} {g} {b}"
        );
        assert!(g > last, "the ramp does not rise at x {x}");
        last = g;
    }
}

#[test]
fn a_look_runs_its_cube_over_the_photo_by_its_amount() {
    let gpu = gpu_or_skip!();
    // A 2³ .cube that inverts: every channel to 1 - itself, in display values.
    let dir = std::env::temp_dir().join(format!("duckfoot-look-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cube = dir.join("invert.cube");
    let mut text = String::from("TITLE \"invert\"\nLUT_3D_SIZE 2\n");
    for b in [1.0, 0.0] {
        for g in [1.0, 0.0] {
            for r in [1.0, 0.0] {
                text.push_str(&format!("{r} {g} {b}\n"));
            }
        }
    }
    // Red fastest: lattice (0,0,0) first, which the inversion sends to white.
    std::fs::write(&cube, &text).unwrap();

    // 25% grey and a highlight at three times white.
    let quarter = testkit::srgb_to_linear(64);
    let raster = testkit::raster_from_fn(16, 8, |x, _| if x < 8 { [quarter; 3] } else { [3.0; 3] });
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let mut stack = PhotoStack::default();
    let shown = |engine: &mut Engine, stack: &PhotoStack| {
        let (w, _, px) = preview(gpu, engine, stack, 0);
        let at = |x: u32| ((4 * w + x) * 4) as usize;
        (px[at(4)], px[at(12)])
    };
    let (plain, _) = shown(&mut engine, &stack);

    stack.look.enabled = true;
    stack.look.params.path = Some(cube.to_string_lossy().into_owned());
    let (inverted, highlight) = shown(&mut engine, &stack);
    assert!(engine.look_loaded());
    // Above white the look passes what it cannot hold: still white, not inverted to black.
    assert_eq!(
        highlight, 255,
        "a highlight above white went through the look"
    );
    assert!(
        inverted.abs_diff(255 - plain) <= 3,
        "25% grey ({plain}) through the inverting look showed {inverted}"
    );
    let export = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    assert!(
        export.rgba[(4 * 16 + 4) * 4].abs_diff(inverted) <= 2,
        "the export's look differs from the preview's"
    );

    stack.look.params.amount = 0.5;
    let (half, _) = shown(&mut engine, &stack);
    assert!(
        half > plain.min(inverted) && half < plain.max(inverted),
        "half the look ({half}) is not between none ({plain}) and all ({inverted})"
    );

    // A file that is not there: nothing applied, and the engine says so.
    stack.look.params.path = Some(dir.join("gone.cube").to_string_lossy().into_owned());
    let (gone, _) = shown(&mut engine, &stack);
    assert!(!engine.look_loaded());
    assert_eq!(gone, plain);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A patch rendered under each version of the colour math, back in linear sRGB.
fn under(gpu: &Gpu, stack: &PhotoStack, patch: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let at = |rendering| {
        let stack = PhotoStack {
            rendering,
            ..stack.clone()
        };
        render_patches(gpu, &stack, &[patch])[0].map(testkit::srgb_to_linear)
    };
    (
        at(photo_engine::params::Rendering::V1),
        at(photo_engine::params::Rendering::V2),
    )
}

#[test]
fn v2_rolls_a_bright_colour_off_without_turning_its_hue() {
    let gpu = gpu_or_skip!();
    let mut stack = PhotoStack::default();
    // Modern roll-off, and an orange the shoulder has to compress.
    stack.exposure.params.mode = photo_engine::params::ExposureMode::ModernSoft;
    stack.exposure.params.rolloff = 1.0;
    // Past the shoulder's knee in Rec.2020 (red about 1.0 there), under white overall.
    let orange = [1.3, 0.55, 0.05];
    let (v1, v2) = under(gpu, &stack, orange);
    let (want, h1, h2) = (hue(orange), hue(v1), hue(v2));
    assert!((h2 - want).abs() < 2.0, "V2 turned {want}° to {h2}°");
    assert!(
        (h1 - want).abs() > (h2 - want).abs() + 1.0,
        "V1 {h1}°, V2 {h2}°, from {want}°"
    );
}

#[test]
fn v2_keeps_a_light_above_white_red_where_v1_sends_it_to_white() {
    let gpu = gpu_or_skip!();
    // Luminance near white: V1 brings it in towards that grey; V2 keeps it red.
    let red = [3.0, 0.3, 0.25];
    let (v1, v2) = under(gpu, &PhotoStack::default(), red);
    let saturation = |c: [f32; 3]| {
        let (hi, lo) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
        (hi - lo) / hi.max(1e-6)
    };
    assert!(saturation(v1) < 0.25, "V1 kept {v1:?} coloured");
    assert!(
        saturation(v2) > saturation(v1) + 0.3,
        "V2 {v2:?} is not much redder than V1 {v1:?}"
    );
    assert!(
        (hue(v2) - hue(red)).abs() < 6.0,
        "V2 turned red to {}°",
        hue(v2)
    );
}

#[test]
fn v2_sharpening_keeps_a_highlight_past_twice_white() {
    let gpu = gpu_or_skip!();
    // Flat, so the sharpening itself does nothing: only its clamp shows, then two stops
    // down so what it left is visible.
    let mut stack = PhotoStack::default();
    stack.sharpen.enabled = true;
    stack.sharpen.params.amount = 1.0;
    stack.exposure.params.ev = -2.0;
    let (v1, v2) = under(gpu, &stack, [3.0; 3]);
    assert!(
        (v1[1] - 0.5).abs() < 0.03,
        "V1's ceiling of twice white: {v1:?}"
    );
    assert!((v2[1] - 0.75).abs() < 0.03, "V2 cut the highlight: {v2:?}");
}

/// A stack with one whole-frame mask that lifts exposure by two stops where its `range`
/// lets it.
fn ranged(range: photo_engine::local::RangeMask) -> PhotoStack {
    let mut stack = PhotoStack::default();
    let mut mask = photo_engine::local::Mask::radial([0.5, 0.5], 10.0);
    mask.adjust.exposure = 2.0;
    mask.range = range;
    stack.local.enabled = true;
    stack.local.params.masks.push(mask);
    stack
}

#[test]
fn v2_reads_a_luminance_range_after_exposure_and_as_displayed() {
    let gpu = gpu_or_skip!();
    use photo_engine::local::RangeMask;
    // 18% grey shows as about 0.46: the range picks it out as displayed.
    let grey = [0.18; 3];
    let mut stack = ranged(RangeMask::Luminance {
        range: [0.4, 0.52],
        feather: 0.01,
    });
    let plain = render_patches(gpu, &PhotoStack::default(), &[grey])[0][1];
    let lifted = |stack: &PhotoStack| {
        let (v1, v2) = under(gpu, stack, grey);
        (v1[1], v2[1])
    };
    let (v1, v2) = lifted(&stack);
    let plain = testkit::srgb_to_linear(plain);
    assert!(
        v2 > plain * 2.0,
        "V2's range missed grey as displayed: {v2} vs {plain}"
    );
    assert!(
        v1 <= plain * 1.05,
        "V1's range (linear, 0.18) should miss it: {v1}"
    );
    // A stop brighter, it shows as about 0.63: out of the range, in V2.
    stack.exposure.params.ev = 1.0;
    let (_, brighter) = lifted(&stack);
    let one_stop = render_patches(
        gpu,
        &PhotoStack {
            exposure: stack.exposure.clone(),
            ..PhotoStack::default()
        },
        &[grey],
    )[0][1];
    assert!(
        brighter <= testkit::srgb_to_linear(one_stop) * 1.05,
        "V2's range did not follow the exposure"
    );
}

#[test]
fn v2_names_a_colour_ranges_hue_as_the_colour_eq_does() {
    let gpu = gpu_or_skip!();
    use photo_engine::local::RangeMask;
    // An orange: about 13 degrees as linear sRGB names it (what the colour EQ goes by),
    // about 21 in raw Rec.2020 values (what V1's range went by).
    let orange = [0.3, 0.064, 0.0];
    let stack = ranged(RangeMask::Color {
        hue: hue(orange),
        spread: 3.0,
        feather: 1.0,
    });
    let (v1, v2) = under(gpu, &stack, orange);
    let plain =
        render_patches(gpu, &PhotoStack::default(), &[orange])[0].map(testkit::srgb_to_linear);
    assert!(
        v2[0] > plain[0] * 2.0,
        "V2 missed the orange it was set to: {v2:?} vs {plain:?}"
    );
    assert!(
        v1[0] <= plain[0] * 1.05,
        "V1 (raw Rec.2020 hue) should miss it: {v1:?}"
    );
}

#[test]
fn a_removed_spot_is_filled_from_the_ground_around_it_with_no_source() {
    use photo_engine::heal::{Mode, Spot};
    let gpu = gpu_or_skip!();
    // Stripes, and a dark blot across them: a fill has to continue the stripes, which no
    // single source shifted in brightness would.
    let (w, h) = (192u32, 128u32);
    let stripe = |x: u32| {
        if (x / 6).is_multiple_of(2) {
            0.15
        } else {
            0.45
        }
    };
    let raster = testkit::raster_from_fn(w, h, |x, y| {
        let blot = ((x as f32 - 96.0).powi(2) + (y as f32 - 64.0).powi(2)).sqrt() < 9.0;
        [if blot { 0.01 } else { stripe(x) }; 3]
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let mut stack = PhotoStack::default();
    stack.heal.enabled = true;
    // The source is nowhere useful: a Remove spot must not read it.
    stack.heal.params.spots.push(Spot {
        mode: Mode::Remove,
        feather: 0.05,
        ..Spot::new(
            [96.5 / w as f32, 64.5 / h as f32],
            [0.0, 0.0],
            11.0 / w as f32,
        )
    });
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    // Inside where the blot was, each column has its stripe's value back.
    let mut wrong = 0;
    for x in 90..103u32 {
        for y in 58..71u32 {
            let got = grey_at(&out, x, y)[1];
            if (got - stripe(x)).abs() > 0.08 {
                wrong += 1;
            }
        }
    }
    assert!(
        wrong <= 8,
        "{wrong} of 169 pixels inside the blot are not their stripe's"
    );

    // The fill is made once: rendering again, or moving another slider, reuses it.
    let again = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    assert!(again.rgba == out.rgba, "the fill changed between renders");
}

#[test]
fn a_healed_spot_takes_the_ground_around_it_and_clone_copies_its_source() {
    use photo_engine::heal::{Mode, Spot};
    let gpu = gpu_or_skip!();
    // A smooth gradient with a dark speck of dust on it.
    let (w, h) = (192u32, 128u32);
    let raster = testkit::raster_from_fn(w, h, |x, y| {
        let ground = 0.1 + 0.3 * x as f32 / w as f32;
        let speck = ((x as f32 - 96.0).powi(2) + (y as f32 - 64.0).powi(2)).sqrt() < 4.0;
        [if speck { 0.01 } else { ground }; 3]
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let mut stack = PhotoStack::default();
    let dirty = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let centre = |out: &photo_engine::ExportedImage| grey_at(out, 96, 64)[1];
    assert!(centre(&dirty) < 0.02);

    let dst = [96.5 / w as f32, 64.5 / h as f32];
    let radius = 7.0 / w as f32;
    let src = engine
        .heal_source(&gpu.device, &gpu.queue, dst, radius)
        .unwrap();
    // Up or down the gradient's line, where the ground matches.
    assert!((src[0] - dst[0]).abs() * (w as f32) < 6.0, "source {src:?}");
    stack.heal.enabled = true;
    stack.heal.params.spots.push(Spot::new(dst, src, radius));
    let healed = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let ground = 0.1 + 0.3 * 96.0 / w as f32;
    assert!(
        (centre(&healed) - ground).abs() < 0.02,
        "healed to {}",
        centre(&healed)
    );

    // Clone, from the dark end, copies what is there without matching it.
    stack.heal.params.spots[0] = Spot {
        src: [30.0 / w as f32, dst[1]],
        mode: Mode::Clone,
        ..stack.heal.params.spots[0]
    };
    let cloned = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let there = 0.1 + 0.3 * 30.0 / w as f32;
    assert!(
        (centre(&cloned) - there).abs() < 0.02,
        "cloned {}",
        centre(&cloned)
    );
}

#[test]
fn a_keystone_takes_each_pixel_from_where_the_cpu_says() {
    let gpu = gpu_or_skip!();
    let (w, h) = (256u32, 128u32);
    let mut stack = PhotoStack::default();
    stack.geometry.perspective_v = 70.0;
    stack.geometry.perspective_h = -30.0;
    let target = (230.0, 20.0);
    let (sx, sy) = photo_engine::sensor_point(&stack, None, (w, h), target);
    assert!(
        (sx - target.0).abs() + (sy - target.1).abs() > 8.0,
        "({sx}, {sy})"
    );
    let raster = testkit::raster_from_fn(w, h, |x, y| {
        let near = (x as f32 + 0.5 - sx).abs() < 3.0 && (y as f32 + 0.5 - sy).abs() < 3.0;
        [if near { 0.8 } else { 0.02 }; 3]
    });
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let out = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let at = ((target.1 as u32 * out.width + target.0 as u32) * 4) as usize;
    assert!(
        out.rgba[at] > 200,
        "the spot is not there: {}",
        out.rgba[at]
    );
    // And the keystone never shows what is off the photo: every corner is photographed.
    for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        let i = ((y * out.width + x) * 4) as usize;
        assert!(out.rgba[i] > 0, "empty corner at ({x}, {y})");
    }
}

/// A grey zone plate (rings getting finer outwards, up to near the sensor's limit) with a
/// hard diagonal edge across it: where demosaics show zippers, mazes and false colour.
fn zone_plate(w: u32, h: u32) -> impl Fn(u32, u32) -> [f32; 3] + Copy {
    move |x: u32, y: u32| {
        let (dx, dy) = (x as f32 - w as f32 / 2.0, y as f32 - h as f32 / 2.0);
        let r2 = dx * dx + dy * dy;
        let rings = 0.5 + 0.35 * (r2 * std::f32::consts::PI / (w as f32 * 1.6)).cos();
        let edge = if (x as f32) * 0.8 + y as f32 > w as f32 {
            1.0
        } else {
            0.45
        };
        [rings * edge * 0.8; 3]
    }
}

/// How a demosaic does on the zone plate: its false colour, and its PSNR against the
/// same scene loaded as a raster, which needs no demosaic.
fn demosaic_quality(gpu: &Gpu) -> (f32, f32) {
    let (w, h) = (256u32, 192u32);
    let scene = zone_plate(w, h);
    let export = |load: &dyn Fn(&mut Engine)| {
        let mut engine = Engine::new(&gpu.device);
        load(&mut engine);
        engine
            .export(&gpu.device, &gpu.queue, &PhotoStack::default())
            .unwrap()
    };
    let mosaic = export(&|e| {
        e.load(&gpu.device, &gpu.queue, testkit::cfa_from_fn(w, h, scene))
            .unwrap()
    });
    let truth = export(&|e| {
        e.load_raster(
            &gpu.device,
            &gpu.queue,
            testkit::raster_from_fn(w, h, scene),
        )
        .unwrap()
    });
    let inner = |img: &photo_engine::ExportedImage| {
        let mut px = Vec::new();
        for y in 6..h - 6 {
            for x in 6..w - 6 {
                let i = ((y * w + x) * 4) as usize;
                px.extend_from_slice(&img.rgba[i..i + 3]);
            }
        }
        px
    };
    (false_colour(&mosaic), psnr(&inner(&mosaic), &inner(&truth)))
}

#[test]
fn the_demosaic_keeps_fine_detail_grey_and_where_it_was() {
    let gpu = gpu_or_skip!();
    let (false_col, fidelity) = demosaic_quality(gpu);
    eprintln!("demosaic: false colour {false_col:.3}, PSNR {fidelity:.2} dB");
    // Hamilton-Adams, which RCD replaced, scored 3.49 and 35.96 dB here; RCD 3.13 and
    // 36.63 dB. Either getting worse than halfway back is a regression.
    assert!(false_col < 3.3, "false colour {false_col:.3}");
    assert!(fidelity > 36.3, "PSNR {fidelity:.2} dB");
}

/// The sensor texture as the adjust pass sees it at white balance 1: `rgb + a`.
fn sensor_rgb(gpu: &Gpu, engine: &Engine) -> (Vec<[f32; 3]>, u32) {
    let (texels, (w, _)) = engine.sensor_level0(&gpu.device, &gpu.queue).unwrap();
    let rgb = texels
        .iter()
        .map(|t| [t[0] + t[3], t[1] + t[3], t[2] + t[3]])
        .collect();
    (rgb, w)
}

#[test]
fn a_clipped_channel_is_rebuilt_from_the_ones_that_were_not() {
    let gpu = gpu_or_skip!();
    let (w, h) = (160u32, 120u32);
    // An orange sky brightening to the right, striped: red clips over most of it, green
    // and blue never do, and all three carry the stripes.
    let sky = move |x: u32, y: u32| {
        let base = 0.3 + 0.6 * x as f32 / w as f32;
        let stripes = 1.0 + 0.2 * (y as f32 * 0.8).sin();
        [
            1.8 * base * stripes,
            0.7 * base * stripes,
            0.4 * base * stripes,
        ]
    };
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::cfa_from_fn(w, h, sky))
        .unwrap();
    let (rgb, stride) = sensor_rgb(gpu, &engine);
    let x = 140;
    let (mut reds, mut greens) = (Vec::new(), Vec::new());
    for y in 8..h - 8 {
        let [r, g, _] = rgb[(y * stride + x) as usize];
        reds.push(r);
        greens.push(g);
    }
    assert!(
        sky(x, 60)[0] > 1.2,
        "red must be clipped here for the test to mean anything"
    );
    let ratio = reds.iter().sum::<f32>() / greens.iter().sum::<f32>();
    assert!(
        (ratio - 1.8 / 0.7).abs() < 0.25,
        "red / green in the blown sky is {ratio:.2}, not the sky's 2.57"
    );
    // And red follows the stripes that green shows.
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let (mr, mg) = (mean(&reds), mean(&greens));
    let cov: f32 = reds
        .iter()
        .zip(&greens)
        .map(|(r, g)| (r - mr) * (g - mg))
        .sum();
    let var_r: f32 = reds.iter().map(|r| (r - mr).powi(2)).sum();
    let var_g: f32 = greens.iter().map(|g| (g - mg).powi(2)).sum();
    let correlation = cov / (var_r * var_g).sqrt().max(1e-9);
    assert!(
        correlation > 0.9,
        "red in the blown sky does not follow its stripes: {correlation:.2}"
    );
}

#[test]
fn a_blown_white_specular_stays_white_to_its_rim() {
    let gpu = gpu_or_skip!();
    let (w, h) = (96u32, 96u32);
    let gains = [2.0f32, 1.0, 1.8];
    // A grey card under light the white balance corrects, with a light far past clipping.
    let scene = move |x: u32, y: u32| {
        let (dx, dy) = (x as f32 - 48.0, y as f32 - 48.0);
        if dx * dx + dy * dy < 14.0 * 14.0 {
            [3.0; 3]
        } else {
            [0.3 / gains[0], 0.3 / gains[1], 0.3 / gains[2]]
        }
    };
    let mut cfa = testkit::cfa_from_fn(w, h, scene);
    cfa.wb = gains;
    let mut engine = Engine::new(&gpu.device);
    engine.load(&gpu.device, &gpu.queue, cfa).unwrap();
    let out = engine
        .export(&gpu.device, &gpu.queue, &PhotoStack::default())
        .unwrap();
    let mut worst = 0u8;
    for y in 30..66u32 {
        for x in 30..66u32 {
            let i = ((y * w + x) * 4) as usize;
            let px = &out.rgba[i..i + 3];
            worst = worst.max(px.iter().max().unwrap() - px.iter().min().unwrap());
        }
    }
    assert!(
        worst <= 12,
        "a tint of {worst} in or around the blown light"
    );
}

#[test]
fn a_wide_colour_survives_to_a_display_p3_export_and_only_there() {
    use image::ImageEncoder;
    let gpu = gpu_or_skip!();
    // A green no sRGB screen can show, in a P3 file.
    let (w, h) = (32u32, 32u32);
    let green = [40u8, 200, 60];
    let px: Vec<u8> = (0..w * h).flat_map(|_| green).collect();
    let mut jpeg = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100);
    enc.set_icc_profile(photo_engine::icc::display_p3_profile().to_vec())
        .unwrap();
    enc.write_image(&px, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    let mut engine = Engine::new(&gpu.device);
    engine
        .load_raster(
            &gpu.device,
            &gpu.queue,
            photo_engine::decode_raster(&jpeg).unwrap(),
        )
        .unwrap();
    let mut centre = |stack: &PhotoStack, space| {
        let out = engine
            .export_in(
                &gpu.device,
                &gpu.queue,
                stack,
                photo_engine::BitDepth::Eight,
                space,
            )
            .unwrap();
        let photo_engine::export::Exported::Eight(img) = out else {
            unreachable!()
        };
        let at = ((16 * w + 16) * 4) as usize;
        [img.rgba[at], img.rgba[at + 1], img.rgba[at + 2]]
    };
    use photo_engine::gamut::{OutputSpace, WorkingSpace};
    let wide = PhotoStack::default();
    assert_eq!(wide.working_space, WorkingSpace::Rec2020);
    let got = centre(&wide, OutputSpace::DisplayP3);
    for c in 0..3 {
        assert!(
            (got[c] as i32 - green[c] as i32).abs() <= 2,
            "P3 in, P3 out: {got:?} for {green:?}"
        );
    }
    // The older working space cut it to sRGB on the way: its red comes back raised.
    let older = PhotoStack {
        working_space: WorkingSpace::Srgb,
        ..PhotoStack::default()
    };
    let clipped = centre(&older, OutputSpace::DisplayP3);
    assert!(
        clipped[0] as i32 - green[0] as i32 >= 10,
        "the sRGB working space kept a colour it cannot hold: {clipped:?}"
    );
}

#[test]
fn a_wide_screen_shows_the_file_and_a_proof_marks_what_srgb_loses() {
    use image::ImageEncoder;
    use photo_engine::engine::Proof;
    use photo_engine::gamut::{OutputSpace, Screen};
    let gpu = gpu_or_skip!();
    // Left half a P3 green no sRGB screen can show, right half a grey any can.
    let (w, h) = (32u32, 16u32);
    let (green, grey) = ([40u8, 200, 60], [120u8, 120, 120]);
    let px: Vec<u8> = (0..w * h)
        .flat_map(|i| if i % w < 16 { green } else { grey })
        .collect();
    let mut jpeg = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100);
    enc.set_icc_profile(photo_engine::icc::display_p3_profile().to_vec())
        .unwrap();
    enc.write_image(&px, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    let mut engine = Engine::new(&gpu.device);
    engine
        .load_raster(
            &gpu.device,
            &gpu.queue,
            photo_engine::decode_raster(&jpeg).unwrap(),
        )
        .unwrap();
    let stack = PhotoStack::default();
    let at = |px: &[u8], x: u32| {
        let i = ((8 * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    let near = |a: [u8; 3], b: [u8; 3]| (0..3).all(|c| (a[c] as i32 - b[c] as i32).abs() <= 2);

    engine.set_screen(Screen::of(OutputSpace::DisplayP3));
    let (_, _, shown) = preview(gpu, &mut engine, &stack, 0);
    assert!(
        near(at(&shown, 6), green),
        "a P3 screen shows {:?}",
        at(&shown, 6)
    );

    engine.set_proof(Some(Proof {
        space: Screen::SRGB,
        warn: true,
        printer: false,
    }));
    let (_, _, proofed) = preview(gpu, &mut engine, &stack, 0);
    assert_eq!(
        at(&proofed, 6),
        [128, 128, 128],
        "the green sRGB cannot hold is marked"
    );
    assert!(
        near(at(&proofed, 24), at(&shown, 24)),
        "a grey sRGB holds is left alone"
    );
    // Without the warning, the proof shows the green as an sRGB export will have it.
    engine.set_proof(Some(Proof {
        space: Screen::SRGB,
        warn: false,
        printer: false,
    }));
    let (_, _, proofed) = preview(gpu, &mut engine, &stack, 0);
    let g = at(&proofed, 6);
    assert!(
        g[0] > green[0] + 10,
        "the proof shows the green as sRGB brings it in: {g:?}"
    );
}

#[test]
fn a_printer_proof_shows_the_file_through_its_table() {
    let gpu = gpu_or_skip!();
    use photo_engine::engine::Proof;
    use photo_engine::gamut::Screen;
    use photo_engine::proof::ProofTable;
    // Three patches: a dark red, mid grey and a light blue, as linear sRGB.
    let patches = [[0.2f32, 0.02, 0.02], [0.214, 0.214, 0.214], [0.4, 0.6, 0.9]];
    let raster = testkit::raster_from_fn(24, 8, |x, _| patches[(x / 8) as usize]);
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let stack = PhotoStack::default();
    let centres = |px: &[u8], w: u32| -> Vec<[u8; 3]> {
        (0..3)
            .map(|i| {
                let at = ((4 * w + i * 8 + 4) * 4) as usize;
                [px[at], px[at + 1], px[at + 2]]
            })
            .collect()
    };
    let (w, _, plain) = preview(gpu, &mut engine, &stack, 0);
    let plain = centres(&plain, w);
    let printer = |warn| {
        Some(Proof {
            space: Screen::SRGB,
            warn,
            printer: true,
        })
    };

    // Asked for before a table is loaded, the proof is against sRGB alone: unchanged.
    engine.set_proof(printer(false));
    let (_, _, before) = preview(gpu, &mut engine, &stack, 0);
    assert_eq!(
        centres(&before, w),
        plain,
        "a proof with no table changed the preview"
    );

    // The identity table shows the file as it is, interpolation and all.
    engine.set_proof_table(
        &gpu.queue,
        Some(&ProofTable::from_fn(|[r, g, b]| [r, g, b, 0.0])),
    );
    let (_, _, same) = preview(gpu, &mut engine, &stack, 0);
    for (got, want) in centres(&same, w).into_iter().zip(&plain) {
        assert!(
            (0..3).all(|c| got[c].abs_diff(want[c]) <= 1),
            "the identity table showed {want:?} as {got:?}"
        );
    }

    // A table that inverts, warning about what was dark: the shader reads the file's
    // encoded value, not the linear one, and marks what the table marks.
    engine.set_proof_table(
        &gpu.queue,
        Some(&ProofTable::from_fn(|[r, g, b]| {
            [1.0 - r, 1.0 - g, 1.0 - b, if g < 0.25 { 1.0 } else { 0.0 }]
        })),
    );
    let (_, _, inverted) = preview(gpu, &mut engine, &stack, 0);
    let inverted = centres(&inverted, w);
    for (&got, want) in inverted.iter().zip(&plain) {
        assert!(
            (0..3).all(|c| got[c].abs_diff(255 - want[c]) <= 1),
            "the inverting table showed {want:?} as {got:?}"
        );
    }
    engine.set_proof(printer(true));
    let (_, _, warned) = preview(gpu, &mut engine, &stack, 0);
    let warned = centres(&warned, w);
    assert_eq!(
        warned[0],
        [128, 128, 128],
        "the colour the table marks was not greyed"
    );
    assert_eq!(
        warned[1..],
        inverted[1..],
        "a colour the table keeps was greyed"
    );

    // Unloading the table goes back to proofing against the space.
    engine.set_proof_table(&gpu.queue, None);
    engine.set_proof(printer(false));
    let (_, _, after) = preview(gpu, &mut engine, &stack, 0);
    assert_eq!(centres(&after, w), plain);
}

#[test]
fn the_screens_own_profile_finishes_the_preview_and_never_the_export() {
    let gpu = gpu_or_skip!();
    use photo_engine::gamut::OutputSpace;
    use photo_engine::proof::ProofTable;
    let patches = [[0.2f32, 0.02, 0.02], [0.214, 0.214, 0.214], [0.4, 0.6, 0.9]];
    let raster = testkit::raster_from_fn(24, 8, |x, _| patches[(x / 8) as usize]);
    let mut engine = Engine::new(&gpu.device);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let stack = PhotoStack::default();
    let centres = |px: &[u8], w: u32| -> Vec<[u8; 3]> {
        (0..3)
            .map(|i| {
                let at = ((4 * w + i * 8 + 4) * 4) as usize;
                [px[at], px[at + 1], px[at + 2]]
            })
            .collect()
    };
    let (w, _, plain) = preview(gpu, &mut engine, &stack, 0);
    let plain = centres(&plain, w);
    let exported = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();

    // sRGB's own profile, through moxcms and the table: what the sRGB screen shows.
    let srgb = ProofTable::bake_screen(OutputSpace::Srgb.icc()).unwrap();
    engine.set_screen_table(&gpu.queue, Some(&srgb));
    let (_, _, through) = preview(gpu, &mut engine, &stack, 0);
    for (got, want) in centres(&through, w).into_iter().zip(&plain) {
        assert!(
            (0..3).all(|c| got[c].abs_diff(want[c]) <= 2),
            "sRGB's profile showed {want:?} as {got:?}"
        );
    }

    // A table that inverts shows in the preview and not in the export.
    engine.set_screen_table(
        &gpu.queue,
        Some(&ProofTable::from_fn(|[r, g, b]| {
            [1.0 - r, 1.0 - g, 1.0 - b, 0.0]
        })),
    );
    let (_, _, inverted) = preview(gpu, &mut engine, &stack, 0);
    assert_ne!(
        centres(&inverted, w),
        plain,
        "the screen table was not used"
    );
    let again = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    assert!(
        again.rgba == exported.rgba,
        "the screen table reached the export"
    );

    engine.set_screen_table(&gpu.queue, None);
    let (_, _, after) = preview(gpu, &mut engine, &stack, 0);
    assert_eq!(centres(&after, w), plain);
}

#[test]
fn auto_upright_levels_a_tilted_facade_from_the_sensor() {
    let gpu = gpu_or_skip!();
    let (w, h) = (480u32, 360u32);
    let tilt = 3.0f32.to_radians();
    // Pillars and sills, the camera turned 3°.
    let scene = move |x: u32, y: u32| {
        let (dx, dy) = (x as f32 - w as f32 / 2.0, y as f32 - h as f32 / 2.0);
        let (s, c) = tilt.sin_cos();
        let (fx, fy) = (dx * c + dy * s, -dx * s + dy * c);
        let pillar = [-0.3f32, -0.1, 0.1, 0.3]
            .iter()
            .any(|p| (fx - p * w as f32).abs() < 4.0);
        let sill = [-0.2f32, 0.2]
            .iter()
            .any(|p| (fy - p * h as f32).abs() < 4.0);
        [if pillar || sill { 0.7 } else { 0.08 }; 3]
    };
    let mut engine = Engine::new(&gpu.device);
    engine
        .load(&gpu.device, &gpu.queue, testkit::cfa_from_fn(w, h, scene))
        .unwrap();
    let g = engine
        .auto_upright(
            &gpu.device,
            &gpu.queue,
            &PhotoStack::default(),
            photo_engine::keystone::Upright::Level,
        )
        .unwrap()
        .expect("lines to level by");
    assert!(
        (g.straighten_deg.abs() - 3.0).abs() < 0.5,
        "straighten {} for a 3° tilt",
        g.straighten_deg
    );
    // A blank frame has nothing to go by.
    let mut blank = Engine::new(&gpu.device);
    blank
        .load(
            &gpu.device,
            &gpu.queue,
            testkit::cfa_from_fn(w, h, |_, _| [0.3; 3]),
        )
        .unwrap();
    assert!(blank
        .auto_upright(
            &gpu.device,
            &gpu.queue,
            &PhotoStack::default(),
            photo_engine::keystone::Upright::Vertical
        )
        .unwrap()
        .is_none());
}

#[test]
fn a_mask_reduces_noise_only_where_it_applies() {
    use photo_engine::local::Mask;
    let gpu = gpu_or_skip!();
    let (w, h) = (128u32, 96u32);
    let mut engine = Engine::new(&gpu.device);
    engine
        .load_raster(&gpu.device, &gpu.queue, noisy_step(w, h, 0.25))
        .unwrap();
    let clean = {
        let mut e = Engine::new(&gpu.device);
        e.load_raster(&gpu.device, &gpu.queue, noisy_step(w, h, 0.0))
            .unwrap();
        e.export(&gpu.device, &gpu.queue, &PhotoStack::default())
            .unwrap()
    };
    let mut stack = PhotoStack::default();
    assert!(!stack.denoise.enabled, "global noise reduction stays off");
    let grainy = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    // A gradient: full on the left, nothing past the middle.
    let mut mask = Mask::linear([0.1, 0.5], [0.45, 0.5]);
    mask.adjust.denoise = 100.0;
    stack.local.enabled = true;
    stack.local.params.masks.push(mask);
    let masked = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let region = |img: &photo_engine::ExportedImage, x0: u32, x1: u32| {
        let mut px = Vec::new();
        for y in 8..h - 8 {
            for x in x0..x1 {
                let i = ((y * w + x) * 4) as usize;
                px.extend_from_slice(&img.rgba[i..i + 3]);
            }
        }
        px
    };
    let (l0, l1) = (4, 12);
    let before = psnr(&region(&grainy, l0, l1), &region(&clean, l0, l1));
    let after = psnr(&region(&masked, l0, l1), &region(&clean, l0, l1));
    assert!(
        after > before + 3.0,
        "under the mask {before:.1} → {after:.1} dB"
    );
    // Past the mask's end, untouched.
    let (r0, r1) = (w * 3 / 4, w - 4);
    assert_eq!(region(&masked, r0, r1), region(&grainy, r0, r1));
}

#[test]
fn gpu_export_resizing_downsamples_to_target_dimensions() {
    let gpu = gpu_or_skip!();
    let (w, h) = (256u32, 256u32);
    let mut engine = photo_engine::Engine::new(&gpu.device);
    let raster = testkit::raster_from_fn(w, h, |_, _| [0.5, 0.5, 0.5]);
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();

    let stack = PhotoStack::default();
    let exported = engine
        .export_sized_in(
            &gpu.device,
            &gpu.queue,
            &stack,
            photo_engine::BitDepth::Eight,
            photo_engine::gamut::OutputSpace::Srgb,
            Some((128, 128)),
        )
        .unwrap();

    let photo_engine::Exported::Eight(img) = exported else {
        panic!("expected 8-bit exported image");
    };
    assert_eq!((img.width, img.height), (128, 128));
    assert_eq!(img.rgba.len(), 128 * 128 * 4);
    // The same grey as the full export, give or take the 8-bit dither: each resized texel
    // blends several dithered ones, so it can land on the neighbouring code.
    let full = engine.export(&gpu.device, &gpu.queue, &stack).unwrap();
    let off = img.rgba[0].abs_diff(full.rgba[0]);
    assert!(
        off <= 1,
        "resized {} against full {}",
        img.rgba[0],
        full.rgba[0]
    );
}

#[test]
fn gpu_gain_map_brightens_only_what_the_export_takes_past_white() {
    let gpu = gpu_or_skip!();
    let (w, h) = (64u32, 32u32);
    let mut engine = photo_engine::Engine::new(&gpu.device);
    // Two stops up: the left half lands at 0.8, under white; the right at 4, two stops
    // past it, which the SDR export takes to white and the HDR rendition keeps.
    let raster = testkit::raster_from_fn(w, h, |x, _| if x < w / 2 { [0.2; 3] } else { [1.0; 3] });
    engine.load_raster(&gpu.device, &gpu.queue, raster).unwrap();
    let mut stack = PhotoStack::default();
    stack.exposure.enabled = true;
    stack.exposure.params.ev = 2.0;

    let map = engine
        .gain_map(
            &gpu.device,
            &gpu.queue,
            &stack,
            photo_engine::gamut::OutputSpace::Srgb,
        )
        .unwrap();
    assert_eq!((map.width, map.height), (w / 4, h / 4));
    let want = ((4.0f32 + 1.0 / 64.0) / (1.0 + 1.0 / 64.0)).log2();
    assert!(
        (map.meta.max - want).abs() < 0.05,
        "{} stops for {want}",
        map.meta.max
    );
    for y in 0..map.height {
        let row = &map.data[(y * map.width) as usize..((y + 1) * map.width) as usize];
        let (left, right) = row.split_at(map.width as usize / 2);
        assert!(left.iter().all(|&c| c <= 2), "row {y}: {row:?}");
        assert!(right.iter().all(|&c| c >= 250), "row {y}: {row:?}");
    }

    // Written after the export's JPEG, which still reads as the SDR image.
    let settings = photo_engine::ExportSettings {
        hdr_gain_map: true,
        ..photo_engine::ExportSettings::new(photo_engine::ExportFormat::Jpeg)
    };
    let dir = std::env::temp_dir().join(format!("duckfoot-gainmap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hdr.jpg");
    let exported = engine
        .export_at(
            &gpu.device,
            &gpu.queue,
            &stack,
            photo_engine::BitDepth::Eight,
        )
        .unwrap();
    exported
        .save_with(&path, &settings, None, &[], Some(&map))
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let base = image::load_from_memory(&bytes).unwrap().to_rgb8();
    assert_eq!((base.width(), base.height()), (w, h));
    assert!(
        base.get_pixel(w - 4, 4)[0] >= 250,
        "the right half is white in SDR"
    );
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains(r#"Item:Semantic="GainMap""#));
    assert!(text.contains("urn:iso:std:iso:ts:21496:-1"));
    assert!(bytes.windows(4).any(|w| w == b"MPF\0"));
}
