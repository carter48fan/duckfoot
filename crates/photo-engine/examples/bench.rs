//! Headless end-to-end proof: decode a real RAW, run the GPU pipeline at full sensor
//! resolution, read pixels back, and time a slider drag.
//!
//! This is the PoC's pass/fail. "It opened a window without crashing" is not evidence that
//! the pipeline produced correct pixels; reading them back is.
//!
//!     cargo run --release --example bench -- /path/to/IMG_0001.ARW

use std::time::Instant;

use photo_engine::{Engine, PhotoStack, RawDecoder, RawloaderBackend};

/// Read back a square from the middle of the image. 256 px keeps `bytes_per_row` at
/// 1024 — already a multiple of wgpu's 256-byte copy alignment, so no padding dance.
const PATCH: u32 = 256;

/// Block until everything submitted so far has actually executed. Without this the
/// timings below would measure how fast we can *queue* work, not how fast the GPU does it.
fn wait(device: &wgpu::Device) -> anyhow::Result<()> {
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    })?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: bench <raw-file>");
        std::process::exit(2);
    };

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))?;
    println!(
        "adapter      {} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("bench"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
        ..Default::default()
    }))?;
    println!(
        "max texture  {} px",
        device.limits().max_texture_dimension_2d
    );

    let t = Instant::now();
    let cfa = RawloaderBackend.decode_file(std::path::Path::new(&path))?;
    let (w, h) = (cfa.width, cfa.height);
    println!(
        "decode       {:.0} ms   {w}×{h} ({:.1} MP)",
        t.elapsed().as_secs_f64() * 1000.0,
        cfa.megapixels()
    );

    let mut engine = Engine::new(&device);

    let t = Instant::now();
    engine.load(&device, &queue, cfa)?;
    wait(&device)?;
    println!(
        "upload+demosaic {:.0} ms",
        t.elapsed().as_secs_f64() * 1000.0
    );

    // Benchmark what the app actually shows by default. Enabling modules here that the
    // UI leaves off is how a broken module hides from its own benchmark.
    let mut stack = PhotoStack::default();
    if std::env::var("BENCH_FILMIC").is_ok() {
        stack.filmic.enabled = true;
    }
    if std::env::var("BENCH_CURVE").is_ok() {
        stack.tone_curve.enabled = true;
    }

    // Warm up: first submit compiles/validates and is not representative.
    engine.render(&device, &queue, &stack);
    wait(&device)?;

    // A slider drag: rewrite the uniform buffer and re-run the chain. Nothing is decoded,
    // nothing is uploaded, nothing is allocated (DESIGN.md invariant 6).
    const N: usize = 60;
    let t = Instant::now();
    for i in 0..N {
        stack.exposure.params.ev = (i as f32) * 0.01;
        engine.render(&device, &queue, &stack);
    }
    wait(&device)?;
    let per_frame = t.elapsed().as_secs_f64() * 1000.0 / N as f64;
    println!("slider drag  {per_frame:.2} ms/frame over {N} frames at FULL resolution");

    // --- read pixels back and prove they are real -------------------------------------
    let image = engine.image().expect("image loaded");
    let bytes = (PATCH * PATCH * 4) as u64;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: image.output_texture(),
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: (w / 2).saturating_sub(PATCH / 2),
                y: (h / 2).saturating_sub(PATCH / 2),
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PATCH * 4),
                rows_per_image: Some(PATCH),
            },
        },
        wgpu::Extent3d {
            width: PATCH,
            height: PATCH,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    wait(&device)?;

    let data = slice.get_mapped_range()?;
    let px = data.chunks_exact(4);
    let n = px.len() as f64;
    let (mut r, mut g, mut b) = (0.0f64, 0.0, 0.0);
    let mut black = 0usize;
    for c in px {
        r += c[0] as f64;
        g += c[1] as f64;
        b += c[2] as f64;
        if c[0] == 0 && c[1] == 0 && c[2] == 0 {
            black += 1;
        }
    }
    println!(
        "centre patch mean RGB [{:.1}, {:.1}, {:.1}]   fully-black pixels {}/{}",
        r / n,
        g / n,
        b / n,
        black,
        n as usize
    );

    anyhow::ensure!(
        black < (n as usize * 9 / 10),
        "the pipeline produced an essentially black image — something upstream is broken"
    );
    anyhow::ensure!(
        r / n > 0.5 && g / n > 0.5 && b / n > 0.5,
        "at least one channel is dead"
    );

    // --- export the full frame and read it back off disk --------------------------------
    let t = Instant::now();
    let exported = engine.export(&device, &queue)?;
    println!(
        "export readback {:.0} ms   {}x{} ({:.1} MP, {} MB)",
        t.elapsed().as_secs_f64() * 1000.0,
        exported.width,
        exported.height,
        exported.megapixels(),
        exported.rgba.len() / 1_048_576
    );

    let out = std::env::temp_dir().join("duckfoot-bench-export.png");
    let t = Instant::now();
    exported.write(&out)?;
    println!(
        "png encode+write {:.0} ms   {}",
        t.elapsed().as_secs_f64() * 1000.0,
        out.display()
    );

    // Decoding it again is the only way to know the file is a real image and not a
    // correctly-sized pile of sheared rows.
    let reread = image::open(&out)?.to_rgba8();
    anyhow::ensure!(
        reread.width() == exported.width && reread.height() == exported.height,
        "wrote {}x{} but read back {}x{}",
        exported.width,
        exported.height,
        reread.width(),
        reread.height()
    );
    anyhow::ensure!(
        reread.as_raw() == &exported.rgba,
        "PNG round trip changed the pixels"
    );

    // The exported frame must equal the preview: same shader, same uniforms, same texture.
    let cx = (exported.width / 2 - PATCH / 2) as usize;
    let cy = (exported.height / 2 - PATCH / 2) as usize;
    let row = exported.width as usize * 4;
    let mut mismatched = 0usize;
    for y in 0..PATCH as usize {
        for x in 0..(PATCH as usize * 4) {
            let from_export = exported.rgba[(cy + y) * row + cx * 4 + x];
            let from_preview = data[y * PATCH as usize * 4 + x];
            if from_export != from_preview {
                mismatched += 1;
            }
        }
    }
    anyhow::ensure!(
        mismatched == 0,
        "export and preview disagree on {mismatched} bytes — DESIGN.md invariant 2 is broken"
    );
    println!("export matches preview byte for byte over the sampled patch");

    // A photograph has contrast. Asserting only "not black, all channels live" passed a
    // build whose filmic module crushed every pixel into a narrow band around mid grey.
    // Measured over the whole frame, not a patch: the centre of a frame is often a flat
    // subject, and a broken pipeline is not.
    //
    //   broken filmic (latitude 200): stddev ~3
    //   working filmic (latitude 20): stddev ~30
    //   no filmic:                    stddev ~33
    let luma: Vec<f64> = exported
        .rgba
        .chunks_exact(4)
        .map(|c| 0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64)
        .collect();
    let mean_luma = luma.iter().sum::<f64>() / luma.len() as f64;
    let stddev =
        (luma.iter().map(|v| (v - mean_luma).powi(2)).sum::<f64>() / luma.len() as f64).sqrt();
    println!("full frame luma mean {mean_luma:.1}  stddev {stddev:.1}");
    anyhow::ensure!(
        stddev > 8.0,
        "full-frame stddev is {stddev:.1} — the image is nearly uniform, so a module is \
         crushing the tonal range instead of mapping it"
    );

    // --- synthetic check: a partly clipped highlight must come out neutral --------------
    //
    // Independent of whatever photograph was passed in, and modelled on the real failure.
    // A *fully* clipped pixel clamps to white with or without reconstruction, so it proves
    // nothing. The magenta comes from PARTIAL clipping: for a neutral subject the green
    // photosite saturates first (its balance gain is 1.0 while red and blue are 2.03 and
    // 1.80), so green pins at the white level while red and blue still have headroom. The
    // colour matrix then subtracts those larger red and blue values from green
    // — the Sony green row is [-0.549, 2.418, -0.869] — and drives output green below the
    // other two. That is the magenta.
    let (sw, sh) = (256u32, 256u32);
    let cfa2x2 = [0u32, 1, 1, 2]; // RGGB
    let white = 4000u16;
    let mut data = vec![0u16; (sw * sh) as usize];
    for y in 0..sh {
        for x in 0..sw {
            let colour = cfa2x2[((y % 2) * 2 + (x % 2)) as usize];
            // Green at saturation, red and blue well short of it.
            data[(y * sw + x) as usize] = if colour == 1 {
                white
            } else {
                (white as f32 * 0.6) as u16
            };
        }
    }
    let clipped = photo_engine::CfaImage {
        data,
        width: sw,
        height: sh,
        cfa2x2,
        black: [0.0; 4],
        white: [white as f32; 4],
        wb: [2.03, 1.0, 1.80],
        // The real Sony A7 III matrix, because the mixing is what produces the cast.
        cam_to_srgb: [
            [1.1939, -0.1153, -0.0786],
            [-0.5488, 2.4177, -0.8689],
            [0.0203, -0.2783, 1.2581],
        ],
        make: "synthetic".into(),
        model: "clipped-green".into(),
    };
    engine.load(&device, &queue, clipped)?;
    engine.render(&device, &queue, &PhotoStack::default());
    wait(&device)?;
    let blown = engine.export(&device, &queue)?;
    // Sample away from the border so the demosaic has full neighbourhoods.
    let mid = ((sh / 2 * sw + sw / 2) * 4) as usize;
    let px = &blown.rgba[mid..mid + 3];
    let spread = px.iter().max().unwrap() - px.iter().min().unwrap();
    println!("clipped-green highlight -> RGB {px:?}  channel spread {spread}");
    anyhow::ensure!(
        spread <= 3,
        "a clipped highlight came out as RGB {px:?} (spread {spread}) — blown highlights \
         are taking on the colour of the white balance gains instead of staying neutral"
    );

    // --- synthetic check: fine neutral detail must not gain colour ----------------------
    //
    // Ground truth, unlike any photograph. The scene is pure luminance — grey vertical
    // stripes, zero chroma everywhere — so a neutral sensor records equal values in all
    // three channels and a correct demosaic returns equal values. Every bit of colour in
    // the output is therefore false colour, measured rather than eyeballed.
    //
    // A stripe period of 4 px is comfortably resolvable by a 2x2 Bayer grid; this is not a
    // Nyquist trick question. The identity matrix and unit white balance keep the
    // measurement about the demosaic and nothing else.
    let (sw, sh) = (256u32, 256u32);
    let cfa2x2 = [0u32, 1, 1, 2];
    let white = 4095u16;
    let mut data = vec![0u16; (sw * sh) as usize];
    for y in 0..sh {
        for x in 0..sw {
            let bright = (x / 2) % 2 == 0;
            data[(y * sw + x) as usize] = if bright { white } else { white / 8 };
        }
    }
    let stripes = photo_engine::CfaImage {
        data,
        width: sw,
        height: sh,
        cfa2x2,
        black: [0.0; 4],
        white: [white as f32; 4],
        wb: [1.0, 1.0, 1.0],
        cam_to_srgb: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        make: "synthetic".into(),
        model: "neutral-stripes".into(),
    };
    engine.load(&device, &queue, stripes)?;
    engine.render(&device, &queue, &PhotoStack::default());
    wait(&device)?;
    let out = engine.export(&device, &queue)?;

    // Skip a 4 px border, where the clamped edge sampling has no real neighbourhood.
    let mut worst = 0i32;
    let mut total = 0i64;
    let mut counted = 0i64;
    for y in 4..(sh - 4) as usize {
        for x in 4..(sw - 4) as usize {
            let i = (y * sw as usize + x) * 4;
            let (r, g, b) = (
                out.rgba[i] as i32,
                out.rgba[i + 1] as i32,
                out.rgba[i + 2] as i32,
            );
            let err = (r - g).abs().max((b - g).abs());
            worst = worst.max(err);
            total += err as i64;
            counted += 1;
        }
    }
    let mean = total as f64 / counted as f64;
    println!("neutral stripes -> false colour: mean {mean:.2}, worst {worst} (of 255)");
    anyhow::ensure!(
        worst <= 24 && mean <= 3.0,
        "a purely neutral test pattern picked up colour: mean {mean:.2}, worst {worst}. \
         The demosaic is inventing chroma on fine detail."
    );

    println!("\nOK — real pixels, all three channels live, export verified.");
    Ok(())
}
