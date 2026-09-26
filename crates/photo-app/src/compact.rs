//! The phone layout: the stage fills the screen, one sheet at a time sits beside it, and
//! a navigation bar picks the sheet. It is the same app as the desktop layout — every
//! sheet is built from the same pieces the desktop's panels are — only arranged for a
//! small, touch-driven screen.
//!
//! Upright, the bar runs along the bottom and the sheet rises above it, only as tall as
//! what it holds. Turned sideways
//! there is little height to spare, so the bar becomes a rail down the right edge and the
//! sheet opens beside it. Tapping the open tab again closes the sheet and gives the stage
//! the whole screen.
//!
//! | Tab | Holds |
//! |---|---|
//! | Library | the roll's thumbnails and the current photo's rating and flag, over one row of actions: open, add, import a folder, and chips for the folder tree, filters and select mode |
//! | Edit | the chosen module's controls over a one-line strip: turn, every module by name, fine adjust, reset, choose modules |
//! | Export | the single export and batch export |
//!
//! Opening a photo from a picker goes straight to Edit: on a phone the photo is the point,
//! and the roll is only how you got to it.

use egui::containers::scroll_area::{DragScroll, ScrollSource};
use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Margin, Painter, Rect, RichText, Sense, Stroke,
};

use crate::widgets::{self, Kind};
use crate::Gpu;
use crate::{
    file_name, module_enabled, module_on, summarise, theme, Export, InspectorSectionId, PhotoApp,
    View,
};
use std::path::PathBuf;

/// Which sheet is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Library,
    Edit,
    Export,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Library, Tab::Edit, Tab::Export];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Library => "library",
            Tab::Edit => "edit",
            Tab::Export => "export",
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
const NAV_RAIL: f32 = 56.0;
/// Height of the Edit sheet's module strip.
const STRIP_H: f32 = 34.0;

impl PhotoApp {
    pub(crate) fn compact_layout(&mut self, ui: &mut egui::Ui, gpu: &Gpu, screen: egui::Vec2) {
        let landscape = self.layout == Layout::Landscape;
        let frame = |margin: Margin| egui::Frame::new().fill(theme::BG_950).inner_margin(margin);

        // Panel order is layout order: the top bar, then the navigation, then the sheet
        // against it, then the status line, and the stage takes what is left.
        self.compact_top_bar(ui);
        if landscape {
            egui::Panel::right("compact_nav")
                .frame(frame(Margin::symmetric(0, 4)).fill(theme::BG_925))
                .resizable(false)
                .exact_size(NAV_RAIL)
                .show(ui, |ui| self.compact_nav(ui, true));
        } else {
            egui::Panel::bottom("compact_nav")
                .frame(frame(Margin::symmetric(6, 0)).fill(theme::BG_925))
                .resizable(false)
                .exact_size(NAV_BAR)
                .show(ui, |ui| self.compact_nav(ui, false));
        }
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
                egui::Panel::right("compact_sheet")
                    .frame(sheet)
                    .resizable(false)
                    .exact_size(w)
                    .show(ui, |ui| {
                        self.compact_sheet_handle(ui, default_w, min_w, max_w);
                        let cap = ui.available_height();
                        self.compact_sheet(ui, gpu, tab, cap);
                    });
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
                let wanted = egui::Panel::bottom("compact_sheet")
                    .frame(sheet)
                    .resizable(false)
                    .exact_size(h)
                    .show(ui, |ui| self.compact_sheet(ui, gpu, tab, cap))
                    .inner;
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
        self.batch_export_dialog(ui);
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
        let rect = Rect::from_min_max(panel.left_top(), pos2(panel.left() + 12.0, panel.bottom()));
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

    /// Brand, the photo's name, and what is wanted with the photo in view: zoom, a / b,
    /// undo, redo.
    fn compact_top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("compact_top")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(10, 0)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::brand_mark(ui, 16.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        // Right to left: redo, undo, a / b, zoom read left to right.
                        let can_redo = !self.history.future.is_empty();
                        let can_undo = !self.history.past.is_empty();
                        if ui
                            .add_enabled_ui(can_redo, |ui| widgets::icon(ui, "↷", false, "Redo"))
                            .inner
                            .clicked()
                            && self.history.redo(&mut self.stack)
                        {
                            self.needs_render = true;
                        }
                        if ui
                            .add_enabled_ui(can_undo, |ui| widgets::icon(ui, "↶", false, "Undo"))
                            .inner
                            .clicked()
                            && self.history.undo(&mut self.stack)
                        {
                            self.needs_render = true;
                        }
                        let loaded = self.engine.image().is_some();
                        ui.add_enabled_ui(loaded, |ui| {
                            if widgets::icon(ui, "◫", self.split_active, "Before / after split")
                                .clicked()
                            {
                                self.split_active = !self.split_active;
                                self.needs_render = true;
                            }
                            // One control for zoom: it reads the zoom, and flips fit ↔ 1:1.
                            let percent = self.zoom_percent(self.last_panel);
                            let at_fit = self.view.zoom <= 1.0 + 1e-3;
                            let label = match percent {
                                Some(p) if at_fit => format!("fit {p:.0}%"),
                                Some(p) => format!("{p:.0}%"),
                                None => "fit".to_string(),
                            };
                            if widgets::chip(ui, !at_fit, &label).clicked() {
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
            });
    }

    /// Five tabs, each an icon over its name. Tapping the open one closes its sheet.
    fn compact_nav(&mut self, ui: &mut egui::Ui, vertical: bool) {
        let area = ui.available_rect_before_wrap();
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
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        match tab {
                            Tab::Library => self.library_sheet(ui, gpu),
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
                self.request(ui.ctx(), crate::platform::Request::BrowseFolder);
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
                    let selection = &self.selection;
                    self.shelf.retain(|it| !selection.is_selected(&it.path));
                    self.selection.clear();
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
    }

    // ── Edit ────────────────────────────────────────────────────────────────────────────

    /// The chosen module's controls over a one-line strip of every module. Tapping the
    /// chosen module again folds its controls away, leaving only the strip.
    /// Returns the height it wants, as [`Self::compact_sheet`] does.
    fn edit_sheet(&mut self, ui: &mut egui::Ui, cap: f32) -> f32 {
        let loaded = self.engine.image().is_some();
        let body_open = !loaded || self.customize_open || self.compact_controls_open;
        let top = ui.cursor().top();
        // How much more the controls want than they were given this frame.
        let mut hidden = 0.0;
        if body_open {
            let body = egui::ScrollArea::vertical()
                .id_salt("compact_edit")
                .auto_shrink([false, true])
                .max_height((cap - STRIP_H - 1.0).max(80.0))
                .scroll_source(ScrollSource {
                    drag: DragScroll::Always,
                    ..Default::default()
                })
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(Margin {
                            left: 12,
                            right: 12,
                            top: 6,
                            bottom: 4,
                        })
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            if self.customize_open {
                                self.customize_body(ui);
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

    /// Turn, then every module by name, then fine / reset / modules — one scrolling line.
    fn module_strip(&mut self, ui: &mut egui::Ui) {
        let ids: Vec<InspectorSectionId> = self
            .inspector_sections
            .iter()
            .filter(|s| s.visible)
            .map(|s| s.id)
            .collect();
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
                        if strip_tab(ui, "✓ apply", true, true, None).clicked() {
                            self.apply_crop();
                        }
                        if strip_tab(ui, "✕ cancel", false, false, None).clicked() {
                            self.cancel_crop();
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
                        let dot = (module_on(&self.stack, id) && modified).then_some(theme::ACCENT);
                        let chosen = self.compact_module == id && !self.customize_open;
                        let open = chosen && self.compact_controls_open;
                        let response = strip_tab(ui, module_short_name(id), chosen, open, dot)
                            .on_hover_text(id.title());
                        if chosen && self.reveal_section == Some(id) {
                            response.scroll_to_me(None);
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
                    if strip_tab(ui, "fine", self.fine_adjust, false, None)
                        .on_hover_text("Sliders move ten times finer")
                        .clicked()
                    {
                        self.fine_adjust = !self.fine_adjust;
                    }
                    let edited = self.is_stack_edited();
                    let reset = ui
                        .add_enabled_ui(edited, |ui| strip_tab(ui, "reset", false, false, None))
                        .inner
                        .on_hover_text("Reset all adjustments to baseline");
                    if reset.clicked() {
                        self.reset_to_default();
                    }
                    if strip_tab(
                        ui,
                        "modules",
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

    /// One module's card, always open: its switch and summary, then its controls. The
    /// same checkpointing as the desktop's pipeline, so a gesture is one undo step.
    fn single_module(&mut self, ui: &mut egui::Ui, id: InspectorSectionId) {
        let snapshot = self.stack.clone();
        let defaults = self.baseline.clone();
        let white = self.white_scale();
        let (summary, modified) = summarise(id, &self.stack, &defaults, &white);
        let summary_colour = if !module_on(&self.stack, id) {
            theme::TEXT_FAINT
        } else if modified {
            theme::ACCENT
        } else {
            theme::TEXT_DIM
        };
        let mut changed = false;
        let mut started = false;
        // No card around it: there is only ever one module on screen here, and a card's
        // border and padding cost the sliders width a phone does not have.
        let head = widgets::module_header(
            ui,
            id.title(),
            module_enabled(&mut self.stack, id),
            &summary,
            summary_colour,
            true,
        );
        if head.toggled {
            changed = true;
            started = true;
        }
        let mut body_changed = false;
        self.module_body(ui, id, &mut body_changed, &mut started);
        if body_changed {
            // As on the desktop: touching a control turns its module on.
            if let Some(on) = module_enabled(&mut self.stack, id) {
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

    /// A quarter turn from the Edit sheet's toolbar, as one undo step.
    fn quick_turn(&mut self, clockwise: bool) {
        let before = self.stack.clone();
        if clockwise {
            self.stack.geometry.rotate_right();
        } else {
            self.stack.geometry.rotate_left();
        }
        self.history.checkpoint(&before);
        self.rebase_crop();
        self.needs_render = true;
    }

    // ── Export ──────────────────────────────────────────────────────────────────────────

    fn export_sheet(&mut self, ui: &mut egui::Ui) {
        self.export_format_line(ui);
        let busy = matches!(self.export, Export::Writing { .. });
        let ready = self.engine.image().is_some() && !busy;
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
        let notice = self
            .status_notification
            .as_ref()
            .filter(|(_, when)| when.elapsed().as_secs_f32() < 4.0);
        let (text, colour) = if let Some(err) = &self.error {
            (err.clone(), theme::DANGER)
        } else if let Export::Writing { path, .. } = &self.export {
            (format!("writing {}", file_name(path)), theme::ACCENT)
        } else if let Export::Done { path, seconds } = &self.export {
            (
                format!("exported {} in {seconds:.1}s", file_name(path)),
                theme::LIME,
            )
        } else if let Export::Failed(e) = &self.export {
            (format!("export failed: {e}"), theme::DANGER)
        } else if let Some((msg, when)) = notice {
            // Wake to take the notice down when its time is up.
            let left = 4.0 - when.elapsed().as_secs_f32();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f32(left.max(0.0)));
            (msg.clone(), theme::ACCENT)
        } else {
            return;
        };
        egui::Panel::bottom("compact_status")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_925)
                    .inner_margin(Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::dot(ui, colour);
                    ui.add(egui::Label::new(theme::num(text).size(11.5).color(colour)).truncate());
                });
            });
    }
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
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 2.0 * pad, STRIP_H), Sense::click());
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
    let (rect, response) = ui.allocate_exact_size(vec2(34.0, STRIP_H), Sense::click());
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
fn module_short_name(id: InspectorSectionId) -> &'static str {
    match id {
        InspectorSectionId::Scope => "scope",
        InspectorSectionId::Geometry => "crop",
        InspectorSectionId::WhiteBalance => "wb",
        InspectorSectionId::Exposure => "exposure",
        InspectorSectionId::Tonal => "tone",
        InspectorSectionId::ToneCurve => "curve",
        InspectorSectionId::Clarity => "clarity",
        InspectorSectionId::Dehaze => "dehaze",
        InspectorSectionId::ColorEq => "color",
        InspectorSectionId::Filmic3D => "filmic",
        InspectorSectionId::ColorBalance => "balance",
        InspectorSectionId::Vibrance => "vibrance",
        InspectorSectionId::Sharpen => "sharpen",
        InspectorSectionId::Vignette => "vignette",
    }
}

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
