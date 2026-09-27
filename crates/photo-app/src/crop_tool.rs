//! The crop tool's geometry, apart from any UI: which part of the crop a pointer is on,
//! and what a drag from there does to it, with or without a fixed aspect. Crops are in
//! 0..1 of the (turned, straightened) frame, as `photo_engine::Crop` keeps them; the frame
//! is `frame` pixels, which is what turns a pixel aspect into a normalised one.

use photo_engine::Crop;

/// What a pointer on the crop grabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grip {
    Move,
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Grip {
    /// (moves left edge, moves right, moves top, moves bottom)
    fn edges(self) -> (bool, bool, bool, bool) {
        match self {
            Grip::Move => (false, false, false, false),
            Grip::N => (false, false, true, false),
            Grip::S => (false, false, false, true),
            Grip::E => (false, true, false, false),
            Grip::W => (true, false, false, false),
            Grip::NE => (false, true, true, false),
            Grip::NW => (true, false, true, false),
            Grip::SE => (false, true, false, true),
            Grip::SW => (true, false, false, true),
        }
    }

    pub fn is_corner(self) -> bool {
        matches!(self, Grip::NE | Grip::NW | Grip::SE | Grip::SW)
    }

    pub fn cursor(self) -> egui::CursorIcon {
        use egui::CursorIcon as C;
        match self {
            Grip::Move => C::Move,
            Grip::N | Grip::S => C::ResizeVertical,
            Grip::E | Grip::W => C::ResizeHorizontal,
            Grip::NE | Grip::SW => C::ResizeNeSw,
            Grip::NW | Grip::SE => C::ResizeNwSe,
        }
    }
}

/// A fixed shape for the crop, or none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Aspect {
    Free,
    /// The photo's own shape.
    Original,
    /// Width to height, in the orientation given (swapped with `X`).
    Ratio(u32, u32),
}

impl Aspect {
    pub const CHOICES: [(Aspect, &'static str); 8] = [
        (Aspect::Free, "free"),
        (Aspect::Original, "original"),
        (Aspect::Ratio(1, 1), "1:1"),
        (Aspect::Ratio(4, 5), "4:5"),
        (Aspect::Ratio(3, 2), "3:2"),
        (Aspect::Ratio(16, 9), "16:9"),
        (Aspect::Ratio(5, 4), "5:4"),
        (Aspect::Ratio(9, 16), "9:16"),
    ];

    /// Width over height in pixels, for a frame `frame` pixels in size.
    pub fn pixel_ratio(self, frame: (f32, f32)) -> Option<f32> {
        match self {
            Aspect::Free => None,
            Aspect::Original => Some(frame.0 / frame.1),
            Aspect::Ratio(w, h) => Some(w as f32 / h as f32),
        }
    }

    /// The same shape turned a quarter: 3:2 to 2:3.
    pub fn swapped(self) -> Self {
        match self {
            Aspect::Ratio(w, h) => Aspect::Ratio(h, w),
            other => other,
        }
    }

    /// A label for the chip that shows it.
    pub fn label(self) -> String {
        match self {
            Aspect::Free => "free".into(),
            Aspect::Original => "original".into(),
            Aspect::Ratio(w, h) => format!("{w}:{h}"),
        }
    }
}

/// Width over height in normalised units, for a pixel ratio on a `frame`-sized frame.
fn normalised(pixel_ratio: f32, frame: (f32, f32)) -> f32 {
    pixel_ratio * frame.1 / frame.0
}

/// The grip under `p` (normalised), with handles `tolerance` pixels wide.
pub fn grip_at(crop: Crop, p: egui::Pos2, frame: (f32, f32), tolerance: f32) -> Option<Grip> {
    let (tx, ty) = (tolerance / frame.0, tolerance / frame.1);
    let (l, r, t, b) = (crop.x, crop.x + crop.width, crop.y, crop.y + crop.height);
    let within_x = p.x > l - tx && p.x < r + tx;
    let within_y = p.y > t - ty && p.y < b + ty;
    if !(within_x && within_y) {
        return None;
    }
    let near = |a: f32, b: f32, tol: f32| (a - b).abs() <= tol;
    let (on_l, on_r, on_t, on_b) = (
        near(p.x, l, tx),
        near(p.x, r, tx),
        near(p.y, t, ty),
        near(p.y, b, ty),
    );
    Some(match (on_l, on_r, on_t, on_b) {
        (true, _, true, _) => Grip::NW,
        (_, true, true, _) => Grip::NE,
        (true, _, _, true) => Grip::SW,
        (_, true, _, true) => Grip::SE,
        (true, ..) => Grip::W,
        (_, true, ..) => Grip::E,
        (_, _, true, _) => Grip::N,
        (_, _, _, true) => Grip::S,
        _ => Grip::Move,
    })
}

/// The smallest crop a drag can make, as a fraction of the frame.
const MIN: f32 = 0.02;

/// `start` dragged by `grip` from `grab` to `now`, keeping to `ratio` (pixel width over
/// height) if given, and inside the frame.
pub fn dragged(
    start: Crop,
    grip: Grip,
    grab: egui::Pos2,
    now: egui::Pos2,
    ratio: Option<f32>,
    frame: (f32, f32),
) -> Crop {
    let d = now - grab;
    if grip == Grip::Move {
        return Crop {
            x: (start.x + d.x).clamp(0.0, 1.0 - start.width),
            y: (start.y + d.y).clamp(0.0, 1.0 - start.height),
            ..start
        };
    }
    let (ml, mr, mt, mb) = grip.edges();
    let (mut l, mut r, mut t, mut b) = (
        start.x,
        start.x + start.width,
        start.y,
        start.y + start.height,
    );
    if ml {
        l = (l + d.x).clamp(0.0, r - MIN);
    }
    if mr {
        r = (r + d.x).clamp(l + MIN, 1.0);
    }
    if mt {
        t = (t + d.y).clamp(0.0, b - MIN);
    }
    if mb {
        b = (b + d.y).clamp(t + MIN, 1.0);
    }
    let Some(ratio) = ratio else {
        return Crop {
            x: l,
            y: t,
            width: r - l,
            height: b - t,
        };
    };
    let k = normalised(ratio, frame);
    let (w, h) = (r - l, b - t);
    if grip.is_corner() {
        // The corner opposite stays put; the size follows whichever way the drag went
        // further, then shrinks as far as the frame requires.
        let anchor_x = if ml { r } else { l };
        let anchor_y = if mt { b } else { t };
        let room_w = if ml { anchor_x } else { 1.0 - anchor_x };
        let room_h = if mt { anchor_y } else { 1.0 - anchor_y };
        let mut nw = w.max(h * k);
        nw = nw.min(room_w).min(room_h * k);
        let nh = nw / k;
        let x = if ml { anchor_x - nw } else { anchor_x };
        let y = if mt { anchor_y - nh } else { anchor_y };
        return Crop {
            x,
            y,
            width: nw,
            height: nh,
        };
    }
    // An edge: the other dimension follows, centred on where it was.
    if ml || mr {
        let cy = (start.y + start.height / 2.0).clamp(0.0, 1.0);
        let room_h = 2.0 * cy.min(1.0 - cy);
        let nw = w.min(room_h * k);
        let nh = nw / k;
        let x = if ml { r - nw } else { l };
        Crop {
            x,
            y: cy - nh / 2.0,
            width: nw,
            height: nh,
        }
    } else {
        let cx = (start.x + start.width / 2.0).clamp(0.0, 1.0);
        let room_w = 2.0 * cx.min(1.0 - cx);
        let nh = h.min(room_w / k);
        let nw = nh * k;
        let y = if mt { b - nh } else { t };
        Crop {
            x: cx - nw / 2.0,
            y,
            width: nw,
            height: nh,
        }
    }
}

/// A crop drawn from `anchor` to `now`, at `ratio` if given.
pub fn drawn(anchor: egui::Pos2, now: egui::Pos2, ratio: Option<f32>, frame: (f32, f32)) -> Crop {
    let start = Crop {
        x: anchor.x,
        y: anchor.y,
        width: 0.0,
        height: 0.0,
    };
    let grip = match (now.x < anchor.x, now.y < anchor.y) {
        (false, false) => Grip::SE,
        (true, false) => Grip::SW,
        (false, true) => Grip::NE,
        (true, true) => Grip::NW,
    };
    let mut c = dragged(start, grip, anchor, now, ratio, frame);
    if ratio.is_none() {
        // A drawn free crop starts at nothing; `dragged` kept the minimum, which is right.
        c.width = c.width.max(MIN);
        c.height = c.height.max(MIN);
    }
    c
}

/// The largest crop of `ratio` that fits `crop`'s centre, for when an aspect is chosen.
pub fn fitted(crop: Crop, ratio: f32, frame: (f32, f32)) -> Crop {
    let k = normalised(ratio, frame);
    let (cx, cy) = (crop.x + crop.width / 2.0, crop.y + crop.height / 2.0);
    let mut w = crop.width.max(crop.height * k);
    let mut h = w / k;
    if w > crop.width {
        w = crop.width;
        h = w / k;
    }
    if h > crop.height {
        h = crop.height;
        w = h * k;
    }
    Crop {
        x: (cx - w / 2.0).clamp(0.0, 1.0 - w),
        y: (cy - h / 2.0).clamp(0.0, 1.0 - h),
        width: w,
        height: h,
    }
}

/// Whether every corner of `crop` shows photographed pixels once the photo is turned
/// `deg` inside a frame of `frame` pixels: the photo is the frame's own size, rotated about
/// its centre, so straightening swings its corners out and the frame's corners go empty.
pub fn covered(crop: Crop, deg: f32, frame: (f32, f32)) -> bool {
    let (s, c) = deg.to_radians().sin_cos();
    let (w, h) = frame;
    let corners = [
        (crop.x, crop.y),
        (crop.x + crop.width, crop.y),
        (crop.x, crop.y + crop.height),
        (crop.x + crop.width, crop.y + crop.height),
    ];
    corners.iter().all(|&(u, v)| {
        // The corner in pixels about the centre, turned back into the photo's own axes.
        let (x, y) = ((u - 0.5) * w, (v - 0.5) * h);
        let (px, py) = (x * c + y * s, -x * s + y * c);
        px.abs() <= w * 0.5 + 1e-3 && py.abs() <= h * 0.5 + 1e-3
    })
}

/// `crop` made to show only photographed pixels after a turn of `deg`: kept if it already
/// does, otherwise moved toward the centre and, only if that is not enough, shrunk about
/// its own shape until every corner is on the photo.
pub fn constrained(crop: Crop, deg: f32, frame: (f32, f32)) -> Crop {
    if deg == 0.0 || covered(crop, deg, frame) {
        return crop;
    }
    let (s, c) = (deg.to_radians().sin().abs(), deg.to_radians().cos().abs());
    let (w, h) = frame;
    // The largest centred crop of this shape: its half-diagonal's projections must fit.
    let aspect = (crop.width * w) / (crop.height * h).max(1e-6);
    let max_h = (w / (aspect * c + s)).min(h / (aspect * s + c));
    let (fit_w, fit_h) = ((max_h * aspect / w).min(1.0), (max_h / h).min(1.0));
    let (cw, ch) = (crop.width.min(fit_w), crop.height.min(fit_h));
    let (cx, cy) = (crop.x + crop.width / 2.0, crop.y + crop.height / 2.0);
    let at = |t: f32| Crop {
        x: cx + (0.5 - cx) * t - cw / 2.0,
        y: cy + (0.5 - cy) * t - ch / 2.0,
        width: cw,
        height: ch,
    };
    // Slide from where it is to the centre, stopping as soon as it is covered.
    if covered(at(0.0), deg, frame) {
        return at(0.0);
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if covered(at(mid), deg, frame) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    at(hi)
}

/// Composition guides drawn over the crop, cycled with `O`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Guide {
    #[default]
    Thirds,
    Golden,
    Diagonals,
    Grid,
    None,
}

impl Guide {
    pub fn next(self) -> Self {
        match self {
            Guide::Thirds => Guide::Golden,
            Guide::Golden => Guide::Diagonals,
            Guide::Diagonals => Guide::Grid,
            Guide::Grid => Guide::None,
            Guide::None => Guide::Thirds,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Guide::Thirds => "thirds",
            Guide::Golden => "golden",
            Guide::Diagonals => "diagonals",
            Guide::Grid => "grid",
            Guide::None => "no guide",
        }
    }

    /// The lines, each as two points in 0..1 of the crop.
    pub fn lines(self) -> Vec<[(f32, f32); 2]> {
        let verticals = |ts: &[f32]| -> Vec<[(f32, f32); 2]> {
            ts.iter()
                .flat_map(|&t| [[(t, 0.0), (t, 1.0)], [(0.0, t), (1.0, t)]])
                .collect()
        };
        match self {
            Guide::Thirds => verticals(&[1.0 / 3.0, 2.0 / 3.0]),
            Guide::Golden => {
                let g = 1.0 / 1.618_034;
                verticals(&[1.0 - g, g])
            }
            Guide::Diagonals => vec![[(0.0, 0.0), (1.0, 1.0)], [(1.0, 0.0), (0.0, 1.0)]],
            Guide::Grid => verticals(&(1..8).map(|i| i as f32 / 8.0).collect::<Vec<_>>()),
            Guide::None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    const FRAME: (f32, f32) = (6000.0, 4000.0);

    fn crop(x: f32, y: f32, w: f32, h: f32) -> Crop {
        Crop {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn pixel_ratio(c: Crop) -> f32 {
        (c.width * FRAME.0) / (c.height * FRAME.1)
    }

    fn inside(c: Crop) -> bool {
        c.x >= -1e-5 && c.y >= -1e-5 && c.x + c.width <= 1.0 + 1e-5 && c.y + c.height <= 1.0 + 1e-5
    }

    #[test]
    fn handles_are_found_where_they_are_drawn() {
        let c = crop(0.2, 0.2, 0.6, 0.6);
        let at = |x, y| grip_at(c, pos2(x, y), FRAME, 12.0);
        assert_eq!(at(0.2, 0.2), Some(Grip::NW));
        assert_eq!(at(0.8, 0.8), Some(Grip::SE));
        assert_eq!(at(0.5, 0.2), Some(Grip::N));
        assert_eq!(at(0.8, 0.5), Some(Grip::E));
        assert_eq!(at(0.5, 0.5), Some(Grip::Move));
        assert_eq!(at(0.05, 0.5), None, "outside is not the crop's");
    }

    #[test]
    fn a_corner_drag_keeps_the_aspect_and_the_frame() {
        let ratio = Some(1.5);
        let start = fitted(crop(0.1, 0.1, 0.5, 0.5), 1.5, FRAME);
        assert!((pixel_ratio(start) - 1.5).abs() < 1e-3);
        for now in [
            pos2(0.9, 0.7),
            pos2(0.3, 0.95),
            pos2(1.4, 1.4),
            pos2(0.12, 0.12),
        ] {
            let grab = pos2(start.x + start.width, start.y + start.height);
            let c = dragged(start, Grip::SE, grab, now, ratio, FRAME);
            assert!((pixel_ratio(c) - 1.5).abs() < 1e-3, "{now:?}: {c:?}");
            assert!(inside(c), "{now:?}: {c:?}");
            assert!(
                (c.x - start.x).abs() < 1e-6 && (c.y - start.y).abs() < 1e-6,
                "anchored"
            );
        }
        // Dragged past the frame's corner, it grows to the frame and no further.
        let c = dragged(
            start,
            Grip::SE,
            pos2(0.5, 0.5),
            pos2(3.0, 3.0),
            ratio,
            FRAME,
        );
        assert!((c.x + c.width - 1.0).abs() < 1e-4 || (c.y + c.height - 1.0).abs() < 1e-4);
    }

    #[test]
    fn an_edge_drag_with_an_aspect_moves_the_other_side_about_the_centre() {
        let start = fitted(crop(0.25, 0.25, 0.5, 0.5), 1.0, FRAME);
        let grab = pos2(start.x + start.width, 0.5);
        let c = dragged(
            start,
            Grip::E,
            grab,
            pos2(grab.x + 0.1, 0.5),
            Some(1.0),
            FRAME,
        );
        assert!((pixel_ratio(c) - 1.0).abs() < 1e-3);
        let centre = |c: Crop| c.y + c.height / 2.0;
        assert!((centre(c) - centre(start)).abs() < 1e-5);
        assert!(inside(c));
        assert!(c.width > start.width);
    }

    #[test]
    fn free_edges_move_alone_and_never_cross() {
        let start = crop(0.2, 0.2, 0.6, 0.6);
        let c = dragged(start, Grip::W, pos2(0.2, 0.5), pos2(0.95, 0.5), None, FRAME);
        assert!(c.width >= MIN - 1e-6);
        assert!((c.x + c.width - 0.8).abs() < 1e-6, "the right edge stays");
        let moved = dragged(
            start,
            Grip::Move,
            pos2(0.5, 0.5),
            pos2(2.0, 2.0),
            None,
            FRAME,
        );
        assert!(inside(moved));
        assert_eq!((moved.width, moved.height), (start.width, start.height));
    }

    #[test]
    fn a_drawn_crop_has_the_asked_shape_in_any_direction() {
        for now in [
            pos2(0.9, 0.9),
            pos2(0.1, 0.9),
            pos2(0.9, 0.1),
            pos2(0.1, 0.1),
        ] {
            let c = drawn(pos2(0.5, 0.5), now, Some(16.0 / 9.0), FRAME);
            assert!((pixel_ratio(c) - 16.0 / 9.0).abs() < 1e-3, "{now:?}");
            assert!(inside(c));
        }
    }

    #[test]
    fn a_straightened_photo_is_cropped_to_what_was_photographed() {
        let full = crop(0.0, 0.0, 1.0, 1.0);
        assert!(covered(full, 0.0, FRAME));
        for deg in [2.0, -5.0, 12.0, 45.0] {
            assert!(!covered(full, deg, FRAME), "{deg}");
            let c = constrained(full, deg, FRAME);
            assert!(covered(c, deg, FRAME), "{deg}: {c:?}");
            // The shape is kept, and it is still as large as it can be (a hair larger
            // would show a corner).
            assert!((pixel_ratio(c) - 1.5).abs() < 1e-3, "{deg}: {c:?}");
            let grown = Crop {
                x: c.x - 0.002,
                y: c.y - 0.002,
                width: c.width + 0.004,
                height: c.height + 0.004,
            };
            assert!(!covered(grown, deg, FRAME), "{deg}: not the largest");
        }
        // A small crop off to one side slides in rather than shrinking.
        let small = crop(0.0, 0.0, 0.3, 0.3);
        let c = constrained(small, 8.0, FRAME);
        assert!(covered(c, 8.0, FRAME));
        assert!((c.width - 0.3).abs() < 1e-6 && (c.height - 0.3).abs() < 1e-6);
        // One already on the photo is left alone.
        let inner = crop(0.3, 0.3, 0.4, 0.4);
        assert_eq!(constrained(inner, 8.0, FRAME), inner);
    }

    #[test]
    fn swapping_turns_the_shape_and_guides_cycle_back() {
        assert_eq!(Aspect::Ratio(3, 2).swapped(), Aspect::Ratio(2, 3));
        assert_eq!(Aspect::Free.swapped(), Aspect::Free);
        let mut g = Guide::default();
        for _ in 0..5 {
            g = g.next();
        }
        assert_eq!(g, Guide::default());
    }
}
