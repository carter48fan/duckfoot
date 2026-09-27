//! Every shortcut and pointer gesture, in one table. The `?` overlay draws it and the
//! README's Controls section is generated from it (a test fails when they differ), so the
//! three can no longer drift apart. `keys.rs` is what acts on the keys.

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    Photos,
    Editing,
    View,
    Library,
    Pointer,
}

impl Group {
    const ALL: [Group; 5] = [
        Group::Photos,
        Group::Editing,
        Group::View,
        Group::Library,
        Group::Pointer,
    ];

    fn title(self) -> &'static str {
        match self {
            Group::Photos => "Photos",
            Group::Editing => "Editing",
            Group::View => "View",
            Group::Library => "Library",
            Group::Pointer => "Mouse and touch",
        }
    }
}

pub(crate) struct Shortcut {
    pub group: Group,
    /// As the README writes it: `Ctrl`, which the overlay shows as `⌘` on a Mac.
    pub keys: &'static str,
    pub action: &'static str,
}

const fn s(group: Group, keys: &'static str, action: &'static str) -> Shortcut {
    Shortcut {
        group,
        keys,
        action,
    }
}

use Group::*;

pub(crate) const KEYMAP: &[Shortcut] = &[
    s(
        Photos,
        "`←` / `→`",
        "previous / next photo, in the filmstrip's order",
    ),
    s(
        Photos,
        "`Ctrl+S`",
        "save now (edits also save on their own)",
    ),
    s(
        Photos,
        "`Ctrl+E`",
        "export this photo: size, format, metadata, then where",
    ),
    s(Photos, "`Ctrl+Shift+E`", "batch export the selection"),
    s(
        Editing,
        "`Ctrl+Z` / `Ctrl+Shift+Z`",
        "undo / redo, per photo",
    ),
    s(
        Editing,
        "`Ctrl+C` / `Ctrl+V`",
        "copy / paste the adjustments (crop and rotation stay put)",
    ),
    s(Editing, "`Ctrl+Shift+V`", "choose what paste carries"),
    s(
        Editing,
        "`Ctrl+'`",
        "virtual copy: another edit of the same photo",
    ),
    s(
        Editing,
        "`Ctrl+Alt+V`",
        "the same edits as the photo open before this one",
    ),
    s(
        Editing,
        "`Esc` with a mask chosen or healing",
        "put the mask's handles away / stop healing",
    ),
    s(Editing, "`C`", "crop tool on / off"),
    s(Editing, "`Enter` / `Esc`", "apply / cancel the crop"),
    s(
        Editing,
        "`X` / `O` while cropping",
        "turn the crop's shape / next guide",
    ),
    s(Editing, "`Shift` while resizing a crop", "keep its shape"),
    s(Editing, "`[` / `]`", "rotate 90° left / right"),
    s(Editing, "`H` / `V`", "mirror / flip"),
    s(
        Editing,
        "`Esc` while dragging",
        "put the control back where the drag started",
    ),
    s(
        Editing,
        "`W`, then click",
        "white balance from something that should be grey",
    ),
    s(View, "`\\`", "before / after split"),
    s(
        View,
        "`Ctrl+0` / `Ctrl+1` / `Z`",
        "fit / 100% / toggle between them",
    ),
    s(View, "`?` or `F1`", "this list of shortcuts"),
    s(
        Library,
        "`0`–`5`",
        "star rating for the selection (`0` clears)",
    ),
    s(
        Library,
        "`P` / `X` / `U`",
        "flag as pick / reject / clear the flag",
    ),
    s(Library, "`Ctrl+A`", "select every photo on the shelf"),
    s(
        Library,
        "`Shift`+click / `Ctrl`+click",
        "select a range / add to the selection",
    ),
    s(
        Pointer,
        "Scroll or pinch over the photo",
        "zoom about the pointer",
    ),
    s(Pointer, "Drag the photo", "pan, once zoomed in"),
    s(Pointer, "Drag a slider", "live; one undo step per gesture"),
    s(
        Pointer,
        "`Shift` while dragging a slider",
        "ten times finer",
    ),
    s(
        Pointer,
        "Double-click a slider or its label",
        "back to its default",
    ),
    s(
        Pointer,
        "Drag in the curve",
        "move the nearest point; the ends keep their place",
    ),
    s(
        Pointer,
        "Drag a 3D view",
        "orbit it; `Ctrl`+scroll zooms, double-click resets",
    ),
    s(
        Pointer,
        "Click a module's header",
        "open it and close the others; `Shift`+click opens it alongside",
    ),
    s(
        Pointer,
        "Touch a control in a module that's off",
        "turns the module on",
    ),
    s(Pointer, "Click a History row", "jump to that step"),
    s(
        Pointer,
        "Drag the split divider",
        "move it; double-click centres it",
    ),
    s(
        Pointer,
        "Hold the scope's clip readout",
        "show clipping while held",
    ),
    s(
        Pointer,
        "Drop files on the window",
        "add them to the shelf and open the first",
    ),
];

/// The README's Controls table, as it should read.
#[cfg(test)]
pub(crate) fn markdown() -> String {
    let mut out = String::from("| | |\n|---|---|\n");
    for group in Group::ALL {
        out.push_str(&format!("| **{}** | |\n", group.title()));
        for k in KEYMAP.iter().filter(|k| k.group == group) {
            out.push_str(&format!("| {} | {} |\n", k.keys, k.action));
        }
    }
    out
}

/// The keys as this platform names them, without the README's backticks.
fn display_keys(keys: &str) -> String {
    let plain = keys.replace('`', "");
    if cfg!(target_os = "macos") {
        plain.replace("Ctrl+", "⌘").replace("Ctrl", "⌘")
    } else {
        plain
    }
}

impl PhotoApp {
    /// The shortcut sheet, over everything, until Esc, `?`, a click outside or its ×.
    pub(crate) fn shortcuts_overlay(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_open {
            return;
        }
        let screen = ctx.content_rect();
        // A dimmed backdrop: a click on it closes the sheet.
        let backdrop = egui::Area::new(egui::Id::new("shortcuts-backdrop"))
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
        let width = 640.0_f32.min(screen.width() - 32.0);
        egui::Area::new(egui::Id::new("shortcuts"))
            .order(egui::Order::Tooltip)
            .fade_in(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::BG_900)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(10.0)
                    .inner_margin(Margin::same(18))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Shortcuts")
                                    .family(theme::display())
                                    .size(17.0)
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
                        ui.add_space(6.0);
                        let max_h = (screen.height() - 120.0).max(200.0);
                        egui::ScrollArea::vertical()
                            .max_height(max_h)
                            .show(ui, |ui| {
                                for group in Group::ALL {
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(group.title().to_uppercase())
                                            .monospace()
                                            .size(10.0)
                                            .extra_letter_spacing(0.8)
                                            .color(theme::TEXT_FAINT),
                                    );
                                    egui::Grid::new(("shortcuts", group.title()))
                                        .num_columns(2)
                                        .spacing(vec2(16.0, 4.0))
                                        .min_col_width(width * 0.34)
                                        .max_col_width(width * 0.6)
                                        .show(ui, |ui| {
                                            for k in KEYMAP.iter().filter(|k| k.group == group) {
                                                ui.label(
                                                    RichText::new(display_keys(k.keys))
                                                        .monospace()
                                                        .color(theme::TEXT_HI),
                                                );
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(k.action)
                                                            .color(theme::TEXT_DIM),
                                                    )
                                                    .wrap(),
                                                );
                                                ui.end_row();
                                            }
                                        });
                                }
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(
                                        "Shortcuts never fire while you are typing in a field.",
                                    )
                                    .size(11.0)
                                    .color(theme::TEXT_FAINT),
                                );
                            });
                    });
            });
        if close {
            self.shortcuts_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "<!-- keymap:start (generated from crates/photo-app/src/keymap.rs) -->\n";
    const END: &str = "<!-- keymap:end -->";

    /// `DUCKFOOT_BLESS=1 cargo test -p photo-app readme` rewrites the README's table.
    #[test]
    fn readme_controls_are_the_keymap() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
        let readme = std::fs::read_to_string(&path).unwrap();
        let start = readme
            .find(START)
            .expect("README has no keymap:start marker")
            + START.len();
        let end = start
            + readme[start..]
                .find(END)
                .expect("README has no keymap:end marker");
        let want = markdown();
        if readme[start..end] == want {
            return;
        }
        if std::env::var_os("DUCKFOOT_BLESS").is_some() {
            let updated = format!("{}{}{}", &readme[..start], want, &readme[end..]);
            std::fs::write(&path, updated).unwrap();
            return;
        }
        panic!(
            "README's Controls table differs from keymap.rs; \
             run with DUCKFOOT_BLESS=1 to regenerate it"
        );
    }

    #[test]
    fn every_group_has_shortcuts_and_none_repeats() {
        for g in Group::ALL {
            assert!(KEYMAP.iter().any(|k| k.group == g), "{:?} is empty", g);
        }
        for (i, a) in KEYMAP.iter().enumerate() {
            assert!(
                KEYMAP[i + 1..].iter().all(|b| b.keys != a.keys),
                "{} is listed twice",
                a.keys
            );
        }
    }
}
