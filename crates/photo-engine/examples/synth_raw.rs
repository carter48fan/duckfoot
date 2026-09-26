//! Write a synthetic, camera-sized DNG for benchmarking the open and thumbnail paths
//! without needing a real camera file on hand.
//!
//! The mosaic is a smooth scene with photosite noise and a clipped highlight region, so
//! every stage the real decode runs (including white-level measurement) has real work to
//! do. An embedded JPEG preview is written the way cameras do, so the thumbnail path is
//! exercised too. The generator itself is `photo_engine::testkit::synth_dng`, shared with
//! the GPU tests.
//!
//!     cargo run --release -p photo-engine --example synth_raw -- out.dng [width height] [--lossless]
//!
//! An output ending in `.jpg` writes a plain camera-sized JPEG instead, for the raster path.

use std::io::BufWriter;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(out) = args.first() else {
        eprintln!("usage: synth_raw <out.dng> [width height] [--lossless]");
        std::process::exit(2);
    };
    let lossless = args.iter().any(|a| a == "--lossless");
    let nums: Vec<usize> = args[1..].iter().filter_map(|a| a.parse().ok()).collect();
    // Sony A7 IV: 33 MP, 14-bit, ~66 MB uncompressed.
    let (w, h) = match nums.as_slice() {
        [w, h, ..] => (*w, *h),
        _ => (7040, 4688),
    };

    if out.ends_with(".jpg") {
        // A camera JPEG, for the raster open path.
        let img = image::RgbImage::from_fn(w as u32, h as u32, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let v = (0.5 + 0.35 * (fx * 9.0).sin() * (fy * 6.0).cos()) * 255.0;
            image::Rgb([v as u8, (v * 0.9) as u8, ((x ^ y) & 0xFF) as u8])
        });
        img.save(out)?;
        println!("{out}: {w}×{h} JPEG");
        return Ok(());
    }

    let bytes = photo_engine::testkit::synth_dng(w, h, lossless)?;
    std::io::copy(
        &mut bytes.as_slice(),
        &mut BufWriter::new(std::fs::File::create(out)?),
    )?;
    println!("{out}: {w}×{h}, {:.1} MB", bytes.len() as f64 / 1e6);
    Ok(())
}
