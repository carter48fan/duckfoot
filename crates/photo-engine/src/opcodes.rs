//! The DNG opcodes that correct a lens, as phone DNGs (Pixel, iPhone ProRAW) and many
//! converted files carry them instead of expecting a lens profile:
//!
//! - `GainMap` (OpcodeList2): a smooth per-photosite gain, one map per Bayer position,
//!   for lens shading — the corner fall-off and its colour cast.
//! - `FixVignetteRadial` (OpcodeList3): a radial gain for the same fall-off.
//! - `WarpRectilinear` (OpcodeList3): radial distortion and, one polynomial per colour
//!   plane, lateral chromatic aberration.
//!
//! The gains are applied to the mosaic at decode, where they are just multiplications;
//! the warp is folded into the adjust shader's inverse mapping (`lens::FileWarp`). Every
//! other opcode is skipped, as the DNG spec allows for those flagged optional and as is
//! harmless for the rest this renderer has no use for.
//!
//! Opcode lists are big-endian whatever the file's byte order (DNG 1.4, chapter 7).

/// An opcode this module understands.
#[derive(Debug, Clone, PartialEq)]
pub enum Opcode {
    GainMap(GainMap),
    Warp(Warp),
    Vignette(Vignette),
}

#[derive(Debug, Clone, PartialEq)]
pub struct GainMap {
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
    pub row_pitch: u32,
    pub col_pitch: u32,
    pub points_v: u32,
    pub points_h: u32,
    pub spacing_v: f64,
    pub spacing_h: f64,
    pub origin_v: f64,
    pub origin_h: f64,
    /// `points_v × points_h` gains, row-major; only the first map plane is used, since a
    /// mosaic has one plane.
    pub gains: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Warp {
    /// kr0..kr3 and kt0, kt1 per plane; one entry means every plane.
    pub planes: Vec<[f64; 6]>,
    /// The optical centre, 0..1 of the image's width and height.
    pub centre: (f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Vignette {
    /// g = 1 + k0·r² + k1·r⁴ + k2·r⁶ + k3·r⁸ + k4·r¹⁰
    pub k: [f64; 5],
    pub centre: (f64, f64),
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Option<u32> {
        let v = u32::from_be_bytes(self.b.get(self.at..self.at + 4)?.try_into().ok()?);
        self.at += 4;
        Some(v)
    }
    fn f64(&mut self) -> Option<f64> {
        let v = f64::from_be_bytes(self.b.get(self.at..self.at + 8)?.try_into().ok()?);
        self.at += 8;
        v.is_finite().then_some(v)
    }
    fn f32(&mut self) -> Option<f32> {
        let v = f32::from_be_bytes(self.b.get(self.at..self.at + 4)?.try_into().ok()?);
        self.at += 4;
        v.is_finite().then_some(v)
    }
}

/// The opcodes in an OpcodeList tag's bytes that this module understands, in order. A
/// malformed opcode ends the list there: what came before it still applies.
pub fn parse_list(bytes: &[u8]) -> Vec<Opcode> {
    let mut out = Vec::new();
    let mut r = Reader { b: bytes, at: 0 };
    let Some(count) = r.u32() else { return out };
    for _ in 0..count.min(256) {
        let (Some(id), Some(_version), Some(_flags), Some(size)) =
            (r.u32(), r.u32(), r.u32(), r.u32())
        else {
            break;
        };
        let start = r.at;
        let Some(end) = start
            .checked_add(size as usize)
            .filter(|e| *e <= bytes.len())
        else {
            break;
        };
        let mut p = Reader {
            b: &bytes[..end],
            at: start,
        };
        let parsed = match id {
            1 => warp(&mut p).map(Opcode::Warp),
            3 => vignette(&mut p).map(Opcode::Vignette),
            9 => gain_map(&mut p).map(Opcode::GainMap),
            _ => None,
        };
        out.extend(parsed);
        r.at = end;
    }
    out
}

fn warp(p: &mut Reader) -> Option<Warp> {
    let n = p.u32()?;
    if n == 0 || n > 4 {
        return None;
    }
    let mut planes = Vec::new();
    for _ in 0..n {
        let mut k = [0.0; 6];
        for v in &mut k {
            *v = p.f64()?;
        }
        planes.push(k);
    }
    Some(Warp {
        planes,
        centre: (p.f64()?, p.f64()?),
    })
}

fn vignette(p: &mut Reader) -> Option<Vignette> {
    let mut k = [0.0; 5];
    for v in &mut k {
        *v = p.f64()?;
    }
    Some(Vignette {
        k,
        centre: (p.f64()?, p.f64()?),
    })
}

fn gain_map(p: &mut Reader) -> Option<GainMap> {
    let (top, left, bottom, right) = (p.u32()?, p.u32()?, p.u32()?, p.u32()?);
    let (_plane, _planes) = (p.u32()?, p.u32()?);
    let (row_pitch, col_pitch) = (p.u32()?.max(1), p.u32()?.max(1));
    let (points_v, points_h) = (p.u32()?, p.u32()?);
    let (spacing_v, spacing_h) = (p.f64()?, p.f64()?);
    let (origin_v, origin_h) = (p.f64()?, p.f64()?);
    let map_planes = p.u32()?.max(1);
    if points_v == 0 || points_h == 0 || points_v * points_h > 1 << 20 {
        return None;
    }
    // Stored point by point with every map plane interleaved; keep plane 0.
    let mut gains = Vec::with_capacity((points_v * points_h) as usize);
    for _ in 0..points_v * points_h {
        gains.push(p.f32()?);
        for _ in 1..map_planes {
            p.f32()?;
        }
    }
    Some(GainMap {
        top,
        left,
        bottom,
        right,
        row_pitch,
        col_pitch,
        points_v,
        points_h,
        spacing_v,
        spacing_h,
        origin_v,
        origin_h,
        gains,
    })
}

impl GainMap {
    /// The gain at a position given as 0..1 of the image, bilinear between map points and
    /// held at the map's edge beyond it.
    pub fn gain(&self, v: f64, h: f64) -> f32 {
        let at = |pos: f64, origin: f64, spacing: f64, points: u32| {
            let f = if spacing > 0.0 {
                ((pos - origin) / spacing).clamp(0.0, (points - 1) as f64)
            } else {
                0.0
            };
            let i = (f.floor() as u32).min(points.saturating_sub(2));
            (i, (f - i as f64) as f32)
        };
        let (iv, fv) = at(v, self.origin_v, self.spacing_v, self.points_v);
        let (ih, fh) = at(h, self.origin_h, self.spacing_h, self.points_h);
        let g = |r: u32, c: u32| {
            let (r, c) = (r.min(self.points_v - 1), c.min(self.points_h - 1));
            self.gains[(r * self.points_h + c) as usize]
        };
        let top = g(iv, ih) + (g(iv, ih + 1) - g(iv, ih)) * fh;
        let bottom = g(iv + 1, ih) + (g(iv + 1, ih + 1) - g(iv + 1, ih)) * fh;
        top + (bottom - top) * fv
    }
}

impl Vignette {
    /// The gain at image position (x, y) of a `w × h` image.
    pub fn gain(&self, x: f64, y: f64, w: f64, h: f64) -> f32 {
        let (cx, cy) = (self.centre.0 * w, self.centre.1 * h);
        let m2 = max_corner_distance2(cx, cy, w, h);
        let r2 = ((x - cx).powi(2) + (y - cy).powi(2)) / m2;
        let k = &self.k;
        (1.0 + r2 * (k[0] + r2 * (k[1] + r2 * (k[2] + r2 * (k[3] + r2 * k[4]))))) as f32
    }
}

/// The squared distance from the optical centre to the image's farthest corner: what
/// both radial opcodes normalise their radius by.
pub fn max_corner_distance2(cx: f64, cy: f64, w: f64, h: f64) -> f64 {
    [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]
        .iter()
        .map(|(x, y)| (x - cx).powi(2) + (y - cy).powi(2))
        .fold(1.0, f64::max)
}

/// Where the stage 2/3 image (the active area) sits in the mosaic that was kept, and how
/// big it is: opcode coordinates are the active area's.
#[derive(Debug, Clone, Copy)]
pub struct Area {
    /// The kept mosaic's origin in active-area coordinates.
    pub offset: (i64, i64),
    pub size: (u32, u32),
}

/// `GainMap` and `FixVignetteRadial` opcodes applied to a mosaic of `width` columns:
/// each photosite's light above its colour's black scaled by the gain. Values past the
/// u16 range clip there, which is already far past white.
pub fn apply_gains(
    ops: &[Opcode],
    data: &mut [u16],
    width: usize,
    area: Area,
    black_at: impl Fn(usize, usize) -> f32 + Sync,
) {
    use rayon::prelude::*;
    if width == 0 || !ops.iter().any(|o| !matches!(o, Opcode::Warp(_))) {
        return;
    }
    let (aw, ah) = (area.size.0 as f64, area.size.1 as f64);
    data.par_chunks_mut(width)
        .enumerate()
        .for_each(|(row, line)| {
            let ay = row as i64 + area.offset.1;
            for (col, v) in line.iter_mut().enumerate() {
                let ax = col as i64 + area.offset.0;
                let mut gain = 1.0f32;
                for op in ops {
                    match op {
                        Opcode::GainMap(m) => {
                            let inside = ay >= m.top as i64
                                && ay < m.bottom as i64
                                && ax >= m.left as i64
                                && ax < m.right as i64
                                && (ay - m.top as i64) % m.row_pitch as i64 == 0
                                && (ax - m.left as i64) % m.col_pitch as i64 == 0;
                            if inside {
                                gain *= m.gain(ay as f64 / ah, ax as f64 / aw);
                            }
                        }
                        Opcode::Vignette(vg) => gain *= vg.gain(ax as f64, ay as f64, aw, ah),
                        Opcode::Warp(_) => {}
                    }
                }
                if gain != 1.0 && gain.is_finite() {
                    let b = black_at(row, col);
                    let lifted = b + (*v as f32 - b).max(0.0) * gain;
                    *v = lifted.round().clamp(0.0, 65535.0) as u16;
                }
            }
        });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An opcode list's bytes, as a DNG writer lays them out.
    pub fn list(ops: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut b = (ops.len() as u32).to_be_bytes().to_vec();
        for (id, params) in ops {
            b.extend_from_slice(&id.to_be_bytes());
            b.extend_from_slice(&0x0103_0000u32.to_be_bytes()); // DNG 1.3 opcode version
            b.extend_from_slice(&0u32.to_be_bytes());
            b.extend_from_slice(&(params.len() as u32).to_be_bytes());
            b.extend_from_slice(params);
        }
        b
    }

    pub fn f64s(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }

    pub fn gain_map_params(
        (top, left, pitch): (u32, u32, u32),
        (points_v, points_h): (u32, u32),
        gains: &[f32],
    ) -> Vec<u8> {
        let mut b = Vec::new();
        for v in [
            top,
            left,
            1 << 20,
            1 << 20,
            0,
            1,
            pitch,
            pitch,
            points_v,
            points_h,
        ] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        let (sv, sh) = (
            1.0 / (points_v.max(2) - 1) as f64,
            1.0 / (points_h.max(2) - 1) as f64,
        );
        b.extend(f64s(&[sv, sh, 0.0, 0.0]));
        b.extend_from_slice(&1u32.to_be_bytes());
        for g in gains {
            b.extend_from_slice(&g.to_be_bytes());
        }
        b
    }

    #[test]
    fn a_list_parses_what_it_knows_and_skips_the_rest() {
        let warp = [
            &1u32.to_be_bytes()[..],
            &f64s(&[1.0, -0.1, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5]),
        ]
        .concat();
        let bytes = list(&[
            (4, vec![0; 12]), // an opcode it has no use for
            (1, warp),
            (3, f64s(&[0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5])),
        ]);
        let ops = parse_list(&bytes);
        assert_eq!(ops.len(), 2);
        assert!(matches!(&ops[0], Opcode::Warp(w) if w.planes[0][1] == -0.1));
        assert!(matches!(&ops[1], Opcode::Vignette(v) if v.k[0] == 0.5));
        // Truncated: what came before still counts.
        assert_eq!(parse_list(&bytes[..bytes.len() - 3]).len(), 1);
        assert!(parse_list(&[]).is_empty());
    }

    #[test]
    fn a_gain_map_interpolates_between_its_points() {
        let bytes = list(&[(9, gain_map_params((0, 0, 1), (2, 2), &[1.0, 2.0, 3.0, 4.0]))]);
        let [Opcode::GainMap(m)] = &parse_list(&bytes)[..] else {
            panic!("not parsed");
        };
        assert_eq!(m.gain(0.0, 0.0), 1.0);
        assert_eq!(m.gain(1.0, 1.0), 4.0);
        assert!((m.gain(0.5, 0.5) - 2.5).abs() < 1e-6);
        // Held at the edge beyond the map.
        assert_eq!(m.gain(-1.0, 2.0), 2.0);
    }

    #[test]
    fn gains_scale_the_light_above_black_at_their_own_photosites() {
        // Only the even columns of even rows (one Bayer position), doubled everywhere.
        let bytes = list(&[(9, gain_map_params((0, 0, 2), (2, 2), &[2.0; 4]))]);
        let ops = parse_list(&bytes);
        let mut data = vec![600u16; 16];
        let area = Area {
            offset: (0, 0),
            size: (4, 4),
        };
        apply_gains(&ops, &mut data, 4, area, |_, _| 100.0);
        assert_eq!(&data[..4], &[1100, 600, 1100, 600]);
        assert_eq!(&data[4..8], &[600; 4]);
        // Kept from one photosite in: the pitch is counted from the active area, so the
        // map lands on the odd columns of the kept mosaic.
        let mut data = vec![600u16; 16];
        let area = Area {
            offset: (1, 0),
            size: (5, 4),
        };
        apply_gains(&ops, &mut data, 4, area, |_, _| 100.0);
        assert_eq!(&data[..4], &[600, 1100, 600, 1100]);
    }

    #[test]
    fn the_radial_vignette_is_one_at_the_centre_and_grows_to_the_corner() {
        let v = Vignette {
            k: [1.0, 0.0, 0.0, 0.0, 0.0],
            centre: (0.5, 0.5),
        };
        assert_eq!(v.gain(50.0, 50.0, 100.0, 100.0), 1.0);
        assert!((v.gain(0.0, 0.0, 100.0, 100.0) - 2.0).abs() < 1e-6);
    }
}
