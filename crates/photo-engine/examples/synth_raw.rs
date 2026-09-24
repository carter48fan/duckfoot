//! Write a synthetic, camera-sized DNG for benchmarking the open and thumbnail paths
//! without needing a real camera file on hand.
//!
//! The mosaic is a smooth scene with photosite noise and a clipped highlight region, so
//! every stage the real decode runs (including white-level measurement) has real work to
//! do. An embedded JPEG preview is written the way cameras do, so the thumbnail path is
//! exercised too.
//!
//!     cargo run --release -p photo-engine --example synth_raw -- out.dng [width height] [--lossless]
//!
//! An output ending in `.jpg` writes a plain camera-sized JPEG instead, for the raster path.

use std::collections::HashMap;
use std::io::{BufWriter, Cursor};

use rawler::decoders::Camera;
use rawler::dng::writer::DngWriter;
use rawler::dng::{CropMode, DngCompression, DngPhotometricConversion, DNG_VERSION_V1_4};
use rawler::imgop::xyz::Illuminant;
use rawler::rawimage::{BlackLevel, CFAConfig, RawPhotometricInterpretation, WhiteLevel};
use rawler::{RawImage, RawImageData, CFA};

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

    let black = 512u16;
    let white = 16383u16;
    let mut data = vec![0u16; w * h];
    let mut seed = 0x2545F4914F6CDD1Du64;
    for y in 0..h {
        for x in 0..w {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let noise = (seed % 64) as f32 - 32.0;
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let colour = (y & 1) * 2 + (x & 1); // RGGB
            let tint = [0.55, 1.0, 1.0, 0.7][colour];
            let scene = 0.5 + 0.35 * (fx * 9.0).sin() * (fy * 6.0).cos();
            // A sun: clipped disc in the upper right.
            let sun = ((fx - 0.8).powi(2) + (fy - 0.2).powi(2)).sqrt() < 0.06;
            let v = if sun {
                white as f32
            } else {
                black as f32 + scene * tint * (white - black) as f32 * 0.9 + noise
            };
            data[y * w + x] = v.clamp(0.0, white as f32) as u16;
        }
    }

    let cfa = CFA::new("RGGB");
    let camera = Camera {
        cfa: cfa.clone(),
        ..Default::default()
    };
    let mut color_matrix = HashMap::new();
    // Sony ILCE-7M4 D65 matrix, as rawler's camera database records it.
    color_matrix.insert(
        Illuminant::D65,
        vec![
            0.7460, -0.2365, -0.0588, -0.5687, 1.3442, 0.2474, -0.0624, 0.1156, 0.6584,
        ],
    );
    let raw = RawImage {
        camera: camera.clone(),
        make: "SONY".into(),
        model: "ILCE-7M4".into(),
        clean_make: "Sony".into(),
        clean_model: "ILCE-7M4".into(),
        width: w,
        height: h,
        cpp: 1,
        bps: 14,
        wb_coeffs: [2.4, 1.0, 1.6, f32::NAN],
        whitelevel: WhiteLevel::new(vec![white as u32]),
        blacklevel: BlackLevel::new(&[black, black, black, black], 2, 2, 1),
        xyz_to_cam: [[0.0; 3]; 4],
        photometric: RawPhotometricInterpretation::Cfa(CFAConfig::new(&cfa, &camera.plane_color)),
        active_area: None,
        crop_area: None,
        blackareas: Vec::new(),
        orientation: rawler::Orientation::Normal,
        data: RawImageData::Integer(data),
        color_matrix,
        dng_tags: HashMap::new(),
        fuji_rotation_width: None,
    };

    // A preview to embed, the way a camera does: a quick RGB rendering of the mosaic.
    let (pw, ph) = (w / 4, h / 4);
    let preview = image::RgbImage::from_fn(pw as u32, ph as u32, |x, y| {
        let (fx, fy) = (x as f32 / pw as f32, y as f32 / ph as f32);
        let v = (0.5 + 0.35 * (fx * 9.0).sin() * (fy * 6.0).cos()) * 255.0;
        image::Rgb([v as u8, (v * 0.9) as u8, (v * 0.8) as u8])
    });
    let preview = image::DynamicImage::ImageRgb8(preview);

    let mut buf = Cursor::new(Vec::new());
    {
        let mut dng = DngWriter::new(&mut buf, DNG_VERSION_V1_4)?;
        let mut sub = dng.subframe(0);
        sub.raw_image(
            &raw,
            CropMode::Best,
            if lossless {
                DngCompression::Lossless
            } else {
                DngCompression::Uncompressed
            },
            DngPhotometricConversion::Original,
            1,
        )?;
        sub.finalize()?;
        let mut prev = dng.subframe(1);
        prev.preview(&preview, 0.9)?;
        prev.finalize()?;
        dng.thumbnail(&preview)?;
        dng.load_base_tags(&raw)?;
        dng.close()?;
    }
    let bytes = buf.into_inner();
    std::io::copy(
        &mut bytes.as_slice(),
        &mut BufWriter::new(std::fs::File::create(out)?),
    )?;
    println!("{out}: {w}×{h}, {:.1} MB", bytes.len() as f64 / 1e6);
    Ok(())
}
