//! The geometry icons: rotate and mirror, painted.

use crate::*;

#[derive(Clone, Copy)]
pub(crate) enum GeometryIcon {
    RotateLeft,
    RotateRight,
    MirrorH,
    MirrorV,
}

pub(crate) fn geometry_icon_button(
    ui: &mut egui::Ui,
    icon: GeometryIcon,
    active: bool,
    tooltip: &str,
) -> egui::Response {
    let size = egui::vec2(
        widgets::sized(ui, 28.0, 48.0),
        widgets::sized(ui, 24.0, 42.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let bg_color = if active {
            theme::ACCENT.linear_multiply(0.25)
        } else if response.hovered() {
            theme::BG_850
        } else {
            theme::BG_900
        };

        let stroke_color = if active {
            theme::ACCENT
        } else if response.hovered() {
            theme::TEXT_DIM
        } else {
            theme::BORDER
        };

        let icon_color = if active {
            theme::ACCENT
        } else if response.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };

        let rad = widgets::sized(ui, 2.0, 4.0) as u8;
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(rad), bg_color);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(rad),
            egui::Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );

        let center = rect.center();
        let s = (size.y / 24.0).clamp(1.0, 2.0);
        let stroke = egui::Stroke::new(1.5 * s, icon_color);

        match icon {
            GeometryIcon::RotateLeft => {
                let r = 5.5 * s;
                let mut points = Vec::new();
                for i in 0..=12 {
                    let angle =
                        std::f32::consts::PI * 0.2 + (i as f32 / 12.0) * std::f32::consts::PI * 1.4;
                    points.push(center + egui::vec2(angle.cos() * r, -angle.sin() * r));
                }
                for pair in points.windows(2) {
                    ui.painter().line_segment([pair[0], pair[1]], stroke);
                }
                let tip = points[points.len() - 1];
                ui.painter()
                    .line_segment([tip, tip + egui::vec2(3.0 * s, 0.0)], stroke);
                ui.painter()
                    .line_segment([tip, tip + egui::vec2(0.5 * s, 3.0 * s)], stroke);
            }
            GeometryIcon::RotateRight => {
                let r = 5.5 * s;
                let mut points = Vec::new();
                for i in 0..=12 {
                    let angle =
                        std::f32::consts::PI * 0.8 - (i as f32 / 12.0) * std::f32::consts::PI * 1.4;
                    points.push(center + egui::vec2(angle.cos() * r, -angle.sin() * r));
                }
                for pair in points.windows(2) {
                    ui.painter().line_segment([pair[0], pair[1]], stroke);
                }
                let tip = points[points.len() - 1];
                ui.painter()
                    .line_segment([tip, tip - egui::vec2(3.0 * s, 0.0)], stroke);
                ui.painter()
                    .line_segment([tip, tip + egui::vec2(-0.5 * s, 3.0 * s)], stroke);
            }
            GeometryIcon::MirrorH => {
                ui.painter().line_segment(
                    [
                        center + egui::vec2(0.0, -7.0 * s),
                        center + egui::vec2(0.0, 7.0 * s),
                    ],
                    egui::Stroke::new(1.0, theme::BORDER),
                );
                let t_left = [
                    center + egui::vec2(-2.0 * s, -4.5 * s),
                    center + egui::vec2(-7.0 * s, 0.0),
                    center + egui::vec2(-2.0 * s, 4.5 * s),
                ];
                ui.painter().add(egui::Shape::convex_polygon(
                    t_left.to_vec(),
                    if active {
                        theme::ACCENT.linear_multiply(0.4)
                    } else {
                        egui::Color32::TRANSPARENT
                    },
                    stroke,
                ));
                let t_right = [
                    center + egui::vec2(2.0 * s, -4.5 * s),
                    center + egui::vec2(7.0 * s, 0.0),
                    center + egui::vec2(2.0 * s, 4.5 * s),
                ];
                ui.painter().add(egui::Shape::convex_polygon(
                    t_right.to_vec(),
                    if active {
                        theme::ACCENT.linear_multiply(0.4)
                    } else {
                        egui::Color32::TRANSPARENT
                    },
                    stroke,
                ));
            }
            GeometryIcon::MirrorV => {
                ui.painter().line_segment(
                    [
                        center + egui::vec2(-7.0 * s, 0.0),
                        center + egui::vec2(7.0 * s, 0.0),
                    ],
                    egui::Stroke::new(1.0, theme::BORDER),
                );
                let t_top = [
                    center + egui::vec2(-4.5 * s, -2.0 * s),
                    center + egui::vec2(0.0, -7.0 * s),
                    center + egui::vec2(4.5 * s, -2.0 * s),
                ];
                ui.painter().add(egui::Shape::convex_polygon(
                    t_top.to_vec(),
                    if active {
                        theme::ACCENT.linear_multiply(0.4)
                    } else {
                        egui::Color32::TRANSPARENT
                    },
                    stroke,
                ));
                let t_bottom = [
                    center + egui::vec2(-4.5 * s, 2.0 * s),
                    center + egui::vec2(0.0, 7.0 * s),
                    center + egui::vec2(4.5 * s, 2.0 * s),
                ];
                ui.painter().add(egui::Shape::convex_polygon(
                    t_bottom.to_vec(),
                    if active {
                        theme::ACCENT.linear_multiply(0.4)
                    } else {
                        egui::Color32::TRANSPARENT
                    },
                    stroke,
                ));
            }
        }
    }

    response.on_hover_text(tooltip)
}
