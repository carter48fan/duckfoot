//! Render a RAW the way the app opens it — `PhotoStack::for_raw()`, the file's own
//! orientation, full resolution — and write it as a 16-bit PNG, for comparing against
//! another raw converter's export of the same file.
//!
//!     cargo run --release --example render -- IMG_0001.ARW out.png [--no-sharpen]
//!         [--ev=0.5] [--curve=0,0;0.25,0.2;0.75,0.8;1,1]
//!
//! `--no-sharpen` turns capture sharpening off, so the demosaic is compared on its own.
//! `--ev` and `--curve` (display-domain nodes) try a base look on top of the defaults.

use photo_engine::curve::CurveNode;
use photo_engine::{DefaultRawDecoder, Engine, PhotoStack, RawDecoder};

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let no_sharpen = args.iter().any(|a| a == "--no-sharpen");
    let flag = |name: &str| {
        args.iter()
            .find_map(|a| a.strip_prefix(&format!("--{name}=")).map(str::to_owned))
    };
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [raw, out] = paths[..] else {
        eprintln!("usage: render <raw-file> <out.png> [--no-sharpen] [--ev=N] [--curve=x,y;...]");
        std::process::exit(2);
    };

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("render"),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))?;

    let cfa = DefaultRawDecoder.decode_file(std::path::Path::new(raw))?;
    let mut stack = PhotoStack::for_raw();
    stack.geometry = cfa.orientation;
    if no_sharpen {
        stack.sharpen.enabled = false;
    }
    if let Some(amount) = flag("sharpen") {
        stack.sharpen.params.amount = amount.parse()?;
    }
    if let Some(ev) = flag("ev") {
        stack.exposure.enabled = true;
        stack.exposure.params.ev = ev.parse()?;
    }
    if let Some(curve) = flag("curve") {
        let nodes = curve
            .split(';')
            .map(|xy| {
                let (x, y) = xy.split_once(',').ok_or_else(|| anyhow::anyhow!("node {xy}"))?;
                Ok(CurveNode::new(x.trim().parse()?, y.trim().parse()?))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        stack.tone_curve.enabled = true;
        stack.tone_curve.params = nodes;
    }

    let mut engine = Engine::new(&device);
    engine.load(&device, &queue, cfa)?;
    engine.set_geometry(&device, stack.geometry);
    let exported = engine.export_16(&device, &queue, &stack)?;

    let out = std::path::Path::new(out);
    let mut settings = photo_engine::ExportSettings::for_path(out)?;
    settings.depth = photo_engine::export::BitDepth::Sixteen;
    exported.write(out, &settings, None)?;
    println!("{}x{} → {}", exported.width, exported.height, out.display());
    Ok(())
}
