use std::path::Path;
use super::types::*;

/// Decodes camera RAW files (CR3, CR2, NEF, ARW, DNG, etc.) into linear float32 RGB scene buffer.
pub fn decode_camera_raw_file(file_path: &str, max_edge: usize) -> Result<RawDecodeResult, String> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(format!("File does not exist: {}", file_path));
    }

    let raw_file = rawloader::decode_file(path)
        .map_err(|e| format!("Failed to decode RAW file {}: {:?}", file_path, e))?;

    let make = raw_file.make.clone();
    let model = raw_file.model.clone();
    let src_w = raw_file.width;
    let src_h = raw_file.height;

    let default_black = raw_file.blacklevels[0] as u16;
    let default_white = raw_file.whitelevels[0] as u16;

    // As-shot kelvin approximation from white balance coefficients if present
    let as_shot_kelvin = if raw_file.wb_coeffs.len() >= 3 && raw_file.wb_coeffs[1] > 0.0 {
        let r = raw_file.wb_coeffs[0] / raw_file.wb_coeffs[1];
        Some((5500.0 * (r / 1.5).powf(1.8)).clamp(2000.0, 10000.0))
    } else {
        None
    };

    let metadata = RawMetadata {
        make,
        model,
        width: src_w,
        height: src_h,
        default_black,
        default_white,
        as_shot_kelvin,
    };

    // Fast demosaic to float32
    // If max_edge > 0 and image is larger, decimate to fit within max_edge (for responsive preview)
    let scale = if max_edge > 0 && (src_w > max_edge || src_h > max_edge) {
        let factor_w = src_w as f32 / max_edge as f32;
        let factor_h = src_h as f32 / max_edge as f32;
        factor_w.max(factor_h)
    } else {
        1.0
    };

    let dst_w = (src_w as f32 / scale).round() as usize;
    let dst_h = (src_h as f32 / scale).round() as usize;

    let mut scene_data = vec![0.0f32; dst_w * dst_h * 3];

    match raw_file.data {
        rawloader::RawImageData::Integer(ref data) => {
            let black = default_black as f32;
            let white = default_white as f32;
            let range = (white - black).max(1.0);

            // Bilinear / box sample demosaic from Bayer mosaic
            for dy in 0..dst_h {
                let sy = ((dy as f32 * scale).floor() as usize).min(src_h - 1);
                for dx in 0..dst_w {
                    let sx = ((dx as f32 * scale).floor() as usize).min(src_w - 1);
                    let di = (dy * dst_w + dx) * 3;

                    // Read local 2x2 sensor neighborhood for Bayer RGB
                    let idx00 = sy * src_w + sx;
                    let idx01 = sy * src_w + (sx + 1).min(src_w - 1);
                    let idx10 = (sy + 1).min(src_h - 1) * src_w + sx;
                    let idx11 = (sy + 1).min(src_h - 1) * src_w + (sx + 1).min(src_w - 1);

                    let v00 = ((data[idx00] as f32 - black) / range).clamp(0.0, 1.0);
                    let v01 = ((data[idx01] as f32 - black) / range).clamp(0.0, 1.0);
                    let v10 = ((data[idx10] as f32 - black) / range).clamp(0.0, 1.0);
                    let v11 = ((data[idx11] as f32 - black) / range).clamp(0.0, 1.0);

                    // Standard RGGB mosaic assumption
                    let is_even_row = (sy % 2) == 0;
                    let is_even_col = (sx % 2) == 0;

                    let (r, g, b) = match (is_even_row, is_even_col) {
                        (true, true) => (v00, (v01 + v10) * 0.5, v11),
                        (true, false) => (v01, (v00 + v11) * 0.5, v10),
                        (false, true) => (v10, (v00 + v11) * 0.5, v01),
                        (false, false) => (v11, (v01 + v10) * 0.5, v00),
                    };

                    scene_data[di] = r;
                    scene_data[di + 1] = g;
                    scene_data[di + 2] = b;
                }
            }
        }
        rawloader::RawImageData::Float(ref data) => {
            for dy in 0..dst_h {
                let sy = ((dy as f32 * scale).floor() as usize).min(src_h - 1);
                for dx in 0..dst_w {
                    let sx = ((dx as f32 * scale).floor() as usize).min(src_w - 1);
                    let di = (dy * dst_w + dx) * 3;
                    let idx = (sy * src_w + sx).min(data.len() - 1);
                    let val = data[idx].clamp(0.0, 1.0);
                    scene_data[di] = val;
                    scene_data[di + 1] = val;
                    scene_data[di + 2] = val;
                }
            }
        }
    }

    Ok(RawDecodeResult {
        metadata,
        width: dst_w,
        height: dst_h,
        scene_data,
    })
}
