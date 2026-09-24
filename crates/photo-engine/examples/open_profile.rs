//! Where the time goes between "open this file" and "pixels are ready", stage by stage.
//!
//!     cargo run --release -p photo-engine --example open_profile -- /path/to/IMG_0001.ARW
//!
//! Each stage runs several times and the fastest is reported, so page-cache warm-up on the
//! first read does not dominate. Run with `RUST_LOG=debug` for the decoder's own timings.

use std::path::Path;
use std::time::Instant;

use photo_engine::{decode_raw, load_embedded_preview};

const RUNS: usize = 5;

fn best<T>(label: &str, mut f: impl FnMut() -> T) -> T {
    let mut fastest = f64::MAX;
    let mut out = None;
    for _ in 0..RUNS {
        let t = Instant::now();
        let v = f();
        fastest = fastest.min(t.elapsed().as_secs_f64() * 1000.0);
        out = Some(v);
    }
    println!("{label:<34} {fastest:>8.1} ms");
    out.unwrap()
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("usage: open_profile <raw-file>");
        std::process::exit(2);
    };
    let path = Path::new(&arg);
    println!(
        "{arg}  ({:.1} MB)",
        std::fs::metadata(path)?.len() as f64 / 1e6
    );

    if photo_engine::is_raster_path(path) {
        let bytes = best("fs::read", || std::fs::read(path).unwrap());
        let img = best("decode_raster", || {
            photo_engine::decode_raster(&bytes).unwrap()
        });
        println!("   → {}×{}", img.width, img.height);
        best("thumbnail (scaled JPEG decode)", || {
            photo_engine::decode_jpeg_scaled(&bytes, 256)
        });
        return Ok(());
    }

    println!("── open");
    let bytes = best("fs::read", || std::fs::read(path).unwrap());
    best("rawler::decode (from slice)", || {
        let src = rawler::rawsource::RawSource::new_from_slice(&bytes);
        rawler::decode(&src, &Default::default()).unwrap()
    });
    best("rawler::decode (mmap)", || {
        let src = rawler::rawsource::RawSource::new(path).unwrap();
        rawler::decode(&src, &Default::default()).unwrap()
    });
    let cfa = best("decode_raw (what the app runs)", || {
        decode_raw(path).unwrap()
    });
    println!(
        "   → {}×{} ({:.1} MP)",
        cfa.width,
        cfa.height,
        cfa.megapixels()
    );

    println!("── embedded preview");
    for want in [256, 2048] {
        match best(&format!("load_embedded_preview({want})"), || {
            load_embedded_preview(path, want)
        }) {
            Some(p) => println!("   → {}×{}", p.width, p.height),
            None => println!("   (none: a thumbnail falls back to the mosaic)"),
        }
    }
    Ok(())
}
