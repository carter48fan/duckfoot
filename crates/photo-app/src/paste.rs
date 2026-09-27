//! Selective paste (W-03): which parts of a copied look Paste, Same as previous and Paste
//! to all carry onto a photo. Every module by default; crop and rotation not, since they
//! belong to a photo rather than to a look. Remembered across sessions.

use photo_engine::PhotoStack;
use serde::{Deserialize, Serialize};

use crate::modules::PIPELINE_MODULES;
use crate::*;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PasteSet {
    /// Crop, rotation, mirroring and straightening too.
    #[serde(default)]
    pub geometry: bool,
    /// Modules left behind. Recorded as what is left out, so a module added in a later
    /// version is carried until someone says otherwise.
    #[serde(default)]
    pub left_out: Vec<InspectorSectionId>,
}

impl PasteSet {
    pub fn carries(&self, id: InspectorSectionId) -> bool {
        match id {
            InspectorSectionId::Scope => false,
            InspectorSectionId::Geometry => self.geometry,
            _ => !self.left_out.contains(&id),
        }
    }

    pub fn set(&mut self, id: InspectorSectionId, on: bool) {
        match id {
            InspectorSectionId::Scope => {}
            InspectorSectionId::Geometry => self.geometry = on,
            _ => {
                self.left_out.retain(|m| *m != id);
                if !on {
                    self.left_out.push(id);
                }
            }
        }
    }

    /// `target` with what this set carries taken from `copied`.
    pub fn apply(&self, copied: &PhotoStack, target: &PhotoStack) -> PhotoStack {
        let mut out = target.clone();
        let mut any = false;
        for &id in PIPELINE_MODULES {
            if self.carries(id) {
                id.reset(&mut out, copied);
                any = true;
            }
        }
        // A look is made in a working space and looks as it did only in it.
        if any {
            out.working_space = copied.working_space;
        }
        if self.geometry {
            out.geometry = copied.geometry;
        }
        out
    }

    /// What a paste carries, for the status line and the button's tip.
    pub fn describe(&self) -> String {
        let total = PIPELINE_MODULES.len();
        let carried = PIPELINE_MODULES
            .iter()
            .filter(|id| self.carries(**id))
            .count();
        match (carried == total, self.geometry, carried) {
            (true, false, _) => "everything but crop and rotation".into(),
            (true, true, _) => "everything, crop and rotation too".into(),
            (false, geometry, 0) => {
                if geometry {
                    "crop and rotation only".into()
                } else {
                    "nothing".into()
                }
            }
            (false, geometry, n) => format!(
                "{n} of {total} modules{}",
                if geometry { ", crop and rotation" } else { "" }
            ),
        }
    }
}

impl PhotoApp {
    /// The paste set's chips: one per module, crop & rotate first, then all and none.
    pub(crate) fn paste_set_editor(&mut self, ui: &mut egui::Ui) {
        ui.label(theme::dim(format!("Paste carries {}.", self.paste_set.describe())).size(12.0));
        ui.horizontal_wrapped(|ui| {
            let ids = std::iter::once(InspectorSectionId::Geometry)
                .chain(PIPELINE_MODULES.iter().copied());
            for id in ids {
                let on = self.paste_set.carries(id);
                if widgets::chip(ui, on, id.title()).clicked() {
                    self.paste_set.set(id, !on);
                }
            }
        });
        ui.horizontal(|ui| {
            if widgets::chip(ui, false, "all modules").clicked() {
                let geometry = self.paste_set.geometry;
                self.paste_set = PasteSet {
                    geometry,
                    left_out: Vec::new(),
                };
            }
            if widgets::chip(ui, false, "none").clicked() {
                self.paste_set = PasteSet {
                    geometry: false,
                    left_out: PIPELINE_MODULES.to_vec(),
                };
            }
        });
    }

    /// The desktop's paste picker, as a small dialog.
    pub(crate) fn paste_picker(&mut self, ctx: &egui::Context) {
        if !self.paste_picker_open || self.layout.is_compact() {
            return;
        }
        let mut open = true;
        egui::Window::new("What paste carries")
            .open(&mut open)
            .fade_in(false)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                self.paste_set_editor(ui);
                ui.add_space(6.0);
                if widgets::button(ui, Kind::Primary, "Done", 90.0).clicked() {
                    self.paste_picker_open = false;
                }
            });
        if !open {
            self.paste_picker_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited() -> PhotoStack {
        let mut s = PhotoStack::default();
        s.exposure.params.ev = 1.0;
        s.vignette.enabled = true;
        s.vignette.params.amount = -0.4;
        s.geometry.quarter_turns = 1;
        s
    }

    #[test]
    fn by_default_everything_but_the_framing_is_carried() {
        let set = PasteSet::default();
        let out = set.apply(&edited(), &PhotoStack::default());
        assert_eq!(out.exposure.params.ev, 1.0);
        assert!(out.vignette.enabled);
        assert_eq!(out.geometry.quarter_turns, 0);
        assert_eq!(set.describe(), "everything but crop and rotation");
    }

    #[test]
    fn what_is_left_out_stays_as_it_was() {
        let mut set = PasteSet::default();
        set.set(InspectorSectionId::Vignette, false);
        set.set(InspectorSectionId::Geometry, true);
        let out = set.apply(&edited(), &PhotoStack::default());
        assert_eq!(out.exposure.params.ev, 1.0);
        assert!(!out.vignette.enabled);
        assert_eq!(out.geometry.quarter_turns, 1);
        assert!(
            set.describe().ends_with("modules, crop and rotation"),
            "{}",
            set.describe()
        );
        // And it survives a round trip through the settings file.
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(serde_json::from_str::<PasteSet>(&json).unwrap(), set);
    }
}
