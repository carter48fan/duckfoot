//! 3D LUT (Look-Up Table) parser and evaluator for `.cube` files (Adobe / DaVinci Resolve specification).
//!
//! A 3D LUT maps RGB colours through an N x N x N lattice, allowing arbitrary non-linear color grading,
//! film emulation, and creative looks. Values between lattice points are interpolated using trilinear filtering.

use anyhow::{anyhow, bail, Result};

/// A 3D or 1D LUT loaded from an industry-standard `.cube` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Lut3D {
    pub title: Option<String>,
    /// Dimension size N (e.g. 17, 33, 65 for 3D; or up to 4096 for 1D).
    pub size: usize,
    /// Minimum domain bound for [R, G, B], default [0.0, 0.0, 0.0].
    pub domain_min: [f32; 3],
    /// Maximum domain bound for [R, G, B], default [1.0, 1.0, 1.0].
    pub domain_max: [f32; 3],
    /// Lattice entries in standard order (r + g * size + b * size * size).
    pub table: Vec<[f32; 3]>,
    /// Whether this is a 3D LUT (true) or 1D LUT (false).
    pub is_3d: bool,
}

impl Lut3D {
    /// Create an identity 3D LUT of dimension `size` (minimum 2).
    pub fn identity(size: usize) -> Self {
        let size = size.max(2);
        let mut table = Vec::with_capacity(size * size * size);
        let step = 1.0 / (size - 1) as f32;
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    table.push([r as f32 * step, g as f32 * step, b as f32 * step]);
                }
            }
        }
        Self {
            title: Some("Identity".to_string()),
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            table,
            is_3d: true,
        }
    }

    /// Sample the LUT using trilinear interpolation for an input RGB color in [0.0, 1.0].
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        if !self.is_3d {
            return self.sample_1d(rgb);
        }
        self.sample_3d(rgb)
    }

    fn sample_3d(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        if n < 2 || self.table.len() < n * n * n {
            return rgb;
        }

        // Normalize within domain bounds
        let d_range = [
            (self.domain_max[0] - self.domain_min[0]).max(1e-6),
            (self.domain_max[1] - self.domain_min[1]).max(1e-6),
            (self.domain_max[2] - self.domain_min[2]).max(1e-6),
        ];

        let nr = ((rgb[0] - self.domain_min[0]) / d_range[0]).clamp(0.0, 1.0);
        let ng = ((rgb[1] - self.domain_min[1]) / d_range[1]).clamp(0.0, 1.0);
        let nb = ((rgb[2] - self.domain_min[2]) / d_range[2]).clamp(0.0, 1.0);

        let max_idx = (n - 1) as f32;
        let pr = nr * max_idx;
        let pg = ng * max_idx;
        let pb = nb * max_idx;

        let r0 = (pr.floor() as usize).min(n - 1);
        let g0 = (pg.floor() as usize).min(n - 1);
        let b0 = (pb.floor() as usize).min(n - 1);

        let r1 = (r0 + 1).min(n - 1);
        let g1 = (g0 + 1).min(n - 1);
        let b1 = (b0 + 1).min(n - 1);

        let fr = pr - r0 as f32;
        let fg = pg - g0 as f32;
        let fb = pb - b0 as f32;

        let idx = |r: usize, g: usize, b: usize| -> usize { r + g * n + b * n * n };

        let c000 = self.table[idx(r0, g0, b0)];
        let c100 = self.table[idx(r1, g0, b0)];
        let c010 = self.table[idx(r0, g1, b0)];
        let c110 = self.table[idx(r1, g1, b0)];
        let c001 = self.table[idx(r0, g0, b1)];
        let c101 = self.table[idx(r1, g0, b1)];
        let c011 = self.table[idx(r0, g1, b1)];
        let c111 = self.table[idx(r1, g1, b1)];

        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        };

        let c00 = lerp(c000, c100, fr);
        let c10 = lerp(c010, c110, fr);
        let c01 = lerp(c001, c101, fr);
        let c11 = lerp(c011, c111, fr);

        let c0 = lerp(c00, c10, fg);
        let c1 = lerp(c01, c11, fg);

        lerp(c0, c1, fb)
    }

    fn sample_1d(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        if n < 2 || self.table.len() < n {
            return rgb;
        }
        let max_idx = (n - 1) as f32;

        let sample_chan = |v: f32, chan: usize| -> f32 {
            let min_d = self.domain_min[chan];
            let max_d = self.domain_max[chan];
            let nv = ((v - min_d) / (max_d - min_d).max(1e-6)).clamp(0.0, 1.0);
            let p = nv * max_idx;
            let i0 = (p.floor() as usize).min(n - 1);
            let i1 = (i0 + 1).min(n - 1);
            let f = p - i0 as f32;
            self.table[i0][chan] + (self.table[i1][chan] - self.table[i0][chan]) * f
        };

        [
            sample_chan(rgb[0], 0),
            sample_chan(rgb[1], 1),
            sample_chan(rgb[2], 2),
        ]
    }

    /// Pack the table into an RGBA f32 buffer suitable for GPU texture upload (`wgpu::TextureFormat::Rgba32Float`).
    pub fn to_rgba32f(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.table.len() * 4);
        for rgb in &self.table {
            out.push(rgb[0]);
            out.push(rgb[1]);
            out.push(rgb[2]);
            out.push(1.0);
        }
        out
    }
}

/// Parse a `.cube` file from text.
pub fn parse_cube(input: &str) -> Result<Lut3D> {
    let mut title = None;
    let mut size = None;
    let mut is_3d = true;
    let mut domain_min = [0.0f32, 0.0, 0.0];
    let mut domain_max = [1.0f32, 1.0, 1.0];
    let mut table = Vec::new();

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let first = parts.next().unwrap_or("");

        if first.eq_ignore_ascii_case("TITLE") {
            let rest = line[5..].trim().trim_matches('"');
            title = Some(rest.to_string());
        } else if first.eq_ignore_ascii_case("LUT_3D_SIZE") {
            let s: usize = parts
                .next()
                .ok_or_else(|| anyhow!("Missing LUT_3D_SIZE value"))?
                .parse()
                .map_err(|e| anyhow!("Invalid LUT_3D_SIZE: {e}"))?;
            size = Some(s);
            is_3d = true;
        } else if first.eq_ignore_ascii_case("LUT_1D_SIZE") {
            let s: usize = parts
                .next()
                .ok_or_else(|| anyhow!("Missing LUT_1D_SIZE value"))?
                .parse()
                .map_err(|e| anyhow!("Invalid LUT_1D_SIZE: {e}"))?;
            size = Some(s);
            is_3d = false;
        } else if first.eq_ignore_ascii_case("DOMAIN_MIN") {
            let r: f32 = parts
                .next()
                .ok_or_else(|| anyhow!("Missing DOMAIN_MIN r"))?
                .parse()?;
            let g: f32 = parts
                .next()
                .ok_or_else(|| anyhow!("Missing DOMAIN_MIN g"))?
                .parse()?;
            let b: f32 = parts
                .next()
                .ok_or_else(|| anyhow!("Missing DOMAIN_MIN b"))?
                .parse()?;
            domain_min = [r, g, b];
        } else if first.eq_ignore_ascii_case("DOMAIN_MAX") {
            let r: f32 = parts
                .next()
                .ok_or_else(|| anyhow!("Missing DOMAIN_MAX r"))?
                .parse()?;
            let g: f32 = parts
                .next()
                .ok_or_else(|| anyhow!("Missing DOMAIN_MAX g"))?
                .parse()?;
            let b: f32 = parts
                .next()
                .ok_or_else(|| anyhow!("Missing DOMAIN_MAX b"))?
                .parse()?;
            domain_max = [r, g, b];
        } else if let Ok(r) = first.parse::<f32>() {
            let g: f32 = parts
                .next()
                .ok_or_else(|| anyhow!("Missing green channel in data row: {line}"))?
                .parse()?;
            let b: f32 = parts
                .next()
                .ok_or_else(|| anyhow!("Missing blue channel in data row: {line}"))?
                .parse()?;
            table.push([r, g, b]);
        }
    }

    let size = size.ok_or_else(|| anyhow!("Missing LUT_3D_SIZE or LUT_1D_SIZE declaration"))?;
    let expected = if is_3d { size * size * size } else { size };
    if table.len() != expected {
        bail!(
            "LUT entry count mismatch: expected {expected} for size {size}, found {}",
            table.len()
        );
    }

    Ok(Lut3D {
        title,
        size,
        domain_min,
        domain_max,
        table,
        is_3d,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_identity_2x2x2_cube() {
        let cube_data = r#"
# Simple 2x2x2 identity cube
TITLE "Simple Identity"
LUT_3D_SIZE 2
DOMAIN_MIN 0.0 0.0 0.0
DOMAIN_MAX 1.0 1.0 1.0

0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
"#;
        let lut = parse_cube(cube_data).expect("valid cube");
        assert_eq!(lut.size, 2);
        assert!(lut.is_3d);
        assert_eq!(lut.title.as_deref(), Some("Simple Identity"));
        assert_eq!(lut.table.len(), 8);

        // Check corners
        assert_eq!(lut.sample([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        assert_eq!(lut.sample([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        assert_eq!(lut.sample([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]);
        assert_eq!(lut.sample([1.0, 1.0, 1.0]), [1.0, 1.0, 1.0]);

        // Check mid-point interpolation
        let mid = lut.sample([0.5, 0.5, 0.5]);
        assert!((mid[0] - 0.5).abs() < 1e-5);
        assert!((mid[1] - 0.5).abs() < 1e-5);
        assert!((mid[2] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn parse_color_grading_cube_shifts_colors() {
        let cube_data = r#"
TITLE "Warm Tint"
LUT_3D_SIZE 2

0.1 0.0 0.0
1.0 0.0 0.0
0.1 0.8 0.0
1.0 0.8 0.0
0.1 0.0 0.5
1.0 0.0 0.5
0.1 0.8 0.5
1.0 0.8 0.5
"#;
        let lut = parse_cube(cube_data).expect("valid cube");
        let black = lut.sample([0.0, 0.0, 0.0]);
        // In this LUT, black (0,0,0) is lifted to [0.1, 0.0, 0.0]
        assert!((black[0] - 0.1).abs() < 1e-5);
        assert!((black[1] - 0.0).abs() < 1e-5);
        assert!((black[2] - 0.0).abs() < 1e-5);
    }

    #[test]
    fn rejects_truncated_cube() {
        let cube_data = r#"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
"#;
        assert!(parse_cube(cube_data).is_err());
    }
}
