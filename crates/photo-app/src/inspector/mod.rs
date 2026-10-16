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

            // One slim header: what the panel is showing, and for Edit the two things you do
            // to its list.
            let editing = self.tool == Tool::Edit;
            widgets::band(ui, |ui| {
                let title = if self.customize_open {
                    "Modules"
                } else {
                    self.tool.label()
                };
                ui.label(theme::head(title));
                if !self.customize_open && loaded && editing {
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
                // Which look the photo is on, while it still is.
                if self.tool == Tool::Looks {
                    if let Some(p) = self.preset.as_ref().filter(|p| p.applied == self.stack) {
                        ui.label(theme::tag(presets::LOOKS[p.look()].name).color(theme::ACCENT));
                    }
                }
                if !editing {
                    return;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let tip = if self.customize_open {
                        "Back to editing"
                    } else {
                        "Choose which modules show, and their order"
                    };
                    if widgets::icon_drawn(ui, self.customize_open, tip, |painter, rect, colour| {
                        widgets::paint_gear(painter, rect, colour)
                    })
                    .clicked()
                    {
                        self.customize_open = !self.customize_open;
                    }
                    // Fold every section, or open them all if any is folded.
                    if !self.customize_open
                        && widgets::icon(ui, "↕", false, "Fold or open every section").clicked()
                    {
                        if self.shut_groups.is_empty() {
                            self.shut_groups.extend(ModuleGroup::ALL);
                        } else {
                            self.shut_groups.clear();
                        }
                    }
                });
            });

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
                            } else if !loaded {
                                ui.add_space(8.0);
                                ui.label(theme::dim("Open a RAW to see its pipeline.").size(12.0));
                            } else {
                                match self.tool {
                                    Tool::Edit => self.pipeline_body(ui),
                                    Tool::Looks => {
                                        ui.spacing_mut().item_spacing.y = 6.0;
                                        self.looks_body(ui, true);
                                    }
                                    Tool::Cutout => {
                                        let (changed, save) = self.cutout_studio.controls(ui);
                                        if changed {
                                            self.cutout_studio.update_preview_texture(ui.ctx());
                                        }
                                        if save {
                                            self.export_cutout(ui.ctx());
                                        }
                                    }
                                    tool => {
                                        if let Some(id) = tool.module() {
                                            self.tool_body(ui, id);
                                        }
                                    }
                                }
                            }
                        });
                    if loaded && !self.customize_open && self.tool == Tool::Edit {
                        self.info_band(ui);
                    }
                });
        });

        self.inspector_width = inner.response.rect.width();
        self.desk_spots.inspector = Some(inner.response.rect);
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
                    // How it was taken, under what it came out as.
                    let taken = self
                        .open_info()
                        .map(info::exposure_line)
                        .unwrap_or_default();
                    if !taken.is_empty() {
                        ui.add_space(4.0);
                        ui.vertical_centered(|ui| {
                            ui.label(theme::num(taken).size(10.5).color(theme::TEXT_DIM));
                        });
                    }
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

    /// The sections, top to bottom: each a heading with a switch, its everyday modules laid
    /// straight in, and its deeper ones folded into drawers. One list, scrolled; no tabs.
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
        let mut shown = 0;
        self.desk_spots.tabs = None;
        ui.spacing_mut().item_spacing.y = 0.0;

        for group in ModuleGroup::ALL {
            // Crop, heal and masks are tools on the rail, not a section of the list.
            if group == ModuleGroup::Retouch {
                continue;
            }
            // In the customised order, and only the ones Customize shows.
            let members: Vec<usize> = (0..self.inspector_sections.len())
                .filter(|&i| {
                    let s = &self.inspector_sections[i];
                    s.visible && group.holds(s.id)
                })
                .collect();
            if members.is_empty() {
                continue;
            }
            shown += members.len();
            let ids: Vec<InspectorSectionId> = members
                .iter()
                .map(|&i| self.inspector_sections[i].id)
                .collect();

            // A section's switch is off only while it holds what it turned off and nothing
            // in it has been switched on since — moving any slider in it ends the bypass.
            let switchable: Vec<InspectorSectionId> = ids
                .iter()
                .copied()
                .filter(|&id| id != InspectorSectionId::Geometry)
                .collect();
            if switchable.iter().any(|id| id.is_on(&self.stack)) {
                self.bypassed.remove(&group);
            }
            let bypassed = self.bypassed.contains_key(&group);
            let edited = ids
                .iter()
                .any(|&id| summarise(id, &self.stack, &defaults, &white).1);
            let open = !self.shut_groups.contains(&group);

            let head = widgets::section_header(
                ui,
                group.label(),
                open,
                edited,
                (!switchable.is_empty()).then_some(!bypassed),
            );
            if self.desk_spots.tabs.is_none() {
                self.desk_spots.tabs = Some(ui.min_rect());
            }
            if head.clicked {
                if open {
                    self.shut_groups.insert(group);
                } else {
                    self.shut_groups.remove(&group);
                }
            }
            if head.toggled {
                if let Some(were_on) = self.bypassed.remove(&group) {
                    for id in were_on {
                        if let Some(on) = id.enabled_mut(&mut self.stack) {
                            *on = true;
                        }
                    }
                } else {
                    let were_on: Vec<_> = switchable
                        .iter()
                        .copied()
                        .filter(|id| id.is_on(&self.stack))
                        .collect();
                    for &id in &were_on {
                        if let Some(on) = id.enabled_mut(&mut self.stack) {
                            *on = false;
                        }
                    }
                    self.bypassed.insert(group, were_on);
                }
                // A toggle is a mutation like any other: DESIGN.md invariant 3.
                changed = true;
                started = true;
            }

            if open {
                ui.scope(|ui| {
                    if bypassed {
                        // Off, but its values kept: shown faded rather than hidden.
                        ui.multiply_opacity(0.35);
                    }
                    ui.spacing_mut().item_spacing.y = 6.0;
                    let mut first = true;
                    for &idx in &members {
                        let id = self.inspector_sections[idx].id;
                        let (module_changed, module_started, opened) = if id.is_drawer() {
                            self.drawer(ui, idx, &defaults, &white)
                        } else {
                            if !first {
                                // Where one module's controls end and the next one's start.
                                ui.add_space(8.0);
                                ui.label(theme::tag(id.title()).color(theme::TEXT_DIM).size(10.0));
                            }
                            let mut body_changed = false;
                            let mut body_started = false;
                            self.module_body(ui, id, &mut body_changed, &mut body_started);
                            if body_changed {
                                if let Some(on) = id.enabled_mut(&mut self.stack) {
                                    *on = true;
                                }
                            }
                            (body_changed, body_started, false)
                        };
                        first = false;
                        if module_changed {
                            self.focused_section = Some(id);
                        }
                        // One drawer open at a time keeps the list short; Shift opens this
                        // one alongside whatever is already open.
                        if opened && self.single_open && !ui.input(|i| i.modifiers.shift) {
                            close_others = Some(id);
                        }
                        changed |= module_changed;
                        started |= module_started;
                    }
                    ui.add_space(10.0);
                });
            }
            let r = ui.min_rect();
            ui.painter().hline(
                r.x_range(),
                ui.cursor().top(),
                Stroke::new(1.0, theme::BORDER),
            );
        }

        if shown == 0 {
            ui.add_space(8.0);
            ui.label(theme::dim("Every module is hidden. ⚙ chooses which show.").size(12.0));
        }

        if let Some(open) = close_others {
            for s in &mut self.inspector_sections {
                if s.id != open && s.id.is_drawer() {
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

    /// One drawer: a hairline box with the module's name, summary and switch, folding its
    /// controls. Returns (changed, started a change, was opened this frame).
    fn drawer(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        defaults: &PhotoStack,
        white: &WhiteScale,
    ) -> (bool, bool, bool) {
        let InspectorSectionConfig {
            id, mut collapsed, ..
        } = self.inspector_sections[idx].clone();
        let (summary, modified) = summarise(id, &self.stack, defaults, white);
        let on = id.is_on(&self.stack);
        let summary_colour = if !on {
            theme::TEXT_FAINT
        } else if modified {
            theme::ACCENT
        } else {
            theme::TEXT_DIM
        };
        let mut changed = false;
        let mut started = false;
        let mut opened = false;
        let frame = egui::Frame::new()
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(Margin {
                left: 10,
                right: 6,
                top: 2,
                bottom: 2,
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let head = widgets::module_header(
                    ui,
                    id.title(),
                    id.enabled_mut(&mut self.stack),
                    &summary,
                    summary_colour,
                    false,
                );
                if head.toggled {
                    changed = true;
                    started = true;
                }
                if head.clicked {
                    opened = collapsed;
                    collapsed = !collapsed;
                }
                if !collapsed {
                    // Controls show whether or not the module is on, and touching one turns
                    // it on — no need to switch it on before you can try it.
                    let r = ui.min_rect();
                    ui.painter().hline(
                        r.x_range(),
                        ui.cursor().top(),
                        Stroke::new(1.0, theme::BORDER),
                    );
                    ui.add_space(8.0);
                    let mut body_changed = false;
                    self.module_body(ui, id, &mut body_changed, &mut started);
                    if body_changed {
                        if let Some(on) = id.enabled_mut(&mut self.stack) {
                            *on = true;
                        }
                    }
                    changed |= body_changed;
                    ui.add_space(6.0);
                }
            });
        if self.reveal_section == Some(id) {
            frame.response.scroll_to_me(None);
            self.reveal_section = None;
        }
        self.inspector_sections[idx].collapsed = collapsed;
        (changed, started, opened)
    }

    /// The tool rail at the window's right edge: Looks, Edit, and the tools that work on
    /// the photo itself. Choosing one shows it in the inspector beside the rail.
    pub(crate) fn tool_rail(&mut self, ui: &mut egui::Ui) {
        let rail = egui::Panel::right("tool_rail")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_950)
                    .inner_margin(Margin::symmetric(6, 10)),
            )
            .exact_size(48.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.vertical_centered(|ui| {
                    for tool in Tool::ALL {
                        if widgets::tool_button(ui, tool, self.tool == tool).clicked() {
                            self.choose_tool(tool);
                        }
                    }
                });
            });
        // Clipped to the window: at its right edge, the panel's rect runs half a point past it.
        self.desk_spots.tools = Some(rail.response.rect.intersect(ui.ctx().content_rect()));
        // Framing a crop is the crop tool, however it was started (C, the top bar's chip).
        if self.cropping {
            self.tool = Tool::Crop;
        }
        // The studio closed another way (Esc): back to editing.
        if self.tool == Tool::Cutout && !self.cutout_studio.open {
            self.tool = Tool::Edit;
        }
    }

    /// The desktop inspector on the rail tool that is `id`'s controls (spot heal, masks…),
    /// for the screenshot tool. Does nothing for a module no tool is.
    pub fn open_tool_for(&mut self, id: InspectorSectionId) {
        if let Some(tool) = Tool::ALL.into_iter().find(|t| t.module() == Some(id)) {
            self.choose_tool(tool);
        }
    }

    /// Switch tools. Crop frames as soon as it is chosen, and leaving it keeps the framing.
    pub(crate) fn choose_tool(&mut self, tool: Tool) {
        self.customize_open = false;
        if tool != Tool::Crop && self.cropping {
            self.apply_crop();
        }
        if tool == Tool::Crop && !self.cropping && self.ready_image().is_some() {
            self.begin_crop();
        }
        // The studio's preview is on the photo only while its tool is.
        self.cutout_studio.open = tool == Tool::Cutout;
        self.tool = tool;
    }

    /// One module as a tool's panel: its name, summary and switch, then all its controls.
    fn tool_body(&mut self, ui: &mut egui::Ui, id: InspectorSectionId) {
        let snapshot = self.stack.clone();
        let defaults = self.baseline.clone();
        let white = self.white_scale();
        let (summary, modified) = summarise(id, &self.stack, &defaults, &white);
        let summary_colour = if !id.is_on(&self.stack) {
            theme::TEXT_FAINT
        } else if modified {
            theme::ACCENT
        } else {
            theme::TEXT_DIM
        };
        let head = widgets::module_header(
            ui,
            id.title(),
            id.enabled_mut(&mut self.stack),
            &summary,
            summary_colour,
            false,
        );
        let mut changed = head.toggled;
        let mut started = head.toggled;
        let mut body_changed = false;
        self.module_body(ui, id, &mut body_changed, &mut started);
        if body_changed {
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
}
