//! The phone layout: the stage fills the screen, one sheet at a time sits beside it, and
//! a navigation bar picks the sheet. It is the same app as the desktop layout — every
//! sheet is built from the same pieces the desktop's panels are — only arranged for a
//! small, touch-driven screen.
//!
//! Upright, the bar runs along the bottom and the sheet rises above it. Turned sideways
//! there is little height to spare, so the bar becomes a rail down the right edge and the
//! sheet opens beside it. Tapping the open tab again closes the sheet and gives the stage
//! the whole screen.
//!
//! | Tab | Holds |
//! |---|---|
//! | Library | open / add / import a folder, select mode, the current photo's rating and flag, filters, thumbnails, the shelf and folder tree |
//! | Edit | crop, a / b, fine adjust, a scroller of the modules, and the chosen module's controls — one group at a time |
//! | Scopes | the scope and its modes |
//! | History | undo, redo, reset, copy / paste stack, save and revert the sidecar, snapshots, history |
//! | Export | the single export and batch export |

use egui::containers::scroll_area::{DragScroll, ScrollSource};
use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Margin, Painter, Rect, RichText, Sense,
    Stroke,
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

const NAV_BAR: f32 = 60.0;
const NAV_RAIL: f32 = 68.0;

impl PhotoApp {
    pub(crate) fn compact_layout(&mut self, ui: &mut egui::Ui, gpu: &Gpu, screen: egui::Vec2) {
        let landscape = self.layout == Layout::Landscape;
        let frame = |margin: Margin| egui::Frame::new().fill(theme::BG_950).inner_margin(margin);

        // Panel order is layout order: the top bar, then the navigation, then the sheet
        // against it, then the status line, and the stage takes what is left.
        self.compact_top_bar(ui);
        if landscape {
            egui::Panel::right("compact_nav")
                .frame(frame(Margin::symmetric(0, 6)).fill(theme::BG_925))
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
                let default_w = (screen.x * 0.48).clamp(340.0, 480.0);
                let min_w = 280.0;
                let max_w = (screen.x * 0.75).clamp(420.0, 680.0);
                let w = self.compact_sheet_width.unwrap_or(default_w).clamp(min_w, max_w);
                egui::Panel::right("compact_sheet")
                    .frame(sheet)
                    .resizable(false)
                    .exact_size(w)
                    .show(ui, |ui| {
                        self.compact_sheet_handle(ui, true, default_w, min_w, max_w);
                        self.compact_sheet(ui, gpu, tab);
                    });
            } else {
                let default_h = (screen.y * 0.55).clamp(380.0, 620.0);
                let min_h = (screen.y * 0.28).clamp(200.0, 320.0);
                let max_h = (screen.y * 0.82).clamp(480.0, 750.0);
                let h = self.compact_sheet_height.unwrap_or(default_h).clamp(min_h, max_h);
                egui::Panel::bottom("compact_sheet")
                    .frame(sheet)
                    .resizable(false)
                    .exact_size(h)
                    .show(ui, |ui| {
                        self.compact_sheet_handle(ui, false, default_h, min_h, max_h);
                        self.compact_sheet(ui, gpu, tab);
                    });
            }
        }
        self.compact_status(ui);
        self.workspace(ui);
        self.batch_export_dialog(ui);
    }

    /// Touch-friendly drawer grab handle at the top of the compact sheet.
    /// Dragging resizes the sheet; double-tapping resets to default size.
    fn compact_sheet_handle(
        &mut self,
        ui: &mut egui::Ui,
        landscape: bool,
        default_size: f32,
        min_size: f32,
        max_size: f32,
    ) {
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), 16.0),
            Sense::click_and_drag(),
        );
        let hot = response.hovered() || response.dragged();
        let pill = Rect::from_center_size(rect.center(), vec2(36.0, 4.0));
        ui.painter().rect_filled(
            pill,
            2.0,
            if hot {
                theme::TEXT_HI
            } else {
                Color32::from_gray(0x44)
            },
        );
        if landscape {
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
        } else {
            if hot {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
            }
            if response.dragged() {
                let dy = response.drag_delta().y;
                let current = self.compact_sheet_height.unwrap_or(default_size);
                self.compact_sheet_height = Some((current - dy).clamp(min_size, max_size));
            } else if response.double_clicked() {
                self.compact_sheet_height = None;
            }
        }
    }

    /// Brand, the photo's name, and what is wanted with the photo in view: zoom, a / b,
    /// undo, redo.
    fn compact_top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("compact_top")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(10, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::brand_mark(ui, 22.0);
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
                        vec2(3.0, 28.0),
                    )
                } else {
                    Rect::from_center_size(pos2(cell.center().x, cell.top() + 1.5), vec2(28.0, 3.0))
                };
                painter.rect_filled(bar, 1.5, theme::ACCENT);
            }
            let icon_centre = pos2(cell.center().x, cell.center().y - 8.0);
            paint_tab_icon(painter, icon_centre, tab, colour);
            painter.text(
                pos2(cell.center().x, cell.center().y + 14.0),
                Align2::CENTER_CENTER,
                tab.label().to_uppercase(),
                FontId::new(9.5, theme::mono_bold()),
                colour,
            );
            if response.clicked() {
                self.compact_tab = if open { None } else { Some(tab) };
            }
        }
        ui.allocate_rect(area, Sense::hover());
    }

    fn compact_sheet(&mut self, ui: &mut egui::Ui, gpu: &Gpu, tab: Tab) {
        // Edit keeps its module scroller pinned and scrolls only the controls under it;
        // every other sheet scrolls as a whole.
        if tab == Tab::Edit {
            self.edit_sheet(ui);
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(("compact_sheet", tab.label()))
            .auto_shrink([false, false])
            .scroll_source(ScrollSource {
                drag: DragScroll::Always,
                ..Default::default()
            })
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        match tab {
                            Tab::Library => self.library_sheet(ui, gpu),
                            Tab::Export => self.export_sheet(ui),
                            Tab::Edit => unreachable!("handled above"),
                        }
                    });
            });
    }

    // ── Library ─────────────────────────────────────────────────────────────────────────

    fn library_sheet(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        ui.horizontal_wrapped(|ui| {
            if widgets::button(ui, Kind::Primary, "Open…", 0.0).clicked() {
                self.pick_and_open(ui.ctx());
            }
            if widgets::button(ui, Kind::Secondary, "+ Add", 0.0).clicked() {
                self.pick_and_shelve(ui.ctx());
            }
            if widgets::button(ui, Kind::Secondary, "Folder…", 0.0).clicked() {
                self.request(ui.ctx(), crate::platform::Request::BrowseFolder);
            }
        });

        self.selection_bar(ui);
        let displayed = self.displayed_indices();
        self.film_tiles(ui, gpu, &displayed);
        self.current_photo_marks(ui);

        // Filters, as the desktop filmstrip has them, wrapped to the width.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            self.rating_filter(ui);
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            self.flag_filter(ui);
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            self.search_field(ui, 120.0);
            self.sort_controls(ui, false);
        });

        ui.horizontal(|ui| {
            let n = self.shelf.len();
            if widgets::chip(ui, !self.folder_browser_open, &format!("Stills ({n})")).clicked() {
                self.folder_browser_open = false;
            }
            if widgets::chip(ui, self.folder_browser_open, "Folders").clicked() {
                self.folder_browser_open = true;
            }
        });
        if self.folder_browser_open {
            self.shelf_folders(ui, gpu);
        } else {
            let mut to_open = None;
            let mut to_remove = None;
            self.shelf_group(ui, &mut to_open, &mut to_remove);
            if let Some(path) = to_open {
                self.open_path(gpu, path);
            }
            if let Some(idx) = to_remove {
                self.shelf.remove(idx);
            }
        }
    }

    /// Select mode and what it acts on. A touch screen has no Ctrl or Shift, so this is
    /// how several photos get chosen for a batch export or a shared rating.
    fn selection_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if widgets::chip(ui, self.select_mode, "select").clicked() {
                self.select_mode = !self.select_mode;
            }
            if self.select_mode {
                if widgets::chip(ui, false, "all").clicked() {
                    let all: Vec<PathBuf> = self.shelf.iter().map(|it| it.path.clone()).collect();
                    self.selection.select_all(&all);
                }
                if widgets::chip(ui, false, "none").clicked() {
                    self.selection.clear();
                }
            }
            // Opening a photo selects it, so one selected is just the open photo; the
            // count and what acts on it matter from two, or once select mode is on.
            let n = self.selection.count();
            if n > 1 || (self.select_mode && n > 0) {
                ui.label(theme::tag(format!("{n} selected")).color(theme::ACCENT));
                if widgets::button(ui, Kind::Secondary, "Remove from shelf", 0.0).clicked() {
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
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(theme::dim(if many { "Selection:" } else { "This photo:" }));
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
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            use photo_engine::Flag;
            for (f, label) in [
                (Flag::Pick, "pick"),
                (Flag::Reject, "reject"),
                (Flag::Unflagged, "unflag"),
            ] {
                if widgets::chip(ui, !many && flag == f && f != Flag::Unflagged, label).clicked() {
                    set_flag = Some(f);
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

    fn edit_sheet(&mut self, ui: &mut egui::Ui) {
        let loaded = self.engine.image().is_some();
        egui::Frame::new()
            .inner_margin(Margin {
                left: 12,
                right: 12,
                top: 8,
                bottom: 4,
            })
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(loaded, |ui| {
                        self.crop_chips(ui);
                        if widgets::chip(ui, self.split_active, "a / b").clicked() {
                            self.split_active = !self.split_active;
                            self.needs_render = true;
                        }
                    });
                    if widgets::chip(ui, self.fine_adjust, "fine")
                        .on_hover_text("Sliders move ten times finer")
                        .clicked()
                    {
                        self.fine_adjust = !self.fine_adjust;
                    }
                    if widgets::chip(ui, self.customize_open, "modules")
                        .on_hover_text("Choose which modules show, and their order")
                        .clicked()
                    {
                        self.customize_open = !self.customize_open;
                    }
                    ui.add_enabled_ui(loaded && self.is_stack_edited(), |ui| {
                        if widgets::chip(ui, false, "reset")
                            .on_hover_text("Reset all adjustments to baseline")
                            .clicked()
                        {
                            self.reset_to_default();
                        }
                    });
                });
                if loaded && !self.customize_open {
                    ui.add_space(6.0);
                    self.module_scroller(ui);
                }
            });

        egui::ScrollArea::vertical()
            .id_salt("compact_edit")
            .auto_shrink([false, false])
            .scroll_source(ScrollSource {
                drag: DragScroll::Always,
                ..Default::default()
            })
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        if self.customize_open {
                            self.customize_body(ui);
                        } else if loaded {
                            self.single_module(ui, self.compact_module);
                        } else {
                            ui.label(theme::dim("Open a photo to edit it.").size(12.0));
                            if widgets::button(ui, Kind::Primary, "Open…", 0.0).clicked() {
                                self.pick_and_open(ui.ctx());
                            }
                        }
                    });
            });
    }

    /// A tile per module, in the order Customize gives them, scrolled sideways.
    fn module_scroller(&mut self, ui: &mut egui::Ui) {
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
            .id_salt("compact_modules")
            .auto_shrink([false, true])
            .scroll_source(ScrollSource {
                drag: DragScroll::Always,
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for id in ids {
                        let (_, modified) = summarise(id, &self.stack, &defaults, &white);
                        let state = if !module_on(&self.stack, id) {
                            None
                        } else if modified {
                            Some(theme::ACCENT)
                        } else {
                            Some(theme::TEXT_DIM)
                        };
                        let chosen = self.compact_module == id;
                        let response = module_tile(ui, id, chosen, state);
                        if chosen && self.reveal_section == Some(id) {
                            response.scroll_to_me(None);
                            self.reveal_section = None;
                        }
                        if response.clicked() {
                            self.compact_module = id;
                            self.focused_section = Some(id);
                        }
                    }
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
        widgets::card(ui, true, |ui| {
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
            ui.add_space(2.0);
            let mut body_changed = false;
            self.module_body(ui, id, &mut body_changed, &mut started);
            if body_changed {
                // As on the desktop: touching a control turns its module on.
                if let Some(on) = module_enabled(&mut self.stack, id) {
                    *on = true;
                }
            }
            changed |= body_changed;
            ui.add_space(4.0);
        });
        if changed {
            self.focused_section = Some(id);
            self.needs_render = true;
        }
        if started {
            self.history.checkpoint(&snapshot);
        }
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

/// A module's tile in the Edit scroller: a monogram over a short name, with a dot when the
/// module is on (amber once it changes the photo).
fn module_tile(
    ui: &mut egui::Ui,
    id: InspectorSectionId,
    chosen: bool,
    state: Option<Color32>,
) -> egui::Response {
    let (monogram, name) = module_glyph(id);
    let (rect, response) = ui.allocate_exact_size(vec2(64.0, 54.0), Sense::click());
    let painter = ui.painter();
    let (fill, stroke, colour) = if chosen {
        (
            theme::BG_850,
            theme::ACCENT.linear_multiply(0.8),
            theme::ACCENT,
        )
    } else if response.hovered() {
        (theme::BG_850, theme::BORDER_HI, theme::TEXT_HI)
    } else {
        (theme::BG_900, theme::BORDER, Color32::from_gray(0xcc))
    };
    painter.rect(
        rect,
        4,
        fill,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.center().x, rect.top() + 20.0),
        Align2::CENTER_CENTER,
        monogram,
        FontId::new(16.0, theme::display()),
        colour,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 12.0),
        Align2::CENTER_CENTER,
        name.to_uppercase(),
        FontId::new(8.5, theme::mono_bold()),
        colour,
    );
    if let Some(dot) = state {
        painter.circle_filled(rect.right_top() + vec2(-8.0, 8.0), 3.0, dot);
    }
    response.on_hover_text(id.title())
}

/// A module's monogram and short name, for the Edit scroller.
fn module_glyph(id: InspectorSectionId) -> (&'static str, &'static str) {
    match id {
        InspectorSectionId::Scope => ("Sc", "scope"),
        InspectorSectionId::Geometry => ("Cr", "crop"),
        InspectorSectionId::WhiteBalance => ("WB", "white bal"),
        InspectorSectionId::Exposure => ("EV", "exposure"),
        InspectorSectionId::Tonal => ("S/H", "shadows"),
        InspectorSectionId::ToneCurve => ("Cv", "curve"),
        InspectorSectionId::Clarity => ("Cl", "clarity"),
        InspectorSectionId::Dehaze => ("Dh", "dehaze"),
        InspectorSectionId::ColorEq => ("EQ", "color eq"),
        InspectorSectionId::Filmic3D => ("Fm", "filmic"),
        InspectorSectionId::ColorBalance => ("CB", "balance"),
        InspectorSectionId::Vibrance => ("Vb", "vibrance"),
        InspectorSectionId::Sharpen => ("Sh", "sharpen"),
        InspectorSectionId::Vignette => ("Vg", "vignette"),
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
