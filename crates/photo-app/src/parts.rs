//! Painted pieces the desktop and phone layouts share: rows, tiles, badges, readouts.

use crate::*;

/// A hairline-bordered pod that groups a few controls, as in the top bar.
pub(crate) fn pod<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, Color32::from_gray(0x2a)))
        .corner_radius(5)
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            // Centred on the height it is given, not on one that grows as its items arrive,
            // so a pod sits on its bar's line (G-01); in the direction the bar runs.
            let dir = match ui.layout().main_dir() {
                egui::Direction::RightToLeft => egui::Direction::RightToLeft,
                _ => egui::Direction::LeftToRight,
            };
            // No taller than a control: in a wrapping row the height left is the panel's.
            let h = ui
                .available_height()
                .min(crate::widgets::sized(ui, 26.0, 32.0));
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), h),
                egui::Layout::from_main_dir_and_cross_align(dir, egui::Align::Center),
                add,
            )
            .inner
        })
        .inner
}

/// A short vertical hairline between groups in a row.
pub(crate) fn vsep(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 16.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, theme::RAIL),
    );
}

/// `LABEL value` in the status bar.
pub(crate) fn readout(ui: &mut egui::Ui, label: &str, value: &str, colour: Color32) {
    // Right-to-left callers, so the value goes in first.
    ui.label(
        RichText::new(value)
            .family(theme::mono_bold())
            .size(11.5)
            .color(colour),
    );
    ui.label(theme::tag(label).color(theme::TEXT_DIM));
    ui.add_space(8.0);
}

/// A bordered card with a heading band, for the Shelf bin's groups.
pub(crate) fn group(
    ui: &mut egui::Ui,
    title: &str,
    right: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(theme::BG_925)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            let head = egui::Frame::new()
                .fill(theme::BG_900)
                .corner_radius(egui::CornerRadius {
                    nw: theme::RADIUS - 1,
                    ne: theme::RADIUS - 1,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    widgets::card_title(ui, title, right);
                });
            let r = head.response.rect;
            ui.painter().hline(
                r.x_range(),
                r.bottom(),
                Stroke::new(1.0, Color32::from_gray(0x1f)),
            );
            egui::Frame::new()
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 3.0;
                    body(ui);
                });
        });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowStyle {
    Normal,
    /// Redo steps: not applied.
    Faint,
    /// The live state.
    Now,
    /// A snapshot identical to the live state.
    Match,
}

/// One row of a Shelf-bin list: mono label on the left, a short tag on the right.
pub(crate) fn list_row(
    ui: &mut egui::Ui,
    label: &str,
    style: RowStyle,
    right: Option<(&str, Color32)>,
) -> egui::Response {
    // A finger's height on touch, where these are the phone's Snapshots and History.
    let height = if widgets::is_touch(ui.ctx()) {
        widgets::MIN_TOUCH_TARGET
    } else {
        22.0
    };
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    widgets::describe(ui, &response, rect, || {
        let text = match right {
            Some((tag, _)) if !tag.is_empty() => format!("{label}, {tag}"),
            _ => label.to_string(),
        };
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            style == RowStyle::Now,
            text,
        )
    });
    let hovered = response.hovered() && style != RowStyle::Now;
    let painter = ui.painter();
    match style {
        RowStyle::Now => {
            painter.rect(
                rect,
                3.0,
                theme::BG_950,
                Stroke::new(1.0, theme::ACCENT.linear_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(
                Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
                egui::CornerRadius {
                    nw: 3,
                    sw: 3,
                    ne: 0,
                    se: 0,
                },
                theme::ACCENT,
            );
        }
        RowStyle::Match => {
            painter.rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0, theme::LIME.linear_multiply(0.5)),
                egui::StrokeKind::Inside,
            );
        }
        _ if hovered => {
            painter.rect_filled(rect, 3.0, theme::BG_850);
        }
        _ => {}
    }
    let (family, colour) = match style {
        RowStyle::Now => (theme::mono_bold(), theme::ACCENT),
        RowStyle::Faint => (egui::FontFamily::Monospace, theme::TEXT_FAINT),
        _ if hovered => (egui::FontFamily::Monospace, theme::TEXT_HI),
        _ => (egui::FontFamily::Monospace, Color32::from_gray(0xcc)),
    };
    painter.text(
        rect.left_center() + vec2(9.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::new(11.5, family),
        colour,
    );
    if let Some((tag, tag_colour)) = right {
        painter.text(
            rect.right_center() - vec2(8.0, 0.0),
            Align2::RIGHT_CENTER,
            tag,
            FontId::new(10.0, theme::mono_bold()),
            tag_colour,
        );
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A shelved file: icon, name, size badge, and a status dot. The size badge turns into a
/// remove button under the pointer. Returns `(response, removed)`.
pub(crate) const SHELF_ROW_HEIGHT: f32 = 30.0;

pub(crate) fn shelf_row(
    ui: &mut egui::Ui,
    item: &ShelfItem,
    active: bool,
    selected: bool,
    status: Option<Color32>,
) -> (egui::Response, bool) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), SHELF_ROW_HEIGHT), Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            active || selected,
            photo_description(item),
        )
    });
    let hovered = ui.rect_contains_pointer(rect);

    let badge_text = if item.missing {
        "missing".to_string()
    } else {
        format_bytes(item.bytes)
    };
    let badge_font = FontId::monospace(10.5);
    let badge_galley = ui
        .painter()
        .layout_no_wrap(badge_text, badge_font, Color32::PLACEHOLDER);
    let badge = Rect::from_min_size(
        pos2(
            rect.right() - 6.0 - badge_galley.size().x - 10.0,
            rect.center().y - 9.0,
        ),
        vec2(badge_galley.size().x + 10.0, 18.0),
    );
    // With a mouse the badge turns into Remove under the pointer. On touch there is no
    // hover to warn first, so a tap near the edge would remove the photo: there it stays
    // a size badge, and removing is in the row's menu.
    let remove_sense = if widgets::is_touch(ui.ctx()) {
        Sense::hover()
    } else {
        Sense::click()
    };
    let remove = ui.interact(badge, response.id.with("remove"), remove_sense);

    let painter = ui.painter();
    let border = if active {
        theme::ACCENT
    } else if selected {
        theme::ACCENT.linear_multiply(0.8)
    } else if hovered {
        theme::BORDER_HI
    } else {
        Color32::from_gray(0x20)
    };
    let fill = if active {
        theme::BG_900
    } else if selected {
        theme::ACCENT.linear_multiply(0.12)
    } else if hovered {
        Color32::from_gray(0x0c)
    } else {
        theme::BG_950
    };
    painter.rect(
        rect,
        3.0,
        fill,
        Stroke::new(if active || selected { 1.5 } else { 1.0 }, border),
        egui::StrokeKind::Inside,
    );

    let is_raster = photo_engine::is_raster_path(&item.path);
    painter.text(
        rect.left_center() + vec2(14.0, 0.0),
        Align2::CENTER_CENTER,
        if is_raster { "🖼" } else { "📷" },
        FontId::proportional(13.0),
        if active {
            theme::ACCENT
        } else {
            Color32::from_gray(0x88)
        },
    );

    let mut right_cursor = badge.left() - 6.0;

    if let Some(colour) = status {
        painter.circle_filled(pos2(right_cursor - 4.0, rect.center().y), 3.0, colour);
        right_cursor -= 12.0;
    }

    if item.metadata.rating > 0 {
        let r_text = format!("★{}", item.metadata.rating);
        let r_galley =
            painter.layout_no_wrap(r_text, FontId::new(10.5, theme::mono_bold()), theme::ACCENT);
        painter.galley(
            pos2(
                right_cursor - r_galley.size().x,
                rect.center().y - r_galley.size().y / 2.0,
            ),
            r_galley.clone(),
            theme::ACCENT,
        );
        right_cursor -= r_galley.size().x + 6.0;
    }

    match item.metadata.flag {
        photo_engine::Flag::Pick => {
            let f_rect =
                Rect::from_center_size(pos2(right_cursor - 7.0, rect.center().y), vec2(14.0, 14.0));
            painter.rect(
                f_rect,
                2.0,
                theme::BG_850,
                Stroke::new(1.0, theme::LIME),
                egui::StrokeKind::Inside,
            );
            painter.text(
                f_rect.center(),
                Align2::CENTER_CENTER,
                "P",
                FontId::new(9.0, theme::mono_bold()),
                theme::LIME,
            );
            right_cursor -= 18.0;
        }
        photo_engine::Flag::Reject => {
            let f_rect =
                Rect::from_center_size(pos2(right_cursor - 7.0, rect.center().y), vec2(14.0, 14.0));
            painter.rect(
                f_rect,
                2.0,
                theme::BG_850,
                Stroke::new(1.0, theme::DANGER),
                egui::StrokeKind::Inside,
            );
            painter.text(
                f_rect.center(),
                Align2::CENTER_CENTER,
                "X",
                FontId::new(9.0, theme::mono_bold()),
                theme::DANGER,
            );
            right_cursor -= 18.0;
        }
        photo_engine::Flag::Unflagged => {}
    }

    let (family, colour) = if active {
        (theme::mono_bold(), theme::ACCENT)
    } else if item.missing {
        (egui::FontFamily::Monospace, theme::TEXT_FAINT)
    } else {
        (egui::FontFamily::Monospace, theme::TEXT)
    };
    let mut job = egui::text::LayoutJob::single_section(
        item.name.clone(),
        egui::TextFormat::simple(FontId::new(12.0, family), colour),
    );
    let name_left = rect.left() + 30.0;
    job.wrap = egui::text::TextWrapping::truncate_at_width((right_cursor - name_left).max(10.0));
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(name_left, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );

    if hovered && !widgets::is_touch(ui.ctx()) {
        let hot = remove.hovered();
        painter.rect(
            badge,
            3.0,
            if hot {
                theme::DANGER.linear_multiply(0.2)
            } else {
                theme::BG_850
            },
            Stroke::new(1.0, if hot { theme::DANGER } else { theme::RAIL }),
            egui::StrokeKind::Inside,
        );
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            "× remove",
            FontId::monospace(10.0),
            if hot { theme::DANGER } else { theme::TEXT_DIM },
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else {
        painter.rect(
            badge,
            3.0,
            theme::BG_850,
            Stroke::new(1.0, theme::RAIL),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            badge.center() - badge_galley.size() / 2.0,
            badge_galley,
            if active {
                theme::TEXT
            } else {
                Color32::from_gray(0x88)
            },
        );
    }

    (response, remove.clicked())
}

/// The camera's own rendering of the photo being opened, fitted to the stage, until the
/// real decode replaces it. Not interactive: there is nothing to edit yet.
pub(crate) fn stage_preview(
    ui: &mut egui::Ui,
    preview: &egui::TextureHandle,
    path: &std::path::Path,
) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let [w, h] = preview.size();
    if rect.width() < 1.0 || rect.height() < 1.0 || w == 0 || h == 0 {
        return;
    }
    let img = egui::vec2(w as f32, h as f32);
    let fit = (rect.width() / img.x).min(rect.height() / img.y);
    let dest = egui::Rect::from_center_size(rect.center(), img * fit);
    let painter = ui.painter().with_clip_rect(rect);
    painter.image(
        preview.id(),
        dest,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    painter.rect_stroke(
        dest,
        0.0,
        Stroke::new(1.0, theme::RAIL),
        egui::StrokeKind::Outside,
    );
    hud_badge(
        &painter,
        dest.left_top() + vec2(12.0, 12.0),
        Align2::LEFT_TOP,
        &format!("opening {}…", file_name(path)),
        theme::ACCENT,
        theme::ACCENT,
    );
}

/// A filmstrip tile, and the distance from one tile's left edge to the next.
pub(crate) const FILM_TILE: egui::Vec2 = vec2(112.0, 70.0);
pub(crate) const FILM_GAP: f32 = 8.0;
pub(crate) const FILM_STRIDE: f32 = FILM_TILE.x + FILM_GAP;

/// Which of `total` tiles overlap the horizontal span `x` of the strip, measured from its
/// left edge. A tile only partly in view is included.
pub(crate) fn visible_tiles(x: egui::Rangef, total: usize) -> std::ops::Range<usize> {
    let first = (x.min / FILM_STRIDE).floor().max(0.0) as usize;
    let last = (x.max / FILM_STRIDE).ceil().max(0.0) as usize;
    first.min(total)..last.min(total)
}

/// A filmstrip frame with thumbnail preview, rating stars, flag badge, and selection border.
pub(crate) fn film_tile(
    ui: &mut egui::Ui,
    item: &ShelfItem,
    active: bool,
    selected: bool,
    pending: bool,
    picture: Option<(egui::TextureId, f32)>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(FILM_TILE, Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            active || selected,
            photo_description(item),
        )
    });
    let hovered = response.hovered();
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::BG_900);

    let inner = rect.shrink(if active { 3.0 } else { 1.0 });
    match picture {
        Some((id, aspect)) => {
            // Cover-fit: crop the texture's UVs to the tile's aspect.
            let tile_aspect = inner.width() / inner.height();
            let uv = if aspect > tile_aspect {
                let w = tile_aspect / aspect;
                Rect::from_min_max(pos2(0.5 - w / 2.0, 0.0), pos2(0.5 + w / 2.0, 1.0))
            } else {
                let h = aspect / tile_aspect;
                Rect::from_min_max(pos2(0.0, 0.5 - h / 2.0), pos2(1.0, 0.5 + h / 2.0))
            };
            painter.image(id, inner, uv, Color32::from_white_alpha(220));
        }
        None if item.missing => {
            painter.text(
                inner.center() - vec2(0.0, 6.0),
                Align2::CENTER_CENTER,
                "MISSING",
                FontId::new(10.0, theme::mono_bold()),
                theme::TEXT_FAINT,
            );
        }
        None => {
            let is_raster = photo_engine::is_raster_path(&item.path);
            painter.text(
                inner.center() - vec2(0.0, 6.0),
                Align2::CENTER_CENTER,
                if is_raster { "🖼" } else { "📷" },
                FontId::proportional(16.0),
                theme::RAIL,
            );
        }
    }

    if selected && !active {
        painter.rect_filled(inner, 0.0, theme::ACCENT.linear_multiply(0.12));
    }
    // The colour label, as a band along the top.
    if let Some(fill) = item
        .metadata
        .color_label
        .as_deref()
        .and_then(crate::labels::colour)
    {
        let band = Rect::from_min_size(inner.left_top(), vec2(inner.width(), 4.0));
        painter.rect_filled(band, 0.0, fill);
    }

    // File stem on a black strip, bottom-left, as the frame number.
    let stem = item
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&item.name)
        .to_string();
    let mut job = egui::text::LayoutJob::single_section(
        stem,
        egui::TextFormat::simple(
            FontId::new(
                10.5,
                if active {
                    theme::mono_bold()
                } else {
                    egui::FontFamily::Monospace
                },
            ),
            if active {
                theme::TEXT_HI
            } else {
                Color32::from_gray(0xcc)
            },
        ),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(inner.width() - 14.0);
    let galley = painter.layout_job(job);
    let strip = Rect::from_min_max(
        pos2(inner.left(), inner.bottom() - galley.size().y - 6.0),
        inner.right_bottom(),
    );
    painter.rect_filled(strip, 0.0, Color32::from_black_alpha(200));
    painter.galley(
        pos2(strip.left() + 6.0, strip.top() + 3.0),
        galley,
        theme::TEXT,
    );

    // Rating badge (top-right)
    if item.metadata.rating > 0 {
        let stars_text = format!("★{}", item.metadata.rating);
        let star_galley = painter.layout_no_wrap(
            stars_text,
            FontId::new(9.5, theme::mono_bold()),
            theme::ACCENT,
        );
        let star_rect = Rect::from_min_size(
            inner.right_top() + vec2(-star_galley.size().x - 6.0, 3.0),
            star_galley.size() + vec2(4.0, 2.0),
        );
        painter.rect_filled(star_rect, 2.0, Color32::from_black_alpha(200));
        painter.galley(
            star_rect.center() - star_galley.size() / 2.0,
            star_galley,
            theme::ACCENT,
        );
    }

    // Flag badge (top-left)
    match item.metadata.flag {
        photo_engine::Flag::Pick => {
            let p_rect = Rect::from_min_size(inner.left_top() + vec2(3.0, 3.0), vec2(14.0, 14.0));
            painter.rect_filled(p_rect, 2.0, theme::LIME.linear_multiply(0.85));
            painter.text(
                p_rect.center(),
                Align2::CENTER_CENTER,
                "P",
                FontId::new(9.0, theme::mono_bold()),
                Color32::BLACK,
            );
        }
        photo_engine::Flag::Reject => {
            let x_rect = Rect::from_min_size(inner.left_top() + vec2(3.0, 3.0), vec2(14.0, 14.0));
            painter.rect_filled(x_rect, 2.0, theme::DANGER.linear_multiply(0.85));
            painter.text(
                x_rect.center(),
                Align2::CENTER_CENTER,
                "X",
                FontId::new(9.0, theme::mono_bold()),
                Color32::WHITE,
            );
        }
        photo_engine::Flag::Unflagged => {}
    }

    let status = if item.has_sidecar {
        Some(theme::LIME)
    } else if item.is_edited {
        Some(theme::ACCENT)
    } else {
        None
    };
    if let Some(colour) = status {
        let dot_pos = if item.metadata.rating > 0 {
            inner.right_top() + vec2(-6.0, 18.0)
        } else {
            inner.right_top() + vec2(-6.0, 6.0)
        };
        painter.circle_filled(dot_pos, 3.0, colour);
    }

    if pending {
        painter.rect_filled(inner, 0.0, Color32::from_black_alpha(140));
        painter.text(
            inner.center(),
            Align2::CENTER_CENTER,
            "loading…",
            FontId::new(10.5, theme::mono_bold()),
            theme::ACCENT,
        );
    }
    let border = if active {
        Stroke::new(2.0, theme::ACCENT)
    } else if selected {
        Stroke::new(2.0, Color32::from_rgb(220, 220, 220))
    } else if pending {
        Stroke::new(2.0, theme::ACCENT.linear_multiply(0.5))
    } else if hovered {
        Stroke::new(1.0, theme::BORDER_HI)
    } else {
        Stroke::new(1.0, theme::BORDER)
    };
    painter.rect_stroke(rect, 3.0, border, egui::StrokeKind::Inside);
    if hovered && !active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.on_hover_text(&item.name)
}

/// Text on a black pill with a hairline, for readouts drawn over the image.
pub(crate) fn hud_badge(
    painter: &egui::Painter,
    anchor: egui::Pos2,
    align: Align2,
    text: &str,
    colour: Color32,
    border: Color32,
) {
    // Wrapped to the view it sits in, so a long hint on a phone takes two lines instead of
    // running off both edges.
    let room = (painter.clip_rect().width() - 32.0).max(80.0);
    let galley = painter.layout(
        text.to_owned(),
        FontId::new(11.0, theme::mono_bold()),
        colour,
        room,
    );
    let size = galley.size() + vec2(16.0, 8.0);
    let mut rect = align.anchor_size(anchor, size);
    let clip = painter.clip_rect().shrink(8.0);
    if rect.width() <= clip.width() {
        let dx = (clip.left() - rect.left()).max(0.0) + (clip.right() - rect.right()).min(0.0);
        rect = rect.translate(vec2(dx, 0.0));
    }
    painter.rect(
        rect,
        3.0,
        Color32::from_black_alpha(235),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(8.0, 4.0), galley, colour);
}

/// A, B, … Z, then 27, 28, … — a short handle for each snapshot.
pub(crate) fn snapshot_key(i: usize) -> String {
    if i < 26 {
        char::from(b'A' + i as u8).to_string()
    } else {
        (i + 1).to_string()
    }
}

/// A shelved photo as a screen reader says it: name, stars, flag, missing.
fn photo_description(item: &ShelfItem) -> String {
    let mut text = item.name.clone();
    if item.metadata.rating > 0 {
        text += &format!(", {} stars", item.metadata.rating);
    }
    match item.metadata.flag {
        photo_engine::Flag::Pick => text += ", pick",
        photo_engine::Flag::Reject => text += ", rejected",
        photo_engine::Flag::Unflagged => {}
    }
    if item.missing {
        text += ", missing";
    }
    text
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}
