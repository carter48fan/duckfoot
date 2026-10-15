//! The phone layout: the photo fills the screen, and one low sheet at a time sits under it.
//! It is the same app as the desktop layout — every sheet is built from the same pieces
//! the desktop's panels are — only arranged for a small, touch-driven screen, after
//! Google Photos and Snapseed.
//!
//! Upright, the bottom bar holds four groups and the sheet rises above it, only as tall
//! as what it holds. Turned sideways the bar becomes a rail down the right edge and the
//! sheet opens beside it. Tapping the open group again closes the sheet and gives the
//! photo the whole screen.
//!
//! | Where | Holds |
//! |---|---|
//! | Looks | one-tap looks and their strength, snapshots, history, copy & paste |
//! | Tune | the chosen value over a ruler that turns it, over a bar of Light · Colour · Detail with a dot per value. Every quick value is in this one list ([`crate::categories`]) |
//! | Crop | crop & rotate, and straighten, keystone and distortion on the ruler |
//! | Tools | a grid of the tools that are not one value: curves, the colour mixer and grade, brush & masks, healing, removing the background, a LUT, filmic, the scope |
//! | the top bar's ＋ | another photo, in place of this one (one photo at a time on a phone) |
//! | the top bar's Export | the export, its options, removing the background and share |
//! | the top bar's ⋯ | before / after, zoom, fine adjust, clipping, reset, the photo's info and the library's storage, the tour |
//!
//! Getting around without a keyboard:
//!
//! - tap a group, or swipe along the bar, to change group;
//! - swipe up or down, on the photo or on the ruler, to choose what to change: up for the
//!   next value in the list. In Tune, Light, Colour and Detail are each their own list,
//!   shown as tabs over it: drift left or right while choosing, swipe the bar under the
//!   ruler, or tap one, to move between them;
//! - swipe sideways, on the photo or on the ruler, to change it; double-tap to put it
//!   back; hold the photo to see the original; the ⋯ beside a value opens all its tool's
//!   settings;
//! - inside a tool, swipe an empty part of its controls to move to the next tool; back
//!   returns to the ruler or the grid.
//!
//! On a phone ([`crate::platform::Platform::single_photo`]) there is one photo at a time:
//! the one picked on opening, edited until another replaces it, and the next launch starts
//! empty. The first run walks through all this (`tour`).

use egui::containers::scroll_area::{DragScroll, ScrollSource};
use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Margin, Painter, Rect, RichText, Sense, Stroke,
};

use crate::categories::{self, Category, Chip, CHIPS_H, RULER_H};
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
            Tab::Library => crate::i18n::tr("tab-library"),
            Tab::Presets => crate::i18n::tr("tab-presets"),
            Tab::Edit => crate::i18n::tr("tab-edit"),
            Tab::Export => crate::i18n::tr("tab-export"),
        }
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
    /// This photo's edits as a whole: revert, reset, copy and paste.
    Edits,
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
    /// Where the photo's stage was allocated, margins taken off.
    pub(crate) stage: Rect,
    /// The Edit sheet's module strip, swiped sideways.
    strip: Rect,
}

impl Default for Rects {
    fn default() -> Self {
        Self {
            top: Rect::NOTHING,
            nav: Rect::NOTHING,
            sheet: Rect::NOTHING,
            stage: Rect::NOTHING,
            strip: Rect::NOTHING,
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

/// The tab bar's height upright, and the rail's width on its side: thumb-sized cells.
pub const NAV_BAR: f32 = 60.0;
pub const NAV_RAIL: f32 = 72.0;
/// Height of the top bar. Fixed, so everything in it can be centred on one line: a row
/// that grows as its items arrive centres the early ones on the height it had so far.
pub const TOP_BAR: f32 = 48.0;
/// Height of a tool's header.
const TOOL_BAR: f32 = 46.0;
/// Height of the row naming the chosen value over the ruler.
const READOUT_H: f32 = 44.0;
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
        self.compact_rects.strip = Rect::NOTHING;
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
        self.compact_rects.stage = Rect::NOTHING;
        self.workspace(ui, gpu);
        self.batch_export_dialog(ui);
        self.tour(ui);
    }

    /// Where a sideways swipe is the app's and not the system's Back (M-24): the bars
    /// swiped along to change tab or module. Android caps what an app may take from each
    /// edge at 200 dp of height, so the photo itself, swiped for the next photo, keeps
    /// the system's gesture at its edges.
    pub(crate) fn gesture_strips(&self) -> Vec<Rect> {
        if !self.layout.is_compact() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.layout == Layout::Portrait {
            out.push(self.compact_rects.nav);
        }
        out.push(self.compact_rects.strip);
        out.retain(|r| r.is_positive() && r.height() <= 200.0);
        out
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

    /// The mark and the photo's name, then what is wanted from any tab: zoom, before /
    /// after, undo and redo, and a menu (⋯) for the rest: fine adjust, the clipping view
    /// and reset. Returns where it was drawn.
    fn compact_top_bar(&mut self, ui: &mut egui::Ui) -> Rect {
        let mut more = Rect::NOTHING;
        let bar = egui::Panel::top("compact_top")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(8, 0)),
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
                    ui.spacing_mut().item_spacing.x = 4.0;
                    // Back to the roll: Library is where a photo is chosen, so it is
                    // "back" from the photo. Tapped from the Library, it returns.
                    let in_library = self.compact_tab == Some(Tab::Library);
                    if self.single_photo() {
                        // One photo at a time: another is chosen in its place.
                        if bar_icon(ui, BarIcon::Open, false, "Open another photo").clicked() {
                            self.pick_and_open(ui.ctx());
                        }
                    } else if bar_icon(
                        ui,
                        BarIcon::Back,
                        in_library,
                        crate::i18n::tr("tab-library"),
                    )
                    .clicked()
                    {
                        self.compact_tab = Some(if in_library { Tab::Edit } else { Tab::Library });
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        // Right to left: export, redo, undo, more read left to right.
                        // The icons are painted rather than taken from the fallback font,
                        // whose arrows and squares each come at their own size and height.
                        let exporting = self.compact_tab == Some(Tab::Export);
                        let export = widgets::button(
                            ui,
                            if exporting {
                                Kind::Secondary
                            } else {
                                Kind::Primary
                            },
                            crate::i18n::tr("tab-export"),
                            0.0,
                        );
                        if export.clicked() {
                            self.compact_tab =
                                Some(if exporting { Tab::Edit } else { Tab::Export });
                        }
                        ui.add_space(4.0);
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
                        let open = self.compact_more_open;
                        let r = bar_icon(ui, BarIcon::More, open, "More");
                        if r.clicked() {
                            self.compact_more_open = !open;
                        }
                        more = r.rect;
                        ui.add_space(4.0);
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
                                        .size(13.0)
                                        .color(theme::TEXT_HI),
                                )
                                .truncate(),
                            );
                        });
                    });
                });
            })
            .response
            .rect;
        if self.compact_more_open {
            self.compact_more_menu(ui.ctx(), more);
        }
        bar
    }

    /// The top bar's ⋯ menu: what is wanted now and then, each a full-width row.
    fn compact_more_menu(&mut self, ctx: &egui::Context, anchor: Rect) {
        let w = 240.0;
        let screen = ctx.content_rect();
        let at = pos2(
            (anchor.right() - w).clamp(screen.left() + 8.0, screen.right() - w - 8.0),
            anchor.bottom() + 4.0,
        );
        let loaded = self.ready_image().is_some();
        let mut close = false;
        let shown = egui::Area::new(egui::Id::new("compact_more"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::BG_900)
                    .stroke(Stroke::new(1.0, theme::BORDER_HI))
                    .corner_radius(10)
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(w - 12.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if menu_row(ui, loaded, "Before / after", Some(self.split_active)).clicked()
                        {
                            self.split_active = !self.split_active;
                            self.needs_render = true;
                        }
                        // Pinching zooms too; this is the one-tap fit ↔ 1:1.
                        let at_fit = self.view.zoom <= 1.0 + 1e-3;
                        let zoom = if at_fit {
                            "Zoom to 100%"
                        } else {
                            "Zoom to fit"
                        };
                        if menu_row(ui, loaded, zoom, None).clicked() {
                            if at_fit {
                                self.zoom_actual();
                            } else {
                                self.view = View::default();
                            }
                            close = true;
                        }
                        if menu_row(ui, true, "Fine adjust", Some(self.fine_adjust)).clicked() {
                            self.fine_adjust = !self.fine_adjust;
                        }
                        if menu_row(ui, true, "Show clipping", Some(self.clip_overlay)).clicked() {
                            self.clip_overlay = !self.clip_overlay;
                        }
                        let edited = loaded && self.is_stack_edited();
                        if menu_row(ui, edited, "Reset every adjustment", None).clicked() {
                            self.reset_to_default();
                            close = true;
                        }
                        if self.single_photo()
                            && menu_row(ui, true, "Photo info & storage", None).clicked()
                        {
                            self.compact_tab = Some(Tab::Library);
                            close = true;
                        }
                        if menu_row(ui, true, "Show the tour", None).clicked() {
                            self.start_tour();
                            close = true;
                        }
                    });
            });
        // A tap anywhere else closes it, as a menu does.
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !shown.response.rect.contains(p) && !anchor.contains(p))
        });
        if close || outside {
            self.compact_more_open = false;
        }
    }

    /// The groups, each an icon over its name. Tapping the open one closes its sheet;
    /// swiping along the bar moves to the next group that way.
    fn compact_nav(&mut self, ui: &mut egui::Ui, vertical: bool) {
        let area = ui.available_rect_before_wrap();
        // The sheet the groups are for: Edit, or Looks.
        let editing = matches!(self.compact_tab, Some(Tab::Edit | Tab::Presets));
        // Registered before the cells, so a tap still reaches them.
        if let Some(step) = swipe(ui, area, ui.id().with("nav_swipe"), vertical) {
            if let Some(to) = self.compact_category.offset(step) {
                self.open_category(to);
            }
        }
        let n = Category::ALL.len() as f32;
        for (i, cat) in Category::ALL.into_iter().enumerate() {
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
            let response = ui.interact(cell, ui.id().with(("cat", cat.label())), Sense::click());
            widgets::describe(ui, &response, cell, || {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    true,
                    editing && self.compact_category == cat,
                    cat.label(),
                )
            });
            let open = editing && self.compact_category == cat;
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
                        vec2(3.0, 32.0),
                    )
                } else {
                    Rect::from_center_size(pos2(cell.center().x, cell.top() + 1.5), vec2(32.0, 3.0))
                };
                painter.rect_filled(bar, 1.5, theme::ACCENT);
            }
            categories::paint_cell(painter, cell, cat, colour);
            if response.clicked() {
                if open {
                    // The open category again gives the photo the whole screen.
                    self.compact_tab = None;
                } else {
                    self.open_category(cat);
                }
            }
        }
        ui.allocate_rect(area, Sense::hover());
    }

    /// Show `cat` in the Edit sheet, on the value last chosen there, else its first; a tool
    /// open from another group closes. Looks opens its looks.
    pub fn open_category(&mut self, cat: Category) {
        if self.compact_category != cat || self.current_chip().is_some_and(Chip::is_tool) {
            self.compact_controls_open = false;
            self.cutout_studio.open = false;
        }
        if cat == Category::Looks {
            self.choose_chip(Chip::Looks);
            return;
        }
        if self.compact_category != cat {
            self.compact_category = cat;
            let back = self.compact_memory[cat.index()]
                .filter(|c| !c.is_tool())
                .or_else(|| cat.first_knob());
            if let Some(chip) = back {
                self.choose_chip(chip);
            }
        }
        self.compact_tab = Some(Tab::Edit);
    }

    /// What the Edit sheet is on now, if anything: a tool open, else the value the ruler
    /// and the photo turn.
    pub fn current_chip(&self) -> Option<Chip> {
        if self.cutout_studio.open {
            return Some(Chip::Cutout);
        }
        if self.compact_tab == Some(Tab::Presets) {
            return Some(Chip::Looks);
        }
        let id = self.compact_module;
        if self.compact_controls_open {
            return Some(Chip::Tool(id));
        }
        self.photo_knob()
            .filter(|_| !crate::knobs::knobs(id).is_empty())
            .map(|(i, _)| Chip::Knob(id, i))
    }

    /// Go to `chip`: its value under the ruler, or its tool open in its place.
    pub fn choose_chip(&mut self, chip: Chip) {
        // Its category comes up with it; a value no chip names (one reached by swiping
        // on the photo) is in its module's.
        match Category::ALL
            .into_iter()
            .find(|c| c.chips().contains(&chip))
        {
            Some(cat) => {
                self.compact_category = cat;
                self.compact_memory[cat.index()] = Some(chip);
            }
            None => {
                if let Some(id) = chip.module() {
                    self.compact_category = Category::of(id);
                }
            }
        }
        self.cutout_studio.open = false;
        self.compact_tab = Some(Tab::Edit);
        match chip {
            Chip::Knob(id, i) => {
                self.compact_module = id;
                self.compact_controls_open = false;
                self.photo_touch.knob = Some((id, i));
                self.focused_section = Some(id);
            }
            Chip::Tool(id) => {
                self.compact_module = id;
                self.compact_controls_open = true;
                self.focused_section = Some(id);
            }
            Chip::Looks => {
                self.compact_controls_open = false;
                self.compact_tab = Some(Tab::Presets);
            }
            Chip::Cutout => {
                self.compact_controls_open = false;
                self.cutout_studio.open = true;
            }
        }
    }

    /// `cap` is the most height the sheet may take; it takes less when it holds less.
    /// Returns the height its content wants, scrolled parts included in full.
    fn compact_sheet(&mut self, ui: &mut egui::Ui, gpu: &Gpu, tab: Tab, cap: f32) -> f32 {
        // Edit keeps its chips pinned and scrolls only what is over them; Library and
        // Export scroll as a whole.
        if matches!(tab, Tab::Edit | Tab::Presets) {
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
                            Tab::Edit | Tab::Presets => unreachable!("handled above"),
                        }
                    });
            })
            .content_size
            .y
    }

    // ── Library ─────────────────────────────────────────────────────────────────────────

    /// The roll, then one row of actions under it. Filters and select mode are chips in
    /// that row, and take room only while they are on.
    fn library_sheet(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        if self.single_photo() {
            self.photo_sheet(ui);
            return;
        }
        if self.storage.is_some() {
            self.storage_panel(ui);
        } else {
            let displayed = self.displayed_indices();
            self.film_tiles(ui, gpu, &displayed);
            self.current_photo_marks(ui);
        }

        // Sorting is not filtering: only something that hides photos lights the chip.
        let filtering = self.filtering();
        let mut make_copy = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if widgets::button(ui, Kind::Primary, "Open", 0.0).clicked() {
                self.pick_and_open(ui.ctx());
            }
            ui.add_space(4.0);
            if widgets::chip(ui, false, "+ add").clicked() {
                self.pick_and_shelve(ui.ctx());
            }
            if widgets::chip(ui, false, "+ folder").clicked() {
                self.pick_folder_and_shelve(ui.ctx());
            }
            divider(ui, 28.0);
            if self.platform.library_dir().is_some()
                && widgets::chip(ui, self.storage.is_some(), "storage")
                    .on_hover_text(
                        "The copies in Duckfoot's library, their sizes, and deleting them",
                    )
                    .clicked()
            {
                if self.storage.is_some() {
                    self.storage = None;
                } else {
                    self.open_storage();
                }
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
            if let Some(path) = self.loaded_path.clone() {
                if widgets::chip(ui, false, "+ copy")
                    .on_hover_text("A virtual copy: another edit of the same photo")
                    .clicked()
                {
                    make_copy = Some(path);
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
        if let Some(path) = make_copy {
            self.make_virtual_copy(gpu, &path);
        }
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
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                self.label_filter(ui);
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                self.saved_filters_row(ui);
            });
        }
        if self.select_mode {
            self.selection_bar(ui);
        }
    }

    /// One photo at a time: no roll to show, only this photo's marks and what the file
    /// says, opening another, and the library's storage (the copies picked photos leave).
    fn photo_sheet(&mut self, ui: &mut egui::Ui) {
        if self.storage.is_some() {
            self.storage_panel(ui);
        } else {
            self.current_photo_marks(ui);
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if widgets::button(ui, Kind::Primary, "Open another", 0.0).clicked() {
                self.pick_and_open(ui.ctx());
            }
            ui.add_space(4.0);
            if self.platform.library_dir().is_some()
                && widgets::chip(ui, self.storage.is_some(), "storage")
                    .on_hover_text(
                        "The copies in Duckfoot's library, their sizes, and deleting them",
                    )
                    .clicked()
            {
                if self.storage.is_some() {
                    self.storage = None;
                } else {
                    self.open_storage();
                }
            }
        });
    }

    /// The Library screen (H11): every copy Duckfoot made from the picker, largest first,
    /// with what it costs, which are on the shelf, and deleting them.
    fn storage_panel(&mut self, ui: &mut egui::Ui) {
        use crate::dam::storage::Confirm;
        let Some(view) = &self.storage else { return };
        let Some(entries) = &view.entries else {
            ui.label(theme::tag("Reading the library…").color(theme::TEXT_DIM));
            return;
        };
        let shelved: std::collections::HashSet<PathBuf> = self
            .shelf
            .iter()
            .map(|it| photo_engine::copies::source_file(&it.path).into_owned())
            .collect();
        let total: u64 = entries.iter().map(|e| e.bytes).sum();
        let unused: Vec<PathBuf> = entries
            .iter()
            .filter(|e| !shelved.contains(&e.path))
            .map(|e| e.path.clone())
            .collect();
        let unused_bytes: u64 = entries
            .iter()
            .filter(|e| !shelved.contains(&e.path))
            .map(|e| e.bytes)
            .sum();
        let chosen: Vec<PathBuf> = entries
            .iter()
            .filter(|e| view.selected.contains(&e.path))
            .map(|e| e.path.clone())
            .collect();
        let confirming = view.confirming;
        let count = entries.len();

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(
                theme::tag(format!(
                    "{count} {} · {}",
                    if count == 1 { "copy" } else { "copies" },
                    crate::parts::format_bytes(total)
                ))
                .color(theme::TEXT),
            );
        });
        let mut toggle = None;
        let mut select_all = None;
        let mut delete = None;
        let mut arm = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if count > 0 {
                if widgets::chip(ui, false, "all").clicked() {
                    select_all = Some(true);
                }
                if widgets::chip(ui, false, "none").clicked() {
                    select_all = Some(false);
                }
            }
            if !chosen.is_empty() {
                let armed = confirming == Some(Confirm::Selected);
                let label = if armed {
                    format!("delete {}? tap again", chosen.len())
                } else {
                    format!("delete {}", chosen.len())
                };
                if widgets::chip(ui, armed, &label).clicked() {
                    if armed {
                        delete = Some(chosen.clone());
                    } else {
                        arm = Some(Some(Confirm::Selected));
                    }
                }
            }
            if !unused.is_empty() {
                let armed = confirming == Some(Confirm::Unused);
                let label = if armed {
                    format!("delete {}? tap again", unused.len())
                } else {
                    format!(
                        "clear unused · {}",
                        crate::parts::format_bytes(unused_bytes)
                    )
                };
                if widgets::chip(ui, armed, &label)
                    .on_hover_text("Delete every copy that is not on the shelf")
                    .clicked()
                {
                    if armed {
                        delete = Some(unused.clone());
                    } else {
                        arm = Some(Some(Confirm::Unused));
                    }
                }
            }
        });

        let row_h = widgets::sized(ui, 26.0, 40.0);
        egui::ScrollArea::vertical()
            .id_salt("library storage")
            .max_height(row_h * 4.5)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if entries.is_empty() {
                    ui.label(
                        theme::tag(
                            "Nothing copied in yet: photos opened from the phone are copied here.",
                        )
                        .color(theme::TEXT_DIM),
                    );
                }
                for entry in entries {
                    let on = view.selected.contains(&entry.path);
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                    let painter = ui.painter();
                    if on {
                        painter.rect_filled(rect, 3.0, Color32::from_gray(0x1c));
                    }
                    let side = 14.0;
                    let check = Rect::from_center_size(
                        pos2(rect.left() + 4.0 + side / 2.0, rect.center().y),
                        vec2(side, side),
                    );
                    widgets::paint_checkbox(painter, check, on, response.hovered());
                    let size = crate::parts::format_bytes(entry.bytes);
                    let size_galley =
                        painter.layout_no_wrap(size, FontId::monospace(11.0), theme::TEXT_DIM);
                    let size_w = size_galley.size().x;
                    painter.galley(
                        pos2(
                            rect.right() - 4.0 - size_w,
                            rect.center().y - size_galley.size().y / 2.0,
                        ),
                        size_galley,
                        theme::TEXT_DIM,
                    );
                    let mut name = file_name(&entry.path);
                    if shelved.contains(&entry.path) {
                        name.push_str("  · on shelf");
                    }
                    let name_left = check.right() + 8.0;
                    let name_w = (rect.right() - 12.0 - size_w - name_left).max(0.0);
                    let galley = painter.layout(
                        name,
                        FontId::new(12.0, theme::sans_strong()),
                        theme::TEXT,
                        f32::INFINITY,
                    );
                    let clip = Rect::from_min_max(
                        pos2(name_left, rect.top()),
                        pos2(name_left + name_w, rect.bottom()),
                    );
                    painter.with_clip_rect(clip).galley(
                        pos2(name_left, rect.center().y - galley.size().y / 2.0),
                        galley,
                        theme::TEXT,
                    );
                    widgets::describe(ui, &response, rect, || {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            true,
                            on,
                            file_name(&entry.path),
                        )
                    });
                    if response.clicked() {
                        toggle = Some(entry.path.clone());
                    }
                }
            });

        if let Some(paths) = delete {
            self.delete_copies(&paths);
            return;
        }
        let Some(view) = &mut self.storage else {
            return;
        };
        if let Some(a) = arm {
            view.confirming = a;
        }
        if let Some(path) = toggle {
            view.confirming = None;
            if !view.selected.remove(&path) {
                view.selected.insert(path);
            }
        }
        match select_all {
            Some(true) => {
                view.confirming = None;
                if let Some(entries) = &view.entries {
                    view.selected = entries.iter().map(|e| e.path.clone()).collect();
                }
            }
            Some(false) => {
                view.confirming = None;
                view.selected.clear();
            }
            None => {}
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
                // Removing leaves the library's copy taking up the phone's space; this
                // deletes it too. Asked twice: there is no undo.
                let copies: Vec<PathBuf> = self
                    .selection
                    .selected
                    .iter()
                    .filter(|p| self.is_library_copy(p))
                    .map(|p| photo_engine::copies::source_file(p).into_owned())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                if !copies.is_empty() {
                    let id = ui.id().with("delete copies");
                    let armed = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
                    let label = if armed {
                        format!("delete {}? tap again", copies.len())
                    } else {
                        "remove + delete copy".to_string()
                    };
                    let response = widgets::chip(ui, armed, &label).on_hover_text(
                        "Take off the shelf and delete Duckfoot's copy, edits and all",
                    );
                    if response.clicked() {
                        if armed {
                            self.delete_copies(&copies);
                        }
                        ui.data_mut(|d| d.insert_temp(id, !armed));
                    }
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
        // Its colour label, then what the file says about it in one line; tapped,
        // everything it says.
        let line = if many {
            String::new()
        } else {
            self.open_info()
                .map(crate::info::one_line)
                .unwrap_or_default()
        };
        let mut tapped = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            self.label_picker(ui);
            if !line.is_empty() {
                ui.add_space(8.0);
                let arrow = if self.info_open { "▾" } else { "▸" };
                tapped = ui
                    .add(
                        egui::Label::new(theme::dim(format!("{arrow} {line}")).size(11.5))
                            .truncate()
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Everything the file says about this photo")
                    .clicked();
            }
        });
        if tapped {
            self.info_open = !self.info_open;
        }
        if !many {
            if let (false, Some(path)) = (line.is_empty(), self.loaded_path.clone()) {
                if self.info_open {
                    egui::Frame::new()
                        .fill(theme::BG_925)
                        .corner_radius(8)
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            self.info_rows(ui, &path);
                            ui.add_space(6.0);
                            self.keywords_editor(ui);
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
        ui.horizontal_wrapped(|ui| {
            for (view, label) in [
                (PresetsView::Looks, "looks"),
                (PresetsView::Snapshots, "snapshots"),
                (PresetsView::History, "history"),
                (PresetsView::Edits, "copy & paste"),
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
            PresetsView::Edits => self.edits_block(ui),
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

    /// The open group: a value over its ruler, the Tools grid, or one tool on its own — its
    /// header, its controls, and the group's row to step on from it. Returns the height it
    /// wants, as [`Self::compact_sheet`] does.
    fn edit_sheet(&mut self, ui: &mut egui::Ui, cap: f32) -> f32 {
        // Editable from the moment a photo is chosen, before it has decoded.
        let loaded = self.can_edit();
        if !loaded {
            let top = ui.cursor().top();
            egui::Frame::new()
                .inner_margin(Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::dim("Open a photo to edit it.").size(13.0));
                        if widgets::button(ui, Kind::Primary, "Open", 0.0).clicked() {
                            self.pick_and_open(ui.ctx());
                        }
                    });
                });
            return ui.cursor().top() - top;
        }
        // The bar shows the category of a tool open, however it was reached. A value left
        // chosen in another category does not pull it back: that category's row shows,
        // with nothing under the ruler until one of its chips is chosen.
        if let Some(chip) = self.current_chip().filter(|c| c.is_tool()) {
            if !self.compact_category.chips().contains(&chip) {
                match Category::ALL
                    .into_iter()
                    .find(|c| c.chips().contains(&chip))
                {
                    Some(cat) => self.compact_category = cat,
                    None => {
                        if let Some(id) = chip.module() {
                            self.compact_category = Category::of(id);
                        }
                    }
                }
            }
        }
        let view = if self.cutout_studio.open {
            ToolView::Cutout
        } else if self.customize_open {
            ToolView::Modules
        } else if self.compact_tab == Some(Tab::Presets) {
            ToolView::Looks
        } else if self.compact_controls_open {
            ToolView::Module(self.compact_module)
        } else if self.compact_category.is_grid() {
            return self.tools_grid(ui);
        } else {
            return self.ruler_sheet(ui);
        };
        self.tool_frame(ui, cap, view)
    }

    /// The chosen value over a ruler that turns it, over the group's row. Nothing scrolls:
    /// it is the same height whatever is chosen, so the photo above stays put.
    fn ruler_sheet(&mut self, ui: &mut egui::Ui) -> f32 {
        let top = ui.cursor().top();
        let chip = self.current_chip().filter(|c| {
            !c.is_tool() && c.module().map(Category::of) == Some(self.compact_category)
        });
        let white = self.white_scale();
        let width = ui.available_width();
        match chip {
            Some(Chip::Knob(id, i)) if crate::knobs::knobs(id).get(i).is_some() => {
                let k = &crate::knobs::knobs(id)[i];
                let value = k.get(&self.stack, &white);
                let home = k.get(&self.baseline, &white);
                let range = k.range(&self.stack);
                let changed = (value - home).abs() > (range.1 - range.0) * 1e-4;
                let label = Chip::Knob(id, i).label();
                let unit = Chip::Knob(id, i).unit();
                let auto = auto_for(Chip::Knob(id, i)).filter(|_| self.ready_image().is_some());
                let mut reset = false;
                let mut more = false;
                let mut run_auto = false;
                // `NAME      +0.27 EV      [AUTO] [⟲] [⋯]`
                ui.allocate_ui_with_layout(
                    vec2(width, READOUT_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(vec2(width, READOUT_H));
                        let row = ui.max_rect();
                        // The controls first, from the right, so the value can be centred
                        // in what they and the name leave.
                        let controls = ui
                            .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.add_space(4.0);
                                more = bar_icon(
                                    ui,
                                    BarIcon::More,
                                    false,
                                    "All of this tool's settings",
                                )
                                .clicked();
                                reset = ui
                                    .add_enabled_ui(changed, |ui| {
                                        bar_icon(ui, BarIcon::Reset, false, "Reset")
                                    })
                                    .inner
                                    .clicked();
                                if let Some((_, tip)) = auto {
                                    run_auto = auto_button(ui, tip).clicked();
                                }
                            })
                            .response
                            .rect;
                        let painter = ui.painter();
                        let name = painter.text(
                            pos2(row.left() + 16.0, row.center().y),
                            Align2::LEFT_CENTER,
                            label.to_uppercase(),
                            FontId::new(11.5, theme::mono_bold()),
                            theme::TEXT_DIM,
                        );
                        let galley = painter.layout_no_wrap(
                            format!("{}{unit}", k.format(value)),
                            FontId::new(20.0, theme::mono_bold()),
                            if changed {
                                theme::ACCENT
                            } else {
                                theme::TEXT_HI
                            },
                        );
                        // Centred on the row when it fits there, else in the gap.
                        let (lo, hi) = (name.right() + 8.0, controls.left() - 4.0);
                        let half = galley.size().x / 2.0;
                        let x = row.center().x.clamp(lo + half, (hi - half).max(lo + half));
                        painter.galley(
                            pos2(x - half, row.center().y - galley.size().y / 2.0),
                            galley,
                            Color32::PLACEHOLDER,
                        );
                    },
                );
                let snapshot = self.stack.clone();
                let mut started = false;
                let mut next = None;
                let mut choose = 0;
                let mut tab = 0;
                let mut choosing = false;
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(16, 0))
                    .show(ui, |ui| {
                        let turned =
                            categories::ruler(ui, label, value, range, home, self.fine_adjust);
                        started = turned.started;
                        next = turned.value;
                        choose = turned.choose;
                        tab = turned.tab;
                        choosing = turned.choosing;
                    });
                if run_auto {
                    if let Some((sample, _)) = auto {
                        self.pending_sample = Some(sample);
                    }
                }
                if reset {
                    started = true;
                    next = Some(home);
                    self.photo_touch.log.reset += 1;
                }
                if let Some(v) = next {
                    k.set(id, &mut self.stack, &white, v);
                    self.focused_section = Some(id);
                    self.needs_render = true;
                }
                if started {
                    self.history.checkpoint(&snapshot);
                }
                if more {
                    self.compact_controls_open = true;
                }
                // Up or down on the ruler walks the value's list, as on the photo, and a
                // drift sideways moves between its tabs; the list shows over the photo
                // while the finger is on it.
                if tab != 0 {
                    self.step_section(tab);
                }
                self.step_choice(choose);
                if choosing {
                    self.show_photo_readout(true);
                }
            }
            _ => {
                // Nothing chosen yet, or only tools to choose from: say so, at the
                // height the value and ruler take, so the row below stays where it is.
                let (rect, _) =
                    ui.allocate_exact_size(vec2(width, READOUT_H + RULER_H), Sense::hover());
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "Choose a tool below",
                    FontId::new(13.0, theme::sans_strong()),
                    theme::TEXT_DIM,
                );
            }
        }
        self.chip_row(ui);
        ui.cursor().top() - top
    }

    /// Tools: every tool that is not one value, as a grid of tiles to tap, all in view at
    /// once. A tile opens its tool in the grid's place, and back returns here.
    fn tools_grid(&mut self, ui: &mut egui::Ui) -> f32 {
        use InspectorSectionId as M;
        let top = ui.cursor().top();
        let white = self.white_scale();
        let mut picked = None;
        egui::Frame::new()
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                let gap = 8.0;
                let cols = (((ui.available_width() + gap) / 128.0).floor() as usize).clamp(2, 4);
                let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
                ui.spacing_mut().item_spacing = vec2(gap, gap);
                for row in Category::Tools.chips().chunks(cols) {
                    ui.horizontal(|ui| {
                        for chip in row {
                            let changed = match *chip {
                                Chip::Tool(M::Scope) => false,
                                Chip::Tool(id) => {
                                    id.is_on(&self.stack)
                                        && summarise(id, &self.stack, &self.baseline, &white).1
                                }
                                _ => false,
                            };
                            let tile = categories::tool_tile(ui, *chip, vec2(w, 58.0), changed);
                            if tile.clicked() {
                                picked = Some(*chip);
                            }
                        }
                    });
                }
            });
        self.compact_rects.strip = Rect::NOTHING;
        if let Some(chip) = picked {
            self.choose_chip(chip);
        }
        ui.cursor().top() - top
    }

    /// The open group's row: its sections (Tune), or its chips to swipe along, the chosen
    /// one filled amber and scrolled into view when it changes.
    fn chip_row(&mut self, ui: &mut egui::Ui) {
        let y = ui.cursor().top();
        ui.painter()
            .hline(ui.max_rect().x_range(), y, Stroke::new(1.0, theme::BORDER));
        let cat = self.compact_category;
        let current = self.current_chip();
        let white = self.white_scale();
        let stack = &self.stack;
        let baseline = &self.baseline;
        // Whether what a chip names differs from how the photo opened.
        let changed = |chip: Chip| match chip {
            Chip::Knob(id, i) => crate::knobs::knobs(id).get(i).is_some_and(|k| {
                let (lo, hi) = k.range(stack);
                (k.get(stack, &white) - k.get(baseline, &white)).abs() > (hi - lo) * 1e-4
            }),
            Chip::Tool(id) => id.is_on(stack) && summarise(id, stack, baseline, &white).1,
            Chip::Looks | Chip::Cutout => false,
        };
        let marks: Vec<bool> = cat.chips().iter().map(|c| changed(*c)).collect();
        if !cat.sections().is_empty() {
            // A group in named stretches (Tune) shows them, a dot per value, in place of a
            // row of chips to scroll: the values are walked by swiping, not by finding pills.
            let row = Rect::from_min_size(
                pos2(ui.max_rect().left(), y + 1.0),
                vec2(ui.max_rect().width(), CHIPS_H),
            );
            // Registered before the sections, so a tap still reaches them.
            let step = swipe(ui, row, ui.id().with("section_swipe"), false);
            // With a value's ⋯ open, its dot stays lit.
            let chosen = current
                .and_then(|c| cat.chips().iter().position(|x| *x == c))
                .or_else(|| {
                    let (i, _) = self.photo_knob()?;
                    let knob = Chip::Knob(self.compact_module, i);
                    cat.chips().iter().position(|x| *x == knob)
                });
            let tapped = categories::section_bar(ui, row, cat, chosen, &marks);
            ui.advance_cursor_after_rect(row);
            self.compact_rects.strip = row;
            if let Some(step) = step {
                // Tune's sections are tabs: a swipe moves to the next one.
                if cat.swipes_by_section() {
                    self.step_section(step);
                } else {
                    self.step_module(step);
                }
            } else if let Some(s) = tapped {
                let range = cat.section_range(s);
                if chosen.is_some_and(|i| range.contains(&i)) && !self.compact_controls_open {
                    // Already there: show where in its list.
                    self.show_photo_readout(true);
                } else {
                    self.choose_chip(cat.chips()[range.start]);
                }
            }
            return;
        }
        let reveal_id = ui.id().with("chip_revealed");
        let revealed: Option<Chip> = ui.data(|d| d.get_temp::<Option<Chip>>(reveal_id)).flatten();
        let mut picked = None;
        // A fixed band, so the chips are centred on it whatever the scroll area adds.
        let row = Rect::from_min_size(
            pos2(ui.max_rect().left(), y + 1.0),
            vec2(ui.max_rect().width(), CHIPS_H),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(("compact_chips", cat.label()))
                .auto_shrink([false, false])
                .max_height(CHIPS_H)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .scroll_source(ScrollSource {
                    drag: DragScroll::Always,
                    ..Default::default()
                })
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.add_space(12.0);
                        for (chip, mark) in cat.chips().iter().zip(&marks) {
                            let on = current == Some(*chip);
                            let r = categories::chip(ui, chip.label(), on, *mark, chip.is_tool());
                            if on && revealed != Some(*chip) {
                                r.scroll_to_me(None);
                            }
                            if r.clicked() {
                                picked = Some(*chip);
                            }
                        }
                        ui.add_space(12.0);
                    });
                });
        });
        ui.advance_cursor_after_rect(row);
        if revealed != current {
            ui.data_mut(|d| d.insert_temp(reveal_id, current));
        }
        self.compact_rects.strip = row;
        if let Some(chip) = picked {
            // The chosen tool again closes it, back to the ruler.
            if chip.is_tool() && current == Some(chip) {
                self.close_tool();
            } else {
                self.choose_chip(chip);
            }
        }
    }

    /// Leave the open tool for the ruler (or the Tools grid).
    fn close_tool(&mut self) {
        match self.current_chip() {
            // Looks is a group of its own: leaving it gives the photo the screen.
            Some(Chip::Looks) => self.compact_tab = None,
            Some(Chip::Cutout) => self.cutout_studio.open = false,
            _ => self.compact_controls_open = false,
        }
        self.customize_open = false;
    }

    /// One tool open: its header, its controls (scrolled when they run past the cap), and
    /// under them its category's chips, to move on without going back.
    fn tool_frame(&mut self, ui: &mut egui::Ui, cap: f32, view: ToolView) -> f32 {
        let top = ui.cursor().top();
        // A sideways swipe on its controls steps on to the next in its group; the group's row
        // shows under it only where it has no row of pills to scroll (Tune's and Crop's
        // sections). A tool from the grid goes back to the grid instead.
        let steps = !matches!(view, ToolView::Modules | ToolView::Looks);
        let chips = steps && !self.compact_category.sections().is_empty();
        // Looks is a group of its own, its sheet its whole content: no header over it.
        if view != ToolView::Looks {
            self.tool_header(ui, view);
        }
        let y = ui.cursor().top();
        ui.painter()
            .hline(ui.max_rect().x_range(), y, Stroke::new(1.0, theme::BORDER));
        // The panel can be shorter than the cap on a small screen. What is under the
        // controls, and the spacing either side of them, comes off what is left.
        let room = cap.min(ui.available_height() + (y - top));
        let gap = ui.spacing().item_spacing.y;
        let below = if chips { CHIPS_H + 1.0 + gap } else { 0.0 };
        let body_h = (top + room - ui.cursor().top() - below - gap - 2.0).max(80.0);
        // Where the controls showed last frame: the swipe below keeps inside it. Its clip
        // is laid out for the most the controls may take, and reaches over the chips
        // when they take less.
        let shown_id = ui.id().with(("tool_shown", view.salt()));
        let last_shown: Option<Rect> = ui.data(|d| d.get_temp(shown_id));
        let body = egui::ScrollArea::vertical()
            .id_salt(("compact_tool", view.salt()))
            .auto_shrink([false, true])
            .max_height(body_h)
            .scroll_source(ScrollSource {
                drag: DragScroll::Always,
                ..Default::default()
            })
            .show(ui, |ui| {
                // A sideways flick on anything that is not a control moves to the next
                // chip.
                if steps {
                    // No lower than the body is given: the clip rect can reach past it,
                    // over the row below, once the controls scroll.
                    let mut area = ui.clip_rect().intersect(ui.max_rect());
                    area.max.y = area.max.y.min(area.min.y + body_h);
                    if let Some(last) = last_shown {
                        area = area.intersect(last);
                    }
                    if let Some(step) = swipe(ui, area, ui.id().with("module_swipe"), false) {
                        self.step_module(step);
                    }
                }
                egui::Frame::new()
                    .inner_margin(Margin {
                        left: 14,
                        right: 14,
                        top: 4,
                        bottom: 4,
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        match view {
                            ToolView::Module(id) => self.single_module(ui, id),
                            ToolView::Modules => self.compact_modules_list(ui),
                            ToolView::Cutout => self.cutout_controls(ui),
                            ToolView::Looks => self.presets_sheet(ui),
                        }
                    });
            });
        let hidden = (body.content_size.y - body.inner_rect.height()).max(0.0);
        // No lower than where the chips begin: the scroll area's own rect can reach past
        // the height it was given.
        let mut shown = body.inner_rect;
        shown.max.y = shown.max.y.min(ui.cursor().top());
        if last_shown != Some(shown) {
            ui.data_mut(|d| d.insert_temp(shown_id, shown));
        }
        if chips {
            self.chip_row(ui);
        }
        ui.cursor().top() - top + hidden
    }

    /// `[‹]  title / summary   [⟲] [on/off]`: back to every tool, what this one is and
    /// what it does now, and putting it back. A module's switch and reset are each an undo
    /// step.
    fn tool_header(&mut self, ui: &mut egui::Ui, view: ToolView) {
        let (title, summary, modified, on) = match view {
            ToolView::Module(id) => {
                let defaults = self.baseline.clone();
                let white = self.white_scale();
                let (summary, modified) = summarise(id, &self.stack, &defaults, &white);
                (
                    id.title().to_string(),
                    summary,
                    modified,
                    id.is_on(&self.stack),
                )
            }
            ToolView::Modules => (
                "tools".into(),
                "which show, and in what order".into(),
                false,
                true,
            ),
            ToolView::Cutout => (
                "remove background".into(),
                "paint the photo to keep or erase".into(),
                false,
                true,
            ),
            ToolView::Looks => (
                crate::i18n::tr("tab-presets").into(),
                "looks, snapshots, history and copy & paste".into(),
                false,
                true,
            ),
        };
        let snapshot = self.stack.clone();
        let mut changed = false;
        let width = ui.available_width();
        ui.allocate_ui_with_layout(
            vec2(width, TOOL_BAR),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(vec2(width, TOOL_BAR));
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_space(4.0);
                if bar_icon(ui, BarIcon::Back, false, "Back to the ruler").clicked() {
                    self.close_tool();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.add_space(6.0);
                    if let ToolView::Module(id) = view {
                        if let Some(enabled) = id.enabled_mut(&mut self.stack) {
                            if widgets::switch(ui, enabled).changed() {
                                changed = true;
                            }
                        }
                        if modified
                            && bar_icon(ui, BarIcon::Reset, false, "Reset this tool").clicked()
                        {
                            self.reset_module(id);
                            changed = true;
                        }
                    }
                    // What is left between the back arrow and the controls: the name over
                    // what the tool does now.
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    let painter = ui.painter().with_clip_rect(rect);
                    let colour = if on { theme::ACCENT } else { theme::TEXT_DIM };
                    painter.text(
                        pos2(rect.left() + 4.0, rect.center().y - 9.0),
                        Align2::LEFT_CENTER,
                        &title,
                        FontId::new(16.0, theme::sans_strong()),
                        colour,
                    );
                    let sub = if !on {
                        theme::TEXT_FAINT
                    } else if modified {
                        theme::ACCENT
                    } else {
                        theme::TEXT_DIM
                    };
                    painter.text(
                        pos2(rect.left() + 4.0, rect.center().y + 10.0),
                        Align2::LEFT_CENTER,
                        &summary,
                        FontId::new(11.5, theme::mono_bold()),
                        sub,
                    );
                });
            },
        );
        if changed {
            if let ToolView::Module(id) = view {
                self.focused_section = Some(id);
            }
            self.history.checkpoint(&snapshot);
            self.needs_render = true;
        }
    }

    /// The cutout's controls in the Tools sheet, its preview on the photo above.
    fn cutout_controls(&mut self, ui: &mut egui::Ui) {
        let (changed, export) = self.cutout_studio.controls(ui);
        if changed {
            self.cutout_studio.update_preview_texture(ui.ctx());
        }
        if export {
            self.export_cutout(ui.ctx());
        }
    }

    /// Where the current chip is in the open category. A tool opened from a value's ⋯
    /// counts as that value's chip.
    fn current_chip_index(&self) -> Option<usize> {
        let chips = self.compact_category.chips();
        let c = self.current_chip()?;
        chips
            .iter()
            .position(|x| *x == c)
            .or_else(|| chips.iter().position(|x| x.module() == c.module()))
    }

    /// Move to the first value of the next (`+1`) or previous (`-1`) section of the open
    /// category, stopping at its ends: Tune's Light, Colour and Detail, as tabs.
    pub(crate) fn step_section(&mut self, step: i32) {
        let cat = self.compact_category;
        let Some(i) = self.current_chip_index() else {
            return;
        };
        let s = cat.section_of(i) as i32 + step;
        if let Ok(s) = usize::try_from(s) {
            if s < cat.sections().len() {
                self.choose_chip(cat.chips()[cat.section_range(s).start]);
                self.show_photo_readout(true);
            }
        }
    }

    /// Move to the next (`+1`) or previous (`-1`) chip in the open category, stopping at
    /// its ends. A tool stays open if the chip it lands on is one.
    pub(crate) fn step_module(&mut self, step: i32) {
        let chips = self.compact_category.chips();
        let Some(i) = self.current_chip_index() else {
            if let Some(first) = chips.first() {
                self.choose_chip(*first);
            }
            return;
        };
        let j = i as i32 + step;
        if let Some(next) = usize::try_from(j).ok().and_then(|j| chips.get(j)) {
            self.choose_chip(*next);
            if let Some(id) = next.module() {
                self.reveal_section = Some(id);
            }
        }
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

    /// One module's controls. The same checkpointing as the desktop's pipeline, so a
    /// gesture is one undo step; its switch and reset are in [`Self::tool_header`].
    fn single_module(&mut self, ui: &mut egui::Ui, id: InspectorSectionId) {
        let snapshot = self.stack.clone();
        let mut started = false;
        let mut changed = false;
        self.module_body(ui, id, &mut changed, &mut started);
        if changed {
            // As on the desktop: touching a control turns its module on.
            if let Some(on) = id.enabled_mut(&mut self.stack) {
                *on = true;
            }
            self.focused_section = Some(id);
            self.needs_render = true;
        }
        if started {
            self.history.checkpoint(&snapshot);
        }
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
            // One photo at a time has no previous photo and no shelf to paste to: a look
            // copied here pastes onto the next photo opened.
            if !self.single_photo() {
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

    // ── Export ──────────────────────────────────────────────────────────────────────────

    /// Export, as a short list: the one big button, then a row for each other way out —
    /// the file's format and size, removing the background (in its own tool, where it can
    /// be seen), sharing what was just written, and many photos at once.
    fn export_sheet(&mut self, ui: &mut egui::Ui) {
        let source = self.export_source_size();
        let width = ui.available_width();
        let busy = matches!(self.export, Export::Writing { .. });
        let ready = self.ready_image().is_some() && !busy;
        let cutout = self.cutout_studio.open;
        ui.label(
            RichText::new(crate::export_ui::summary(&self.export_settings, source))
                .family(theme::mono_bold())
                .size(12.0)
                .color(theme::ACCENT),
        );
        let label = if cutout {
            "Export the cutout"
        } else {
            "Export this photo"
        };
        if widgets::button_sized(ui, ready, Kind::Primary, label, width, 48.0).clicked() {
            if cutout {
                self.export_cutout(ui.ctx());
            } else {
                self.pick_and_export(ui.ctx());
            }
        }
        let share = self.export_feedback(ui);

        ui.add_space(4.0);
        let options = if self.export_options_open {
            "hide the format, size and quality"
        } else {
            "format, size, quality, colour and metadata"
        };
        if export_row(ui, true, "Options", options, self.export_options_open).clicked() {
            self.export_options_open = !self.export_options_open;
        }
        if self.export_options_open {
            egui::Frame::new()
                .fill(theme::BG_925)
                .corner_radius(8)
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut settings = self.export_settings;
                    crate::export_ui::settings_editor_with(ui, &mut settings, source, false);
                    self.export_settings = settings;
                });
        }
        let (title, sub) = if cutout {
            ("Remove background", "on: its preview is on the photo")
        } else {
            ("Remove background", "cut the subject out, with a preview")
        };
        if export_row(ui, self.can_edit(), title, sub, cutout).clicked() {
            self.cutout_studio.open = true;
            self.compact_tab = Some(Tab::Edit);
        }
        if let Some(path) = share {
            if export_row(
                ui,
                true,
                "Share",
                &format!("send {} to another app", file_name(&path)),
                false,
            )
            .clicked()
            {
                self.request(ui.ctx(), crate::platform::Request::Share { path });
            }
        }
        if self.single_photo() {
            return;
        }
        let n = self.selection.count();
        let sub = if n > 1 {
            format!("the {n} photos selected in Library")
        } else {
            "several photos: choose them with select in Library".to_string()
        };
        if export_row(ui, true, "Batch export", &sub, false).clicked() {
            self.batch_dialog_open = true;
        }
    }

    /// How the last export went, under the button; the file just written, if it can be
    /// shared.
    fn export_feedback(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let (text, colour, share) = match &self.export {
            Export::Writing { path, started, .. } => (
                format!(
                    "writing {} ({:.1}s)…",
                    file_name(path),
                    started.elapsed().as_secs_f32()
                ),
                theme::ACCENT,
                None,
            ),
            Export::Done { path, seconds } => (
                format!("exported {} in {seconds:.1}s", file_name(path)),
                theme::LIME,
                self.platform.can_share().then(|| path.clone()),
            ),
            Export::Failed(e) => (format!("failed: {e}"), theme::DANGER, None),
            Export::Idle => return None,
        };
        ui.horizontal_wrapped(|ui| {
            widgets::dot(ui, colour);
            ui.label(
                RichText::new(text)
                    .family(theme::mono_bold())
                    .size(12.0)
                    .color(colour),
            );
        });
        share
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
                            if widgets::chip(ui, true, a.label()).clicked() {
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
    /// The tour (G-04): coach marks on the real photo, each waiting for its gesture. The
    /// screen dims around what the step is about, which still takes touches; the card sits
    /// on the other side of it.
    fn tour(&mut self, ui: &mut egui::Ui) {
        let Some(step) = self.tour_step else { return };
        let Some(s) = TOUR.get(step) else {
            self.end_tour();
            return;
        };
        if self.tour_mark.as_ref().is_none_or(|m| m.step != step) {
            if let Some(tab) = s.tab {
                self.compact_tab = Some(tab);
            }
            // The photo turns a value, and the chips are under the ruler: a value it is.
            if s.goal.on_photo() || s.goal == Goal::Module {
                self.customize_open = false;
                if !matches!(self.current_chip(), Some(Chip::Knob(..))) {
                    self.choose_chip(Chip::Knob(InspectorSectionId::Exposure, 0));
                }
            }
            self.tour_mark = Some(TourMark {
                step,
                log: self.photo_touch.log,
                tab: self.compact_tab,
                category: self.compact_category,
                chip: self.current_chip(),
            });
        }
        let mark = self.tour_mark.clone().expect("set above");
        let log = self.photo_touch.log;
        let done = match s.goal {
            Goal::Tab => self.compact_tab != mark.tab || self.compact_category != mark.category,
            Goal::Choose => log.chose > mark.log.chose,
            Goal::Adjust => log.adjusted > mark.log.adjusted,
            Goal::Reset => log.reset > mark.log.reset,
            Goal::Hold => log.held > mark.log.held,
            Goal::Module => self.current_chip() != mark.chip,
            Goal::Done => false,
        };
        if done {
            self.tour_step = Some(step + 1);
            ui.ctx().request_repaint();
            return;
        }
        // Gestures on a photo need one open.
        let possible = !s.goal.on_photo() || self.ready_image().is_some();

        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        let r = self.compact_rects;
        let target = match s.target {
            Target::Nav => r.nav,
            Target::Stage => r.stage,
            Target::Strip => r.strip,
            Target::Nothing => Rect::NOTHING,
        }
        .intersect(screen);
        let mut next = false;
        let mut skip = false;
        egui::Area::new(egui::Id::new("duckfoot_tour"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(&ctx, |ui| {
                let dim = Color32::from_black_alpha(170);
                let bands = if target.is_positive() {
                    vec![
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
                    ]
                } else {
                    vec![screen]
                };
                // The dim takes the touches around the target; the target keeps its own.
                for (i, band) in bands.iter().enumerate() {
                    if band.is_positive() {
                        ui.painter().rect_filled(*band, 0, dim);
                        ui.interact(
                            *band,
                            egui::Id::new(("tour_dim", i)),
                            Sense::click_and_drag(),
                        );
                    }
                }
                if target.is_positive() {
                    ui.painter().rect_stroke(
                        target.shrink(1.0),
                        4,
                        Stroke::new(2.0, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }

                // The card sits on whichever side of the target has more room.
                let w = (screen.width() - 32.0).min(360.0);
                let h_id = egui::Id::new("duckfoot_tour_card_h");
                let h: f32 = ui.data(|d| d.get_temp(h_id)).unwrap_or(140.0);
                let y = if !target.is_positive() {
                    screen.center().y - h / 2.0
                } else if target.top() - screen.top() > screen.bottom() - target.bottom() {
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
                                RichText::new(crate::i18n::tr(s.title_id))
                                    .family(theme::sans_strong())
                                    .size(16.0)
                                    .color(theme::TEXT_HI),
                            );
                            ui.label(
                                RichText::new(crate::i18n::tr(if possible {
                                    s.text_id
                                } else {
                                    s.fallback_id
                                }))
                                .size(13.0)
                                .color(theme::TEXT),
                            );
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if widgets::chip(ui, false, crate::i18n::tr("tour-skip")).clicked()
                                {
                                    skip = true;
                                }
                                // Only a step whose gesture cannot be made here, and the
                                // last, has a button to go on: the others wait for it.
                                if s.goal == Goal::Done || !possible {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let label = crate::i18n::tr(if s.goal == Goal::Done {
                                                "tour-done"
                                            } else {
                                                "tour-next"
                                            });
                                            if widgets::button(ui, Kind::Primary, label, 72.0)
                                                .clicked()
                                            {
                                                next = true;
                                            }
                                        },
                                    );
                                }
                            });
                        })
                        .response
                        .rect
                });
                let drawn = shown.inner;
                if (drawn.height() - h).abs() > 0.5 {
                    ui.data_mut(|d| d.insert_temp(h_id, drawn.height()));
                    ui.ctx().request_repaint();
                }
                self.tour_card = Some((drawn, target));
            });
        if skip {
            self.skip_tour();
        } else if next {
            if step + 1 >= TOUR.len() {
                self.end_tour();
            } else {
                self.tour_step = Some(step + 1);
            }
        }
    }

    /// Where the row of chips was drawn last frame.
    pub fn strip_rect(&self) -> Rect {
        self.compact_rects.strip
    }

    /// Where the phone layout's sheet was drawn last frame (nothing while it is closed).
    pub fn sheet_rect(&self) -> Rect {
        self.compact_rects.sheet
    }

    /// Close the tour and remember it was seen.
    pub(crate) fn end_tour(&mut self) {
        self.tour_step = None;
        self.tour_mark = None;
        self.tour_card = None;
        if !self.settings.tour_done || self.settings.tour_again {
            self.settings.tour_done = true;
            self.settings.tour_again = false;
            self.save_settings();
        }
    }

    /// Skipped: closed, with a notice that offers it once more on the next launch.
    fn skip_tour(&mut self) {
        self.end_tour();
        self.set_status_undoable(
            crate::i18n::tr("tour-skipped"),
            crate::NoticeAction::TourAgain,
        );
    }
}

/// What a tour step points at.
#[derive(Clone, Copy)]
enum Target {
    Nav,
    Stage,
    /// The open group's row: Tune's sections.
    Strip,
    Nothing,
}

/// The gesture a step waits for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Goal {
    Tab,
    Choose,
    Adjust,
    Reset,
    Hold,
    Module,
    /// The last card: a button closes it.
    Done,
}

impl Goal {
    fn on_photo(self) -> bool {
        matches!(self, Goal::Choose | Goal::Adjust | Goal::Reset | Goal::Hold)
    }
}

/// Where the app stood when a step began: what its gesture must change.
#[derive(Clone)]
pub(crate) struct TourMark {
    step: usize,
    log: crate::on_photo::GestureLog,
    tab: Option<Tab>,
    category: Category,
    chip: Option<Chip>,
}

struct TourStep {
    /// Message ids, in `locales/`.
    title_id: &'static str,
    text_id: &'static str,
    /// Said instead when the gesture cannot be made yet (no photo open).
    fallback_id: &'static str,
    target: Target,
    /// The tab to open as the step begins, if it is about one.
    tab: Option<Tab>,
    goal: Goal,
}

pub(crate) const TOUR_LEN: usize = TOUR.len();

const TOUR: [TourStep; 7] = [
    TourStep {
        title_id: "tour-tabs-title",
        text_id: "tour-tabs-text",
        fallback_id: "tour-tabs-text",
        target: Target::Nav,
        tab: None,
        goal: Goal::Tab,
    },
    TourStep {
        title_id: "tour-choose-title",
        text_id: "tour-choose-text",
        fallback_id: "tour-needs-photo",
        target: Target::Stage,
        tab: Some(Tab::Edit),
        goal: Goal::Choose,
    },
    TourStep {
        title_id: "tour-adjust-title",
        text_id: "tour-adjust-text",
        fallback_id: "tour-needs-photo",
        target: Target::Stage,
        tab: Some(Tab::Edit),
        goal: Goal::Adjust,
    },
    TourStep {
        title_id: "tour-reset-title",
        text_id: "tour-reset-text",
        fallback_id: "tour-needs-photo",
        target: Target::Stage,
        tab: Some(Tab::Edit),
        goal: Goal::Reset,
    },
    TourStep {
        title_id: "tour-hold-title",
        text_id: "tour-hold-text",
        fallback_id: "tour-needs-photo",
        target: Target::Stage,
        tab: Some(Tab::Edit),
        goal: Goal::Hold,
    },
    TourStep {
        title_id: "tour-strip-title",
        text_id: "tour-strip-text",
        fallback_id: "tour-strip-text",
        target: Target::Strip,
        tab: Some(Tab::Edit),
        goal: Goal::Module,
    },
    TourStep {
        title_id: "tour-end-title",
        text_id: "tour-end-text",
        fallback_id: "tour-end-text",
        target: Target::Nothing,
        tab: None,
        goal: Goal::Done,
    },
];

/// The one-tap "auto" beside a value, where there is one for it: tone for light, white
/// balance for colour, upright for straightening. With what it does, for its tip.
fn auto_for(chip: Chip) -> Option<(crate::Sample, &'static str)> {
    use crate::Sample;
    use photo_engine::keystone::Upright;
    use InspectorSectionId as M;
    let Chip::Knob(id, _) = chip else {
        return None;
    };
    match id {
        M::Exposure | M::Tonal | M::Dehaze | M::Vignette => Some((
            Sample::AutoTone,
            "Auto tone: exposure, highlights and shadows",
        )),
        M::WhiteBalance | M::Vibrance => Some((Sample::AutoWb, "Auto white balance")),
        M::Geometry if chip == Chip::Knob(M::Geometry, 0) => {
            Some((Sample::Upright(Upright::Level), "Auto straighten"))
        }
        M::Geometry => Some((
            Sample::Upright(Upright::Full),
            "Auto upright: straighten and stand the lines up",
        )),
        _ => None,
    }
}

/// "AUTO", in a 44-point target on the readout's line.
fn auto_button(ui: &mut egui::Ui, tip: &str) -> egui::Response {
    let (rect, response) = widgets::allocate_control(ui, vec2(56.0, 44.0), Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip)
    });
    let hot = response.hovered() || response.is_pointer_button_down_on();
    let pill = Rect::from_center_size(rect.center(), vec2(50.0, 26.0));
    let painter = ui.painter();
    painter.rect_stroke(
        pill,
        13.0,
        Stroke::new(1.0, if hot { theme::ACCENT } else { theme::BORDER_HI }),
        egui::StrokeKind::Inside,
    );
    painter.text(
        pill.center(),
        Align2::CENTER_CENTER,
        "AUTO",
        FontId::new(11.0, theme::mono_bold()),
        if hot { theme::ACCENT } else { theme::TEXT },
    );
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(tip)
}

/// The top bar's icons.
#[derive(Clone, Copy)]
enum BarIcon {
    Undo,
    Redo,
    Reset,
    /// Three dots: more.
    More,
    /// A chevron pointing left: back.
    Back,
    /// A plus: another photo.
    Open,
}

/// A top-bar icon: painted strokes in a 44 × 44 target, all one size and centred on the
/// bar's line, as the font's glyphs are not.
fn bar_icon(ui: &mut egui::Ui, icon: BarIcon, active: bool, tip: &str) -> egui::Response {
    let enabled = ui.is_enabled();
    let (rect, response) = widgets::allocate_control(ui, vec2(44.0, 44.0), Sense::click());
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
    let stroke = Stroke::new(2.0, colour);
    match icon {
        // A hook over the top, arrowhead down at the end it turns back to.
        BarIcon::Undo => arc_arrow(painter, c + vec2(0.0, 2.0), 7.0, 30.0, -180.0, stroke),
        BarIcon::Redo => arc_arrow(painter, c + vec2(0.0, 2.0), 7.0, 150.0, 360.0, stroke),
        // Most of a circle, turning back on itself.
        BarIcon::Reset => arc_arrow(painter, c, 7.5, 200.0, -95.0, stroke),
        BarIcon::More => {
            for dx in [-7.0, 0.0, 7.0] {
                painter.circle_filled(c + vec2(dx, 0.0), 2.2, colour);
            }
        }
        BarIcon::Back => {
            painter.line_segment([c + vec2(4.0, -8.0), c + vec2(-4.0, 0.0)], stroke);
            painter.line_segment([c + vec2(-4.0, 0.0), c + vec2(4.0, 8.0)], stroke);
        }
        BarIcon::Open => {
            painter.rect_stroke(
                Rect::from_center_size(c, vec2(20.0, 20.0)),
                4.0,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], stroke);
            painter.line_segment([c + vec2(0.0, -5.0), c + vec2(0.0, 5.0)], stroke);
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

/// What a Tools sheet shows: one module, the list of which show, or the cutout.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolView {
    Module(InspectorSectionId),
    Modules,
    Cutout,
    Looks,
}

impl ToolView {
    /// Each its own scroll position.
    fn salt(self) -> String {
        match self {
            ToolView::Module(id) => format!("module {}", id.title()),
            ToolView::Modules => "modules".into(),
            ToolView::Cutout => "cutout".into(),
            ToolView::Looks => "looks".into(),
        }
    }
}

/// A row in the Export sheet: a title over what it does, the whole row the target.
fn export_row(
    ui: &mut egui::Ui,
    enabled: bool,
    title: &str,
    sub: &str,
    on: bool,
) -> egui::Response {
    let enabled = enabled && ui.is_enabled();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), sense);
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, title)
    });
    let painter = ui.painter();
    let hot = enabled && (response.hovered() || response.is_pointer_button_down_on());
    painter.rect(
        rect,
        8,
        if hot { theme::BG_850 } else { theme::BG_900 },
        Stroke::new(
            1.0,
            if on {
                theme::ACCENT.linear_multiply(0.6)
            } else {
                theme::BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    let (main, faint) = if enabled {
        (
            if on { theme::ACCENT } else { theme::TEXT_HI },
            theme::TEXT_DIM,
        )
    } else {
        (theme::TEXT_FAINT, theme::TEXT_FAINT)
    };
    let clip = painter.with_clip_rect(rect.shrink(2.0));
    clip.text(
        pos2(rect.left() + 14.0, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        title,
        FontId::new(14.0, theme::sans_strong()),
        main,
    );
    clip.text(
        pos2(rect.left() + 14.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        sub,
        FontId::proportional(12.0),
        faint,
    );
    clip.text(
        pos2(rect.right() - 16.0, rect.center().y),
        Align2::CENTER_CENTER,
        "›",
        FontId::proportional(20.0),
        faint,
    );
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// A row in the top bar's ⋯ menu: its label, and a tick while it is on.
fn menu_row(ui: &mut egui::Ui, enabled: bool, label: &str, on: Option<bool>) -> egui::Response {
    let enabled = enabled && ui.is_enabled();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), sense);
    widgets::describe(ui, &response, rect, || match on {
        Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, label),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label),
    });
    let painter = ui.painter();
    if enabled && response.hovered() {
        painter.rect_filled(rect, 6, theme::BG_850);
    }
    let colour = if !enabled {
        theme::TEXT_FAINT
    } else if on == Some(true) {
        theme::ACCENT
    } else {
        theme::TEXT_HI
    };
    painter.text(
        pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::new(14.0, theme::sans_strong()),
        colour,
    );
    if let Some(on) = on {
        let box_rect =
            Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(14.0, 14.0));
        widgets::paint_checkbox(painter, box_rect, on, response.hovered());
    }
    response
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
