//! Help on the desktop: the guide, and a tour of the window.
//!
//! The guide is `docs/GUIDE.md`, compiled in: each `## ` heading is one topic in the Help
//! window, so the file people read on the web and the one in the app are the same file and
//! cannot drift. Only the little Markdown the guide uses is understood — paragraphs, `- `
//! and `1. ` lists, `**bold**` and `*italic*` — which is all a plain-language guide needs.
//!
//! The tour points at the window's parts one at a time. Like the phone's, it is offered
//! (Help is lit until it has been seen) and never starts on its own (DESIGN.md).

use crate::*;
use egui::{CursorIcon, Pos2, Response, StrokeKind, Vec2};

const GUIDE: &str = include_str!("../../../docs/GUIDE.md");

/// One topic: a `## ` heading of the guide and what follows it.
pub(crate) struct Topic {
    pub title: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Block {
    Para(String),
    Bullet(String),
    /// A numbered step, with its number as written.
    Step(String, String),
}

/// The guide's topics, in order. What comes before the first `## ` is the guide's own
/// title and introduction, for readers of the file; the app has its own heading.
pub(crate) fn topics() -> Vec<Topic> {
    parse(GUIDE)
}

fn parse(text: &str) -> Vec<Topic> {
    let mut topics: Vec<Topic> = Vec::new();
    let mut in_comment = false;
    // Whether the next line continues the block above (a wrapped line), or starts anew.
    let mut open = false;
    for line in text.lines() {
        // HTML comments are notes for whoever edits the file.
        if in_comment {
            in_comment = !line.contains("-->");
            open = false;
            continue;
        }
        if line.trim_start().starts_with("<!--") {
            in_comment = !line.contains("-->");
            open = false;
            continue;
        }
        if let Some(title) = line.strip_prefix("## ") {
            topics.push(Topic {
                title: title.trim().to_string(),
                blocks: Vec::new(),
            });
            open = false;
            continue;
        }
        let Some(topic) = topics.last_mut() else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            open = false;
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            topic.blocks.push(Block::Bullet(item.to_string()));
        } else if let Some((n, rest)) = numbered(trimmed) {
            topic
                .blocks
                .push(Block::Step(n.to_string(), rest.to_string()));
        } else if open {
            if let Some(Block::Para(s) | Block::Bullet(s) | Block::Step(_, s)) =
                topic.blocks.last_mut()
            {
                s.push(' ');
                s.push_str(trimmed);
            }
        } else {
            topic.blocks.push(Block::Para(trimmed.to_string()));
        }
        open = true;
    }
    topics
}

/// `"3. Do this"` → `("3", "Do this")`.
fn numbered(line: &str) -> Option<(&str, &str)> {
    let (n, rest) = line.split_once(". ")?;
    (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then_some((n, rest))
}

/// A line of the guide as egui text: `**bold**` brighter and heavier, `*italic*` slanted,
/// and Markdown's `\\` as the one backslash it stands for.
fn inline(text: &str, size: f32, width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    let plain = egui::TextFormat::simple(FontId::proportional(size), theme::TEXT);
    let bold = egui::TextFormat::simple(FontId::new(size, theme::sans_strong()), theme::TEXT_HI);
    let italic = egui::TextFormat {
        italics: true,
        ..plain.clone()
    };
    let text = text.replace("\\\\", "\\");
    let mut rest = text.as_str();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                job.append(&after[..end], 0.0, bold.clone());
                rest = &after[end + 2..];
                continue;
            }
        } else if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = after.find('*') {
                job.append(&after[..end], 0.0, italic.clone());
                rest = &after[end + 1..];
                continue;
            }
        }
        // Up to the next marker, or the end. Past the first character whole, which may be
        // more than one byte (⚙, ←).
        let first = rest.chars().next().map_or(1, char::len_utf8);
        let next = rest[first..].find('*').map_or(rest.len(), |i| i + first);
        job.append(&rest[..next], 0.0, plain.clone());
        rest = &rest[next..];
    }
    job
}

/// The parts of the desktop window the tour points at, where they were last frame.
#[derive(Default)]
pub(crate) struct DeskSpots {
    pub stage: Option<Rect>,
    pub toolbar: Option<Rect>,
    pub filmstrip: Option<Rect>,
    pub rail: Option<Rect>,
    pub tabs: Option<Rect>,
    pub footer: Option<Rect>,
    pub export: Option<Rect>,
    /// The whole editing panel, and the tool rail beside it.
    pub inspector: Option<Rect>,
    pub tools: Option<Rect>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Spot {
    Stage,
    Toolbar,
    Filmstrip,
    Rail,
    Tabs,
    Footer,
    Export,
    Tools,
}

struct TourStep {
    spot: Spot,
    title: &'static str,
    body: &'static str,
}

pub(crate) const DESK_TOUR_LEN: usize = TOUR.len();

const TOUR: [TourStep; 8] = [
    TourStep {
        spot: Spot::Stage,
        title: "Your photo",
        body: "The photo you're working on. Scroll over it to zoom in, and drag to look \
               around. Your original file is never changed.",
    },
    TourStep {
        spot: Spot::Filmstrip,
        title: "All your photos",
        body: "Click a picture to open it, or use the arrow keys. The button at its start \
               shows only the photos you want, such as your five-star keepers.",
    },
    TourStep {
        spot: Spot::Rail,
        title: "The left strip",
        body: "The top two buttons hide the shelf or show it as a grid. Below them: your \
               folders, this photo's history and snapshots, and + to add photos.",
    },
    TourStep {
        spot: Spot::Tabs,
        title: "Editing tools",
        body: "The tools are in sections: Light and Colour have what most photos need. Drag \
               a slider to change the photo, and double-click it to reset. A section's \
               switch turns it off to compare. The gear chooses which tools show.",
    },
    TourStep {
        spot: Spot::Tools,
        title: "Tools",
        body: "Looks, Edit, Crop, Spot heal and Masks. Choose one and the panel beside it \
               shows it; Edit is the sections of sliders.",
    },
    TourStep {
        spot: Spot::Toolbar,
        title: "Check your work",
        body: "Crop and straighten, compare before and after with A / B, and Clip shows \
               where bright or dark detail is lost.",
    },
    TourStep {
        spot: Spot::Footer,
        title: "Reuse your edits",
        body: "Copy this photo's edits and paste them onto others. Your work saves by \
               itself as you go.",
    },
    TourStep {
        spot: Spot::Export,
        title: "Share it",
        body: "When you like it, Export saves a copy to share or print. Made a mistake? The \
               curved arrow beside it undoes anything. Help is in the ⋯ menu at its end.",
    },
];

impl PhotoApp {
    pub(crate) fn open_help(&mut self) {
        self.help_open = true;
        self.shortcuts_open = false;
    }

    pub(crate) fn start_desk_tour(&mut self) {
        self.help_open = false;
        self.shortcuts_open = false;
        self.desk_tour_step = Some(0);
    }

    /// Finished or skipped, the tour is seen: Help stops asking for attention.
    pub(crate) fn end_desk_tour(&mut self) {
        self.desk_tour_step = None;
        if !self.settings.desk_tour_done {
            self.settings.desk_tour_done = true;
            self.save_settings();
        }
    }

    /// Esc closes Help or the tour; while the tour shows, ← and → step through it. True
    /// when either is up, so no other key reaches the photo behind them.
    pub(crate) fn help_keys(&mut self, ctx: &egui::Context) -> bool {
        let (esc, next, back) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::ArrowLeft),
            )
        });
        if let Some(step) = self.desk_tour_step {
            if esc {
                self.end_desk_tour();
            } else if next {
                self.tour_step_to(step + 1);
            } else if back {
                self.desk_tour_step = Some(step.saturating_sub(1));
            }
            return true;
        }
        if self.help_open {
            if esc {
                self.help_open = false;
            }
            return true;
        }
        false
    }

    /// Where tour step `step`'s part of the window was last drawn.
    pub(crate) fn desk_spot(&self, step: usize) -> Option<Rect> {
        let spots = &self.desk_spots;
        match TOUR.get(step)?.spot {
            Spot::Stage => spots.stage,
            Spot::Toolbar => spots.toolbar,
            Spot::Filmstrip => spots.filmstrip,
            Spot::Rail => spots.rail,
            Spot::Tabs => spots.tabs,
            Spot::Footer => spots.footer,
            Spot::Export => spots.export,
            Spot::Tools => spots.tools,
        }
    }

    fn tour_step_to(&mut self, step: usize) {
        if step >= TOUR.len() {
            self.end_desk_tour();
        } else {
            self.desk_tour_step = Some(step);
        }
    }

    /// The Help window: topics down the left, the chosen one on the right.
    pub(crate) fn help_window(&mut self, ctx: &egui::Context) {
        if !self.help_open {
            return;
        }
        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(egui::Id::new("help-backdrop"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let r = ui.allocate_rect(screen, Sense::click());
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
                r
            })
            .inner;
        let mut close = backdrop.clicked();
        let topics = topics();
        let size = vec2(
            900.0_f32.min(screen.width() - 32.0),
            640.0_f32.min(screen.height() - 48.0),
        );
        let list_w = 230.0_f32.min(size.x * 0.34);
        // The frame's margins (20 a side) and the gap between the columns.
        let body_w = size.x - list_w - 20.0;
        let mut tour = false;
        let mut shortcuts = false;
        let mut show_me = None;
        let mut go_to = None;
        egui::Area::new(egui::Id::new("help"))
            .order(egui::Order::Tooltip)
            .fade_in(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::BG_900)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(10.0)
                    .inner_margin(Margin::same(20))
                    .show(ui, |ui| {
                        ui.set_width(size.x);
                        ui.set_height(size.y);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Help")
                                    .family(theme::display())
                                    .size(19.0)
                                    .color(theme::TEXT_HI),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if widgets::icon(ui, "×", false, "Close (Esc)").clicked() {
                                        close = true;
                                    }
                                },
                            );
                        });
                        ui.add_space(12.0);
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(list_w);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                egui::ScrollArea::vertical()
                                    .id_salt("help-topics")
                                    .max_height(size.y - 150.0)
                                    .show(ui, |ui| {
                                        let mut last_group = "";
                                        for (i, t) in topics.iter().enumerate() {
                                            // A caption where the kind of topic changes.
                                            let group = topic_group(&t.title);
                                            if group != last_group {
                                                if i > 0 {
                                                    ui.add_space(10.0);
                                                }
                                                ui.label(
                                                    theme::tag(group).color(theme::TEXT_FAINT),
                                                );
                                                ui.add_space(2.0);
                                                last_group = group;
                                            }
                                            if topic_row(ui, i == self.help_topic, &t.title)
                                                .clicked()
                                            {
                                                self.help_topic = i;
                                            }
                                        }
                                    });
                                ui.add_space(12.0);
                                ui.spacing_mut().item_spacing.y = 8.0;
                                tour = widgets::button(ui, Kind::Accent, "Take the tour", list_w)
                                    .on_hover_text("A one-minute walk round the window")
                                    .clicked();
                                shortcuts = widgets::button(
                                    ui,
                                    Kind::Secondary,
                                    "Keyboard shortcuts",
                                    list_w,
                                )
                                .on_hover_text("? or F1")
                                .clicked();
                            });
                            ui.add_space(20.0);
                            let Some(topic) = topics.get(self.help_topic) else {
                                return;
                            };
                            let spot = topic_spot(&topic.title);
                            let step = spot.and_then(|s| TOUR.iter().position(|t| t.spot == s));
                            ui.vertical(|ui| {
                                // A set width, so the text wraps to it rather than widening
                                // the window. Only the text scrolls: the title, the picture
                                // and the buttons under it stay where they are from topic
                                // to topic.
                                ui.set_width(body_w);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.allocate_ui_with_layout(
                                    vec2(body_w, 32.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(&topic.title)
                                                .family(theme::display())
                                                .size(17.0)
                                                .color(theme::TEXT_HI),
                                        );
                                        if let Some(step) = step {
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if widgets::button(
                                                        ui,
                                                        Kind::Accent,
                                                        "Show me",
                                                        0.0,
                                                    )
                                                    .on_hover_text("Point at it in the window")
                                                    .clicked()
                                                    {
                                                        show_me = Some(step);
                                                    }
                                                },
                                            );
                                        }
                                    },
                                );
                                ui.add_space(10.0);
                                let (picture, _) =
                                    ui.allocate_exact_size(vec2(body_w, PICTURE_H), Sense::hover());
                                self.paint_window_map(ui.painter(), picture, spot);
                                ui.add_space(14.0);
                                let footer_h = 44.0;
                                egui::ScrollArea::vertical()
                                    .id_salt(("help-topic", self.help_topic))
                                    .auto_shrink([false, false])
                                    .max_height(
                                        size.y - 60.0 - 32.0 - 10.0 - PICTURE_H - 14.0 - footer_h,
                                    )
                                    .show(ui, |ui| {
                                        topic_body(ui, topic, body_w - 16.0);
                                    });
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    let last = topics.len().saturating_sub(1);
                                    let here = self.help_topic;
                                    if here > 0 {
                                        let back = format!("← {}", topics[here - 1].title);
                                        if widgets::button(ui, Kind::Secondary, &back, 0.0)
                                            .clicked()
                                        {
                                            go_to = Some(here - 1);
                                        }
                                    }
                                    if here < last {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let next = format!("{} →", topics[here + 1].title);
                                                if widgets::button(ui, Kind::Secondary, &next, 0.0)
                                                    .clicked()
                                                {
                                                    go_to = Some(here + 1);
                                                }
                                            },
                                        );
                                    }
                                });
                            });
                        });
                    });
            });
        if let Some(i) = go_to {
            self.help_topic = i;
        }
        if let Some(step) = show_me {
            self.help_open = false;
            self.desk_tour_step = Some(step);
        } else if tour {
            self.start_desk_tour();
        } else if shortcuts {
            self.help_open = false;
            self.shortcuts_open = true;
        } else if close {
            self.help_open = false;
        }
    }

    /// The tour: everything dimmed but the part it is about, and a card beside that.
    pub(crate) fn desk_tour(&mut self, ctx: &egui::Context) {
        let Some(step) = self.desk_tour_step else {
            return;
        };
        let Some(s) = TOUR.get(step) else {
            self.end_desk_tour();
            return;
        };
        let target = self.desk_spot(step).map(|r| r.expand(4.0));
        let screen = ctx.content_rect();

        // The dimming, in four pieces round the target, and a click catcher over all of
        // it: the window is not for using while the tour talks about it.
        egui::Area::new(egui::Id::new("desk-tour-shade"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.allocate_rect(screen, Sense::click());
                let shade = Color32::from_black_alpha(170);
                let p = ui.painter();
                match target {
                    Some(t) => {
                        for r in [
                            Rect::from_min_max(screen.min, pos2(screen.max.x, t.min.y)),
                            Rect::from_min_max(pos2(screen.min.x, t.max.y), screen.max),
                            Rect::from_min_max(pos2(screen.min.x, t.min.y), pos2(t.min.x, t.max.y)),
                            Rect::from_min_max(pos2(t.max.x, t.min.y), pos2(screen.max.x, t.max.y)),
                        ] {
                            p.rect_filled(r, 0.0, shade);
                        }
                        p.rect_stroke(t, 6, Stroke::new(2.0, theme::ACCENT), StrokeKind::Outside);
                    }
                    None => {
                        p.rect_filled(screen, 0.0, shade);
                    }
                }
            });

        let card = vec2(340.0, 190.0);
        let pos = card_place(screen, target, card);
        let mut go = None;
        egui::Area::new(egui::Id::new("desk-tour-card"))
            .order(egui::Order::Tooltip)
            .fade_in(false)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::BG_900)
                    .stroke(Stroke::new(1.0, theme::ACCENT))
                    .corner_radius(8.0)
                    .inner_margin(Margin::same(18))
                    .show(ui, |ui| {
                        ui.set_width(card.x - 36.0);
                        ui.spacing_mut().item_spacing.y = 8.0;
                        ui.label(
                            theme::tag(format!("{} of {}", step + 1, TOUR.len()))
                                .color(theme::ACCENT),
                        );
                        ui.label(
                            RichText::new(s.title)
                                .family(theme::display())
                                .size(17.0)
                                .color(theme::TEXT_HI),
                        );
                        ui.label(RichText::new(s.body).size(14.0).color(theme::TEXT));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if widgets::button(ui, Kind::Secondary, "Skip", 0.0)
                                .on_hover_text("End the tour (Esc)")
                                .clicked()
                            {
                                go = Some(None);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let last = step + 1 == TOUR.len();
                                    let label = if last { "Done" } else { "Next →" };
                                    if widgets::button(ui, Kind::Primary, label, 84.0).clicked() {
                                        go = Some(Some(step + 1));
                                    }
                                    if step > 0
                                        && widgets::button(ui, Kind::Secondary, "← Back", 0.0)
                                            .clicked()
                                    {
                                        go = Some(Some(step - 1));
                                    }
                                },
                            );
                        });
                    });
            });
        match go {
            Some(None) => self.end_desk_tour(),
            Some(Some(n)) => self.tour_step_to(n),
            None => {}
        }
    }
}

/// Beside the target where the card fits: left of it, then right, above, below; centred
/// when there is no target or no side has room.
fn card_place(screen: Rect, target: Option<Rect>, card: Vec2) -> Pos2 {
    let centred = screen.center() - card / 2.0;
    let Some(t) = target else {
        return centred;
    };
    let gap = 16.0;
    let clamp_y = |y: f32| y.clamp(screen.min.y + 8.0, screen.max.y - card.y - 8.0);
    let clamp_x = |x: f32| x.clamp(screen.min.x + 8.0, screen.max.x - card.x - 8.0);
    let candidates = [
        pos2(t.min.x - gap - card.x, clamp_y(t.center().y - card.y / 2.0)),
        pos2(t.max.x + gap, clamp_y(t.center().y - card.y / 2.0)),
        pos2(clamp_x(t.center().x - card.x / 2.0), t.min.y - gap - card.y),
        pos2(clamp_x(t.center().x - card.x / 2.0), t.max.y + gap),
    ];
    candidates
        .into_iter()
        .find(|p| {
            screen
                .shrink(4.0)
                .contains_rect(Rect::from_min_size(*p, card))
        })
        // The stage fills most of the window: the card sits inside it, near the top.
        .unwrap_or_else(|| pos2(clamp_x(t.center().x - card.x / 2.0), t.min.y + 24.0))
}

fn topic_row(ui: &mut egui::Ui, selected: bool, title: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, title)
    });
    let p = ui.painter();
    if selected || response.hovered() {
        p.rect_filled(rect, 4, theme::BG_850);
    }
    if selected {
        p.rect_filled(
            Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0,
            theme::ACCENT,
        );
    }
    p.text(
        rect.left_center() + vec2(12.0, 0.0),
        Align2::LEFT_CENTER,
        title,
        FontId::new(13.5, theme::sans_strong()),
        if selected { theme::ACCENT } else { theme::TEXT },
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

fn topic_body(ui: &mut egui::Ui, topic: &Topic, width: f32) {
    ui.spacing_mut().item_spacing.y = 10.0;
    let size = 14.5;
    for block in &topic.blocks {
        match block {
            Block::Para(text) => {
                ui.label(inline(text, size, width));
            }
            Block::Bullet(text) | Block::Step(_, text) => {
                let mark = match block {
                    Block::Step(n, _) => format!("{n}."),
                    _ => "•".to_string(),
                };
                ui.horizontal_top(|ui| {
                    ui.add_sized(
                        vec2(22.0, size + 4.0),
                        egui::Label::new(
                            RichText::new(mark)
                                .family(theme::sans_strong())
                                .size(size)
                                .color(theme::ACCENT),
                        ),
                    );
                    ui.label(inline(text, size, width - 30.0));
                });
            }
        }
    }
}

/// The height of a topic's picture: the same for every topic, so nothing below it moves.
const PICTURE_H: f32 = 190.0;

/// The kind of topic, for the captions down the topic list. The guide's own order is kept;
/// a topic it adds later goes under "More" until it is given a home here.
fn topic_group(title: &str) -> &'static str {
    match title {
        "Welcome" | "Your first edit" | "Bringing photos in" => "Start here",
        "Choosing and sorting photos" => "Your photos",
        "The editing panel"
        | "Choosing which tools show"
        | "Cropping and straightening"
        | "Comparing before and after"
        | "Starting from a look"
        | "Undo, snapshots and history" => "Editing",
        "Reusing your edits"
        | "Saving your work"
        | "Exporting a finished photo"
        | "Cutting out a background" => "Saving and sharing",
        "Keyboard shortcuts" | "If something goes wrong" => "Reference",
        _ => "More",
    }
}

/// The part of the window a guide topic is about, if it is about one.
fn topic_spot(title: &str) -> Option<Spot> {
    Some(match title {
        "Your first edit" | "The editing panel" | "Choosing which tools show" => Spot::Tabs,
        "Bringing photos in" | "Undo, snapshots and history" => Spot::Rail,
        "Choosing and sorting photos" => Spot::Filmstrip,
        "Cropping and straightening" | "Starting from a look" => Spot::Tools,
        "Comparing before and after" | "Cutting out a background" => Spot::Toolbar,
        "Reusing your edits" | "Saving your work" => Spot::Footer,
        "Exporting a finished photo" => Spot::Export,
        _ => return None,
    })
}

impl PhotoApp {
    /// A topic's picture: this window, drawn small from where each part was last laid out,
    /// with the part the topic is about outlined in amber. It is always the window as it
    /// is, whatever the panels' sizes, so the picture never disagrees with the screen.
    fn paint_window_map(&self, painter: &egui::Painter, area: Rect, lit: Option<Spot>) {
        let screen = painter.ctx().content_rect();
        if screen.width() < 1.0 || screen.height() < 1.0 {
            return;
        }
        let scale = (area.width() / screen.width()).min(area.height() / screen.height());
        let size = screen.size() * scale;
        let origin = area.center() - size / 2.0;
        let map = |r: Rect| {
            Rect::from_min_max(
                origin + (r.min - screen.min) * scale,
                origin + (r.max - screen.min) * scale,
            )
        };
        painter.rect_stroke(
            Rect::from_min_size(origin, size),
            2,
            Stroke::new(1.0, theme::BORDER),
            StrokeKind::Inside,
        );
        let spots = &self.desk_spots;
        // The editing panel stands for its sections and its footer both.
        let parts = [
            (Spot::Toolbar, spots.toolbar, "top bar"),
            (Spot::Rail, spots.rail, "left strip"),
            (Spot::Stage, spots.stage, "photo"),
            (Spot::Filmstrip, spots.filmstrip, "shelf"),
            (Spot::Tabs, spots.inspector, "editing panel"),
            (Spot::Tools, spots.tools, "tools"),
            (Spot::Export, spots.export, "export"),
        ];
        let is_lit =
            |part: Spot| lit == Some(part) || (part == Spot::Tabs && lit == Some(Spot::Footer));
        // The lit part last, so its outline is on top of its neighbours'.
        let order = parts
            .iter()
            .filter(|p| !is_lit(p.0))
            .chain(parts.iter().filter(|p| is_lit(p.0)));
        for &(part, rect, name) in order {
            let Some(rect) = rect else {
                continue;
            };
            let r = map(rect).shrink(1.0);
            let on = is_lit(part);
            let colour = if on { theme::ACCENT } else { theme::TEXT_FAINT };
            painter.rect_stroke(
                r,
                1,
                Stroke::new(1.0, if on { theme::ACCENT } else { theme::BORDER_HI }),
                StrokeKind::Inside,
            );
            let label = painter.layout_no_wrap(name.to_owned(), FontId::monospace(10.0), colour);
            if label.size().x + 6.0 < r.width() && label.size().y + 4.0 < r.height() {
                painter.galley(r.center() - label.size() / 2.0, label, colour);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every topic either points at a part of the window or is known not to: renaming a
    /// heading in the guide cannot quietly leave its picture with nothing lit.
    #[test]
    fn every_topic_knows_its_part_of_the_window() {
        let about_no_part = ["Welcome", "Keyboard shortcuts", "If something goes wrong"];
        for t in topics() {
            assert!(
                topic_spot(&t.title).is_some() || about_no_part.contains(&t.title.as_str()),
                "{:?} points at no part of the window",
                t.title
            );
            assert_ne!(
                topic_group(&t.title),
                "More",
                "{:?} has no group in the list",
                t.title
            );
        }
    }

    /// The guide reads as topics, every one with something in it.
    #[test]
    fn the_guide_parses_into_topics() {
        let topics = topics();
        assert!(topics.len() >= 10, "only {} topics", topics.len());
        assert_eq!(topics[0].title, "Welcome");
        for t in &topics {
            assert!(!t.blocks.is_empty(), "{} is empty", t.title);
        }
    }

    #[test]
    fn wrapped_lines_join_and_lists_split() {
        let t = parse(
            "# Title\nintro\n## One\nfirst line\nsecond line\n\n- a\n  more a\n- b\n1. step\n<!-- note\nhidden -->\nafter\n",
        );
        assert_eq!(t.len(), 1);
        assert_eq!(
            t[0].blocks,
            vec![
                Block::Para("first line second line".into()),
                Block::Bullet("a more a".into()),
                Block::Bullet("b".into()),
                Block::Step("1".into(), "step".into()),
                Block::Para("after".into()),
            ]
        );
    }

    /// Every tour card fits on a laptop screen beside, or inside, whatever it points at.
    #[test]
    fn tour_cards_stay_on_screen() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
        let card = vec2(340.0, 190.0);
        for t in [
            Rect::from_min_max(pos2(0.0, 60.0), pos2(48.0, 720.0)),
            Rect::from_min_max(pos2(48.0, 110.0), pos2(900.0, 560.0)),
            Rect::from_min_max(pos2(900.0, 0.0), pos2(1280.0, 720.0)),
            Rect::from_min_max(pos2(1180.0, 10.0), pos2(1270.0, 46.0)),
        ] {
            let p = card_place(screen, Some(t), card);
            assert!(
                screen.contains_rect(Rect::from_min_size(p, card)),
                "{t:?} put the card at {p:?}"
            );
        }
    }
}
