//! The phone layout: the photo fills the screen, one low sheet at a time sits under it,
//! and a navigation bar picks the sheet. It is the same app as the desktop layout — every
//! sheet is built from the same pieces the desktop's panels are — only arranged for a
//! small, touch-driven screen.
//!
//! Upright, the bar runs along the bottom and the sheet rises above it, only as tall as
//! what it holds. Turned sideways the bar becomes a rail down the right edge and the sheet
//! opens beside it. Tapping the open tab again closes the sheet and gives the photo the
//! whole screen.
//!
//! | Tab | Holds |
//! |---|---|
//! | Library | the roll's thumbnails and the current photo's rating and flag, over one row of actions: open, add, import a folder, and chips for the folder tree, filters, select mode and the tour |
//! | Presets | one-tap looks — sepia, mono, film… — and how strongly to apply them |
//! | Edit | the chosen module's controls over a one-line strip: turn, every module by name, and the list that chooses them |
//! | Export | the single export and batch export |
//!
//! Getting around without a keyboard:
//!
//! - swipe the nav bar, or an empty part of any sheet but Edit, to change tab;
//! - swipe an empty part of Edit's controls, or tap ‹ ›, to change module;
//! - swipe across the photo for the next or previous photo;
//! - fine adjust and reset are in the top bar, on every tab.
//!
//! Opening a photo from a picker goes straight to Edit: on a phone the photo is the point,
//! and the roll is only how you got to it. The first run walks through all this (`tour`).

use egui::containers::scroll_area::{DragScroll, ScrollSource};
use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Margin, Painter, Rect, RichText, Sense, Stroke,
};

use crate::presets::{self, LOOKS};
use crate::widgets::{self, Kind};
use crate::Gpu;
use crate::{file_name, summarise, theme, Export, InspectorSectionId, PhotoApp, View};
use photo_engine::PhotoStack;
use std::path::PathBuf;

/// The side sheet's resize grip, along its left edge; its content starts after it.
const GRIP_W: f32 = 12.0;

/// Which sheet is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Library,
    Presets,
    Edit,
    Export,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Library, Tab::Presets, Tab::Edit, Tab::Export];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Library => "library",
            Tab::Presets => "presets",
            Tab::Edit => "edit",
            Tab::Export => "export",
        }
    }

    /// The tab `step` places along, if there is one.
    fn offset(self, step: i32) -> Option<Tab> {
        let i = Tab::ALL.iter().position(|t| *t == self)? as i32 + step;
        Tab::ALL.get(usize::try_from(i).ok()?).copied()
    }
}

/// What the Presets sheet lists: the built-in looks, this photo's snapshots, or its
/// history. Snapshots and history are saved looks of one photo, so they live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresetsView {
    #[default]
    Looks,
    Snapshots,
    History,
}

/// The look last applied from Presets. `applied` is the stack it produced: while the
/// photo's stack still equals it, the look is live and its strength can be changed; once
/// anything else is edited the look is simply part of the photo.
#[derive(Debug, Clone)]
pub struct PresetState {
    look: usize,
    strength: f32,
    base: PhotoStack,
    pub(crate) applied: PhotoStack,
}

impl PresetState {
    /// Which of [`LOOKS`] it is.
    pub(crate) fn look(&self) -> usize {
        self.look
    }
}

/// Where the phone layout's parts were last frame.
#[derive(Debug, Clone, Copy)]
pub struct Rects {
    top: Rect,
    nav: Rect,
    sheet: Rect,
    stage: Rect,
}

impl Default for Rects {
    fn default() -> Self {
        Self {
            top: Rect::NOTHING,
            nav: Rect::NOTHING,
            sheet: Rect::NOTHING,
            stage: Rect::NOTHING,
        }
    }
}

/// Which layout a screen gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// Shelf, stage and inspector side by side.
    #[default]
    Desktop,
    /// Navigation bar along the bottom, sheet above it.
    Portrait,
    /// Navigation rail down the right, sheet beside it.
    Landscape,
}

impl Layout {
    /// Below this the desktop's side panels leave the stage too little room. The desktop
    /// window's minimum size (900 × 600) is inside it, so a desktop never gets the phone
    /// layout; phones do either way up, and so do tablets held upright.
    pub const MIN_DESKTOP: egui::Vec2 = vec2(900.0, 560.0);

    pub fn for_size(size: egui::Vec2) -> Self {
        if size.x >= Self::MIN_DESKTOP.x && size.y >= Self::MIN_DESKTOP.y {
            Layout::Desktop
        } else if size.x > size.y {
            Layout::Landscape
        } else {
            Layout::Portrait
        }
    }

    pub fn is_compact(self) -> bool {
        self != Layout::Desktop
    }
}

const NAV_BAR: f32 = 48.0;
/// Height of the top bar. Fixed, so everything in it can be centred on one line: a row
/// that grows as its items arrive centres the early ones on the height it had so far.
const TOP_BAR: f32 = 40.0;
const NAV_RAIL: f32 = 56.0;
/// Height of the Edit sheet's module strip.
const STRIP_H: f32 = 34.0;
/// How far a flick must travel sideways to count.
const SWIPE: f32 = 56.0;

impl PhotoApp {
    pub(crate) fn compact_layout(&mut self, ui: &mut egui::Ui, gpu: &Gpu, screen: egui::Vec2) {
        let landscape = self.layout == Layout::Landscape;
        let frame = |margin: Margin| egui::Frame::new().fill(theme::BG_950).inner_margin(margin);

        // Panel order is layout order: the top bar, then the navigation, then the sheet
        // against it, then the status line, and the stage takes what is left.
        self.compact_rects.top = self.compact_top_bar(ui);
        self.compact_rects.nav = if landscape {
            egui::Panel::right("compact_nav")
                .frame(frame(Margin::symmetric(0, 4)).fill(theme::BG_925))
                .resizable(false)
                .exact_size(NAV_RAIL)
                .show(ui, |ui| self.compact_nav(ui, true))
                .response
                .rect
        } else {
            egui::Panel::bottom("compact_nav")
                .frame(frame(Margin::symmetric(6, 0)).fill(theme::BG_925))
                .resizable(false)
                .exact_size(NAV_BAR)
                .show(ui, |ui| self.compact_nav(ui, false))
                .response
                .rect
        };
        self.compact_rects.sheet = Rect::NOTHING;
        if let Some(tab) = self.compact_tab {
            let sheet = frame(Margin::same(0));
            if landscape {
                let default_w = (screen.x * 0.38).clamp(280.0, 400.0);
                let min_w = 260.0;
                let max_w = (screen.x * 0.7).clamp(400.0, 640.0);
                let w = self
                    .compact_sheet_width
                    .unwrap_or(default_w)
                    .clamp(min_w, max_w);
                self.compact_rects.sheet = egui::Panel::right("compact_sheet")
                    .frame(sheet)
                    .resizable(false)
                    .exact_size(w)
                    .show(ui, |ui| {
                        self.compact_sheet_handle(ui, default_w, min_w, max_w);
                        // The content starts where the grip ends, not under it.
                        let content = ui.max_rect().with_min_x(ui.max_rect().left() + GRIP_W);
                        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                            let cap = ui.available_height();
                            self.compact_sheet(ui, gpu, tab, cap);
                        });
                    })
                    .response
                    .rect;
            } else {
                // Only as tall as what it holds, and never taller than this: the photo is
                // the subject, and keeps the rest of the screen. What it holds is measured
                // as it is laid out, so the height follows it a frame behind.
                let cap = (screen.y * 0.45).clamp(200.0, 480.0);
                // A couple of points' slack for the panel's own hairline and rounding, so
                // content that fits never shows a scroll bar.
                let h = self
                    .compact_sheet_fit
                    .map_or(cap, |fit| fit + 2.0)
                    .clamp(40.0, cap);
                let shown = egui::Panel::bottom("compact_sheet")
                    .frame(sheet)
                    .resizable(false)
                    .exact_size(h)
                    .show(ui, |ui| self.compact_sheet(ui, gpu, tab, cap));
                self.compact_rects.sheet = shown.response.rect;
                let wanted = shown.inner;
                if self
                    .compact_sheet_fit
                    .is_none_or(|fit| (fit - wanted).abs() > 0.5)
                {
                    self.compact_sheet_fit = Some(wanted);
                    ui.ctx().request_repaint();
                }
            }
        }
        self.compact_status(ui);
        self.workspace(ui);
        self.compact_rects.stage = Rect::from_min_size(
            pos2(
                self.compact_rects.top.left(),
                self.compact_rects.top.bottom(),
            ),
            self.last_panel,
        );
        self.batch_export_dialog(ui);
        self.tour(ui);
    }

    /// A thin grip down the side sheet's inner edge: drag to widen or narrow the sheet,
    /// double-tap to put it back. Upright, the sheet sizes itself to what it holds.
    fn compact_sheet_handle(
        &mut self,
        ui: &mut egui::Ui,
        default_size: f32,
        min_size: f32,
        max_size: f32,
    ) {
        let panel = ui.max_rect();
        let rect = Rect::from_min_max(
            panel.left_top(),
            pos2(panel.left() + GRIP_W, panel.bottom()),
        );
        let response = ui.interact(rect, ui.id().with("sheet_grip"), Sense::click_and_drag());
        let hot = response.hovered() || response.dragged();
        let pill =
            Rect::from_center_size(pos2(rect.left() + 4.0, rect.center().y), vec2(3.0, 32.0));
        ui.painter().rect_filled(
            pill,
            1.5,
            if hot {
                theme::TEXT_HI
            } else {
                Color32::from_gray(0x33)
            },
        );
        if hot {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if response.dragged() {
            let dx = response.drag_delta().x;
            let current = self.compact_sheet_width.unwrap_or(default_size);
            self.compact_sheet_width = Some((current - dx).clamp(min_size, max_size));
        } else if response.double_clicked() {
            self.compact_sheet_width = None;
        }
    }

    /// The mark and the photo's name, then what is wanted from any tab: zoom, fine
    /// adjust, reset, a / b, undo and redo. Returns where it was drawn.
    fn compact_top_bar(&mut self, ui: &mut egui::Ui) -> Rect {
        egui::Panel::top("compact_top")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(10, 0)),
            )
            .resizable(false)
            .exact_size(TOP_BAR)
            .show(ui, |ui| {
                // The whole bar's height up front, so every item is centred on it.
                let row = ui.available_rect_before_wrap();
                let line = egui::UiBuilder::new()
                    .max_rect(row)
                    .layout(egui::Layout::left_to_right(egui::Align::Center));
                ui.scope_builder(line, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    widgets::brand_mark(ui, 18.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        // Right to left: redo, undo, a / b, reset, fine, zoom read left to
                        // right. The icons are painted rather than taken from the fallback
                        // font, whose arrows and squares each come at their own size and
                        // height.
                        let can_redo = !self.history.future.is_empty();
                        let can_undo = !self.history.past.is_empty();
                        if ui
                            .add_enabled_ui(can_redo, |ui| {
                                bar_icon(ui, BarIcon::Redo, false, "Redo")
                            })
                            .inner
                            .clicked()
                        {
                            self.redo();
                        }
                        if ui
                            .add_enabled_ui(can_undo, |ui| {
                                bar_icon(ui, BarIcon::Undo, false, "Undo")
                            })
                            .inner
                            .clicked()
                        {
                            self.undo();
                        }
                        let loaded = self.ready_image().is_some();
                        ui.add_enabled_ui(loaded, |ui| {
                            if bar_icon(ui, BarIcon::Split, self.split_active, "Before / after")
                                .clicked()
                            {
                                self.split_active = !self.split_active;
                                self.needs_render = true;
                            }
                        });
                        ui.add_enabled_ui(loaded && self.is_stack_edited(), |ui| {
                            if bar_icon(ui, BarIcon::Reset, false, "Reset every adjustment")
                                .clicked()
                            {
                                self.reset_to_default();
                            }
                        });
                        if widgets::chip(ui, self.fine_adjust, "fine")
                            .on_hover_text("Sliders move ten times finer")
                            .clicked()
                        {
                            self.fine_adjust = !self.fine_adjust;
                        }
                        if widgets::chip(ui, self.clip_overlay, "clip")
                            .on_hover_text("Show clipping: red where blown, blue where crushed")
                            .clicked()
                        {
                            self.clip_overlay = !self.clip_overlay;
                        }
                        ui.add_enabled_ui(loaded, |ui| {
                            // One control for zoom: it reads the zoom, and flips fit ↔ 1:1.
                            let percent = self.zoom_percent(self.last_panel);
                            let at_fit = self.view.zoom <= 1.0 + 1e-3;
                            let label = match percent {
                                Some(p) if at_fit => format!("{p:.0}%"),
                                Some(p) => format!("{p:.0}%"),
                                None => "fit".to_string(),
                            };
                            if widgets::chip(ui, !at_fit, &label)
                                .on_hover_text("Fit ↔ 100%")
                                .clicked()
                            {
                                if at_fit {
                                    self.zoom_actual();
                                } else {
                                    self.view = View::default();
                                }
                            }
                        });
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let name = self
                                .loaded_path
                                .as_deref()
                                .map(file_name)
                                .unwrap_or_else(|| "DUCKFOOT".to_string());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(name)
                                        .family(theme::mono_bold())
                                        .size(12.0)
                                        .color(theme::TEXT_HI),
                                )
                                .truncate(),
                            );
                        });
                    });
                });
            })
            .response
            .rect
    }

    /// Four tabs, each an icon over its name. Tapping the open one closes its sheet;
    /// swiping along the bar moves to the next tab that way.
    fn compact_nav(&mut self, ui: &mut egui::Ui, vertical: bool) {
        let area = ui.available_rect_before_wrap();
        // Registered before the tabs, so a tap still reaches them.
        if let Some(step) = swipe(ui, area, ui.id().with("nav_swipe"), vertical) {
            let from = self.compact_tab.unwrap_or(Tab::Edit);
            if let Some(to) = from.offset(step) {
                self.compact_tab = Some(to);
            }
        }
        let n = Tab::ALL.len() as f32;
        for (i, tab) in Tab::ALL.into_iter().enumerate() {
            let i = i as f32;
            let cell = if vertical {
                let h = area.height() / n;
                Rect::from_min_size(pos2(area.left(), area.top() + i * h), vec2(area.width(), h))
            } else {
                let w = area.width() / n;
                Rect::from_min_size(
                    pos2(area.left() + i * w, area.top()),
                    vec2(w, area.height()),
                )
            };
            let response = ui.interact(cell, ui.id().with(("tab", tab.label())), Sense::click());
            let open = self.compact_tab == Some(tab);
            let colour = if open {
                theme::ACCENT
            } else if response.hovered() {
                theme::TEXT_HI
            } else {
                Color32::from_gray(0xaa)
            };
            let painter = ui.painter();
            if open {
                // A short amber bar on the edge that faces the sheet.
                let bar = if vertical {
                    Rect::from_center_size(
                        pos2(cell.left() + 1.5, cell.center().y),
                        vec2(2.0, 24.0),
                    )
                } else {
                    Rect::from_center_size(pos2(cell.center().x, cell.top() + 1.0), vec2(24.0, 2.0))
                };
                painter.rect_filled(bar, 1.5, theme::ACCENT);
            }
            let icon_centre = pos2(cell.center().x, cell.center().y - 6.0);
            paint_tab_icon(painter, icon_centre, tab, colour);
            painter.text(
                pos2(cell.center().x, cell.center().y + 13.0),
                Align2::CENTER_CENTER,
                tab.label().to_uppercase(),
                FontId::new(8.5, theme::mono_bold()),
                colour,
            );
            if response.clicked() {
                self.compact_tab = if open { None } else { Some(tab) };
            }
        }
        ui.allocate_rect(area, Sense::hover());
    }

    /// `cap` is the most height the sheet may take; it takes less when it holds less.
    /// Returns the height its content wants, scrolled parts included in full.
    fn compact_sheet(&mut self, ui: &mut egui::Ui, gpu: &Gpu, tab: Tab, cap: f32) -> f32 {
        // Edit keeps its module strip pinned and scrolls only the controls over it; every
        // other sheet scrolls as a whole.
        if tab == Tab::Edit {
            return self.edit_sheet(ui, cap);
        }
        egui::ScrollArea::vertical()
            .id_salt(("compact_sheet", tab.label()))
            .auto_shrink([false, true])
            .max_height(cap)
            .scroll_source(ScrollSource {
                drag: DragScroll::Always,
                ..Default::default()
            })
            .show(ui, |ui| {
                // An empty part of the sheet swiped sideways moves to the next tab.
                let area = ui.clip_rect();
                if let Some(step) = swipe(ui, area, ui.id().with("sheet_swipe"), false) {
                    if let Some(to) = tab.offset(step) {
                        self.compact_tab = Some(to);
                    }
                }
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        match tab {
                            Tab::Library => self.library_sheet(ui, gpu),
                            Tab::Presets => self.presets_sheet(ui),
                            Tab::Export => self.export_sheet(ui),
                            Tab::Edit => unreachable!("handled above"),
                        }
                    });
            })
            .content_size
            .y
    }

    // ── Library ─────────────────────────────────────────────────────────────────────────

    /// The roll, then one row of actions under it. Filters, select mode and the folder
    /// tree are chips in that row, and take room only while they are on.
    fn library_sheet(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        if self.folder_browser_open {
            self.shelf_folders(ui, gpu);
        } else {
            let displayed = self.displayed_indices();
            self.film_tiles(ui, gpu, &displayed);
            self.current_photo_marks(ui);
        }

        // Sorting is not filtering: only something that hides photos lights the chip.
        let filtering = self.filter
            != crate::dam::FilterState {
                sort_by: self.filter.sort_by,
                sort_descending: self.filter.sort_descending,
                ..Default::default()
            };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if widgets::button(ui, Kind::Primary, "Open", 0.0).clicked() {
                self.pick_and_open(ui.ctx());
            }
            ui.add_space(4.0);
            if widgets::chip(ui, false, "+ add").clicked() {
                self.pick_and_shelve(ui.ctx());
            }
            if widgets::chip(ui, false, "folder…").clicked() {
                self.request(
                    ui.ctx(),
                    crate::platform::Request::BrowseFolder {
                        from: self.settings.import_dir.clone(),
                    },
                );
            }
            divider(ui, 28.0);
            if widgets::chip(ui, self.folder_browser_open, "folders").clicked() {
                self.folder_browser_open = !self.folder_browser_open;
            }
            if !self.shelf.is_empty() {
                let label = if filtering { "filter •" } else { "filter" };
                if widgets::chip(ui, self.library_filters_open || filtering, label).clicked() {
                    self.library_filters_open = !self.library_filters_open;
                }
                if widgets::chip(ui, self.select_mode, "select").clicked() {
                    self.select_mode = !self.select_mode;
                }
            }
            // Offered, not imposed (DESIGN.md: no onboarding): lit until it has been seen.
            if widgets::chip(ui, !self.settings.tour_done, "? tour")
                .on_hover_text("How to get around on a phone")
                .clicked()
            {
                self.start_tour();
            }
        });
        if self.library_filters_open && !self.shelf.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                self.rating_filter(ui);
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                self.flag_filter(ui);
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                self.search_field(ui, 120.0);
                self.sort_controls(ui, false);
            });
        }
        if self.select_mode {
            self.selection_bar(ui);
        }
    }

    /// Select mode and what it acts on. A touch screen has no Ctrl or Shift, so this is
    /// how several photos get chosen for a batch export or a shared rating.
    fn selection_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if widgets::chip(ui, false, "all").clicked() {
                let all: Vec<PathBuf> = self.shelf.iter().map(|it| it.path.clone()).collect();
                self.selection.select_all(&all);
            }
            if widgets::chip(ui, false, "none").clicked() {
                self.selection.clear();
            }
            let n = self.selection.count();
            if n > 0 {
                ui.label(theme::tag(format!("{n} selected")).color(theme::ACCENT));
                if widgets::chip(ui, false, "remove").clicked() {
                    let paths: Vec<PathBuf> = self.selection.selected.iter().cloned().collect();
                    self.remove_from_shelf(&paths);
                }
            }
        });
    }

    /// Stars and flag for the open photo, or the selection when there is one. On the
    /// desktop these are keys (0–5, P / X / U) and a right-click menu.
    fn current_photo_marks(&mut self, ui: &mut egui::Ui) {
        // One selected is the open photo itself (opening selects it).
        let many = self.selection.count() > 1;
        let current = self
            .loaded_path
            .as_ref()
            .and_then(|p| self.shelf.iter().find(|it| &it.path == p))
            .map(|it| (it.path.clone(), it.metadata.rating, it.metadata.flag));
        if !many && current.is_none() {
            return;
        }
        let (rating, flag) = current
            .as_ref()
            .map(|(_, r, f)| (*r, *f))
            .unwrap_or((0, photo_engine::Flag::Unflagged));
        let mut set_rating = None;
        let mut set_flag = None;
        // One row: stars, then pick / reject, each of which a second tap takes back off.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if many {
                ui.label(theme::tag("selection").color(theme::ACCENT));
            }
            for r in 0..=5u8 {
                let label = if r == 0 {
                    "0".to_string()
                } else {
                    format!("{r}★")
                };
                if widgets::chip(ui, !many && rating == r, &label).clicked() {
                    set_rating = Some(r);
                }
            }
            ui.add_space(6.0);
            use photo_engine::Flag;
            for (f, label) in [(Flag::Pick, "pick"), (Flag::Reject, "reject")] {
                let on = !many && flag == f;
                if widgets::chip(ui, on, label).clicked() {
                    set_flag = Some(if on { Flag::Unflagged } else { f });
                }
            }
        });
        match (many, current) {
            (true, _) => {
                if let Some(r) = set_rating {
                    self.set_rating_for_selection(r);
                }
                if let Some(f) = set_flag {
                    self.set_flag_for_selection(f);
                }
            }
            (false, Some((path, ..))) => {
                if let Some(r) = set_rating {
                    self.set_rating_for_path(&path, r);
                }
                if let Some(f) = set_flag {
                    self.set_flag_for_path(&path, f);
                }
            }
            (false, None) => {}
        }
        // What the file says about it, in one line; tapped, everything it says.
        if !many {
            let line = self
                .open_info()
                .map(crate::info::one_line)
                .unwrap_or_default();
            if let (false, Some(path)) = (line.is_empty(), self.loaded_path.clone()) {
                let arrow = if self.info_open { "▾" } else { "▸" };
                let tapped = ui
                    .add(
                        egui::Label::new(theme::dim(format!("{arrow} {line}")).size(11.5))
                            .truncate()
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Everything the file says about this photo")
                    .clicked();
                if tapped {
                    self.info_open = !self.info_open;
                }
                if self.info_open {
                    egui::Frame::new()
                        .fill(theme::BG_925)
                        .corner_radius(8)
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            self.info_rows(ui, &path);
                        });
                }
            }
        }
    }

    // ── Presets ─────────────────────────────────────────────────────────────────────────

    /// A row of looks, "original" first, and — while a look is live — how strong it is.
    fn presets_sheet(&mut self, ui: &mut egui::Ui) {
        if !self.can_edit() {
            ui.label(theme::dim("Open a photo to try a look on it.").size(12.0));
            return;
        }
        ui.horizontal(|ui| {
            for (view, label) in [
                (PresetsView::Looks, "looks"),
                (PresetsView::Snapshots, "snapshots"),
                (PresetsView::History, "history"),
            ] {
                if widgets::chip(ui, self.presets_view == view, label).clicked() {
                    self.presets_view = view;
                }
            }
        });
        match self.presets_view {
            PresetsView::Looks => self.looks_body(ui, false),
            PresetsView::Snapshots => self.phone_snapshots(ui),
            PresetsView::History => self.history_rows(ui),
        }
    }

    /// The looks as tiles, and the live look's strength. `wrap` lays the tiles out in
    /// rows (the desktop inspector); otherwise they are one row to swipe (the phone).
    pub(crate) fn looks_body(&mut self, ui: &mut egui::Ui, wrap: bool) {
        // A look is live while nothing else has been edited since it was applied.
        let live = self
            .preset
            .as_ref()
            .filter(|p| p.applied == self.stack)
            .map(|p| (p.look, p.strength));
        let mut pick: Option<Option<usize>> = None;
        // The user's own presets follow the built-in looks: (apply, export, delete).
        let mut mine: (Option<usize>, Option<usize>, Option<usize>) = (None, None, None);
        let user: Vec<String> = self
            .user_presets
            .iter()
            .map(|(_, p)| p.name.clone())
            .collect();
        let tiles = |ui: &mut egui::Ui,
                     pick: &mut Option<Option<usize>>,
                     mine: &mut (Option<usize>, Option<usize>, Option<usize>)| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let original = [Color32::from_gray(0x55), Color32::from_gray(0x22)];
            if look_tile(ui, "original", original, live.is_none())
                .on_hover_text("Without a look")
                .clicked()
            {
                *pick = Some(None);
            }
            for (i, look) in LOOKS.iter().enumerate() {
                let chosen = live.is_some_and(|(l, _)| l == i);
                if look_tile(ui, look.name, look.swatch, chosen)
                    .on_hover_text(look.tip)
                    .clicked()
                {
                    *pick = Some(Some(i));
                }
            }
            let swatch = [
                Color32::from_rgb(0x5c, 0x4b, 0x2e),
                Color32::from_gray(0x26),
            ];
            for (i, name) in user.iter().enumerate() {
                // Eleven letters fit under a tile.
                let short: String = if name.chars().count() > 11 {
                    name.chars().take(10).chain(std::iter::once('…')).collect()
                } else {
                    name.clone()
                };
                let tile = look_tile(ui, &short, swatch, false).on_hover_text(format!(
                    "{name}: your preset. Right-click or hold for more."
                ));
                if tile.clicked() {
                    mine.0 = Some(i);
                }
                tile.context_menu(|ui| {
                    if ui.button("Export…").clicked() {
                        mine.1 = Some(i);
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        mine.2 = Some(i);
                        ui.close();
                    }
                });
            }
        };
        if wrap {
            ui.horizontal_wrapped(|ui| tiles(ui, &mut pick, &mut mine));
        } else {
            egui::ScrollArea::horizontal()
                .id_salt("compact_looks")
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .scroll_source(ScrollSource {
                    drag: DragScroll::Always,
                    ..Default::default()
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| tiles(ui, &mut pick, &mut mine));
                });
        }
        if let Some(i) = mine.0 {
            self.preset = None;
            self.apply_user_preset(i);
        }
        if let Some(i) = mine.1 {
            self.export_user_preset(ui.ctx(), i);
        }
        if let Some(i) = mine.2 {
            self.delete_user_preset(i);
        }

        match pick {
            Some(Some(i)) if live.map(|(l, _)| l) != Some(i) => {
                let strength = live.map_or(1.0, |(_, s)| s);
                self.apply_look(i, strength);
            }
            Some(None) if live.is_some() => {
                if let Some(p) = self.preset.take() {
                    let before = self.stack.clone();
                    self.stack = p.base;
                    self.history.checkpoint(&before);
                    self.needs_render = true;
                }
            }
            _ => {}
        }

        if let Some((look, strength)) = live {
            let mut pct = strength * 100.0;
            let before = self.stack.clone();
            let r = widgets::param_slider(ui, "strength", &mut pct, 0.0..=100.0, 0, 100.0);
            if r.changed() {
                if r.drag_started() || !r.dragged() {
                    self.history.checkpoint(&before);
                }
                self.apply_look_quietly(look, pct / 100.0);
            } else if r.drag_started() {
                self.history.checkpoint(&before);
            }
            ui.label(
                theme::dim(format!(
                    "{} — {}. Every look is an ordinary edit: refine it in Edit.",
                    LOOKS[look].name, LOOKS[look].tip
                ))
                .size(11.5),
            );
        } else if wrap {
            ui.label(theme::dim("Click a look to try it; its strength appears here.").size(11.5));
        } else {
            ui.label(theme::dim("Tap a look to try it. Swipe the row for more.").size(11.5));
        }
        self.preset_saver(ui);
    }

    /// Save this photo's look as a preset, or bring one in from a file.
    fn preset_saver(&mut self, ui: &mut egui::Ui) {
        let editable = self.can_edit();
        match self.preset_draft.take() {
            None => {
                ui.horizontal_wrapped(|ui| {
                    if widgets::chip_enabled(ui, editable, false, "save as preset")
                        .on_hover_text("Keep this photo's look to use again")
                        .clicked()
                    {
                        self.preset_draft = Some(String::new());
                    }
                    if widgets::chip(ui, false, "import…")
                        .on_hover_text("Add a .duckfoot-preset file")
                        .clicked()
                    {
                        let from = self.settings.import_dir.clone();
                        self.request(ui.ctx(), crate::platform::Request::ImportPreset { from });
                    }
                });
            }
            Some(mut name) => {
                let mut done = false;
                let mut cancel = false;
                ui.horizontal(|ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text("preset name")
                            .desired_width((ui.available_width() - 150.0).max(80.0)),
                    );
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if widgets::button_enabled(
                        ui,
                        !name.trim().is_empty(),
                        Kind::Primary,
                        "Save",
                        0.0,
                    )
                    .clicked()
                        || (enter && !name.trim().is_empty())
                    {
                        done = true;
                    }
                    if widgets::button(ui, Kind::Secondary, "Cancel", 0.0).clicked() {
                        cancel = true;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        theme::dim(format!("It carries {}.", self.paste_set.describe())).size(11.5),
                    );
                    if widgets::chip(ui, self.paste_picker_open, "what it carries").clicked() {
                        self.paste_picker_open = !self.paste_picker_open;
                    }
                });
                if self.paste_picker_open && self.layout.is_compact() {
                    self.paste_set_editor(ui);
                }
                if done {
                    self.save_user_preset(&name);
                } else if !cancel {
                    self.preset_draft = Some(name);
                }
            }
        }
    }

    /// This photo's snapshots: tap one to go back to it, hold it to delete it.
    fn phone_snapshots(&mut self, ui: &mut egui::Ui) {
        use crate::parts::{list_row, RowStyle};
        let mut recall = None;
        let mut delete = None;
        for (i, (name, stack)) in self.snapshots.iter().enumerate() {
            let style = if *stack == self.stack {
                RowStyle::Match
            } else {
                RowStyle::Normal
            };
            let row = list_row(ui, name, style, None);
            if row.clicked() {
                recall = Some(i);
            }
            row.context_menu(|ui| {
                if ui.button("Delete snapshot").clicked() {
                    delete = Some(i);
                    ui.close();
                }
            });
        }
        if self.snapshots.is_empty() {
            ui.label(
                theme::dim("None yet. A snapshot keeps this photo's edits as they are now.")
                    .size(11.5),
            );
        } else {
            ui.label(theme::dim("Tap to go back to one; hold to delete it.").size(11.5));
        }
        let ready = self.ready_image().is_some();
        let width = ui.available_width();
        if widgets::button_enabled(ui, ready, Kind::Secondary, "+ snapshot", width).clicked() {
            self.take_snapshot("");
        }
        if let Some(i) = recall {
            self.recall_snapshot(i);
        }
        if let Some(i) = delete {
            self.delete_snapshot(i);
        }
    }

    /// Apply look `i` over the photo as it is (or over what the live look was applied
    /// to), as one undo step.
    fn apply_look(&mut self, i: usize, strength: f32) {
        let before = self.stack.clone();
        self.apply_look_quietly(i, strength);
        self.history.checkpoint(&before);
    }

    /// [`Self::apply_look`] without the undo step, for a strength drag already
    /// checkpointed at its start.
    fn apply_look_quietly(&mut self, i: usize, strength: f32) {
        let base = match &self.preset {
            Some(p) if p.applied == self.stack => p.base.clone(),
            _ => self.stack.clone(),
        };
        let applied = presets::apply(&base, &LOOKS[i], strength);
        self.stack = applied.clone();
        self.needs_render = true;
        self.preset = Some(PresetState {
            look: i,
            strength,
            base,
            applied,
        });
    }

    // ── Edit ────────────────────────────────────────────────────────────────────────────

    /// The chosen module's controls over a one-line strip of every module. Tapping the
    /// chosen module again folds its controls away, leaving only the strip. Returns the
    /// height it wants, as [`Self::compact_sheet`] does.
    fn edit_sheet(&mut self, ui: &mut egui::Ui, cap: f32) -> f32 {
        // Editable from the moment a photo is chosen, before it has decoded.
        let loaded = self.can_edit();
        let body_open = !loaded || self.customize_open || self.compact_controls_open;
        let top = ui.cursor().top();
        // How much more the controls want than they were given this frame.
        let mut hidden = 0.0;
        if body_open {
            // Under the cap with the strip and the gap before it, or at the cap the sheet
            // would be that gap taller than its panel and run under the photo.
            let gap = if loaded {
                ui.spacing().item_spacing.y
            } else {
                0.0
            };
            let body = egui::ScrollArea::vertical()
                .id_salt("compact_edit")
                .auto_shrink([false, true])
                .max_height((cap - STRIP_H - 1.0 - gap).max(80.0))
                .scroll_source(ScrollSource {
                    drag: DragScroll::Always,
                    ..Default::default()
                })
                .show(ui, |ui| {
                    // A sideways flick on anything that is not a control moves module.
                    if loaded && !self.customize_open {
                        let area = ui.clip_rect();
                        if let Some(step) = swipe(ui, area, ui.id().with("module_swipe"), false) {
                            self.step_module(step);
                        }
                    }
                    egui::Frame::new()
                        .inner_margin(Margin {
                            left: 12,
                            right: 12,
                            top: 4,
                            bottom: 6,
                        })
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            if self.customize_open {
                                self.compact_modules_list(ui);
                            } else if loaded {
                                self.single_module(ui, self.compact_module);
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label(theme::dim("Open a photo to edit it.").size(12.0));
                                    if widgets::button(ui, Kind::Primary, "Open", 0.0).clicked() {
                                        self.pick_and_open(ui.ctx());
                                    }
                                });
                            }
                        });
                });
            hidden = (body.content_size.y - body.inner_rect.height()).max(0.0);
        }
        if loaded {
            if body_open {
                let y = ui.cursor().top();
                ui.painter()
                    .hline(ui.max_rect().x_range(), y, Stroke::new(1.0, theme::BORDER));
            }
            self.module_strip(ui);
        }
        ui.cursor().top() - top + hidden
    }

    /// The modules the phone's strip shows, in order.
    fn compact_ids(&self) -> Vec<InspectorSectionId> {
        self.compact_sections
            .iter()
            .filter(|s| s.visible)
            .map(|s| s.id)
            .collect()
    }

    /// Move to the next (`+1`) or previous (`-1`) module, stopping at the ends.
    fn step_module(&mut self, step: i32) {
        let ids = self.compact_ids();
        let Some(i) = ids.iter().position(|id| *id == self.compact_module) else {
            return;
        };
        let j = i as i32 + step;
        if let Some(next) = usize::try_from(j).ok().and_then(|j| ids.get(j)) {
            self.compact_module = *next;
            self.compact_controls_open = true;
            self.focused_section = Some(*next);
            // Bring its name into view in the strip.
            self.reveal_section = Some(*next);
        }
    }

    /// Turn, then every module by name, then the list that chooses them — one scrolling
    /// line.
    fn module_strip(&mut self, ui: &mut egui::Ui) {
        let ids = self.compact_ids();
        if !ids.contains(&self.compact_module) {
            if let Some(first) = ids.first() {
                self.compact_module = *first;
            }
        }
        let defaults = self.baseline.clone();
        let white = self.white_scale();
        egui::ScrollArea::horizontal()
            .id_salt("compact_strip")
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .scroll_source(ScrollSource {
                drag: DragScroll::Always,
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(4.0);
                    // Framing a crop: finishing it comes before anything else.
                    if self.cropping {
                        if strip_tab(ui, "apply", true, true, None).clicked() {
                            self.apply_crop();
                        }
                        if strip_tab(ui, "× cancel", false, false, None).clicked() {
                            self.cancel_crop();
                        }
                        divider(ui, STRIP_H);
                        for (aspect, label) in crate::crop_tool::Aspect::CHOICES {
                            if strip_tab(ui, label, self.crop_aspect == aspect, false, None)
                                .clicked()
                            {
                                self.set_crop_aspect(aspect);
                            }
                        }
                        if strip_icon(ui, "⇄", "Turn the crop's shape a quarter").clicked() {
                            self.swap_crop_aspect();
                        }
                        divider(ui, STRIP_H);
                    }
                    if strip_icon(ui, "↶", "Turn 90° anticlockwise").clicked() {
                        self.quick_turn(false);
                    }
                    if strip_icon(ui, "↷", "Turn 90° clockwise").clicked() {
                        self.quick_turn(true);
                    }
                    divider(ui, STRIP_H);
                    for id in ids {
                        let (_, modified) = summarise(id, &self.stack, &defaults, &white);
                        let dot = (id.is_on(&self.stack) && modified).then_some(theme::ACCENT);
                        let chosen = self.compact_module == id && !self.customize_open;
                        let open = chosen && self.compact_controls_open;
                        let response = strip_tab(ui, id.short_name(), chosen, open, dot)
                            .on_hover_text(id.title());
                        if chosen && self.reveal_section == Some(id) {
                            response.scroll_to_me(Some(egui::Align::Center));
                            self.reveal_section = None;
                        }
                        if response.clicked() {
                            if chosen {
                                self.compact_controls_open = !self.compact_controls_open;
                            } else {
                                self.compact_module = id;
                                self.compact_controls_open = true;
                                self.customize_open = false;
                            }
                            self.focused_section = Some(id);
                        }
                    }
                    divider(ui, STRIP_H);
                    if strip_tab(
                        ui,
                        "＋ modules",
                        self.customize_open,
                        self.customize_open,
                        None,
                    )
                    .on_hover_text("Choose which modules show, and their order")
                    .clicked()
                    {
                        self.customize_open = !self.customize_open;
                    }
                    ui.add_space(8.0);
                });
            });
    }

    /// Every module with a switch for whether the strip shows it, and arrows to reorder.
    /// The scope and filmic start off here: flat on a phone, and a switch away.
    fn compact_modules_list(&mut self, ui: &mut egui::Ui) {
        self.ui_scale_row(ui);
        ui.add_space(4.0);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(theme::tag("modules in the strip").color(theme::TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::chip(ui, false, "done").clicked() {
                    self.customize_open = false;
                }
                if widgets::chip(ui, false, "defaults").clicked() {
                    self.compact_sections = crate::settings::default_compact_sections();
                    changed = true;
                }
            });
        });
        let total = self.compact_sections.len();
        let mut swap = None;
        for (i, sec) in self.compact_sections.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if widgets::switch(ui, &mut sec.visible).changed() {
                    changed = true;
                }
                let colour = if sec.visible {
                    theme::TEXT_HI
                } else {
                    theme::TEXT_DIM
                };
                ui.label(
                    RichText::new(sec.id.title())
                        .family(theme::sans_strong())
                        .size(13.0)
                        .color(colour),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(i + 1 < total, |ui| {
                        if widgets::icon(ui, "↓", false, "Later in the strip").clicked() {
                            swap = Some((i, i + 1));
                        }
                    });
                    ui.add_enabled_ui(i > 0, |ui| {
                        if widgets::icon(ui, "↑", false, "Earlier in the strip").clicked() {
                            swap = Some((i, i - 1));
                        }
                    });
                });
            });
        }
        if let Some((a, b)) = swap {
            self.compact_sections.swap(a, b);
            changed = true;
        }
        if changed {
            self.settings.compact_sections = Some(self.compact_sections.clone());
            self.save_settings();
        }
    }

    /// One module, always open: its header, then its controls. The same checkpointing as
    /// the desktop's pipeline, so a gesture is one undo step.
    fn single_module(&mut self, ui: &mut egui::Ui, id: InspectorSectionId) {
        let snapshot = self.stack.clone();
        let mut changed = false;
        let mut started = false;
        if self.compact_module_header(ui, id) {
            changed = true;
            started = true;
        }
        let mut body_changed = false;
        self.module_body(ui, id, &mut body_changed, &mut started);
        if body_changed {
            // As on the desktop: touching a control turns its module on.
            if let Some(on) = id.enabled_mut(&mut self.stack) {
                *on = true;
            }
        }
        changed |= body_changed;
        if changed {
            self.focused_section = Some(id);
            self.needs_render = true;
        }
        if started {
            self.history.checkpoint(&snapshot);
        }
    }

    /// `[on/off]  title  summary   [⟲] [‹] [›]`. The switch is thumb-sized; ⟲ puts just this
    /// module back and shows only once it has been changed; ‹ › step through the modules,
    /// as a swipe does. True when the module's state changed.
    fn compact_module_header(&mut self, ui: &mut egui::Ui, id: InspectorSectionId) -> bool {
        let defaults = self.baseline.clone();
        let white = self.white_scale();
        let (summary, modified) = summarise(id, &self.stack, &defaults, &white);
        let on = id.is_on(&self.stack);
        let ids = self.compact_ids();
        let at = ids.iter().position(|i| *i == id);
        let mut changed = false;
        let mut step = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if let Some(enabled) = id.enabled_mut(&mut self.stack) {
                if widgets::switch(ui, enabled).changed() {
                    changed = true;
                }
            }
            ui.label(
                RichText::new(id.title())
                    .family(theme::sans_strong())
                    .size(14.0)
                    .color(if on { theme::ACCENT } else { theme::TEXT_DIM }),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let last = at.is_some_and(|i| i + 1 >= ids.len());
                if ui
                    .add_enabled_ui(!last, |ui| widgets::icon(ui, "›", false, "Next module"))
                    .inner
                    .clicked()
                {
                    step = Some(1);
                }
                let first = at.is_none_or(|i| i == 0);
                if ui
                    .add_enabled_ui(!first, |ui| {
                        widgets::icon(ui, "‹", false, "Previous module")
                    })
                    .inner
                    .clicked()
                {
                    step = Some(-1);
                }
                if modified && widgets::icon(ui, "⟲", false, "Reset this module").clicked() {
                    self.reset_module(id);
                    changed = true;
                }
                ui.add_space(6.0);
                let colour = if !on {
                    theme::TEXT_FAINT
                } else if modified {
                    theme::ACCENT
                } else {
                    theme::TEXT_DIM
                };
                ui.add(egui::Label::new(theme::num(summary).size(11.0).color(colour)).truncate());
            });
        });
        if let Some(step) = step {
            self.step_module(step);
        }
        changed
    }

    /// Put one module back as the photo opened. Crop & rotate returns to the camera's
    /// orientation, uncropped.
    fn reset_module(&mut self, id: InspectorSectionId) {
        id.reset(&mut self.stack, &self.baseline);
        if id == InspectorSectionId::Geometry {
            self.cropping = false;
        }
    }

    /// Where this photo's edits stand, and what can be done to them as a whole: the
    /// phone's counterpart of the desktop's sidecar footer and filmstrip actions.
    fn edits_block(&mut self, ui: &mut egui::Ui) {
        use widgets::button_enabled as button;
        let editable = self.can_edit();
        let (text, colour) = if !editable {
            ("no photo to edit".to_string(), theme::TEXT_FAINT)
        } else if self.sidecar_locked {
            ("edits here are not being saved".to_string(), theme::DANGER)
        } else if self.saved_stack.is_some() || self.is_stack_edited() {
            ("edits save automatically".to_string(), theme::TEXT_DIM)
        } else {
            ("no edits yet".to_string(), theme::TEXT_FAINT)
        };
        ui.label(
            RichText::new(text)
                .family(theme::mono_bold())
                .size(11.5)
                .color(colour),
        );
        ui.add_space(4.0);
        let has_saved = self.saved_stack.is_some();
        let has_clip = self.clipboard_stack.is_some();
        ui.horizontal_wrapped(|ui| {
            if button(ui, editable && has_saved, Kind::Secondary, "Revert", 0.0).clicked() {
                self.revert_to_sidecar();
            }
            if button(ui, editable, Kind::Secondary, "Reset all", 0.0).clicked() {
                self.reset_to_default();
            }
            if button(ui, editable, Kind::Secondary, "Copy look", 0.0).clicked() {
                self.copy_adjustments();
            }
            if button(ui, editable && has_clip, Kind::Secondary, "Paste", 0.0).clicked() {
                self.paste_adjustments();
            }
            if button(
                ui,
                self.can_same_as_previous(),
                Kind::Secondary,
                "Same as previous",
                0.0,
            )
            .clicked()
            {
                self.same_as_previous();
            }
            if self.can_undo_paste_to_all() {
                if button(ui, true, Kind::Secondary, "Undo paste", 0.0).clicked() {
                    self.undo_paste_to_all();
                }
            } else {
                let label = format!("Paste to all {}", self.shelf.len());
                if button(
                    ui,
                    has_clip && !self.shelf.is_empty(),
                    Kind::Secondary,
                    &label,
                    0.0,
                )
                .clicked()
                {
                    self.paste_to_all_in_shelf();
                }
            }
            if widgets::chip(ui, self.paste_picker_open, "what pastes").clicked() {
                self.paste_picker_open = !self.paste_picker_open;
            }
        });
        if self.paste_picker_open {
            egui::Frame::new()
                .fill(theme::BG_925)
                .corner_radius(8)
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.paste_set_editor(ui);
                });
        }
    }

    /// A quarter turn from the Edit strip, as one undo step.
    fn quick_turn(&mut self, clockwise: bool) {
        if clockwise {
            self.geometry_edit(photo_engine::Geometry::rotate_right);
        } else {
            self.geometry_edit(photo_engine::Geometry::rotate_left);
        }
    }

    // ── Export ──────────────────────────────────────────────────────────────────────────

    fn export_sheet(&mut self, ui: &mut egui::Ui) {
        self.edits_block(ui);
        ui.add_space(8.0);
        // What the export will be, and the settings behind it a tap away.
        let source = self.export_source_size();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(crate::export_ui::summary(&self.export_settings, source))
                    .family(theme::mono_bold())
                    .size(11.0)
                    .color(theme::ACCENT),
            );
            if widgets::chip(ui, self.export_options_open, "options").clicked() {
                self.export_options_open = !self.export_options_open;
            }
        });
        if self.export_options_open {
            let mut settings = self.export_settings;
            crate::export_ui::settings_editor(ui, &mut settings, source);
            self.export_settings = settings;
        }
        let busy = matches!(self.export, Export::Writing { .. });
        let ready = self.ready_image().is_some() && !busy;
        let width = ui.available_width();
        let export = ui
            .add_enabled_ui(ready, |ui| {
                widgets::button(ui, Kind::Primary, "Export this photo", width)
            })
            .inner;
        if export.clicked() {
            self.pick_and_export(ui.ctx());
        }

        // Real-time export feedback in the sheet
        match &self.export {
            Export::Writing { path, started, .. } => {
                ui.horizontal(|ui| {
                    widgets::dot(ui, theme::ACCENT);
                    ui.label(
                        RichText::new(format!(
                            "writing {} ({:.1}s)…",
                            file_name(path),
                            started.elapsed().as_secs_f32()
                        ))
                        .family(theme::mono_bold())
                        .size(11.5)
                        .color(theme::ACCENT),
                    );
                });
            }
            Export::Done { path, seconds } => {
                ui.horizontal(|ui| {
                    widgets::dot(ui, theme::LIME);
                    ui.label(
                        RichText::new(format!("exported {} in {seconds:.1}s", file_name(path)))
                            .family(theme::mono_bold())
                            .size(11.5)
                            .color(theme::LIME),
                    );
                });
            }
            Export::Failed(e) => {
                ui.horizontal(|ui| {
                    widgets::dot(ui, theme::DANGER);
                    ui.label(
                        RichText::new(format!("failed: {e}"))
                            .family(theme::mono_bold())
                            .size(11.5)
                            .color(theme::DANGER),
                    );
                });
            }
            Export::Idle => {}
        }

        let n = self.selection.count();
        let label = if n > 1 {
            format!("Batch export {n} selected…")
        } else {
            "Batch export…".to_string()
        };
        if widgets::button(ui, Kind::Accent, &label, width).clicked() {
            self.batch_dialog_open = true;
        }
        ui.label(theme::dim("To export several, choose them with select in Library.").size(11.5));
    }

    /// One line over the stage when there is something to say: an error, a notice, or an
    /// export being written / completed. The desktop's status bar has no room here.
    fn compact_status(&mut self, ui: &mut egui::Ui) {
        let mut action = None;
        let (text, colour) = if let Some(err) = &self.error {
            (err.clone(), theme::DANGER)
        } else if let Export::Writing { path, .. } = &self.export {
            (format!("writing {}", file_name(path)), theme::ACCENT)
        } else if let Some((msg, undo, left)) = self.live_notice() {
            // A finished export arrives here as a notice, so it comes down with the rest
            // rather than holding the line until the next export.
            // Wake to take the notice down when its time is up.
            ui.ctx().request_repaint_after(left);
            action = undo;
            (msg.to_string(), theme::ACCENT)
        } else {
            return;
        };
        let mut run = None;
        egui::Panel::bottom("compact_status")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_925)
                    .inner_margin(Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::dot(ui, colour);
                    if let Some(a) = action {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::chip(ui, true, "undo").clicked() {
                                run = Some(a);
                            }
                            ui.add(
                                egui::Label::new(theme::num(&text).size(11.5).color(colour))
                                    .truncate(),
                            );
                        });
                    } else {
                        ui.add(
                            egui::Label::new(theme::num(text).size(11.5).color(colour)).truncate(),
                        );
                    }
                });
            });
        if let Some(a) = run {
            self.run_notice_action(a);
        }
    }
    // ── Tour ────────────────────────────────────────────────────────────────────────────

    /// The tour's current step over everything else: the rest of the screen dimmed, the
    /// part it is about outlined, and a card saying how to use it.
    fn tour(&mut self, ui: &mut egui::Ui) {
        let Some(step) = self.tour_step else { return };
        let Some(s) = TOUR.get(step) else {
            self.end_tour();
            return;
        };
        if let Some(tab) = s.tab {
            self.compact_tab = Some(tab);
        }
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        let r = self.compact_rects;
        let target = match s.target {
            Target::Nav => r.nav,
            Target::Sheet => r.sheet,
            Target::Top => r.top,
            Target::Stage => r.stage,
        }
        .intersect(screen);
        let mut next = false;
        let mut skip = false;
        egui::Area::new(egui::Id::new("duckfoot_tour"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(&ctx, |ui| {
                // Nothing under the tour takes a touch while it shows.
                ui.allocate_rect(screen, Sense::click_and_drag());
                let painter = ui.painter();
                let dim = Color32::from_black_alpha(190);
                if target.is_positive() {
                    for band in [
                        Rect::from_min_max(screen.min, pos2(screen.max.x, target.min.y)),
                        Rect::from_min_max(pos2(screen.min.x, target.max.y), screen.max),
                        Rect::from_min_max(
                            pos2(screen.min.x, target.min.y),
                            pos2(target.min.x, target.max.y),
                        ),
                        Rect::from_min_max(
                            pos2(target.max.x, target.min.y),
                            pos2(screen.max.x, target.max.y),
                        ),
                    ] {
                        painter.rect_filled(band, 0, dim);
                    }
                    painter.rect_stroke(
                        target.shrink(1.0),
                        4,
                        Stroke::new(2.0, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    painter.rect_filled(screen, 0, dim);
                }

                // The card sits on whichever side of the target has more room.
                let w = (screen.width() - 32.0).min(360.0);
                let h_id = egui::Id::new("duckfoot_tour_card_h");
                let h: f32 = ui.data(|d| d.get_temp(h_id)).unwrap_or(160.0);
                let above = target.center().y > screen.center().y;
                let y = if !target.is_positive() {
                    screen.center().y - h / 2.0
                } else if above {
                    target.top() - 12.0 - h
                } else {
                    target.bottom() + 12.0
                };
                let y = y.clamp(screen.top() + 8.0, (screen.bottom() - h - 8.0).max(8.0));
                let card = Rect::from_min_size(pos2(screen.center().x - w / 2.0, y), vec2(w, h));
                let shown = ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                    egui::Frame::new()
                        .fill(theme::BG_900)
                        .stroke(Stroke::new(1.0, theme::ACCENT.linear_multiply(0.6)))
                        .corner_radius(8)
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.set_width(w - 28.0);
                            ui.spacing_mut().item_spacing.y = 6.0;
                            ui.label(
                                RichText::new(format!("{}/{}", step + 1, TOUR.len()))
                                    .family(theme::mono_bold())
                                    .size(10.0)
                                    .color(theme::TEXT_FAINT),
                            );
                            ui.label(
                                RichText::new(s.title)
                                    .family(theme::sans_strong())
                                    .size(16.0)
                                    .color(theme::TEXT_HI),
                            );
                            ui.label(RichText::new(s.text).size(13.0).color(theme::TEXT));
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                let last = step + 1 == TOUR.len();
                                if widgets::chip(ui, false, "skip").clicked() {
                                    skip = true;
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let label = if last { "Done" } else { "Next" };
                                        if widgets::button(ui, Kind::Primary, label, 72.0).clicked()
                                        {
                                            next = true;
                                        }
                                    },
                                );
                            });
                        })
                        .response
                        .rect
                        .height()
                });
                if (shown.inner - h).abs() > 0.5 {
                    ui.data_mut(|d| d.insert_temp(h_id, shown.inner));
                    ui.ctx().request_repaint();
                }
            });
        if skip {
            self.end_tour();
        } else if next {
            if step + 1 >= TOUR.len() {
                self.end_tour();
            } else {
                self.tour_step = Some(step + 1);
            }
        }
    }

    /// Where the phone layout's sheet was drawn last frame (nothing while it is closed).
    pub fn sheet_rect(&self) -> Rect {
        self.compact_rects.sheet
    }

    /// Close the tour and remember it was seen.
    pub(crate) fn end_tour(&mut self) {
        self.tour_step = None;
        if !self.settings.tour_done {
            self.settings.tour_done = true;
            self.save_settings();
        }
    }
}

/// What a tour step points at.
#[derive(Clone, Copy)]
enum Target {
    Nav,
    Sheet,
    Top,
    Stage,
}

struct TourStep {
    title: &'static str,
    text: &'static str,
    target: Target,
    /// The tab to open for the step, if it is about one.
    tab: Option<Tab>,
}

const TOUR: [TourStep; 6] = [
    TourStep {
        title: "Four tabs",
        text: "Library, Presets, Edit and Export. Swipe along this bar to move between them, \
               or tap one. Tap the open tab again to hide its sheet and see the whole photo.",
        target: Target::Nav,
        tab: None,
    },
    TourStep {
        title: "Edit, one module at a time",
        text: "Swipe the controls sideways — or tap ‹ › — for the next module. Tap a name in \
               the strip to jump straight to it; tap it again to fold the controls away.",
        target: Target::Sheet,
        tab: Some(Tab::Edit),
    },
    TourStep {
        title: "On, off, back",
        text: "The switch turns a module on or off, and ⟲ beside ‹ › puts just that module \
               back. ＋ modules at the end of the strip chooses which show: the scope and \
               filmic are in there, off to start.",
        target: Target::Sheet,
        tab: Some(Tab::Edit),
    },
    TourStep {
        title: "Fine and reset, on every tab",
        text: "FINE makes every slider ten times finer. ⟲ up here resets the whole photo, \
               and undo and redo sit beside it. Double-tap any slider to reset just that one.",
        target: Target::Top,
        tab: None,
    },
    TourStep {
        title: "Presets",
        text: "One tap for sepia, mono, film and more, with a strength slider. A look is an \
               ordinary edit, so you can keep refining it in Edit.",
        target: Target::Sheet,
        tab: Some(Tab::Presets),
    },
    TourStep {
        title: "The photo",
        text: "Pinch to zoom, and swipe across the photo for the next one. This tour is \
               under Library → ? tour whenever you want it again.",
        target: Target::Stage,
        tab: None,
    },
];

/// The top bar's icons.
#[derive(Clone, Copy)]
enum BarIcon {
    Undo,
    Redo,
    Split,
    Reset,
}

/// A top-bar icon: painted strokes in a 32 × 32 target, all one size and centred on the
/// bar's line, as the font's glyphs are not.
fn bar_icon(ui: &mut egui::Ui, icon: BarIcon, active: bool, tip: &str) -> egui::Response {
    let enabled = ui.is_enabled();
    let (rect, response) = widgets::allocate_control(ui, vec2(32.0, 32.0), Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, tip)
    });
    let hovered = enabled && response.hovered();
    let colour = match (enabled, active, hovered) {
        (false, ..) => theme::TEXT_FAINT,
        (true, true, _) => theme::ACCENT,
        (true, false, true) => theme::TEXT_HI,
        (true, false, false) => Color32::from_gray(0xcc),
    };
    let painter = ui.painter();
    if hovered || active {
        painter.rect_filled(rect, 3, theme::BG_850);
    }
    let c = rect.center();
    let stroke = Stroke::new(1.6, colour);
    match icon {
        // A hook over the top, arrowhead down at the end it turns back to.
        BarIcon::Undo => arc_arrow(painter, c + vec2(0.0, 1.5), 5.5, 30.0, -180.0, stroke),
        BarIcon::Redo => arc_arrow(painter, c + vec2(0.0, 1.5), 5.5, 150.0, 360.0, stroke),
        // Most of a circle, turning back on itself.
        BarIcon::Reset => arc_arrow(painter, c, 6.0, 200.0, -95.0, stroke),
        // Before | after.
        BarIcon::Split => {
            let frame = Rect::from_center_size(c, vec2(15.0, 12.0));
            painter.rect_stroke(frame, 1.5, stroke, egui::StrokeKind::Middle);
            painter.vline(c.x, frame.y_range(), stroke);
            let left = Rect::from_min_max(frame.min, pos2(c.x, frame.max.y)).shrink(2.0);
            painter.rect_filled(left, 0.5, colour.gamma_multiply(0.45));
        }
    }
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(tip)
}

/// An arc about `c` from `from` to `to` degrees (screen angles: 0 is right, 90 down),
/// with an arrowhead at `to` pointing the way the arc runs.
fn arc_arrow(painter: &Painter, c: egui::Pos2, r: f32, from: f32, to: f32, stroke: Stroke) {
    let at = |deg: f32| {
        let a = deg.to_radians();
        c + vec2(a.cos(), a.sin()) * r
    };
    let steps = 24;
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|i| at(from + (to - from) * i as f32 / steps as f32))
        .collect();
    let end = at(to);
    let dir = (end - at(to - (to - from).signum() * 8.0)).normalized();
    let perp = vec2(-dir.y, dir.x);
    let tip = end + dir * 2.0;
    painter.add(egui::Shape::line(points, stroke));
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            tip - dir * 4.5 + perp * 3.2,
            tip - dir * 4.5 - perp * 3.2,
        ],
        stroke.color,
        Stroke::NONE,
    ));
}

/// A sideways flick across `rect` that no control under it claimed: `+1` for a flick
/// towards the start (left, or up when `vertical`), meaning "next", `-1` the other way.
/// Registered before the controls it sits under, so they keep their own touches. A drag
/// the other way scrolls whatever scroll area it is in, as a drag on its background
/// would have.
fn swipe(ui: &mut egui::Ui, rect: Rect, id: egui::Id, vertical: bool) -> Option<i32> {
    let r = ui.interact(rect, id, Sense::drag());
    let travel_id = id.with("travel");
    let along = |v: egui::Vec2| if vertical { v.y } else { v.x };
    let across = |v: egui::Vec2| if vertical { v.x } else { v.y };
    if r.dragged() {
        let d = r.drag_delta();
        let travel: egui::Vec2 = ui
            .data(|m| m.get_temp::<egui::Vec2>(travel_id))
            .unwrap_or_default()
            + d;
        ui.data_mut(|m| m.insert_temp(travel_id, travel));
        // Decided by where the gesture has gone so far, so a wobble does not flip it.
        if !vertical && across(travel).abs() > along(travel).abs() {
            ui.scroll_with_delta(vec2(0.0, d.y));
        }
    }
    if r.drag_stopped() {
        let travel: egui::Vec2 = ui
            .data_mut(|m| m.remove_temp::<egui::Vec2>(travel_id))
            .unwrap_or_default();
        if along(travel).abs() > SWIPE && along(travel).abs() > 1.5 * across(travel).abs() {
            return Some(if along(travel) < 0.0 { 1 } else { -1 });
        }
    }
    None
}

/// A look in the Presets row: its two colours over its name, outlined when chosen.
fn look_tile(ui: &mut egui::Ui, name: &str, swatch: [Color32; 2], chosen: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(72.0, 70.0), Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            true,
            chosen,
            format!("look: {name}"),
        )
    });
    let painter = ui.painter();
    let chip = Rect::from_min_size(rect.min + vec2(4.0, 2.0), vec2(64.0, 44.0));
    // Two flat halves, not a blend: DESIGN.md keeps gradients off everything next to the
    // photo, and this row sits right under it.
    let (top, bottom) = chip.split_top_bottom_at_fraction(0.5);
    painter.rect_filled(
        top,
        egui::CornerRadius {
            nw: 4,
            ne: 4,
            sw: 0,
            se: 0,
        },
        swatch[0],
    );
    painter.rect_filled(
        bottom,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: 4,
            se: 4,
        },
        swatch[1],
    );
    let outline = if chosen {
        Stroke::new(2.0, theme::ACCENT)
    } else if response.hovered() {
        Stroke::new(1.0, theme::BORDER_HI)
    } else {
        Stroke::new(1.0, theme::BORDER)
    };
    painter.rect_stroke(chip, 4, outline, egui::StrokeKind::Outside);
    let colour = if chosen {
        theme::ACCENT
    } else {
        Color32::from_gray(0xcc)
    };
    let galley = painter.layout(
        name.to_uppercase(),
        FontId::new(8.5, theme::mono_bold()),
        colour,
        rect.width(),
    );
    painter.galley(
        pos2(rect.center().x - galley.size().x / 2.0, chip.bottom() + 5.0),
        galley,
        colour,
    );
    response
}

/// A name in the Edit strip: amber when it is the chosen one, underlined while its
/// controls are open, with a dot when the module changes the photo.
fn strip_tab(
    ui: &mut egui::Ui,
    text: &str,
    chosen: bool,
    open: bool,
    dot: Option<Color32>,
) -> egui::Response {
    let enabled = ui.is_enabled();
    let galley = ui.painter().layout_no_wrap(
        text.to_uppercase(),
        FontId::new(11.0, theme::mono_bold()),
        Color32::PLACEHOLDER,
    );
    let pad = 10.0;
    let (rect, response) = widgets::allocate_control(
        ui,
        vec2(galley.size().x + 2.0 * pad, STRIP_H),
        Sense::click(),
    );
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, chosen, text)
    });
    let colour = if !enabled {
        theme::TEXT_FAINT
    } else if chosen {
        theme::ACCENT
    } else if response.hovered() {
        theme::TEXT_HI
    } else {
        Color32::from_gray(0xa8)
    };
    let painter = ui.painter();
    let text_rect = Rect::from_center_size(rect.center(), galley.size());
    painter.galley(text_rect.min, galley, colour);
    if open {
        let bar = Rect::from_min_max(
            pos2(text_rect.left(), rect.bottom() - 3.0),
            pos2(text_rect.right(), rect.bottom() - 1.0),
        );
        painter.rect_filled(bar, 1.0, theme::ACCENT);
    }
    if let Some(dot) = dot {
        painter.circle_filled(
            pos2(text_rect.right() + 4.0, text_rect.top() + 1.0),
            2.0,
            dot,
        );
    }
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// A glyph in the Edit strip, for an action rather than a module.
fn strip_icon(ui: &mut egui::Ui, glyph: &str, tip: &str) -> egui::Response {
    let (rect, response) = widgets::allocate_control(ui, vec2(34.0, STRIP_H), Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip)
    });
    let colour = if response.hovered() {
        theme::TEXT_HI
    } else {
        Color32::from_gray(0xcc)
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(17.0),
        colour,
    );
    response.on_hover_text(tip)
}

/// A short hairline between groups in a row.
fn divider(ui: &mut egui::Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, height), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        Rect::from_center_size(rect.center(), vec2(1.0, height * 0.5)).y_range(),
        Stroke::new(1.0, theme::BORDER_HI),
    );
}

/// A module's name in the Edit strip: short enough that several fit across a phone.
/// The tab icons, painted rather than taken from a font: the bundled fonts have no glyph
/// for sliders or a scope, and painted strokes match the rest of the controls.
fn paint_tab_icon(painter: &Painter, c: egui::Pos2, tab: Tab, colour: Color32) {
    let stroke = Stroke::new(1.6, colour);
    match tab {
        // A two-by-two grid of prints.
        Tab::Library => {
            for (dx, dy) in [(-5.0, -5.0), (5.0, -5.0), (-5.0, 5.0), (5.0, 5.0)] {
                painter.rect_stroke(
                    Rect::from_center_size(c + vec2(dx, dy), vec2(8.0, 8.0)),
                    1.5,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
        }
        // Three overlapping swatches: looks.
        Tab::Presets => {
            for (dx, dy) in [(-4.0, 3.0), (4.0, 3.0), (0.0, -4.0)] {
                painter.circle_stroke(c + vec2(dx, dy), 5.5, stroke);
            }
        }
        // Three sliders at different settings.
        Tab::Edit => {
            for (row, knob) in [(-6.0, -3.0), (0.0, 4.0), (6.0, -6.0)] {
                painter.line_segment([c + vec2(-9.0, row), c + vec2(9.0, row)], stroke);
                painter.circle_filled(c + vec2(knob, row), 2.6, colour);
            }
        }
        // An arrow rising out of a tray.
        Tab::Export => {
            painter.line_segment([c + vec2(0.0, 4.0), c + vec2(0.0, -8.0)], stroke);
            painter.line_segment([c + vec2(-4.0, -4.0), c + vec2(0.0, -8.0)], stroke);
            painter.line_segment([c + vec2(4.0, -4.0), c + vec2(0.0, -8.0)], stroke);
            painter.line_segment([c + vec2(-8.0, 2.0), c + vec2(-8.0, 8.0)], stroke);
            painter.line_segment([c + vec2(-8.0, 8.0), c + vec2(8.0, 8.0)], stroke);
            painter.line_segment([c + vec2(8.0, 8.0), c + vec2(8.0, 2.0)], stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktops_keep_the_desktop_layout_and_phones_get_the_bar() {
        let layout = |w, h| Layout::for_size(vec2(w, h));
        assert_eq!(layout(1440.0, 900.0), Layout::Desktop);
        // The desktop window's minimum size.
        assert_eq!(layout(900.0, 600.0), Layout::Desktop);
        // A tablet on its side keeps the desktop layout; upright it does not fit.
        assert_eq!(layout(1280.0, 800.0), Layout::Desktop);
        assert_eq!(layout(800.0, 1280.0), Layout::Portrait);
        assert_eq!(layout(412.0, 892.0), Layout::Portrait);
        assert_eq!(layout(892.0, 412.0), Layout::Landscape);
    }
}
