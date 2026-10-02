//! The tone curve editor.

use crate::*;

/// A minimal draggable curve editor. Endpoints are locked in x so the curve always spans
/// the full range.
/// Most nodes a curve takes. More than this is a curve nobody can steer.
pub(crate) const CURVE_MAX_NODES: usize = 16;
/// Closest two nodes may sit in x, so neither can pass the other.
pub(crate) const CURVE_MIN_GAP: f32 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CurveChannel {
    #[default]
    Rgb,
    Red,
    Green,
    Blue,
}

impl CurveChannel {
    pub fn color(self) -> egui::Color32 {
        match self {
            Self::Rgb => theme::ACCENT,
            Self::Red => egui::Color32::from_rgb(239, 68, 68),
            Self::Green => egui::Color32::from_rgb(34, 197, 94),
            Self::Blue => egui::Color32::from_rgb(59, 130, 246),
        }
    }

    pub fn hi_color(self) -> egui::Color32 {
        match self {
            Self::Rgb => theme::ACCENT_HI,
            Self::Red => egui::Color32::from_rgb(252, 165, 165),
            Self::Green => egui::Color32::from_rgb(134, 239, 172),
            Self::Blue => egui::Color32::from_rgb(147, 197, 253),
        }
    }
}

/// The tone curve. Tap the graph to add a node on the curve; drag a node to move it (the
/// grab keeps its offset, so it never jumps under the finger); double-tap a node, or drag
/// it well off the graph, to remove it. The two end nodes stay at black and white in x.
#[allow(dead_code)]
pub(crate) fn curve_editor(ui: &mut egui::Ui, nodes: &mut Vec<CurveNode>) -> egui::Response {
    curve_editor_channel(ui, nodes, CurveChannel::Rgb)
}

pub(crate) fn curve_editor_channel(
    ui: &mut egui::Ui,
    nodes: &mut Vec<CurveNode>,
    channel: CurveChannel,
) -> egui::Response {
    if nodes.len() < 2 {
        *nodes = identity_nodes();
    }
    if !nodes.is_sorted_by(|a, b| a.x <= b.x) {
        nodes.sort_by(|a, b| a.x.total_cmp(&b.x));
    }
    let touch = widgets::is_touch(ui.ctx());
    let width = ui.available_width();
    let height = if touch {
        // As tall as the sheet has room for under it, less the hint row, so the whole
        // curve is in reach without scrolling the sheet.
        let visible = ui.clip_rect().bottom() - ui.cursor().top() - 40.0;
        (width * 0.7).min(visible).clamp(140.0, 280.0)
    } else {
        (width * 0.62).clamp(120.0, 220.0)
    };
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
    let hit = widgets::sized(ui, 10.0, 22.0);
    // Past this far outside the graph, a dragged node is dropped on release.
    let tear = widgets::sized(ui, 24.0, 40.0);

    let to_screen = |n: &CurveNode| {
        egui::pos2(
            rect.left() + n.x.clamp(0.0, 1.0) * rect.width(),
            rect.bottom() - n.y.clamp(0.0, 1.0) * rect.height(),
        )
    };
    let from_screen = |p: egui::Pos2| {
        (
            (p.x - rect.left()) / rect.width(),
            (rect.bottom() - p.y) / rect.height(),
        )
    };
    let nearest = |nodes: &[CurveNode], p: egui::Pos2| {
        nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (i, to_screen(n).distance(p)))
            .filter(|(_, d)| *d <= hit)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    // Insert a node on the curve at `x`, if there is room for one there.
    let insert = |nodes: &mut Vec<CurveNode>, x: f32| -> Option<usize> {
        if nodes.len() >= CURVE_MAX_NODES || x <= CURVE_MIN_GAP || x >= 1.0 - CURVE_MIN_GAP {
            return None;
        }
        let at = nodes.partition_point(|n| n.x < x);
        let crowded = [at.checked_sub(1), Some(at)]
            .into_iter()
            .flatten()
            .filter_map(|i| nodes.get(i))
            .any(|n| (n.x - x).abs() < CURVE_MIN_GAP);
        if crowded {
            return None;
        }
        let lut = photo_engine::curve::build_lut(nodes);
        let y = lut[(x * (lut.len() - 1) as f32).round() as usize].clamp(0.0, 1.0);
        nodes.insert(at, CurveNode::new(x, y));
        Some(at)
    };

    // The node being dragged and where on it it was grabbed, for the length of a gesture.
    let chan_key = match channel {
        CurveChannel::Rgb => "rgb",
        CurveChannel::Red => "red",
        CurveChannel::Green => "green",
        CurveChannel::Blue => "blue",
    };
    let grab_id = response.id.with(chan_key).with("grab");
    let mut grab: Option<(usize, egui::Vec2)> = ui.data(|d| d.get_temp(grab_id));
    let mut changed = false;
    let pointer = response.interact_pointer_pos();

    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or(pointer)
            .unwrap_or(rect.center());
        let picked = nearest(nodes, origin).or_else(|| {
            let (x, _) = from_screen(origin);
            let added = insert(nodes, x);
            changed |= added.is_some();
            added
        });
        grab = picked.map(|i| (i, to_screen(&nodes[i]) - origin));
    }
    let mut tearing = None;
    if let (Some((i, offset)), Some(p), true) = (grab, pointer, response.dragged()) {
        if i < nodes.len() {
            let target = p + offset;
            let (mut x, y) = from_screen(target);
            let last = nodes.len() - 1;
            x = if i == 0 {
                0.0
            } else if i == last {
                1.0
            } else {
                x.clamp(
                    nodes[i - 1].x + CURVE_MIN_GAP,
                    nodes[i + 1].x - CURVE_MIN_GAP,
                )
            };
            let moved = CurveNode::new(x, y.clamp(0.0, 1.0));
            if nodes[i] != moved {
                nodes[i] = moved;
                changed = true;
            }
            let outside = rect.expand(tear);
            if i != 0 && i != last && !outside.contains(target) {
                tearing = Some(i);
            }
        }
    }
    if response.drag_stopped() {
        if let (Some((i, offset)), Some(p)) = (grab, pointer) {
            let last = nodes.len().saturating_sub(1);
            if i != 0 && i < last && !rect.expand(tear).contains(p + offset) {
                nodes.remove(i);
                changed = true;
            }
        }
        grab = None;
    }
    if response.double_clicked() {
        if let Some(i) = pointer.and_then(|p| nearest(nodes, p)) {
            if i != 0 && i != nodes.len() - 1 {
                nodes.remove(i);
                changed = true;
            }
        }
    } else if response.clicked() {
        if let Some(p) = pointer {
            if nearest(nodes, p).is_none() {
                changed |= insert(nodes, from_screen(p).0).is_some();
            }
        }
    }
    ui.data_mut(|d| match grab {
        Some(g) => {
            d.insert_temp(grab_id, g);
        }
        None => {
            d.remove_temp::<(usize, egui::Vec2)>(grab_id);
        }
    });
    if changed {
        response.mark_changed();
    }

    let painter = ui.painter_at(rect.expand(tear));
    painter.rect_filled(rect, 3.0, theme::BG_950);
    // Quarter reticle and the identity diagonal, so a move reads against neutral.
    let grid = egui::Stroke::new(1.0, egui::Color32::from_gray(0x1a));
    for i in 1..4 {
        let t = i as f32 / 4.0;
        painter.vline(rect.left() + t * rect.width(), rect.y_range(), grid);
        painter.hline(rect.x_range(), rect.top() + t * rect.height(), grid);
    }
    painter.extend(egui::Shape::dashed_line(
        &[rect.left_bottom(), rect.right_top()],
        egui::Stroke::new(1.0, theme::BORDER_HI),
        4.0,
        4.0,
    ));

    // The curve as it will be once the gesture ends: without a node being torn off. Its
    // samples are kept between frames and made again only when a node moves (WP5.4).
    let shown = || {
        nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != tearing)
            .map(|(_, n)| *n)
    };
    let samples = ui.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<CurveSamples>(response.id.with(chan_key).with("samples"));
        if cache.values.is_empty() || !cache.nodes.iter().copied().eq(shown()) {
            cache.nodes = shown().collect();
            let lut = photo_engine::curve::build_lut(&cache.nodes);
            cache.values = std::sync::Arc::new(
                (0..128)
                    .map(|i| {
                        let t = i as f32 / 127.0;
                        lut[(t * (lut.len() - 1) as f32) as usize].clamp(0.0, 1.0)
                    })
                    .collect(),
            );
        }
        cache.values.clone()
    });
    let points: Vec<egui::Pos2> = samples
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let t = i as f32 / 127.0;
            egui::pos2(
                rect.left() + t * rect.width(),
                rect.bottom() - v * rect.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(widgets::sized(ui, 1.75, 2.25), channel.color()),
    ));

    let active = grab.map(|(i, _)| i);
    let r = widgets::sized(ui, 3.5, 6.0);
    for (i, n) in nodes.iter().enumerate() {
        let (fill, stroke) = if Some(i) == tearing {
            (theme::BG_950, theme::DANGER)
        } else if Some(i) == active {
            (channel.color(), channel.hi_color())
        } else {
            (theme::TEXT_HI, channel.color())
        };
        painter.circle(to_screen(n), r, fill, egui::Stroke::new(1.5, stroke));
    }
    // What the dragged node maps, in 8-bit levels, as the numbers every curve tool shows.
    if let Some(n) = active.and_then(|i| nodes.get(i)) {
        let text = if tearing.is_some() {
            "release to remove".to_string()
        } else {
            format!("{:.0} → {:.0}", n.x * 255.0, n.y * 255.0)
        };
        painter.text(
            rect.left_top() + egui::vec2(6.0, 5.0),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::monospace(widgets::sized(ui, 10.5, 12.0)),
            if tearing.is_some() {
                theme::DANGER
            } else {
                theme::TEXT
            },
        );
    }
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );
    if response.hovered() {
        let over_node = response
            .hover_pos()
            .and_then(|p| nearest(nodes, p))
            .is_some();
        ui.ctx().set_cursor_icon(if over_node || active.is_some() {
            egui::CursorIcon::Grab
        } else {
            egui::CursorIcon::Crosshair
        });
    }
    response
}

/// The curve's plotted samples and the nodes they were made from.
#[derive(Clone, Default)]
struct CurveSamples {
    nodes: Vec<CurveNode>,
    values: std::sync::Arc<Vec<f32>>,
}
