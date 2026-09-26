//! Synthetic inputs for the GPU tests and the examples. Not part of the engine's API.
//!
//! Every input here is ground truth by construction — a scene whose correct rendering is
//! known in advance — which is what lets a test assert on pixels instead of eyeballing a
//! photograph. They are small on purpose: a software rasteriser runs them in CI.

use std::collections::HashMap;
use std::io::Cursor;

use anyhow::{bail, Result};
use rawler::decoders::Camera;
use rawler::dng::writer::DngWriter;
use rawler::dng::{CropMode, DngCompression, DngPhotometricConversion, DNG_VERSION_V1_4};
use rawler::imgop::xyz::Illuminant;
use rawler::rawimage::{BlackLevel, CFAConfig, RawPhotometricInterpretation, WhiteLevel};
use rawler::{RawImage, RawImageData, CFA};

use crate::decode::{CfaImage, RasterImage};
use crate::params::Geometry;

pub const RGGB: [u32; 4] = [0, 1, 1, 2];

/// Identity: camera RGB is already linear sRGB, so a test measures the stage under test
/// and not the colour matrix.
pub const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The Sony A7 III matrix. Its negative coefficients are what drive a saturated blue to a
/// negative luminance, which is the input half the numerics bugs need.
pub const SONY_A7III: [[f32; 3]; 3] = [
    [1.1939, -0.1153, -0.0786],
    [-0.5488, 2.4177, -0.8689],
    [0.0203, -0.2783, 1.2581],
];

/// White level for the synthetic mosaics: 14-bit, like most current sensors.
pub const WHITE: u16 = 16383;

/// A Bayer mosaic of a scene given in linear camera RGB (1.0 = the white level). Each
/// photosite records only its own channel, exactly as a sensor does, with unit white
/// balance and an identity matrix.
pub fn cfa_from_fn(width: u32, height: u32, scene: impl Fn(u32, u32) -> [f32; 3]) -> CfaImage {
    let mut data = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            let colour = RGGB[((y % 2) * 2 + (x % 2)) as usize] as usize;
            let v = scene(x, y)[colour].clamp(0.0, 1.0) * WHITE as f32;
            data.push(v.round() as u16);
        }
    }
    CfaImage {
        data,
        width,
        height,
        cfa2x2: RGGB,
        black: [0.0; 4],
        white: [WHITE as f32; 4],
        wb: [1.0, 1.0, 1.0],
        cam_to_srgb: IDENTITY,
        daylight_wb: [1.0, 1.0, 1.0],
        orientation: Geometry::default(),
        make: "synthetic".into(),
        model: "testkit".into(),
    }
}

/// A uniform grey field at `level` of the white level.
pub fn flat(width: u32, height: u32, level: f32) -> CfaImage {
    cfa_from_fn(width, height, |_, _| [level; 3])
}

/// A smooth scene with detail at several scales, for comparing resolutions: nothing in it
/// is finer than ~8 px, so a box-filtered export and a mip-level preview should agree.
pub fn smooth_scene(width: u32, height: u32) -> impl Fn(u32, u32) -> [f32; 3] {
    let (w, h) = (width as f32, height as f32);
    move |x, y| {
        let (fx, fy) = (x as f32 / w, y as f32 / h);
        let base = 0.3 + 0.12 * (fx * 7.0).sin() * (fy * 5.0).cos();
        let detail = 0.05 * ((x as f32) * 0.35).sin() * ((y as f32) * 0.3).cos();
        let v = base + detail;
        [v * 0.9, v, v * 0.8 + 0.05 * fx]
    }
}

/// A raster (the JPEG/PNG path) straight from linear RGB, with no 8-bit quantisation in
/// the way — for tests that need exact scene values, like filmic's mid-grey.
pub fn raster_from_fn(
    width: u32,
    height: u32,
    scene: impl Fn(u32, u32) -> [f32; 3],
) -> RasterImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 8);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = scene(x, y);
            for v in [r, g, b, 0.0] {
                data.extend_from_slice(&half::f16::from_f32(v).to_le_bytes());
            }
        }
    }
    RasterImage {
        width,
        height,
        data,
        orientation: Geometry::default(),
        make: "synthetic".into(),
        model: "testkit".into(),
    }
}

/// Read an `OUTPUT_FORMAT` texture back, tightly packed RGBA8. The same banded path export
/// uses, so a test that compares the two compares like with like.
pub fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Result<Vec<u8>> {
    let size = texture.size();
    if texture.format() != crate::engine::OUTPUT_FORMAT {
        bail!("read_texture reads {:?} only", crate::engine::OUTPUT_FORMAT);
    }
    crate::engine::read_back(
        device,
        queue,
        texture,
        (0, 0, size.width, size.height),
        4,
        crate::engine::MAX_BAND_BYTES.min(device.limits().max_buffer_size),
    )
}

/// sRGB-encoded byte back to linear light.
pub fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A synthetic, camera-shaped DNG: a smooth scene with photosite noise and a clipped sun,
/// Sony A7 IV metadata and an embedded JPEG preview, so every stage of a real open —
/// white-level measurement, the thumbnail scan — has real work to do.
pub fn synth_dng(width: usize, height: usize, lossless: bool) -> Result<Vec<u8>> {
    let (w, h) = (width, height);
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
    let (pw, ph) = ((w / 4).max(1), (h / 4).max(1));
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
    Ok(buf.into_inner())
}
