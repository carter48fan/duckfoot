//! Monotone cubic (Fritsch–Carlson) tone curve, baked to a LUT for the GPU.
//!
//! A plain Catmull–Rom spline overshoots between close control points, which on a tone
//! curve shows up as a reversal — the image gets *darker* as you drag a node up. Monotone
//! interpolation cannot do that.
//!
//! The LUT is rebuilt on the CPU and re-uploaded only when a node moves. It is 4 KB, so
//! that upload is irrelevant; what matters is that it does not happen per frame.

pub const LUT_SIZE: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveNode {
    pub x: f32,
    pub y: f32,
}

impl CurveNode {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// The identity curve — input maps to output unchanged.
pub fn identity_nodes() -> Vec<CurveNode> {
    vec![CurveNode::new(0.0, 0.0), CurveNode::new(1.0, 1.0)]
}

pub fn build_lut(nodes: &[CurveNode]) -> Vec<f32> {
    let mut pts: Vec<CurveNode> = nodes.to_vec();
    pts.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    pts.dedup_by(|a, b| (a.x - b.x).abs() < 1e-6);

    if pts.len() < 2 {
        return (0..LUT_SIZE)
            .map(|i| i as f32 / (LUT_SIZE - 1) as f32)
            .collect();
    }

    let n = pts.len();
    let mut dx = Vec::with_capacity(n - 1);
    let mut slope = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        let h = pts[i + 1].x - pts[i].x;
        dx.push(h);
        slope.push(if h == 0.0 {
            0.0
        } else {
            (pts[i + 1].y - pts[i].y) / h
        });
    }

    let mut tangent = vec![0.0f32; n];
    tangent[0] = slope[0];
    tangent[n - 1] = slope[n - 2];
    for i in 1..n - 1 {
        if slope[i - 1] * slope[i] <= 0.0 {
            // A local extremum: a zero tangent is what forbids the overshoot.
            tangent[i] = 0.0;
        } else {
            let t = (slope[i - 1] + slope[i]) / 2.0;
            let limit = 3.0 * slope[i - 1].abs().min(slope[i].abs());
            tangent[i] = if t.abs() > limit {
                t.signum() * limit
            } else {
                t
            };
        }
    }

    let mut lut = Vec::with_capacity(LUT_SIZE);
    let mut seg = 0usize;
    for i in 0..LUT_SIZE {
        let x = i as f32 / (LUT_SIZE - 1) as f32;
        while seg < n - 2 && x > pts[seg + 1].x {
            seg += 1;
        }
        let h = dx[seg];
        if h == 0.0 {
            lut.push(pts[seg].y);
            continue;
        }
        let t = ((x - pts[seg].x) / h).clamp(0.0, 1.0);
        let t2 = t * t;
        let t3 = t2 * t;
        lut.push(
            (2.0 * t3 - 3.0 * t2 + 1.0) * pts[seg].y
                + (t3 - 2.0 * t2 + t) * h * tangent[seg]
                + (-2.0 * t3 + 3.0 * t2) * pts[seg + 1].y
                + (t3 - t2) * h * tangent[seg + 1],
        );
    }
    lut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_curve_is_identity() {
        let lut = build_lut(&identity_nodes());
        assert_eq!(lut.len(), LUT_SIZE);
        for (i, v) in lut.iter().enumerate() {
            let x = i as f32 / (LUT_SIZE - 1) as f32;
            assert!((v - x).abs() < 1e-4, "at {x}: {v}");
        }
    }

    #[test]
    fn never_reverses_between_close_nodes() {
        // The exact shape that makes a Catmull-Rom spline overshoot.
        let nodes = vec![
            CurveNode::new(0.0, 0.0),
            CurveNode::new(0.45, 0.20),
            CurveNode::new(0.50, 0.85),
            CurveNode::new(1.0, 1.0),
        ];
        let lut = build_lut(&nodes);
        for w in lut.windows(2) {
            assert!(w[1] >= w[0] - 1e-5, "curve reversed: {} -> {}", w[0], w[1]);
        }
    }

    #[test]
    fn passes_through_its_control_points() {
        let nodes = vec![
            CurveNode::new(0.0, 0.1),
            CurveNode::new(0.5, 0.4),
            CurveNode::new(1.0, 0.9),
        ];
        let lut = build_lut(&nodes);
        assert!((lut[0] - 0.1).abs() < 1e-3);
        assert!((lut[LUT_SIZE / 2] - 0.4).abs() < 2e-3);
        assert!((lut[LUT_SIZE - 1] - 0.9).abs() < 1e-3);
    }

    #[test]
    fn survives_degenerate_input() {
        assert_eq!(build_lut(&[]).len(), LUT_SIZE);
        assert_eq!(build_lut(&[CurveNode::new(0.5, 0.5)]).len(), LUT_SIZE);
        // Duplicate x values must not divide by zero.
        let dup = build_lut(&[
            CurveNode::new(0.0, 0.0),
            CurveNode::new(0.5, 0.3),
            CurveNode::new(0.5, 0.7),
            CurveNode::new(1.0, 1.0),
        ]);
        assert!(dup.iter().all(|v| v.is_finite()));
    }
}
