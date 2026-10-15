//! The inspector (right): scopes, then the pipeline of module cards.

use crate::*;

mod footer;
mod modules;
pub(crate) mod summary;

impl PhotoApp {
    // ── Inspector (right) ───────────────────────────────────────────────────────────────

    pub(crate) fn inspector(&mut self, ui: &mut egui::Ui) {
        let mut panel = egui::Panel::right("inspector")
            .frame(egui::Frame::new().fill(theme::BG_950))
            .resizable(true)
            .default_size(self.inspector_width)
            .min_size(270.0)
            .max_size(850.0);

        if let Some(target_w) = self.inspector_force_width.take() {
            panel = panel.exact_size(target_w);
        }

        let inner = panel.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            self.inspector_footer(ui);

            self.scope_block(ui);
            // The pipeline follows the photo being edited, which is ahead of the pixels
            // while a newly chosen photo decodes.
            let loaded = self.can_edit();

            // One slim header: what the list below is, and the two things you do to it.
            widgets::band(ui, |ui| {
                let title = if self.customize_open {
                    "Modules"
                } else {
                    "Pipeline"
                };
                ui.label(theme::head(title));
                if !self.customize_open && loaded {
                    let on = InspectorSectionId::all()
                        .iter()
                        .filter(|id| {
                            // Scope and geometry have no on/off; everything else does.
                            !matches!(id, InspectorSectionId::Scope | InspectorSectionId::Geometry)
                                && id.is_on(&self.stack)
                        })
                        .count();
                    ui.label(theme::tag(format!("{on} on")).color(theme::TEXT_DIM));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let tip = if self.customize_open {
                        "Back to the pipeline"
                    } else {
                        "Choose which modules show, and their order"
                    };
                    if widgets::icon(ui, "⚙", self.customize_open, tip).clicked() {
                        self.customize_open = !self.customize_open;
                    }
                    // Only All folds; the other tabs hold their modules open.
                    if !self.customize_open
                        && self.inspector_group == ModuleGroup::All
                        && widgets::icon(ui, "↕", false, "Expand or collapse every module")
                            .clicked()
                    {
                        let any_collapsed = self
                            .inspector_sections
                            .iter()
                            .any(|s| s.collapsed && s.id != InspectorSectionId::Scope);
                        for s in &mut self.inspector_sections {
                            if s.id != InspectorSectionId::Scope {
                                s.collapsed = !any_collapsed;
                            }
                        }
                    }
                });
            });

            self.desk_spots.tabs = None;
            if loaded && !self.customize_open {
                self.group_tabs(ui);
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(Margin::symmetric(12, 12))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 10.0;
                            if self.customize_open {
                                self.customize_body(ui);
                            } else if loaded {
                                self.pipeline_body(ui);
                            } else {
                                ui.add_space(8.0);
                                ui.label(theme::dim("Open a RAW to see its pipeline.").size(12.0));
                            }
                        });
                    if loaded && !self.customize_open {
                        self.info_band(ui);
                    }
                    // The phone's Presets, folded to one line until asked for. Info and Looks sit
                    // under the modules, so on a short window the sliders come first.
                    if loaded && !self.customize_open {
                        let head = widgets::band(ui, |ui| {
                            ui.label(theme::head("Looks"));
                            if let Some(p) =
                                self.preset.as_ref().filter(|p| p.applied == self.stack)
                            {
                                ui.label(
                                    theme::tag(presets::LOOKS[p.look()].name).color(theme::ACCENT),
                                );
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let tip = if self.looks_open {
                                    "Fold the looks away"
                                } else {
                                    "Start from a look"
                                };
                                widgets::icon(
                                    ui,
                                    if self.looks_open { "▾" } else { "▸" },
                                    false,
                                    tip,
                                )
                            })
                            .inner
                        });
                        if head.inner.clicked() || head.response.interact(Sense::click()).clicked()
                        {
                            self.looks_open = !self.looks_open;
                        }
                        if self.looks_open {
                            egui::Frame::new()
                                .inner_margin(Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 6.0;
                                    self.looks_body(ui, true);
                                });
                        }
                    }
                });
        });

        self.inspector_width = inner.response.rect.width();
    }

    pub(crate) fn section_mut(
        &mut self,
        id: InspectorSectionId,
    ) -> Option<&mut InspectorSectionConfig> {
        self.inspector_sections.iter_mut().find(|s| s.id == id)
    }

    /// The scope, pinned above the tabs as in every darkroom: it is what you watch while
    /// you drag everything below it. Its visibility still comes from Customize.
    pub(crate) fn scope_block(&mut self, ui: &mut egui::Ui) {
        let Some(cfg) = self.section_mut(InspectorSectionId::Scope) else {
            return;
        };
        if !cfg.visible {
            return;
        }
        let mut collapsed = cfg.collapsed;

        widgets::band(ui, |ui| {
            let title = ui
                .add(egui::Label::new(theme::head("Scope")).sense(Sense::click()))
                .on_hover_text(if collapsed {
                    "Show the scope"
                } else {
                    "Hide the scope"
                });
            if title.clicked() {
                collapsed = !collapsed;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.scope_mode_chips(ui, true) {
                    collapsed = false;
                }
            });
        });
        if !collapsed {
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 12,
                    right: 12,
                    top: 10,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    // Held at its size while a photo decodes, empty rather than gone: the
                    // engine's histogram is still the last photo's, and dropping the scope
                    // shifted every card below it.
                    let empty;
                    let hist = if self.ready_image().is_some() {
                        self.engine.histogram()
                    } else {
                        empty = photo_engine::Histogram::default();
                        &empty
                    };
                    inspector_map_3d::render_inspector_map(ui, &mut self.inspector_map_state, hist);
                });
            let r = ui.min_rect();
            ui.painter()
                .hline(r.x_range(), r.bottom(), Stroke::new(1.0, theme::BORDER));
        }
        if let Some(cfg) = self.section_mut(InspectorSectionId::Scope) {
            cfg.collapsed = collapsed;
        }
    }

    /// The scope's mode chips. True when one was clicked. `right_to_left` callers get them
    /// reversed, so they read the same way on screen either way.
    pub(crate) fn scope_mode_chips(&mut self, ui: &mut egui::Ui, right_to_left: bool) -> bool {
        ui.spacing_mut().item_spacing.x = 3.0;
        let mut clicked = false;
        let modes: Vec<_> = if right_to_left {
            inspector_map_3d::MODES.iter().rev().collect()
        } else {
            inspector_map_3d::MODES.iter().collect()
        };
        for (mode, label, tip) in modes {
            let on = self.inspector_map_state.mode == *mode;
            if widgets::chip(ui, on, label).on_hover_text(*tip).clicked() {
                self.inspector_map_state.mode = *mode;
                clicked = true;
            }
        }
        clicked
    }

    /// All, then the six groups, four to a row. Each names how many of its modules are on.
    fn group_tabs(&mut self, ui: &mut egui::Ui) {
        let tabs = egui::Frame::new()
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                let w = ((ui.available_width() - 3.0 * 4.0) / 4.0).floor();
                for row in ModuleGroup::TABS.chunks(4) {
                    ui.horizontal(|ui| {
                        for &group in row {
                            let on = InspectorSectionId::all()
                                .iter()
                                .filter(|&&id| {
                                    group != ModuleGroup::All
                                        && group.holds(id)
                                        // Crop has no on/off to count.
                                        && id != InspectorSectionId::Geometry
                                        && id.is_on(&self.stack)
                                })
                                .count();
                            let mut text = group.label().to_string();
                            if let Some(first) = text.get_mut(0..1) {
                                first.make_ascii_uppercase();
                            }
                            if on > 0 {
                                text = format!("{text} {on}");
                            }
                            let r = widgets::tab(ui, self.inspector_group == group, &text, w)
                                .on_hover_text(if group == ModuleGroup::All {
                                    "Every module in your order, folded"
                                } else {
                                    "This group's modules, held open"
                                });
                            if r.clicked() {
                                self.inspector_group = group;
                            }
                        }
                    });
                }
            });
        let r = tabs.response.rect;
        self.desk_spots.tabs = Some(r);
        ui.painter()
            .hline(r.x_range(), r.bottom(), Stroke::new(1.0, theme::BORDER));
    }

    pub(crate) fn pipeline_body(&mut self, ui: &mut egui::Ui) {
        // Everything that happens this frame is checkpointed against this one snapshot.
        let snapshot = self.stack.clone();
        // "Modified" means changed from how this photo opened — so the camera's own
        // orientation is not an edit.
        let defaults = self.baseline.clone();
        let white = self.white_scale();
        let mut changed = false;
        let mut started = false;
        let mut close_others = None;
        let group = self.inspector_group;
        // Off All, a tab's few modules all stay open: the sliders are the point.
        let held_open = group != ModuleGroup::All;
        let mut shown = 0;

        for idx in 0..self.inspector_sections.len() {
            let InspectorSectionConfig {
                id,
                visible,
                mut collapsed,
            } = self.inspector_sections[idx].clone();
            if !visible || !group.holds(id) {
                continue;
            }
            shown += 1;

            let (summary, modified) = summarise(id, &self.stack, &defaults, &white);
            let on = id.is_on(&self.stack);
            let summary_colour = if !on {
                theme::TEXT_FAINT
            } else if modified {
                theme::ACCENT
            } else {
                theme::TEXT_DIM
            };
            let focused = self.focused_section == Some(id);

            let mut card_changed = false;
            let mut card_started = false;
            let card = widgets::card(ui, focused, |ui| {
                let head = widgets::module_header(
                    ui,
                    id.title(),
                    id.enabled_mut(&mut self.stack),
                    &summary,
                    summary_colour,
                    focused,
                );
                if head.toggled {
                    // A toggle is a mutation like any other: DESIGN.md invariant 3.
                    card_changed = true;
                    card_started = true;
                }
                if head.clicked && !held_open {
                    let opening = collapsed;
                    collapsed = !collapsed;
                    // One open at a time keeps the list one screen tall; Shift opens this
                    // one alongside whatever is already open.
                    if opening && self.single_open && !ui.input(|i| i.modifiers.shift) {
                        close_others = Some(id);
                    }
                }
                if !collapsed || held_open {
                    // Controls show whether or not the module is on, and touching one turns
                    // it on — no need to tick the box before you can try it.
                    ui.add_space(2.0);
                    let mut body_changed = false;
                    self.module_body(ui, id, &mut body_changed, &mut card_started);
                    if body_changed {
                        if let Some(on) = id.enabled_mut(&mut self.stack) {
                            *on = true;
                        }
                    }
                    card_changed |= body_changed;
                    ui.add_space(4.0);
                }
                head.clicked || head.toggled
            });
            let touched = card.inner;

            if touched || card_changed {
                self.focused_section = Some(id);
            }
            if self.reveal_section == Some(id) {
                // The cards above it just collapsed; bring this one fully into view.
                card.response.scroll_to_me(None);
                self.reveal_section = None;
            }
            changed |= card_changed;
            started |= card_started;
            self.inspector_sections[idx].collapsed = collapsed;
        }

        if shown == 0 {
            ui.add_space(8.0);
            ui.label(
                theme::dim("Every module on this tab is hidden. ⚙ chooses which show.").size(12.0),
            );
        }

        if let Some(open) = close_others {
            for s in &mut self.inspector_sections {
                if s.id != open && s.id != InspectorSectionId::Scope {
                    s.collapsed = true;
                }
            }
            self.reveal_section = Some(open);
        }

        if started {
            self.history.checkpoint(&snapshot);
        }
        if changed {
            self.needs_render = true;
        }
    }
}
