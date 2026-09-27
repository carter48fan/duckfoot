//! Colour labels and keywords (W-11): marks a photographer sorts a roll by, beside stars
//! and flags. Both live in the photo's sidecar metadata; the shelf's search finds a photo
//! by its keywords as well as its name.

use crate::*;

/// Every label, as the sidecar names it, with its colour. `6`–`9` set the first four.
pub(crate) const LABELS: [(&str, Color32); 5] = [
    ("red", Color32::from_rgb(0xe5, 0x48, 0x4d)),
    ("yellow", Color32::from_rgb(0xf0, 0xc0, 0x2a)),
    ("green", Color32::from_rgb(0x46, 0xb8, 0x5c)),
    ("blue", Color32::from_rgb(0x3e, 0x8e, 0xf0)),
    ("purple", Color32::from_rgb(0xa8, 0x6c, 0xe0)),
];

pub(crate) fn colour(label: &str) -> Option<Color32> {
    LABELS.iter().find(|(n, _)| *n == label).map(|(_, c)| *c)
}

/// A round swatch in a label's colour, ringed when `on`; a finger's size to hit on touch.
pub(crate) fn swatch(ui: &mut egui::Ui, name: &str, fill: Color32, on: bool) -> egui::Response {
    let side = if widgets::is_touch(ui.ctx()) {
        22.0
    } else {
        16.0
    };
    let (rect, response) = widgets::allocate_control(ui, vec2(side, side), Sense::click());
    widgets::describe(ui, &response, rect, || {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            on,
            format!("{name} label"),
        )
    });
    let painter = ui.painter();
    let r = side / 2.0 - 3.0;
    painter.circle_filled(rect.center(), r, fill);
    if on {
        painter.circle_stroke(rect.center(), r + 2.0, Stroke::new(1.5, theme::TEXT_HI));
    } else if response.hovered() {
        painter.circle_stroke(rect.center(), r + 2.0, Stroke::new(1.0, theme::TEXT_DIM));
    }
    response.on_hover_text(format!("{name} label"))
}

impl PhotoApp {
    /// Give the selection (or the open photo) `label`; if every one of them has it
    /// already, take it off instead, so the same key or swatch toggles it.
    pub fn set_label_for_selection(&mut self, label: Option<&str>) {
        let paths: Vec<PathBuf> = if !self.selection.is_empty() {
            self.selection.selected.iter().cloned().collect()
        } else if let Some(p) = &self.loaded_path {
            vec![p.clone()]
        } else {
            return;
        };
        self.set_label_for_paths(&paths, label);
    }

    /// [`Self::set_label_for_selection`], for `paths`.
    pub fn set_label_for_paths(&mut self, paths: &[PathBuf], label: Option<&str>) {
        let all_have = label.is_some()
            && paths.iter().all(|p| {
                self.shelf
                    .iter()
                    .find(|it| &it.path == p)
                    .is_some_and(|it| it.metadata.color_label.as_deref() == label)
            });
        let label = if all_have { None } else { label };
        for p in paths {
            self.update_metadata(p, |m| m.color_label = label.map(str::to_string));
        }
        let n = paths.len();
        self.set_status(match label {
            Some(l) => format!("Labelled {n} photo(s) {l}"),
            None => format!("Took the label off {n} photo(s)"),
        });
    }

    /// Show only one label's photos, or all.
    pub(crate) fn label_filter(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::dim("Label:"));
        pod(ui, |ui| {
            if widgets::chip(ui, self.filter.color_label.is_none(), "All").clicked() {
                self.filter.color_label = None;
            }
            for (name, fill) in LABELS {
                let on = self.filter.color_label.as_deref() == Some(name);
                if swatch(ui, name, fill, on).clicked() {
                    self.filter.color_label = (!on).then(|| name.to_string());
                }
            }
        });
    }

    /// The open photo's label as swatches: a tap sets it, a second takes it off.
    pub(crate) fn label_picker(&mut self, ui: &mut egui::Ui) {
        let current = self
            .loaded_path
            .as_ref()
            .and_then(|p| self.shelf.iter().find(|it| &it.path == p))
            .and_then(|it| it.metadata.color_label.clone());
        let many = self.selection.count() > 1;
        for (name, fill) in LABELS {
            let on = !many && current.as_deref() == Some(name);
            if swatch(ui, name, fill, on).clicked() {
                self.set_label_for_selection(Some(name));
            }
        }
    }

    /// The open photo's keywords, each with a × to take it off, and a field that adds
    /// what is typed (commas for several) to it, or to every selected photo.
    pub(crate) fn keywords_editor(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.loaded_path.clone() else {
            return;
        };
        let keywords = self
            .shelf
            .iter()
            .find(|it| it.path == path)
            .map(|it| it.metadata.keywords.clone())
            .unwrap_or_default();
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(theme::dim("keywords").size(11.5));
            for word in &keywords {
                if widgets::chip(ui, true, &format!("{word} ×"))
                    .on_hover_text("Take this keyword off")
                    .clicked()
                {
                    remove = Some(word.clone());
                }
            }
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.keyword_draft)
                    .hint_text("add…")
                    .desired_width(110.0),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if entered && !self.keyword_draft.trim().is_empty() {
                let text = std::mem::take(&mut self.keyword_draft);
                let paths: Vec<PathBuf> = if self.selection.count() > 1 {
                    self.selection.selected.iter().cloned().collect()
                } else {
                    vec![path.clone()]
                };
                for p in &paths {
                    self.update_metadata(p, |m| {
                        m.add_keywords(&text);
                    });
                }
                field.request_focus();
            }
        });
        if let Some(word) = remove {
            self.update_metadata(&path, |m| m.remove_keyword(&word));
        }
    }
}
