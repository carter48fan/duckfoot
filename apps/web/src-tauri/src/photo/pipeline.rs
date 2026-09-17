use rayon::prelude::*;
use super::types::*;

/// Monotone cubic (Fritsch-Carlson) spline LUT for the tone curve.
pub fn build_curve_lut(nodes: &[CurveNode], size: usize) -> Vec<f32> {
    let mut pts = nodes.to_vec();
    pts.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));

    if pts.len() < 2 {
        return (0..size).map(|i| i as f32 / (size - 1) as f32).collect();
    }

    let n = pts.len();
    let mut dx = Vec::with_capacity(n - 1);
    let mut slope = Vec::with_capacity(n - 1);

    for i in 0..n - 1 {
        let h = pts[i + 1].x - pts[i].x;
        dx.push(h);
        slope.push(if h == 0.0 { 0.0 } else { (pts[i + 1].y - pts[i].y) / h });
    }

    let mut tangent = vec![0.0f32; n];
    tangent[0] = slope[0];
    tangent[n - 1] = slope[n - 2];

    for i in 1..n - 1 {
        if slope[i - 1] * slope[i] <= 0.0 {
            tangent[i] = 0.0;
        } else {
            tangent[i] = (slope[i - 1] + slope[i]) / 2.0;
            let limit = 3.0 * slope[i - 1].abs().min(slope[i].abs());
            if tangent[i].abs() > limit {
                tangent[i] = tangent[i].signum() * limit;
            }
        }
    }

    let mut lut = Vec::with_capacity(size);
    let mut seg = 0;

    for i in 0..size {
        let x = i as f32 / (size - 1) as f32;
        while seg < n - 2 && x > pts[seg + 1].x {
            seg += 1;
        }
        let h = dx[seg];
        if h == 0.0 {
            lut.push(pts[seg].y);
            continue;
        }
        let t = (x - pts[seg].x) / h;
        let t2 = t * t;
        let t3 = t2 * t;

        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * pts[seg].y
            + (t3 - 2.0 * t2 + t) * h * tangent[seg]
            + (-2.0 * t3 + 3.0 * t2) * pts[seg + 1].y
            + (t3 - t2) * h * tangent[seg + 1];
        lut.push(y);
    }

    lut
}

#[inline(always)]
pub fn sample_lut(lut: &[f32], value: f32) -> f32 {
    if value <= 0.0 {
        return lut[0];
    }
    let max_idx = lut.len() - 1;
    if value >= 1.0 {
        return lut[max_idx] + (value - 1.0);
    }
    let pos = value * max_idx as f32;
    let i = pos.floor() as usize;
    let frac = pos - i as f32;
    lut[i] + (lut[(i + 1).min(max_idx)] - lut[i]) * frac
}

#[inline(always)]
fn balance_channel(value: f32, lift: f32, gamma: f32, gain: f32, offset: f32) -> f32 {
    let mut v = value + offset;
    v = v * (1.0 + gain) + lift * (1.0 - v);
    if v > 0.0 && gamma != 0.0 {
        v = v.powf(1.0 / (1.0 + gamma));
    }
    v
}

/// Executes the full scene-referred RAW pipeline on linear RGB float pixels with Rayon parallelism.
pub fn process_scene(
    pixels: &mut [f32],
    _width: usize,
    _height: usize,
    stack: &RawStackParams,
) {
    // 1. Raw Levels
    if let Some(ref raw_levels) = stack.raw_levels {
        if raw_levels.enabled {
            let p = &raw_levels.params;
            let range = p.white - p.black;
            if range > 0.0 {
                let scale = 1.0 / range;
                let black = p.black / 65535.0;
                let norm = 65535.0 * scale;

                pixels.par_chunks_mut(3).for_each(|rgb| {
                    rgb[0] = ((rgb[0] - black) * norm).max(0.0);
                    rgb[1] = ((rgb[1] - black) * norm).max(0.0);
                    rgb[2] = ((rgb[2] - black) * norm).max(0.0);
                });
            }
        }
    }

    // 2. White Balance
    if let Some(ref wb) = stack.white_balance {
        if wb.enabled {
            let p = &wb.params;
            let t = p.temperature_k / 5500.0;
            let r_gain = t.powf(0.55);
            let b_gain = (1.0 / t).powf(0.55);
            let g_gain = 1.0 - p.tint / 400.0;

            pixels.par_chunks_mut(3).for_each(|rgb| {
                rgb[0] *= r_gain;
                rgb[1] *= g_gain;
                rgb[2] *= b_gain;
            });
        }
    }

    // 3. Exposure
    if let Some(ref exp) = stack.exposure {
        if exp.enabled {
            let p = &exp.params;
            let gain = 2.0f32.powf(p.ev);
            let black = p.black_level;

            pixels.par_chunks_mut(3).for_each(|rgb| {
                rgb[0] = (rgb[0] - black) * gain;
                rgb[1] = (rgb[1] - black) * gain;
                rgb[2] = (rgb[2] - black) * gain;
            });
        }
    }

    // 4. Tone Curve
    if let Some(ref tc) = stack.tone_curve {
        if tc.enabled && !tc.params.nodes.is_empty() {
            let lut = build_curve_lut(&tc.params.nodes, 1024);
            let channel = tc.params.channel.as_str();

            pixels.par_chunks_mut(3).for_each(|rgb| {
                match channel {
                    "r" => rgb[0] = sample_lut(&lut, rgb[0]),
                    "g" => rgb[1] = sample_lut(&lut, rgb[1]),
                    "b" => rgb[2] = sample_lut(&lut, rgb[2]),
                    "l" => {
                        let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
                        if luma > 0.0 {
                            let scale = sample_lut(&lut, luma) / luma;
                            rgb[0] *= scale;
                            rgb[1] *= scale;
                            rgb[2] *= scale;
                        }
                    }
                    _ => {
                        rgb[0] = sample_lut(&lut, rgb[0]);
                        rgb[1] = sample_lut(&lut, rgb[1]);
                        rgb[2] = sample_lut(&lut, rgb[2]);
                    }
                }
            });
        }
    }

    // 5. Filmic RGB
    if let Some(ref filmic) = stack.filmic_rgb {
        if filmic.enabled {
            let p = &filmic.params;
            let white = 2.0f32.powf(p.white_rel_ev);
            let black = 2.0f32.powf(p.black_rel_ev);
            let latitude = (p.latitude / 100.0).max(0.01);
            let contrast = p.contrast;
            let log_range = (white / black).log2();

            pixels.par_chunks_mut(3).for_each(|rgb| {
                for c in 0..3 {
                    let v = rgb[c];
                    if v <= 0.0 {
                        rgb[c] = 0.0;
                    } else {
                        let norm = ((v / black).log2() / log_range - 0.5) * contrast;
                        let s = 1.0 / (1.0 + (-norm / latitude).exp());
                        rgb[c] = s.clamp(0.0, 1.0);
                    }
                }
            });
        }
    }

    // 6. Color Balance RGB (4-way lift/gamma/gain/offset)
    if let Some(ref cb) = stack.color_balance_rgb {
        if cb.enabled {
            let p = &cb.params;
            pixels.par_chunks_mut(3).for_each(|rgb| {
                rgb[0] = balance_channel(rgb[0], p.lift.r, p.gamma.r, p.gain.r, p.offset.r);
                rgb[1] = balance_channel(rgb[1], p.lift.g, p.gamma.g, p.gain.g, p.offset.g);
                rgb[2] = balance_channel(rgb[2], p.lift.b, p.gamma.b, p.gain.b, p.offset.b);
            });
        }
    }

    // 7. Output Profile & Gamut clamp
    pixels.par_chunks_mut(3).for_each(|rgb| {
        rgb[0] = rgb[0].clamp(0.0, 1.0);
        rgb[1] = rgb[1].clamp(0.0, 1.0);
        rgb[2] = rgb[2].clamp(0.0, 1.0);
    });
}

/// Converts linear float32 RGB to 8-bit RGBA and computes 256-bin histogram in a single parallel pass.
pub fn scene_to_rgba_and_histogram(
    pixels: &[f32],
    _width: usize,
    _height: usize,
) -> (Vec<u8>, HistogramResult, f32) {
    let num_pixels = pixels.len() / 3;
    let mut rgba = vec![0u8; num_pixels * 4];

    // Compute RGBA conversion in parallel
    rgba.par_chunks_mut(4)
        .zip(pixels.par_chunks(3))
        .for_each(|(out, rgb)| {
            out[0] = (rgb[0] * 255.0).clamp(0.0, 255.0) as u8;
            out[1] = (rgb[1] * 255.0).clamp(0.0, 255.0) as u8;
            out[2] = (rgb[2] * 255.0).clamp(0.0, 255.0) as u8;
            out[3] = 255;
        });

    // Compute histogram
    let mut r_bins = vec![0u32; 256];
    let mut g_bins = vec![0u32; 256];
    let mut b_bins = vec![0u32; 256];
    let mut l_bins = vec![0u32; 256];
    let mut clipped_high = 0usize;
    let mut clipped_low = 0usize;

    for chunk in rgba.chunks_exact(4) {
        let r = chunk[0] as usize;
        let g = chunk[1] as usize;
        let b = chunk[2] as usize;
        let luma = ((0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32).round() as usize).min(255);

        r_bins[r] += 1;
        g_bins[g] += 1;
        b_bins[b] += 1;
        l_bins[luma] += 1;

        if r == 255 || g == 255 || b == 255 {
            clipped_high += 1;
        }
        if r == 0 && g == 0 && b == 0 {
            clipped_low += 1;
        }
    }

    let mut max_bin = 0;
    for i in 0..256 {
        max_bin = max_bin.max(r_bins[i]).max(g_bins[i]).max(b_bins[i]).max(l_bins[i]);
    }

    let clipped_high_fraction = if num_pixels > 0 { clipped_high as f32 / num_pixels as f32 } else { 0.0 };
    let clipped_low_fraction = if num_pixels > 0 { clipped_low as f32 / num_pixels as f32 } else { 0.0 };

    let histogram = HistogramResult {
        r: r_bins,
        g: g_bins,
        b: b_bins,
        l: l_bins,
        max_bin,
        clipped_low_fraction,
        clipped_high_fraction,
    };

    (rgba, histogram, clipped_high_fraction * 100.0)
}
