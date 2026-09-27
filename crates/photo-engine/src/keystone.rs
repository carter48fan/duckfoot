//! Auto upright (W-07): find the straight edges a photo has, and the straighten and
//! keystone that stand its verticals up and level its horizontals. CPU only, on an
//! overview of the sensor: a few hundred pixels across is plenty to measure an angle.

use crate::params::Geometry;

/// A straight edge found in the photo, in sensor pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub a: (f32, f32),
    pub b: (f32, f32),
}

impl Segment {
    fn length(&self) -> f32 {
        ((self.b.0 - self.a.0).powi(2) + (self.b.1 - self.a.1).powi(2)).sqrt()
    }
}

/// What to correct, as Lightroom's Upright names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upright {
    /// Straighten only, from the horizon and the verticals together.
    Level,
    /// Straighten, and stand converging verticals up.
    Vertical,
    /// Both keystones and straighten, from verticals and horizontals.
    Full,
}

/// How far from upright (or level) a line may lean and still be taken as meant to be.
const MAX_LEAN_DEG: f32 = 25.0;

/// The straight edges of a luminance image `w × h`, near-vertical ones first, then
/// near-horizontal ones. Each is found by a Hough transform whose votes are steered by
/// the edge's own gradient, then measured to where its edge pixels actually run.
pub fn find_lines(lum: &[f32], w: usize, h: usize) -> (Vec<Segment>, Vec<Segment>) {
    if w < 16 || h < 16 || lum.len() < w * h {
        return (Vec::new(), Vec::new());
    }
    // Sobel, on a perceptual scale so shadows' edges count as much as highlights', after
    // a light blur so a stair-stepped edge still gives its true direction.
    let raw: Vec<f32> = lum.iter().map(|v| v.max(0.0).sqrt()).collect();
    let mut l = raw.clone();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut sum = 0.0;
            for (dy, dx) in [
                (0, 0),
                (0, 1),
                (1, 0),
                (1, 1),
                (0, 2),
                (2, 0),
                (1, 2),
                (2, 1),
                (2, 2),
            ] {
                let weight = [1.0, 2.0, 1.0][dx] * [1.0, 2.0, 1.0][dy];
                sum += weight * raw[(y + dy - 1) * w + x + dx - 1];
            }
            l[y * w + x] = sum / 16.0;
        }
    }
    let at = |x: usize, y: usize| l[y * w + x];
    let mut grad = vec![(0.0f32, 0.0f32); w * h];
    let mut mags = Vec::with_capacity(w * h);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let gx = (at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1))
                - (at(x - 1, y - 1) + 2.0 * at(x - 1, y) + at(x - 1, y + 1));
            let gy = (at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1))
                - (at(x - 1, y - 1) + 2.0 * at(x, y - 1) + at(x + 1, y - 1));
            grad[y * w + x] = (gx, gy);
            mags.push((gx * gx + gy * gy).sqrt());
        }
    }
    // Only the strongest edges vote: the top tenth, and never mere noise.
    mags.sort_by(|a, b| a.total_cmp(b));
    let threshold = mags[mags.len() * 9 / 10].max(0.02);
    let vertical = lines_near(&grad, w, h, threshold, false);
    let horizontal = lines_near(&grad, w, h, threshold, true);
    (vertical, horizontal)
}

/// Lines within `MAX_LEAN_DEG` of vertical, or of horizontal when `across`.
fn lines_near(
    grad: &[(f32, f32)],
    w: usize,
    h: usize,
    threshold: f32,
    across: bool,
) -> Vec<Segment> {
    // In the frame where the lines sought are near vertical: x along, y down it.
    let (fw, fh) = if across { (h, w) } else { (w, h) };
    let get = |u: usize, v: usize| {
        if across {
            let (gx, gy) = grad[u * w + v];
            (gy, gx)
        } else {
            grad[v * w + u]
        }
    };
    const STEP: f32 = 0.5;
    let bins = (2.0 * MAX_LEAN_DEG / STEP) as usize + 1;
    let diag = ((fw * fw + fh * fh) as f32).sqrt();
    let rhos = (2.0 * diag) as usize + 1;
    // One accumulator per edge polarity (dark to light, or light to dark, across the line):
    // a real edge keeps one along its length, and a line slanting across a thin bar from
    // one of its sides to the other does not.
    let mut acc = vec![0.0f32; 2 * bins * rhos];
    let theta_of = |i: usize| (i as f32 * STEP - MAX_LEAN_DEG).to_radians();
    // A line leaning θ from vertical: x cos θ - y sin θ is constant along it, and its
    // gradient points along (cos θ, -sin θ).
    let lean_of = |gx: f32, gy: f32| {
        let (gx, gy) = if gx < 0.0 { (-gx, -gy) } else { (gx, gy) };
        (-gy).atan2(gx).to_degrees()
    };
    let mut voters = Vec::new();
    for v in 1..fh - 1 {
        for u in 1..fw - 1 {
            let (gx, gy) = get(u, v);
            let m = (gx * gx + gy * gy).sqrt();
            if m < threshold || gx.abs() < gy.abs() {
                continue;
            }
            let lean = lean_of(gx, gy);
            if lean.abs() > MAX_LEAN_DEG + 4.0 {
                continue;
            }
            let polarity = usize::from(gx < 0.0);
            voters.push((u, v, lean, m, polarity));
            let centre = ((lean + MAX_LEAN_DEG) / STEP).round() as i64;
            for i in (centre - 5).max(0)..=(centre + 5).min(bins as i64 - 1) {
                let t = theta_of(i as usize);
                // Spread over neighbouring distances, as an edge a few pixels wide is.
                let rho = (u as f32 * t.cos() - v as f32 * t.sin() + diag).round() as usize;
                let row = (polarity * bins + i as usize) * rhos;
                acc[row + rho] += m;
                if rho > 0 {
                    acc[row + rho - 1] += 0.5 * m;
                }
                if rho + 1 < rhos {
                    acc[row + rho + 1] += 0.5 * m;
                }
            }
        }
    }
    // Peaks, strongest first, none within 3° and 6 px of a stronger one.
    let mut order: Vec<usize> = (0..acc.len()).filter(|&k| acc[k] > 0.0).collect();
    order.sort_by(|&a, &b| acc[b].total_cmp(&acc[a]));
    let mut peaks: Vec<(usize, usize, usize)> = Vec::new();
    let need = threshold * fh as f32 * 0.12;
    for k in order {
        if acc[k] < need || peaks.len() >= 16 {
            break;
        }
        let (pol, i, r) = (k / (bins * rhos), (k / rhos) % bins, k % rhos);
        let near = peaks.iter().any(|&(pp, pi, pr)| {
            pp == pol && (pi as i64 - i as i64).abs() <= 6 && (pr as i64 - r as i64).abs() <= 6
        });
        if !near {
            peaks.push((pol, i, r));
        }
    }
    // Each peak refitted to its own edge pixels (least squares, u against v), and
    // measured to where they run from and to.
    let mut out: Vec<(Segment, f32)> = Vec::new();
    for (pol, i, r) in peaks {
        let t = theta_of(i);
        let rho = r as f32 - diag;
        let on: Vec<(f32, f32, f32)> = voters
            .iter()
            .filter(|&&(u, v, lean, _, p)| {
                p == pol
                    && (lean - t.to_degrees()).abs() <= 2.5
                    && (u as f32 * t.cos() - v as f32 * t.sin() - rho).abs() <= 1.5
            })
            .map(|&(u, v, _, m, _)| (u as f32, v as f32, m))
            .collect();
        if on.len() < 8 {
            continue;
        }
        let sw: f32 = on.iter().map(|p| p.2).sum();
        let (mu, mv) = (
            on.iter().map(|p| p.0 * p.2).sum::<f32>() / sw,
            on.iter().map(|p| p.1 * p.2).sum::<f32>() / sw,
        );
        let (mut suv, mut svv) = (0.0, 0.0);
        for &(u, v, m) in &on {
            suv += m * (u - mu) * (v - mv);
            svv += m * (v - mv) * (v - mv);
        }
        if svv <= 0.0 {
            continue;
        }
        let slope = suv / svv;
        let (lo, hi) = on
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.1), hi.max(p.1))
            });
        let length = (hi - lo) * (1.0 + slope * slope).sqrt();
        // Long, and mostly edge along its length: not a scatter of short bits in a row.
        let rows = on
            .iter()
            .map(|p| p.1 as i32)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if length < fh as f32 * 0.15 || (rows as f32) < (hi - lo) * 0.6 {
            continue;
        }
        let point = |v: f32| {
            let u = mu + slope * (v - mv);
            if across {
                (v, u)
            } else {
                (u, v)
            }
        };
        let seg = Segment {
            a: point(lo),
            b: point(hi),
        };
        // The same edge found twice (a band's two sides, a neighbouring bin): keep one.
        let angle = slope.atan().to_degrees();
        let mid = mu + slope * ((lo + hi) * 0.5 - mv);
        let twin = out.iter().position(|(o, oa)| {
            let (om, _) = if across {
                (o.a.1 + o.b.1, 0)
            } else {
                (o.a.0 + o.b.0, 0)
            };
            (oa - angle).abs() < 1.5 && (om * 0.5 - mid).abs() < 5.0
        });
        match twin {
            Some(k) if out[k].0.length() >= seg.length() => {}
            Some(k) => out[k] = (seg, angle),
            None => out.push((seg, angle)),
        }
    }
    out.into_iter().map(|(s, _)| s).collect()
}

/// How far a segment leans once `g` has placed it, in degrees: from vertical, or from
/// horizontal when `across`.
fn lean(g: &Geometry, sensor: (u32, u32), s: &Segment, across: bool) -> f32 {
    let a = crate::engine::output_point(g, sensor, s.a);
    let b = crate::engine::output_point(g, sensor, s.b);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let deg = if across {
        dy.atan2(dx).to_degrees()
    } else {
        dx.atan2(dy).to_degrees()
    };
    // A line has no direction: fold to within 90° of the axis.
    let mut d = deg % 180.0;
    if d > 90.0 {
        d -= 180.0;
    } else if d < -90.0 {
        d += 180.0;
    }
    d
}

/// The straighten and keystones that make `vertical` upright and `horizontal` level,
/// as far as `mode` corrects, starting from `g` (its turns, flips and crop are kept).
/// `None` when the photo has too few lines to go by.
pub fn solve(
    g: &Geometry,
    sensor: (u32, u32),
    vertical: &[Segment],
    horizontal: &[Segment],
    mode: Upright,
) -> Option<Geometry> {
    let (uses_v, uses_h) = match mode {
        Upright::Level | Upright::Full => (true, true),
        Upright::Vertical => (true, false),
    };
    let count = if uses_v { vertical.len() } else { 0 } + if uses_h { horizontal.len() } else { 0 };
    let enough = match mode {
        Upright::Level => count >= 1,
        Upright::Vertical => vertical.len() >= 2,
        Upright::Full => vertical.len() >= 2 && horizontal.len() >= 2,
    };
    if !enough {
        return None;
    }
    let with = |p: [f32; 3]| {
        let mut out = *g;
        out.straighten_deg = p[0];
        out.perspective_v = if mode == Upright::Level { 0.0 } else { p[1] };
        out.perspective_h = if mode == Upright::Full { p[2] } else { 0.0 };
        out
    };
    let cost = |p: [f32; 3]| {
        let t = with(p);
        let mut c = 0.0;
        if uses_v {
            c += vertical
                .iter()
                .map(|s| s.length() * lean(&t, sensor, s, false).powi(2))
                .sum::<f32>();
        }
        if uses_h {
            c += horizontal
                .iter()
                .map(|s| s.length() * lean(&t, sensor, s, true).powi(2))
                .sum::<f32>();
        }
        // A little pull towards no keystone, so a few stray lines do not warp the frame.
        let total: f32 = vertical.iter().chain(horizontal).map(Segment::length).sum();
        c + total * 1e-4 * (p[1] * p[1] + p[2] * p[2])
    };
    let best = nelder_mead(cost, [0.0, 0.0, 0.0], [2.0, 15.0, 15.0], 300);
    let bounded = [
        best[0].clamp(-45.0, 45.0),
        best[1].clamp(-100.0, 100.0),
        best[2].clamp(-100.0, 100.0),
    ];
    Some(with(bounded))
}

/// Minimise `f` from `start`, the first simplex `step` from it in each coordinate.
fn nelder_mead(
    f: impl Fn([f32; 3]) -> f32,
    start: [f32; 3],
    step: [f32; 3],
    iters: usize,
) -> [f32; 3] {
    let mut pts: Vec<([f32; 3], f32)> = (0..4)
        .map(|i| {
            let mut p = start;
            if i > 0 {
                p[i - 1] += step[i - 1];
            }
            (p, f(p))
        })
        .collect();
    let add = |a: [f32; 3], b: [f32; 3], s: f32| std::array::from_fn(|k| a[k] + s * (b[k] - a[k]));
    for _ in 0..iters {
        pts.sort_by(|a, b| a.1.total_cmp(&b.1));
        if (pts[3].1 - pts[0].1).abs() < 1e-7 * (1.0 + pts[0].1.abs()) {
            break;
        }
        let c: [f32; 3] = std::array::from_fn(|k| (pts[0].0[k] + pts[1].0[k] + pts[2].0[k]) / 3.0);
        let worst = pts[3].0;
        let r = add(c, worst, -1.0);
        let fr = f(r);
        if fr < pts[0].1 {
            let e = add(c, worst, -2.0);
            let fe = f(e);
            pts[3] = if fe < fr { (e, fe) } else { (r, fr) };
        } else if fr < pts[2].1 {
            pts[3] = (r, fr);
        } else {
            let k = add(c, worst, 0.5);
            let fk = f(k);
            if fk < pts[3].1 {
                pts[3] = (k, fk);
            } else {
                let best = pts[0].0;
                for p in pts.iter_mut().skip(1) {
                    p.0 = add(best, p.0, 0.5);
                    p.1 = f(p.0);
                }
            }
        }
    }
    pts.sort_by(|a, b| a.1.total_cmp(&b.1));
    pts[0].0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A façade shot looking up: verticals converging towards the top, and a level
    /// window line; drawn as bright bars on dark.
    fn facade(w: usize, h: usize, lean_top: f32, tilt_deg: f32) -> Vec<f32> {
        let mut img = vec![0.05f32; w * h];
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let (s, c) = tilt_deg.to_radians().sin_cos();
        for y in 0..h {
            for x in 0..w {
                // Undo the camera's tilt to find where this pixel is on the façade.
                let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                let (fx, fy) = (dx * c + dy * s, -dx * s + dy * c);
                // Converging: the top is narrower by `lean_top`.
                let t = (fy + cy) / h as f32;
                let squeeze = 1.0 - lean_top * (1.0 - t);
                let u = fx / squeeze;
                let pillar = [-0.3, -0.1, 0.1, 0.3]
                    .iter()
                    .any(|p| (u - p * w as f32).abs() < 2.0);
                let sill = [-0.2f32, 0.2]
                    .iter()
                    .any(|p| (fy - p * h as f32).abs() < 2.0);
                if pillar || sill {
                    img[y * w + x] = 0.8;
                }
            }
        }
        img
    }

    #[test]
    fn it_finds_verticals_and_horizontals_where_they_are() {
        let (w, h) = (240, 180);
        let (v, hz) = find_lines(&facade(w, h, 0.0, 0.0), w, h);
        assert!(v.len() >= 4, "{v:?}");
        assert!(hz.len() >= 2, "{hz:?}");
        let g = Geometry::default();
        for s in &v {
            assert!(
                lean(&g, (w as u32, h as u32), s, false).abs() < 1.0,
                "{s:?}"
            );
        }
    }

    #[test]
    fn a_tilted_photo_is_levelled() {
        let (w, h) = (240, 180);
        let (v, hz) = find_lines(&facade(w, h, 0.0, 4.0), w, h);
        let g = solve(
            &Geometry::default(),
            (w as u32, h as u32),
            &v,
            &hz,
            Upright::Level,
        )
        .expect("lines to level by");
        // The picture turned 4°, so straightening turns it back.
        assert!(
            (g.straighten_deg.abs() - 4.0).abs() < 0.6,
            "straighten {}",
            g.straighten_deg
        );
        assert_eq!(g.perspective_v, 0.0);
        for s in &v {
            assert!(lean(&g, (w as u32, h as u32), s, false).abs() < 0.8);
        }
    }

    #[test]
    fn converging_verticals_are_stood_up() {
        let (w, h) = (240, 180);
        let sensor = (w as u32, h as u32);
        let (v, hz) = find_lines(&facade(w, h, 0.25, 0.0), w, h);
        let before: f32 = v
            .iter()
            .map(|s| lean(&Geometry::default(), sensor, s, false).abs())
            .fold(0.0, f32::max);
        assert!(before > 3.0, "the test's verticals must lean: {before}");
        let g = solve(&Geometry::default(), sensor, &v, &hz, Upright::Vertical).expect("lines");
        assert!(g.perspective_v.abs() > 5.0, "{g:?}");
        for s in &v {
            let after = lean(&g, sensor, s, false);
            assert!(after.abs() < 1.0, "a vertical still leans {after}° ({g:?})");
        }
    }

    #[test]
    fn a_photo_without_lines_is_left_alone() {
        let (w, h) = (120, 90);
        let blank = vec![0.3f32; w * h];
        let (v, hz) = find_lines(&blank, w, h);
        assert!(v.is_empty() && hz.is_empty());
        assert!(solve(&Geometry::default(), (120, 90), &v, &hz, Upright::Vertical).is_none());
    }
}
