//! Spot heal, clone and remove (W-02, DEC-5): up to 32 circular spots, each covering a
//! destination with what is at its source. Clone copies the source as it is; heal also
//! shifts it to the brightness and colour around the destination, so a dust spot on a sky
//! is covered by sky of the right shade. Remove has no source: the engine fills it from the
//! ground around it (PatchMatch, `lc-inpaint`), once per placement, into its fill atlas. Both happen as the adjust pass samples the sensor,
//! before every adjustment, so the spot is gone from everything downstream.
//!
//! Positions are sensor coordinates (0..1), radii fractions of the long side, as masks are.

use serde::{Deserialize, Serialize};

pub const MAX_SPOTS: usize = 32;

/// Remove spots that can be filled at once, each in a slot of the engine's fill atlas.
pub const FILL_SLOTS: usize = 16;
/// A slot's side, in texels: the most a Remove spot's window is filled at. A larger
/// window is filled at a coarser level of the sensor until it fits.
pub const FILL_TILE: u32 = 256;
/// The atlas's side: 4 × 4 slots.
pub const FILL_ATLAS: u32 = 1024;
/// A Remove spot's window, in radii across: the spot and two radii of ground on every
/// side, which is where PatchMatch looks for what to fill it with.
pub const FILL_CONTEXT: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Heal,
    Clone,
    /// Content-aware: filled from the ground around it (PatchMatch), with no source.
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    pub dst: [f32; 2],
    pub src: [f32; 2],
    pub radius: f32,
    /// The outer part of the radius that blends into the photo, 0..1.
    pub feather: f32,
    #[serde(default)]
    pub mode: Mode,
}

impl Spot {
    pub fn new(dst: [f32; 2], src: [f32; 2], radius: f32) -> Self {
        Self {
            dst,
            src,
            radius,
            feather: 0.4,
            mode: Mode::Heal,
        }
    }

    fn sanitised(self) -> Self {
        let f = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        Self {
            dst: self.dst.map(|v| f(v, -1.0, 2.0, 0.5)),
            src: self.src.map(|v| f(v, -1.0, 2.0, 0.5)),
            radius: f(self.radius, 0.001, 0.25, 0.01),
            feather: f(self.feather, 0.01, 1.0, 0.4),
            mode: self.mode,
        }
    }
}

/// The Spot heal module's parameters.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Spots {
    #[serde(default)]
    pub spots: Vec<Spot>,
}

impl Spots {
    pub fn sanitised(&self) -> Self {
        Self {
            spots: self
                .spots
                .iter()
                .take(MAX_SPOTS)
                .map(|s| s.sanitised())
                .collect(),
        }
    }
}

/// The best place to take a patch for a spot at `dst` of `radius` (sensor coordinates,
/// long-side fraction), searched in an overview of the sensor: a `w × h` grid of linear
/// camera RGB. The patch that best matches the ring just outside the spot is the one that
/// will blend in; candidates are 2.2 to 5 radii away, so the spot never samples itself,
/// and inside the frame.
pub fn find_source(
    overview: &[[f32; 4]],
    (w, h): (u32, u32),
    dst: [f32; 2],
    radius: f32,
) -> [f32; 2] {
    let (fw, fh) = (w as f32, h as f32);
    let long = fw.max(fh);
    let r = (radius * long).max(1.0);
    let at = |x: f32, y: f32| -> Option<[f32; 3]> {
        if x < 0.0 || y < 0.0 || x >= fw || y >= fh {
            return None;
        }
        let t = overview[(y as u32 * w + x as u32) as usize];
        Some([t[0] + t[3], t[1] + t[3], t[2] + t[3]])
    };
    let ring: Vec<(f32, f32)> = (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            (a.cos() * r * 1.3, a.sin() * r * 1.3)
        })
        .collect();
    let (dx, dy) = (dst[0] * fw, dst[1] * fh);
    let target: Vec<Option<[f32; 3]>> = ring.iter().map(|(ox, oy)| at(dx + ox, dy + oy)).collect();
    let mut best = (f32::MAX, [dst[0] + 3.0 * radius * long / fw, dst[1]]);
    for step in [2.2f32, 3.0, 4.0, 5.0] {
        for k in 0..24 {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            let (cx, cy) = (dx + a.cos() * r * step, dy + a.sin() * r * step);
            if cx - r < 0.0 || cy - r < 0.0 || cx + r >= fw || cy + r >= fh {
                continue;
            }
            let mut cost = 0.0;
            let mut n = 0.0;
            for ((ox, oy), t) in ring.iter().zip(&target) {
                if let (Some(t), Some(c)) = (t, at(cx + ox, cy + oy)) {
                    cost += (0..3).map(|i| (t[i] - c[i]).powi(2)).sum::<f32>();
                    n += 1.0;
                }
            }
            // The inside should be plain too: a patch with its own detail carries it over.
            if let Some(mid) = at(cx, cy) {
                let edge = ring
                    .iter()
                    .filter_map(|(ox, oy)| at(cx + ox * 0.5, cy + oy * 0.5));
                for e in edge {
                    cost += (0..3).map(|i| (mid[i] - e[i]).powi(2)).sum::<f32>() * 0.25;
                }
            }
            // Nearer is better, all else equal.
            let cost = if n > 0.0 { cost / n } else { f32::MAX } * (1.0 + 0.05 * step);
            if cost < best.0 {
                best = (cost, [cx / fw, cy / fh]);
            }
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_is_found_on_matching_ground_and_away_from_the_spot() {
        // Left half dark, right half bright; a spot on the bright side near the edge must
        // take bright ground, not the dark half, and never itself.
        let (w, h) = (128u32, 64u32);
        let grid: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let x = i % w;
                let v = if x < 64 { 0.05 } else { 0.6 };
                [v, v, v, 0.0]
            })
            .collect();
        let dst = [80.0 / 128.0, 0.5];
        let src = find_source(&grid, (w, h), dst, 4.0 / 128.0);
        assert!(src[0] * 128.0 > 70.0, "took dark ground: {src:?}");
        let d = ((src[0] - dst[0]) * 128.0).hypot((src[1] - dst[1]) * 64.0);
        assert!(d >= 8.0, "too close to the spot: {d}");
    }

    #[test]
    fn nonsense_is_brought_back() {
        let s = Spots {
            spots: vec![Spot::new([f32::NAN, 0.2], [0.3, 9.0], -1.0); 40],
        }
        .sanitised();
        assert_eq!(s.spots.len(), MAX_SPOTS);
        assert_eq!(s.spots[0].dst, [0.5, 0.2]);
        assert_eq!(s.spots[0].src, [0.3, 2.0]);
        assert_eq!(s.spots[0].radius, 0.001);
    }
}
