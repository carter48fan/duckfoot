//! Interactive 3D Inspector Map (Histogram & Color Scopes) for Barrel 1.
//!
//! Provides a 3D rotatable visualization of the image's tonal and color distribution:
//! - 3D Channels / Ribbons: Depth-separated RGB + Luminance ribbons in 3D space with
//!   smooth area fills and glowing peak outlines.
//! - 3D Surface / Terrain: Continuous spectral density mesh over Tone (0..255) and Spectrum.
//! - 3D RGB Cube: Color gamut distribution volume inside a 3D coordinate cube.
//! - 2D Plot: Classic 2D histogram plot.
//!
//! Features an interactive orbit camera (drag to rotate, Ctrl+scroll to zoom, double-click
//! or button to reset) with depth-sorted immediate-mode rendering.

use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use photo_engine::Histogram;

use crate::theme;
use crate::tonemap_3d::project_3d;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum InspectorMapMode {
    Channels3D,
    Surface3D,
    Cube3D,
    Plot2D,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InspectorMapState {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub mode: InspectorMapMode,
    pub large_view: bool,
}

impl Default for InspectorMapState {
    fn default() -> Self {
        Self {
            yaw: -0.65,  // ~ -37 degrees
            pitch: 0.38, // ~ +22 degrees
            zoom: 1.0,
            mode: InspectorMapMode::Channels3D,
            large_view: false,
        }
    }
}

/// The scope's modes, in display order, for the header band that switches between them:
/// `(mode, chip label, tooltip)`.
pub const MODES: [(InspectorMapMode, &str, &str); 4] = [
    (InspectorMapMode::Plot2D, "hist", "Classic 2D histogram"),
    (
        InspectorMapMode::Channels3D,
        "ribbon",
        "3D layered channel ribbons (red, green, blue, luma)",
    ),
    (
        InspectorMapMode::Surface3D,
        "surface",
        "3D continuous spectral density surface",
    ),
    (InspectorMapMode::Cube3D, "cube", "3D RGB gamut distribution cube"),
];

/// Draws the scope itself. The mode switch lives in the caller's header band — see [`MODES`].
pub fn render_inspector_map(
    ui: &mut egui::Ui,
    state: &mut InspectorMapState,
    hist: &Histogram,
) {
    let view_height = if state.mode == InspectorMapMode::Plot2D {
        if state.large_view { 140.0 } else { 96.0 }
    } else {
        if state.large_view { 240.0 } else { 165.0 }
    };

    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(200.0), view_height),
        if state.mode == InspectorMapMode::Plot2D {
            egui::Sense::hover()
        } else {
            egui::Sense::click_and_drag()
        },
    );

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(3), theme::BG_950);

    if state.mode == InspectorMapMode::Plot2D {
        render_2d_plot(&painter, rect, hist);
    } else {
        // Orbit camera handling
        if response.dragged() {
            let delta = response.drag_delta();
            state.yaw += delta.x * 0.012;
            state.pitch = (state.pitch - delta.y * 0.012).clamp(-1.45, 1.45);
        }

        // Ctrl+wheel or pinch, which egui reports as a zoom factor. A plain wheel is left
        // alone: this view sits in a scrolling sidebar, and one gesture must not do both.
        if response.hovered() {
            let zoom = ui.ctx().input(|i| i.zoom_delta());
            if zoom != 1.0 {
                state.zoom = (state.zoom * zoom).clamp(0.5, 2.8);
            }
        }

        if response.double_clicked() {
            state.yaw = -0.65;
            state.pitch = 0.38;
            state.zoom = 1.0;
        }

        render_3d_scope(&painter, rect, state, hist);
    }

    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(3),
        Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );

    // Readout line: clipping, then the view controls.
    let low = hist.clipped_low() * 100.0;
    let high = hist.clipped_high() * 100.0;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let warn = |v: f32| {
            if v >= 0.1 {
                theme::num(format!("{v:.2}%")).color(theme::DANGER)
            } else {
                theme::num(format!("{v:.2}%"))
            }
        };
        ui.label(theme::tag("clip lo").color(theme::TEXT_DIM));
        ui.label(warn(low));
        ui.label(theme::tag("hi").color(theme::TEXT_DIM));
        ui.label(warn(high));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let expand_icon = if state.large_view { "⊟" } else { "⊞" };
            let expand_tip = if state.large_view {
                "Standard scope height"
            } else {
                "Taller scope"
            };
            if crate::widgets::icon(ui, expand_icon, state.large_view, expand_tip).clicked() {
                state.large_view = !state.large_view;
            }
            if state.mode != InspectorMapMode::Plot2D
                && crate::widgets::icon(
                    ui,
                    "⟲",
                    false,
                    "Reset the 3D camera (or double-click the view)",
                )
                .clicked()
            {
                state.yaw = -0.65;
                state.pitch = 0.38;
                state.zoom = 1.0;
            }
        });
    });
}

fn render_2d_plot(painter: &egui::Painter, rect: Rect, hist: &Histogram) {
    if hist.peak == 0 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No data",
            egui::FontId::proportional(12.0),
            theme::TEXT_DIM,
        );
        return;
    }

    let bins = hist.luma.len().max(1);
    let step = rect.width() / bins as f32;
    let peak = hist.peak as f32;
    let height = |v: u32| (v as f32 / peak).min(1.0) * (rect.height() - 4.0);

    // Luminance solid body
    for (i, v) in hist.luma.iter().enumerate() {
        let h = height(*v);
        if h <= 0.0 {
            continue;
        }
        let x = rect.left() + i as f32 * step;
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x, rect.bottom() - h - 2.0),
                Pos2::new(x + step.max(1.0), rect.bottom() - 2.0),
            ),
            0.0,
            theme::TEXT_DIM.linear_multiply(0.28),
        );
    }

    // Channels outlines
    let channels: [(&Vec<u32>, Color32); 3] = [
        (&hist.red, Color32::from_rgb(0xd0, 0x5a, 0x50)),
        (&hist.green, Color32::from_rgb(0x6a, 0xb0, 0x6a)),
        (&hist.blue, Color32::from_rgb(0x5a, 0x86, 0xd0)),
    ];
    for (data, colour) in channels {
        let points: Vec<Pos2> = data
            .iter()
            .enumerate()
            .map(|(i, v)| Pos2::new(rect.left() + i as f32 * step, rect.bottom() - height(*v) - 2.0))
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, colour)));
    }
}

fn render_3d_scope(
    painter: &egui::Painter,
    rect: Rect,
    state: &InspectorMapState,
    hist: &Histogram,
) {
    let center = rect.center();
    let scale = (rect.width().min(rect.height()) * 0.44).max(35.0);

    // 3D coordinate floor grid
    draw_3d_grid(painter, center, scale, state);

    // Mode-specific 3D geometry
    match state.mode {
        InspectorMapMode::Channels3D => {
            render_3d_channels(painter, center, scale, state, hist);
        }
        InspectorMapMode::Surface3D => {
            render_3d_surface(painter, center, scale, state, hist);
        }
        InspectorMapMode::Cube3D => {
            render_3d_cube(painter, center, scale, state, hist);
        }
        InspectorMapMode::Plot2D => {}
    }

    // Interactive instruction footer
    painter.text(
        Pos2::new(rect.min.x + 8.0, rect.max.y - 12.0),
        egui::Align2::LEFT_BOTTOM,
        "Drag to rotate 3D view • Ctrl+scroll to zoom • Double-click to reset",
        egui::FontId::proportional(9.5),
        theme::TEXT_DIM.linear_multiply(0.75),
    );
}

fn draw_3d_grid(
    painter: &egui::Painter,
    center: Pos2,
    scale: f32,
    state: &InspectorMapState,
) {
    // Floor boundary
    let floor_pts = [
        [-1.0, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
    ];
    let mut floor_proj = Vec::new();
    for p in &floor_pts {
        if let Some((sp, _)) = project_3d(*p, center, scale, state.yaw, state.pitch, state.zoom) {
            floor_proj.push(sp);
        }
    }
    if floor_proj.len() == 4 {
        for i in 0..4 {
            painter.line_segment(
                [floor_proj[i], floor_proj[(i + 1) % 4]],
                Stroke::new(1.0, theme::BORDER.linear_multiply(0.7)),
            );
        }
    }

    // Floor grid lines along tonal levels (Shadows, Midtones, Highlights)
    for frac in [-0.5, 0.0, 0.5] {
        let p_start = project_3d([frac, -1.0, -1.0], center, scale, state.yaw, state.pitch, state.zoom);
        let p_end = project_3d([frac, 1.0, -1.0], center, scale, state.yaw, state.pitch, state.zoom);
        if let (Some((s, _)), Some((e, _))) = (p_start, p_end) {
            painter.line_segment([s, e], Stroke::new(0.8, theme::BORDER.linear_multiply(0.35)));
        }
    }

    // 3D Axis indicators
    let origin_proj = project_3d([-1.0, -1.0, -1.0], center, scale, state.yaw, state.pitch, state.zoom);
    let x_proj = project_3d([1.25, -1.0, -1.0], center, scale, state.yaw, state.pitch, state.zoom);
    let y_proj = project_3d([-1.0, 1.25, -1.0], center, scale, state.yaw, state.pitch, state.zoom);
    let z_proj = project_3d([-1.0, -1.0, 1.15], center, scale, state.yaw, state.pitch, state.zoom);

    if let (Some((o, _)), Some((x, _))) = (origin_proj, x_proj) {
        painter.line_segment([o, x], Stroke::new(1.1, Color32::from_rgb(180, 70, 70)));
        painter.text(
            x,
            egui::Align2::LEFT_CENTER,
            "Tones (0..255)",
            egui::FontId::monospace(8.5),
            theme::TEXT_DIM,
        );
    }
    if let (Some((o, _)), Some((y, _))) = (origin_proj, y_proj) {
        painter.line_segment([o, y], Stroke::new(1.1, Color32::from_rgb(70, 160, 70)));
        painter.text(
            y,
            egui::Align2::LEFT_CENTER,
            "Channels",
            egui::FontId::monospace(8.5),
            theme::TEXT_DIM,
        );
    }
    if let (Some((o, _)), Some((z, _))) = (origin_proj, z_proj) {
        painter.line_segment([o, z], Stroke::new(1.1, Color32::from_rgb(70, 120, 200)));
        painter.text(
            z,
            egui::Align2::CENTER_BOTTOM,
            "Density",
            egui::FontId::monospace(8.5),
            theme::TEXT_DIM,
        );
    }
}

struct DepthPoly {
    points: Vec<Pos2>,
    depth: f32,
    fill_col: Color32,
    stroke_col: Color32,
    stroke_width: f32,
}

fn render_3d_channels(
    painter: &egui::Painter,
    center: Pos2,
    scale: f32,
    state: &InspectorMapState,
    hist: &Histogram,
) {
    if hist.peak == 0 {
        return;
    }
    let peak = hist.peak as f32;

    const SAMPLES: usize = 64;
    let step_bin = 256.0 / (SAMPLES - 1) as f32;

    let channel_defs = [
        (&hist.red, -0.65, Color32::from_rgb(0xe0, 0x48, 0x40), "RED"),
        (&hist.green, -0.22, Color32::from_rgb(0x40, 0xb8, 0x50), "GREEN"),
        (&hist.blue, 0.22, Color32::from_rgb(0x45, 0x85, 0xe5), "BLUE"),
        (&hist.luma, 0.65, Color32::from_rgb(0xdf, 0xd0, 0xb5), "LUMA"),
    ];

    let mut depth_items: Vec<DepthPoly> = Vec::new();
    let mut channel_labels: Vec<(Pos2, &str, Color32)> = Vec::new();

    for (data, y_coord, base_color, label) in channel_defs {
        let fill_col = base_color.linear_multiply(0.24);
        let stroke_col = base_color;

        let mut top_pts: Vec<(Pos2, f32)> = Vec::with_capacity(SAMPLES);
        let mut bot_pts: Vec<(Pos2, f32)> = Vec::with_capacity(SAMPLES);

        for i in 0..SAMPLES {
            let bin_idx = ((i as f32 * step_bin).round() as usize).min(255);
            let val = data[bin_idx] as f32;
            let norm_height = (val / peak).clamp(0.0, 1.0);

            let x = (i as f32 / (SAMPLES - 1) as f32 - 0.5) * 2.0;
            let z_top = -1.0 + norm_height * 1.95;
            let z_bot = -1.0;

            if let Some(pt) = project_3d([x, y_coord, z_top], center, scale, state.yaw, state.pitch, state.zoom) {
                top_pts.push(pt);
            }
            if let Some(pt) = project_3d([x, y_coord, z_bot], center, scale, state.yaw, state.pitch, state.zoom) {
                bot_pts.push(pt);
            }
        }

        // Add channel label in 3D near the ribbon start
        if let Some((lbl_pos, _)) = project_3d([-1.1, y_coord, -0.95], center, scale, state.yaw, state.pitch, state.zoom) {
            channel_labels.push((lbl_pos, label, stroke_col));
        }

        // Slice ribbon into quads for accurate depth sorting
        if top_pts.len() == SAMPLES && bot_pts.len() == SAMPLES {
            for i in 0..(SAMPLES - 1) {
                let p_t0 = top_pts[i];
                let p_t1 = top_pts[i + 1];
                let p_b0 = bot_pts[i];
                let p_b1 = bot_pts[i + 1];

                let avg_depth = (p_t0.1 + p_t1.1 + p_b0.1 + p_b1.1) * 0.25;
                depth_items.push(DepthPoly {
                    points: vec![p_b0.0, p_t0.0, p_t1.0, p_b1.0],
                    depth: avg_depth,
                    fill_col,
                    stroke_col,
                    stroke_width: 1.2,
                });
            }
        }
    }

    // Sort descending by depth (Painter's algorithm: farthest first)
    depth_items.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));

    for item in depth_items {
        painter.add(egui::Shape::convex_polygon(
            item.points.clone(),
            item.fill_col,
            Stroke::NONE,
        ));
        // Top curve stroke
        if item.points.len() == 4 {
            painter.line_segment([item.points[1], item.points[2]], Stroke::new(item.stroke_width, item.stroke_col));
        }
    }

    // Draw floating channel badges
    for (pos, lbl, col) in channel_labels {
        painter.circle_filled(pos, 2.5, col);
        painter.text(
            pos + Vec2::new(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            lbl,
            egui::FontId::monospace(8.0),
            col,
        );
    }
}

fn render_3d_surface(
    painter: &egui::Painter,
    center: Pos2,
    scale: f32,
    state: &InspectorMapState,
    hist: &Histogram,
) {
    if hist.peak == 0 {
        return;
    }
    let peak = hist.peak as f32;

    const NX: usize = 32;
    const NY: usize = 4;

    let channel_data = [&hist.red, &hist.green, &hist.blue, &hist.luma];
    let y_coords = [-0.65, -0.22, 0.22, 0.65];

    let mut grid = [[(Pos2::ZERO, 0.0_f32, Color32::BLACK); NY]; NX];
    let mut valid = [[false; NY]; NX];

    for (i, col) in grid.iter_mut().enumerate().take(NX) {
        let u = i as f32 / (NX - 1) as f32;
        let bin_idx = ((u * 255.0).round() as usize).min(255);
        let x = (u - 0.5) * 2.0;

        let r_val = (hist.red[bin_idx] as f32 / peak).clamp(0.0, 1.0);
        let g_val = (hist.green[bin_idx] as f32 / peak).clamp(0.0, 1.0);
        let b_val = (hist.blue[bin_idx] as f32 / peak).clamp(0.0, 1.0);

        for (j, pt) in col.iter_mut().enumerate().take(NY) {
            let y = y_coords[j];
            let val = (channel_data[j][bin_idx] as f32 / peak).clamp(0.0, 1.0);
            let z = -1.0 + val * 1.95;

            if let Some((sp, depth)) = project_3d([x, y, z], center, scale, state.yaw, state.pitch, state.zoom) {
                // Color blended from spectrum and channel position
                let vertex_col = match j {
                    0 => Color32::from_rgb((160.0 + r_val * 95.0) as u8, 60, 50),
                    1 => Color32::from_rgb(50, (150.0 + g_val * 105.0) as u8, 60),
                    2 => Color32::from_rgb(60, 100, (160.0 + b_val * 95.0) as u8),
                    _ => Color32::from_rgb(200, 190, 175),
                };
                *pt = (sp, depth, vertex_col);
                valid[i][j] = true;
            }
        }
    }

    struct SurfaceQuad {
        pts: [Pos2; 4],
        depth: f32,
        color: Color32,
    }
    let mut quads = Vec::with_capacity((NX - 1) * (NY - 1));

    for i in 0..(NX - 1) {
        for j in 0..(NY - 1) {
            if valid[i][j] && valid[i + 1][j] && valid[i + 1][j + 1] && valid[i][j + 1] {
                let p0 = grid[i][j];
                let p1 = grid[i + 1][j];
                let p2 = grid[i + 1][j + 1];
                let p3 = grid[i][j + 1];
                let avg_depth = (p0.1 + p1.1 + p2.1 + p3.1) * 0.25;

                let r_avg = ((p0.2.r() as u16 + p1.2.r() as u16 + p2.2.r() as u16 + p3.2.r() as u16) / 4) as u8;
                let g_avg = ((p0.2.g() as u16 + p1.2.g() as u16 + p2.2.g() as u16 + p3.2.g() as u16) / 4) as u8;
                let b_avg = ((p0.2.b() as u16 + p1.2.b() as u16 + p2.2.b() as u16 + p3.2.b() as u16) / 4) as u8;

                let quad_col = Color32::from_rgb(r_avg, g_avg, b_avg).linear_multiply(0.45);
                quads.push(SurfaceQuad {
                    pts: [p0.0, p1.0, p2.0, p3.0],
                    depth: avg_depth,
                    color: quad_col,
                });
            }
        }
    }

    quads.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));

    for q in quads {
        painter.add(egui::Shape::convex_polygon(
            q.pts.to_vec(),
            q.color,
            Stroke::new(0.6, q.color.linear_multiply(1.5)),
        ));
    }
}

fn render_3d_cube(
    painter: &egui::Painter,
    center: Pos2,
    scale: f32,
    state: &InspectorMapState,
    hist: &Histogram,
) {
    if hist.peak == 0 {
        return;
    }
    let peak = hist.peak as f32;

    // Draw 3D Color Cube wireframe
    let corners = [
        [-1.0, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
    ];

    let edges = [
        (0, 1), (1, 2), (2, 3), (3, 0), // Bottom
        (4, 5), (5, 6), (6, 7), (7, 4), // Top
        (0, 4), (1, 5), (2, 6), (3, 7), // Verticals
    ];

    let mut proj_corners = [None; 8];
    for (i, c) in corners.iter().enumerate() {
        proj_corners[i] = project_3d(*c, center, scale, state.yaw, state.pitch, state.zoom);
    }

    for (a, b) in edges {
        if let (Some((p1, _)), Some((p2, _))) = (proj_corners[a], proj_corners[b]) {
            painter.line_segment([p1, p2], Stroke::new(1.0, theme::BORDER.linear_multiply(0.6)));
        }
    }

    // Corner RGB tags
    let tag_defs = [
        (0, "Black (0,0,0)", theme::TEXT_DIM),
        (1, "Red", Color32::from_rgb(220, 80, 70)),
        (2, "Yellow", Color32::from_rgb(220, 200, 60)),
        (3, "Green", Color32::from_rgb(70, 190, 70)),
        (4, "Blue", Color32::from_rgb(70, 130, 230)),
        (5, "Magenta", Color32::from_rgb(210, 80, 200)),
        (6, "White", theme::TEXT),
        (7, "Cyan", Color32::from_rgb(60, 200, 210)),
    ];
    for (idx, lbl, col) in tag_defs {
        if let Some((pos, _)) = proj_corners[idx] {
            painter.circle_filled(pos, 2.5, col);
            painter.text(
                pos + Vec2::new(3.0, -3.0),
                egui::Align2::LEFT_BOTTOM,
                lbl,
                egui::FontId::monospace(7.5),
                col,
            );
        }
    }

    // Sample RGB histogram bins to plot the 3D RGB distribution curve and point cloud
    const SAMPLES: usize = 36;
    let step = 256.0 / SAMPLES as f32;

    let mut points_3d: Vec<(Pos2, f32, Color32, f32)> = Vec::new();
    let mut curve_pts: Vec<Pos2> = Vec::new();

    for i in 0..SAMPLES {
        let b = ((i as f32 * step).round() as usize).min(255);
        let r_norm = (b as f32 / 255.0 - 0.5) * 2.0;
        let g_norm = (hist.green[b] as f32 / peak).clamp(0.0, 1.0) * 2.0 - 1.0;
        let b_norm = (hist.blue[b] as f32 / peak).clamp(0.0, 1.0) * 2.0 - 1.0;

        let density = (hist.luma[b] as f32 / peak).clamp(0.0, 1.0);
        let dot_color = Color32::from_rgb(
            b as u8,
            ((g_norm * 0.5 + 0.5) * 255.0) as u8,
            ((b_norm * 0.5 + 0.5) * 255.0) as u8,
        );

        if let Some((sp, depth)) = project_3d([r_norm, g_norm, b_norm], center, scale, state.yaw, state.pitch, state.zoom) {
            curve_pts.push(sp);
            points_3d.push((sp, depth, dot_color, density));
        }
    }

    // Connect 3D trajectory
    for pair in curve_pts.windows(2) {
        painter.line_segment([pair[0], pair[1]], Stroke::new(1.0, theme::ACCENT.linear_multiply(0.4)));
    }

    // Depth sort points
    points_3d.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (sp, _, col, density) in points_3d {
        let radius = 2.0 + density * 3.5;
        painter.circle_filled(sp, radius, col);
        painter.circle_stroke(sp, radius, Stroke::new(0.5, theme::BG_950));
    }
}
