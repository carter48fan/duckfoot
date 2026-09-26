//! Keyboard shortcuts, and the edits they share with buttons.

use crate::*;

impl PhotoApp {
    pub(crate) fn handle_keys(&mut self, ctx: &egui::Context, gpu: &Gpu) {
        // Android's back gesture: close one thing per press, innermost first, and leave
        // the app only once there is nothing left to close.
        if ctx.input(|i| i.key_pressed(egui::Key::BrowserBack)) {
            if let Some(focused) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(focused));
            } else if self.shortcuts_open {
                self.shortcuts_open = false;
            } else if self.tour_step.is_some() {
                self.end_tour();
            } else if self.cropping {
                // As Esc: the framing in progress is thrown away.
                self.cancel_crop();
            } else if self.batch_dialog_open {
                self.batch_dialog_open = false;
            } else if self.customize_open {
                self.customize_open = false;
            } else if self.select_mode {
                self.select_mode = false;
            } else if self.folder_browser_open {
                self.folder_browser_open = false;
            } else if self.library_filters_open {
                self.library_filters_open = false;
            } else if (self.view.zoom - 1.0).abs() > 0.01 {
                // Zoomed in, back returns to the whole photo before it leaves anything.
                self.view = View::default();
            } else if self.split_active {
                self.split_active = false;
                self.needs_render = true;
            } else if self.layout.is_compact() && self.compact_tab.is_some() {
                self.compact_tab = None;
            } else {
                self.platform.leave();
            }
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.cancel_drag(ctx) {
            return;
        }
        // DESIGN.md: hotkeys never fire while a text field has focus. Only a text field: a
        // button reached with Tab has focus too, and must not swallow every shortcut.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // `?` (or F1) opens the shortcut sheet; while it is up, keys only close it.
        let sheet = ctx.input(|i| {
            i.key_pressed(egui::Key::F1)
                || i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Text(t) if t == "?"))
        });
        if self.shortcuts_open {
            if sheet || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.shortcuts_open = false;
            }
            return;
        }
        if sheet && !self.layout.is_compact() {
            self.shortcuts_open = true;
            return;
        }
        // Nor behind a dialog, where they would change the photo out of sight.
        if self.batch_dialog_open {
            return;
        }
        let (prev, next) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        if prev {
            self.step_photo(gpu, -1);
        }
        if next {
            self.step_photo(gpu, 1);
        }
        let (undo, redo) = ctx.input(|i| {
            (
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Z),
            )
        });
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }

        let (save, copy, paste, split_toggle, ctrl_a, batch_export) = ctx.input(|i| {
            (
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::S),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::C),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::V),
                i.key_pressed(egui::Key::Backslash),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::A),
                i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::E),
            )
        });
        if save {
            self.save_current_sidecar();
        }
        if copy {
            self.copy_adjustments();
        }
        if paste {
            self.paste_adjustments();
        }
        if split_toggle {
            self.split_active = !self.split_active;
            self.needs_render = true;
        }
        if ctrl_a {
            let all_paths: Vec<PathBuf> = self.shelf.iter().map(|it| it.path.clone()).collect();
            self.selection.select_all(&all_paths);
        }
        if batch_export {
            self.batch_dialog_open = true;
        }

        let (pick, reject, unflag) = ctx.input(|i| {
            (
                !i.modifiers.command && i.key_pressed(egui::Key::P),
                !i.modifiers.command && i.key_pressed(egui::Key::X),
                !i.modifiers.command && i.key_pressed(egui::Key::U),
            )
        });
        if pick {
            self.set_flag_for_selection(photo_engine::Flag::Pick);
        }
        if reject {
            self.set_flag_for_selection(photo_engine::Flag::Reject);
        }
        if unflag {
            self.set_flag_for_selection(photo_engine::Flag::Unflagged);
        }

        let rating_cmd = ctx.input(|i| {
            if !i.modifiers.command {
                if i.key_pressed(egui::Key::Num1) {
                    Some(1)
                } else if i.key_pressed(egui::Key::Num2) {
                    Some(2)
                } else if i.key_pressed(egui::Key::Num3) {
                    Some(3)
                } else if i.key_pressed(egui::Key::Num4) {
                    Some(4)
                } else if i.key_pressed(egui::Key::Num5) {
                    Some(5)
                } else if i.key_pressed(egui::Key::Num0) {
                    Some(0)
                } else {
                    None
                }
            } else {
                None
            }
        });
        if let Some(r) = rating_cmd {
            self.set_rating_for_selection(r);
        }

        let (fit, actual, zoom_toggle) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::Num0),
                i.modifiers.command && i.key_pressed(egui::Key::Num1),
                !i.modifiers.command && i.key_pressed(egui::Key::Z),
            )
        });
        if fit {
            self.view = View::default();
        }
        if actual {
            self.zoom_actual();
        }
        if zoom_toggle {
            if (self.view.zoom - 1.0).abs() < 0.05 {
                self.zoom_actual();
            } else {
                self.view = View::default();
            }
        }

        let (rot_left, rot_right, flip_h, flip_v, crop_toggle, crop_esc, crop_enter) =
            ctx.input(|i| {
                (
                    !i.modifiers.command && i.key_pressed(egui::Key::OpenBracket),
                    !i.modifiers.command && i.key_pressed(egui::Key::CloseBracket),
                    !i.modifiers.command && i.key_pressed(egui::Key::H),
                    !i.modifiers.command && i.key_pressed(egui::Key::V),
                    !i.modifiers.command && i.key_pressed(egui::Key::C),
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::Enter),
                )
            });
        if rot_left {
            self.geometry_edit(Geometry::rotate_left);
        }
        if rot_right {
            self.geometry_edit(Geometry::rotate_right);
        }
        if flip_h {
            self.geometry_edit(Geometry::flip_h);
        }
        if flip_v {
            self.geometry_edit(Geometry::flip_v);
        }
        if crop_toggle {
            self.toggle_crop();
        }
        if crop_esc {
            self.cancel_crop();
        }
        if crop_enter {
            self.apply_crop();
        }
    }

    /// Turn or mirror the open photo as one undo step. Waits for the decode: until then
    /// the photo's true orientation is not known, so there is nothing sound to turn.
    pub(crate) fn geometry_edit(&mut self, edit: impl FnOnce(&mut Geometry)) {
        if self.ready_image().is_none() {
            return;
        }
        let before = self.stack.clone();
        edit(&mut self.stack.geometry);
        if self.stack != before {
            self.history.checkpoint(&before);
            self.rebase_crop();
            self.needs_render = true;
        }
    }
}
