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
    let (sx, sy) = photo_engine::sensor_point(&stack, (w, h), target);
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
