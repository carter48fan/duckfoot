//! Decode a RAW file and print what the engine actually extracted from it.
//!
//! This is the check that the decode is doing its job — every field printed here is one
//! the previous build ignored, and each one has a visible failure mode if it is wrong.
//!
//!     cargo run --release --example probe -- /path/to/IMG_0001.ARW

use std::time::Instant;

use photo_engine::{RawDecoder, RawloaderBackend};

fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).is_none() {
        eprintln!("usage: probe <raw-file> [more files...]");
        std::process::exit(2);
    }

    for arg in std::env::args().skip(1) {
        let started = Instant::now();
        match RawloaderBackend.decode_file(std::path::Path::new(&arg)) {
            Ok(img) => {
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                println!("── {arg}");
                println!("   camera     {} {}", img.make, img.model);
                println!(
                    "   cropped    {}×{}  ({:.1} MP)",
                    img.width,
                    img.height,
                    img.megapixels()
                );
                println!(
                    "   cfa 2x2    [{} {} / {} {}]   (0=R 1=G 2=B)",
                    img.cfa2x2[0], img.cfa2x2[1], img.cfa2x2[2], img.cfa2x2[3]
                );
                println!(
                    "   black      {:?}\n   white      {:?}",
                    &img.black[..3],
                    &img.white[..3]
                );
                println!(
                    "   wb mult    [{:.4}, {:.4}, {:.4}]",
                    img.wb[0], img.wb[1], img.wb[2]
                );
                println!("   cam→sRGB   {:.4?}", img.cam_to_srgb);
                for (i, row) in img.cam_to_srgb.iter().enumerate() {
                    let sum: f32 = row.iter().sum();
                    assert!(
                        (sum - 1.0).abs() < 1e-3,
                        "row {i} sums to {sum}, so neutral will not map to neutral"
                    );
                }
                assert_eq!(
                    img.data.len(),
                    img.width as usize * img.height as usize,
                    "sample count does not match the cropped dimensions"
                );
                println!("   decode     {ms:.0} ms");
            }
            Err(e) => println!("── {arg}\n   FAILED: {e:#}"),
        }
        println!();
    }
    Ok(())
}
