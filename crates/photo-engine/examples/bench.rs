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

    let mut stack = PhotoStack::default();
    stack.filmic.enabled = true;
    stack.tone_curve.enabled = true;
    stack.color_balance.enabled = true;

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
    println!("\nOK — real pixels, all three channels live.");
    Ok(())
}
