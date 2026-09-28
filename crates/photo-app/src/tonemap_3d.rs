//! Interactive 3D tonemap visualization and manipulation for Barrel 1.
//!
//! Visualizes the logarithmic dynamic range transfer function (filmic tonemap) in 3D space:
//! - Surface mode: 3D transfer surface over scene dynamic range (EV) and exposure (±1 EV),
//!   with interactive 3D control handles for White EV, Black EV, Latitude, and Contrast.
//! - RGB Cube mode: a lattice of scene-linear colours, placed where filmic sends them on
//!   the display — brightness compressed, ratios (and so hues) kept.
//!
//! Interactive orbit camera (drag to rotate, wheel to zoom, double-click to reset) with
//! zero latency and depth-sorted immediate-mode rendering.

use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use photo_engine::params::Filmic;

use crate::{theme, widgets};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tonemap3DMode {
    Surface,
    RgbCube,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TonemapHandle {
    WhiteEv,
    BlackEv,
    Contrast,
    Latitude,
}

#[derive(Debug, Clone)]
pub struct Tonemap3DState {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub mode: Tonemap3DMode,
    pub active_handle: Option<TonemapHandle>,
}

impl Default for Tonemap3DState {
    fn default() -> Self {
        Self {
            yaw: -0.65,  // ~ -37 degrees
            pitch: 0.42, // ~ +24 degrees
            zoom: 1.0,
            mode: Tonemap3DMode::Surface,
            active_handle: None,
        }
    }
}

/// The filmic presets as a row of chips. A click sets all four values as one step.
pub fn filmic_presets(
    ui: &mut egui::Ui,
    filmic: &mut Filmic,
    changed: &mut bool,
    started: &mut bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(theme::tag("presets").color(theme::TEXT_DIM));
        if widgets::chip(ui, false, "neutral")
            .on_hover_text("Balanced photographic tone curve")
            .clicked()
        {
            *started = true;
            filmic.white_ev = 3.5;
            filmic.black_ev = -8.0;
            filmic.latitude = 20.0;
            filmic.contrast = 1.1;
            *changed = true;
        }
        if widgets::chip(ui, false, "dynamic")
            .on_hover_text("Wide dynamic range with highlight & shadow preservation")
            .clicked()
        {
            *started = true;
            filmic.white_ev = 5.0;
            filmic.black_ev = -10.0;
            filmic.latitude = 35.0;
            filmic.contrast = 1.0;
            *changed = true;
        }
        if widgets::chip(ui, false, "punchy")
            .on_hover_text("High-contrast commercial look with crisp blacks")
            .clicked()
        {
            *started = true;
            filmic.white_ev = 3.0;
            filmic.black_ev = -7.0;
            filmic.latitude = 16.0;
            filmic.contrast = 1.35;
            *changed = true;
        }
        if widgets::chip(ui, false, "film flat")
            .on_hover_text("Soft shoulder and toe for cinematic film grading")
            .clicked()
        {
            *started = true;
            filmic.white_ev = 4.2;
            filmic.black_ev = -6.5;
            filmic.latitude = 42.0;
            filmic.contrast = 0.92;
            *changed = true;
        }
    });
}

/// The filmic curve flat: scene EV across, display value up, with the white and black
/// points marked. Read-only — the sliders under it set the values — so on a phone it
/// never fights the sheet's scrolling the way the orbiting 3D view does.
pub fn render_filmic_2d(ui: &mut egui::Ui, filmic: &Filmic) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, (width * 0.42).clamp(110.0, 170.0)),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, theme::BG_950);
    let ev_min = (filmic.black_ev - 1.5).min(-12.0);
    let ev_max = (filmic.white_ev + 1.5).max(6.0);
    let x_of = |ev: f32| rect.left() + (ev - ev_min) / (ev_max - ev_min) * rect.width();
    let y_of = |v: f32| rect.bottom() - 6.0 - v.clamp(0.0, 1.0) * (rect.height() - 12.0);
    let grid = Stroke::new(1.0, Color32::from_gray(0x1a));
    let mut ev = ev_min.ceil();
    while ev <= ev_max {
        if (ev as i32) % 2 == 0 {
            painter.vline(x_of(ev), rect.y_range(), grid);
        }
        ev += 1.0;
    }
    for v in [0.25, 0.5, 0.75] {
        painter.hline(rect.x_range(), y_of(v), grid);
    }
    for (ev, colour, label) in [
        (
            filmic.black_ev,
            Color32::from_rgb(0x6a, 0x8c, 0xff),
            "black",
        ),
        (
            filmic.white_ev,
            Color32::from_rgb(0xff, 0xd0, 0x6a),
            "white",
        ),
    ] {
        let x = x_of(ev);
        painter.vline(
            x,
            rect.y_range(),
            Stroke::new(1.0, colour.gamma_multiply(0.6)),
        );
        painter.text(
            egui::pos2(x + 3.0, rect.top() + 3.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::monospace(9.5),
            colour,
        );
    }
    let points: Vec<Pos2> = (0..=96)
        .map(|i| {
            let ev = ev_min + (ev_max - ev_min) * i as f32 / 96.0;
            let v = eval_filmic(2.0_f32.powf(ev), filmic);
            egui::pos2(x_of(ev), y_of(v))
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.0, theme::ACCENT)));
    painter.text(
        egui::pos2(rect.right() - 4.0, rect.bottom() - 3.0),
        egui::Align2::RIGHT_BOTTOM,
        "scene EV →",
        egui::FontId::monospace(9.5),
        theme::TEXT_FAINT,
    );
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );
}

/// Evaluates the filmic sigmoid transfer function for a linear value `v`.
/// Filmic's output for a scene-linear value, as a display value (0..=1): the engine's own
/// curve (`Filmic::display_value`), so what is drawn here is what the shader does.
pub fn eval_filmic(v: f32, f: &Filmic) -> f32 {
    f.display_value(v)
}

/// Perspective projection of a 3D point (X, Y, Z) to 2D screen coordinates.
/// Coordinate system: X is right, Y is depth (forward), Z is up.
pub fn project_3d(
    p: [f32; 3],
    center: Pos2,
    scale: f32,
    yaw: f32,
    pitch: f32,
    zoom: f32,
) -> Option<(Pos2, f32)> {
    let (sin_y, cos_y) = yaw.sin_cos();
    let (sin_p, cos_p) = pitch.sin_cos();

    // Yaw rotation around Z axis
    let x1 = p[0] * cos_y + p[1] * sin_y;
    let y1 = -p[0] * sin_y + p[1] * cos_y;
    let z1 = p[2];

    // Pitch rotation around X axis
    let x2 = x1;
    let y2 = y1 * cos_p - z1 * sin_p;
    let z2 = y1 * sin_p + z1 * cos_p;

    // Perspective projection
    let dist = 3.6;
    let depth = dist - y2;
    if depth < 0.2 {
        return None;
    }

    let fov = scale * zoom * dist / depth;
    let screen_x = center.x + x2 * fov;
    let screen_y = center.y - z2 * fov; // screen Y is downwards
    Some((Pos2::new(screen_x, screen_y), depth))
}

pub fn render_tonemap_3d(
    ui: &mut egui::Ui,
    state: &mut Tonemap3DState,
    filmic: &mut Filmic,
    changed: &mut bool,
    started: &mut bool,
) {
    // Mode switcher and toolbar
    ui.horizontal(|ui| {
        if widgets::chip(ui, state.mode == Tonemap3DMode::Surface, "surface")
            .on_hover_text("3D Dynamic Range & Tone Response Surface")
            .clicked()
        {
            state.mode = Tonemap3DMode::Surface;
        }
        if widgets::chip(ui, state.mode == Tonemap3DMode::RgbCube, "rgb cube")
            .on_hover_text("3D Color Gamut Compression Volume")
            .clicked()
        {
            state.mode = Tonemap3DMode::RgbCube;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon(
                ui,
                "⟲",
                false,
                "Reset the 3D camera (or double-click the view)",
            )
            .clicked()
            {
                state.yaw = -0.65;
                state.pitch = 0.42;
                state.zoom = 1.0;
            }
        });
    });

    filmic_presets(ui, filmic, changed, started);

    let height = 230.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(200.0), height),
        egui::Sense::click_and_drag(),
    );

    let center = rect.center();
    let scale = (rect.width().min(rect.height()) * 0.42).max(40.0);

    // Dynamic EV range bounds
    let ev_min = (filmic.black_ev - 1.5).min(-12.0);
    let ev_max = (filmic.white_ev + 1.5).max(6.0);
    let ev_span = (ev_max - ev_min).max(1.0);

    // Compute interactive 3D handles positions in screen space
    let mut handle_screen: Vec<(TonemapHandle, Pos2, String)> = Vec::new();
    if state.mode == Tonemap3DMode::Surface {
        // 1. Black EV handle (toe)
        let b_u = (filmic.black_ev - ev_min) / ev_span;
        let b_x = (b_u - 0.5) * 2.0;
        let b_z = (eval_filmic(2.0_f32.powf(filmic.black_ev), filmic) - 0.5) * 2.0;
        if let Some((pos, _)) = project_3d(
            [b_x, 0.0, b_z],
            center,
            scale,
            state.yaw,
            state.pitch,
            state.zoom,
        ) {
            handle_screen.push((
                TonemapHandle::BlackEv,
                pos,
                format!("Black: {:.1} EV", filmic.black_ev),
            ));
        }

        // 2. White EV handle (shoulder)
        let w_u = (filmic.white_ev - ev_min) / ev_span;
        let w_x = (w_u - 0.5) * 2.0;
        let w_z = (eval_filmic(2.0_f32.powf(filmic.white_ev), filmic) - 0.5) * 2.0;
        if let Some((pos, _)) = project_3d(
            [w_x, 0.0, w_z],
            center,
            scale,
            state.yaw,
            state.pitch,
            state.zoom,
        ) {
            handle_screen.push((
                TonemapHandle::WhiteEv,
                pos,
                format!("White: +{:.1} EV", filmic.white_ev),
            ));
        }

        // 3. Contrast handle at midtone (0 EV, 18% grey)
        let m_u = (0.0 - ev_min) / ev_span;
        let m_x = (m_u - 0.5) * 2.0;
        let m_z = (eval_filmic(1.0, filmic) - 0.5) * 2.0;
        if let Some((pos, _)) = project_3d(
            [m_x, 0.0, m_z],
            center,
            scale,
            state.yaw,
            state.pitch,
            state.zoom,
        ) {
            handle_screen.push((
                TonemapHandle::Contrast,
                pos,
                format!("Contrast: {:.2}", filmic.contrast),
            ));
        }

        // 4. Latitude handle
        let lat_ev = filmic.black_ev + (filmic.white_ev - filmic.black_ev) * 0.5;
        let l_u = (lat_ev - ev_min) / ev_span;
        let l_x = (l_u - 0.5) * 2.0;
        let l_z = (eval_filmic(2.0_f32.powf(lat_ev), filmic) - 0.5) * 2.0;
        if let Some((pos, _)) = project_3d(
            [l_x, 0.5, l_z],
            center,
            scale,
            state.yaw,
            state.pitch,
            state.zoom,
        ) {
            handle_screen.push((
                TonemapHandle::Latitude,
                pos,
                format!("Latitude: {:.0}%", filmic.latitude),
            ));
        }
    }

    // Interaction handling
    let hover_pos = response.hover_pos();
    // A fingertip covers far more than a pointer does.
    let reach = if crate::widgets::is_touch(ui.ctx()) {
        24.0
    } else {
        12.0
    };
    let hovered_handle = hover_pos.and_then(|hpos| {
        handle_screen.iter().find_map(|(handle, spos, _)| {
            if hpos.distance(*spos) <= reach {
                Some(*handle)
            } else {
                None
            }
        })
    });

    if response.drag_started() {
        state.active_handle = hovered_handle;
        if state.active_handle.is_some() {
            *started = true;
        }
    }

    if response.dragged() {
        let delta = response.drag_delta();
        if let Some(active) = state.active_handle {
            match active {
                TonemapHandle::WhiteEv => {
                    filmic.white_ev = (filmic.white_ev + delta.x * 0.04).clamp(0.0, 8.0);
                    *changed = true;
                }
                TonemapHandle::BlackEv => {
                    filmic.black_ev = (filmic.black_ev + delta.x * 0.05).clamp(-16.0, -1.0);
                    *changed = true;
                }
                TonemapHandle::Contrast => {
                    filmic.contrast =
                        (filmic.contrast - delta.y * 0.015 + delta.x * 0.01).clamp(0.1, 3.0);
                    *changed = true;
                }
                TonemapHandle::Latitude => {
                    filmic.latitude = (filmic.latitude + delta.x * 0.25).clamp(5.0, 80.0);
                    *changed = true;
                }
            }
        } else {
            // Orbit camera
            state.yaw += delta.x * 0.012;
            state.pitch = (state.pitch - delta.y * 0.012).clamp(-1.45, 1.45);
        }
    }

    if response.drag_stopped() {
        state.active_handle = None;
    }

    // Ctrl+wheel or pinch, which egui reports as a zoom factor. A plain wheel is left
    // alone: this view sits in a scrolling sidebar, and one gesture must not do both.
    if response.hovered() {
        let zoom = ui.ctx().input(|i| i.zoom_delta());
        if zoom != 1.0 {
            state.zoom = (state.zoom * zoom).clamp(0.6, 2.5);
        }
    }

    // Double click resets view
    if response.double_clicked() {
        state.yaw = -0.65;
        state.pitch = 0.42;
        state.zoom = 1.0;
    }

    // Paint 3D Viewport
    let painter = ui.painter().with_clip_rect(rect);

    // Background and frame
    painter.rect_filled(rect, egui::CornerRadius::same(3), theme::BG_950);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(3),
        Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );

    // Draw 3D bounding box / grid floor
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

    // Draw Axis lines & Labels
    let origin_proj = project_3d(
        [-1.0, -1.0, -1.0],
        center,
        scale,
        state.yaw,
        state.pitch,
        state.zoom,
    );
    let x_proj = project_3d(
        [1.2, -1.0, -1.0],
        center,
        scale,
        state.yaw,
        state.pitch,
        state.zoom,
    );
    let y_proj = project_3d(
        [-1.0, 1.2, -1.0],
        center,
        scale,
        state.yaw,
        state.pitch,
        state.zoom,
    );
    let z_proj = project_3d(
        [-1.0, -1.0, 1.2],
        center,
        scale,
        state.yaw,
        state.pitch,
        state.zoom,
    );

    if let (Some((o, _)), Some((x, _))) = (origin_proj, x_proj) {
        painter.line_segment([o, x], Stroke::new(1.2, Color32::from_rgb(180, 70, 70)));
        painter.text(
            x,
            egui::Align2::LEFT_CENTER,
            "Input EV",
            egui::FontId::monospace(9.0),
            theme::TEXT_DIM,
        );
    }
    if let (Some((o, _)), Some((y, _))) = (origin_proj, y_proj) {
        painter.line_segment([o, y], Stroke::new(1.2, Color32::from_rgb(70, 160, 70)));
        painter.text(
            y,
            egui::Align2::LEFT_CENTER,
            if state.mode == Tonemap3DMode::Surface {
                "Exposure ±1 EV"
            } else {
                "Green"
            },
            egui::FontId::monospace(9.0),
            theme::TEXT_DIM,
        );
    }
    if let (Some((o, _)), Some((z, _))) = (origin_proj, z_proj) {
        painter.line_segment([o, z], Stroke::new(1.2, Color32::from_rgb(70, 120, 200)));
        painter.text(
            z,
            egui::Align2::CENTER_BOTTOM,
            "Display LDR",
            egui::FontId::monospace(9.0),
            theme::TEXT_DIM,
        );
    }

    match state.mode {
        Tonemap3DMode::Surface => {
            render_surface_mode(
                &painter,
                center,
                scale,
                state,
                filmic,
                ev_min,
                ev_span,
                &handle_screen,
                hovered_handle,
            );
        }
        Tonemap3DMode::RgbCube => {
            render_rgb_cube_mode(&painter, center, scale, state, filmic);
        }
    }

    // Hover instruction footer
    let hint = if state.active_handle.is_some() {
        "Dragging handle: real-time live tonemap adjust"
    } else if hovered_handle.is_some() {
        "Click & drag handle to adjust parameter"
    } else {
        crate::widgets::orbit_hint(ui.ctx())
    };
    painter.text(
        Pos2::new(rect.min.x + 8.0, rect.max.y - 14.0),
        egui::Align2::LEFT_BOTTOM,
        hint,
        egui::FontId::proportional(10.0),
        theme::TEXT_DIM.linear_multiply(0.8),
    );
}

#[allow(clippy::too_many_arguments)]
fn render_surface_mode(
    painter: &egui::Painter,
    center: Pos2,
    scale: f32,
    state: &Tonemap3DState,
    filmic: &Filmic,
    ev_min: f32,
    ev_span: f32,
    handles: &[(TonemapHandle, Pos2, String)],
    hovered_handle: Option<TonemapHandle>,
) {
    const NX: usize = 28;
    const NY: usize = 7;

    // Grid of 3D points
    let mut grid_3d = [[(Pos2::ZERO, 0.0_f32, Color32::BLACK); NY]; NX];
    let mut valid = [[false; NY]; NX];

    for (i, col) in grid_3d.iter_mut().enumerate().take(NX) {
        let u = i as f32 / (NX - 1) as f32;
        let ev = ev_min + u * ev_span;
        let x = (u - 0.5) * 2.0;
        let v_lin = 2.0_f32.powf(ev);

        for (j, pt) in col.iter_mut().enumerate().take(NY) {
            let v_norm = j as f32 / (NY - 1) as f32;
            let y = (v_norm - 0.5) * 1.6;

            // Depth is exposure, −1 EV to +1 EV: the same curve met by a darker or brighter
            // version of the scene, which is what an exposure change does to it.
            let exposure_gain = 2.0_f32.powf((v_norm - 0.5) * 2.0);
            let mapped = eval_filmic(v_lin * exposure_gain, filmic);
            let z = (mapped - 0.5) * 2.0;

            if let Some((sp, depth)) =
                project_3d([x, y, z], center, scale, state.yaw, state.pitch, state.zoom)
            {
                // Coloured by the display value it maps to: grey, as the output is.
                let col = Color32::from_gray((mapped * 240.0) as u8);

                *pt = (sp, depth, col);
                valid[i][j] = true;
            }
        }
    }

    // Depth-sorted quads for painter's algorithm
    struct Quad {
        p: [Pos2; 4],
        depth: f32,
        color: Color32,
    }
    let mut quads = Vec::with_capacity((NX - 1) * (NY - 1));

    for i in 0..(NX - 1) {
        for j in 0..(NY - 1) {
            if valid[i][j] && valid[i + 1][j] && valid[i + 1][j + 1] && valid[i][j + 1] {
                let p0 = grid_3d[i][j];
                let p1 = grid_3d[i + 1][j];
                let p2 = grid_3d[i + 1][j + 1];
                let p3 = grid_3d[i][j + 1];
                let avg_depth = (p0.1 + p1.1 + p2.1 + p3.1) * 0.25;

                // Base surface quad color
                let u = i as f32 / NX as f32;
                let ev = ev_min + u * ev_span;
                let in_latitude = ev >= filmic.black_ev && ev <= filmic.white_ev;

                let mut q_col = Color32::from_rgb(
                    ((p0.2.r() as u16 + p1.2.r() as u16 + p2.2.r() as u16 + p3.2.r() as u16) / 4)
                        as u8,
                    ((p0.2.g() as u16 + p1.2.g() as u16 + p2.2.g() as u16 + p3.2.g() as u16) / 4)
                        as u8,
                    ((p0.2.b() as u16 + p1.2.b() as u16 + p2.2.b() as u16 + p3.2.b() as u16) / 4)
                        as u8,
                );

                if in_latitude {
                    // Warm subtle brass tint inside the linear latitude region
                    q_col = Color32::from_rgb(
                        (q_col.r() as f32 * 0.85 + theme::ACCENT.r() as f32 * 0.15) as u8,
                        (q_col.g() as f32 * 0.85 + theme::ACCENT.g() as f32 * 0.15) as u8,
                        (q_col.b() as f32 * 0.85 + theme::ACCENT.b() as f32 * 0.15) as u8,
                    );
                }

                quads.push(Quad {
                    p: [p0.0, p1.0, p2.0, p3.0],
                    depth: avg_depth,
                    color: q_col,
                });
            }
        }
    }

    // Sort by depth descending (furthest first)
    quads.sort_by(|a, b| {
        b.depth
            .partial_cmp(&a.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for q in quads {
        painter.add(egui::Shape::convex_polygon(
            q.p.to_vec(),
            q.color,
            Stroke::new(0.6, theme::BORDER.linear_multiply(0.8)),
        ));
    }

    // Draw primary center S-curve line (middle spectrum row)
    let mid_j = NY / 2;
    for i in 0..(NX - 1) {
        if valid[i][mid_j] && valid[i + 1][mid_j] {
            painter.line_segment(
                [grid_3d[i][mid_j].0, grid_3d[i + 1][mid_j].0],
                Stroke::new(2.2, theme::ACCENT),
            );
        }
    }

    // Draw handles on top
    for (handle, pos, label) in handles {
        let is_hovered = hovered_handle == Some(*handle) || state.active_handle == Some(*handle);
        let node_color = if is_hovered {
            theme::ACCENT
        } else {
            theme::TEXT
        };
        let node_radius = if is_hovered { 6.0 } else { 4.5 };

        // A plain ring when it is the one under the pointer: no glow (DESIGN.md).
        if is_hovered {
            painter.circle_stroke(*pos, node_radius + 2.5, Stroke::new(1.0, theme::ACCENT));
        }
        painter.circle_filled(*pos, node_radius, node_color);
        painter.circle_stroke(*pos, node_radius, Stroke::new(1.0, theme::BG_950));

        // Label badge for hovered handle
        if is_hovered {
            let badge_pos = *pos + Vec2::new(10.0, -10.0);
            let badge_rect = Rect::from_min_size(badge_pos, Vec2::new(95.0, 16.0));
            painter.rect_filled(
                badge_rect,
                egui::CornerRadius::same(2),
                theme::BG_900.linear_multiply(0.9),
            );
            painter.rect_stroke(
                badge_rect,
                egui::CornerRadius::same(2),
                Stroke::new(1.0, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
            painter.text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::monospace(9.5),
                theme::TEXT,
            );
        }
    }
}

fn render_rgb_cube_mode(
    painter: &egui::Painter,
    center: Pos2,
    scale: f32,
    state: &Tonemap3DState,
    filmic: &Filmic,
) {
    const STEPS: usize = 5;

    // 3D RGB lattice points deformed by the tonemapper
    let mut lattice = Vec::new();

    for r in 0..STEPS {
        let fr = r as f32 / (STEPS - 1) as f32;
        for g in 0..STEPS {
            let fg = g as f32 / (STEPS - 1) as f32;
            for b in 0..STEPS {
                let fb = b as f32 / (STEPS - 1) as f32;

                // Scene-linear input, through filmic as the shader applies it: the curve on
                // the brightest channel, and every channel scaled by the same ratio.
                let (r_in, g_in, b_in) = (fr * 4.0, fg * 4.0, fb * 4.0);
                let norm = r_in.max(g_in).max(b_in);
                let ratio = if norm > 0.0 {
                    srgb_to_linear(eval_filmic(norm, filmic)) / norm
                } else {
                    0.0
                };
                let r_mapped = linear_to_srgb(r_in * ratio);
                let g_mapped = linear_to_srgb(g_in * ratio);
                let b_mapped = linear_to_srgb(b_in * ratio);

                // Center in [-1, 1]
                let x = (r_mapped - 0.5) * 2.0;
                let y = (g_mapped - 0.5) * 2.0;
                let z = (b_mapped - 0.5) * 2.0;

                if let Some((sp, depth)) =
                    project_3d([x, y, z], center, scale, state.yaw, state.pitch, state.zoom)
                {
                    let col = Color32::from_rgb(
                        (r_mapped * 255.0) as u8,
                        (g_mapped * 255.0) as u8,
                        (b_mapped * 255.0) as u8,
                    );
                    lattice.push((sp, depth, col, [r, g, b]));
                }
            }
        }
    }

    // Draw connecting edges along the lattice
    for i in 0..lattice.len() {
        let (p1, _, _, idx1) = lattice[i];
        for (p2, _, _, idx2) in lattice.iter().skip(i + 1) {
            let dr = idx1[0].abs_diff(idx2[0]);
            let dg = idx1[1].abs_diff(idx2[1]);
            let db = idx1[2].abs_diff(idx2[2]);
            if dr + dg + db == 1 {
                painter.line_segment([p1, *p2], Stroke::new(1.0, theme::BORDER));
            }
        }
    }

    // Sort nodes by depth descending
    lattice.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (sp, _, col, _) in lattice {
        painter.circle_filled(sp, 3.5, col);
        painter.circle_stroke(sp, 3.5, Stroke::new(0.5, theme::BG_950));
    }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
