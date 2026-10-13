//! The screen the preview is shown on (W-14), and soft proofing (W-10): both change only
//! the preview's last step, never what an export writes.

use crate::*;
use photo_engine::engine::Proof;
use photo_engine::gamut::{OutputSpace, Screen};
use photo_engine::proof::{Intent, Printing, ProofTable};

/// A printer profile to proof through: read once when chosen, and baked into the engine's
/// table again whenever the export's space or the screen it is baked for changes.
pub(crate) struct Printer {
    icc: Vec<u8>,
    name: String,
    baked_for: Option<(OutputSpace, Screen, Printing)>,
}

impl PhotoApp {
    /// Work out the screen the settings name, once, when they change: a profile file
    /// that reads wins; one that does not falls back to the named space and says why.
    pub(crate) fn refresh_screen(&mut self) {
        let named = self.settings.display.map_or(Screen::SRGB, Screen::of);
        self.screen_note = None;
        self.screen = match &self.settings.display_icc {
            Some(path) => match std::fs::read(path)
                .map_err(|e| e.to_string())
                .and_then(|b| Screen::from_icc(&b).map_err(str::to_string))
            {
                Ok(screen) => screen,
                Err(e) => {
                    self.screen_note = Some(format!("{}: {e}", file_name(path)));
                    named
                }
            },
            None => named,
        };
    }

    /// Read the printer profile the settings name, once, when it changes. One that does
    /// not bake is dropped, and the note says why.
    pub(crate) fn refresh_printer(&mut self) {
        self.printer = None;
        self.printer_note = None;
        let Some(path) = &self.settings.proof_icc else {
            return;
        };
        let read = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|icc| {
                // A trial bake: a profile is only kept if it can be proofed through.
                ProofTable::bake(OutputSpace::Srgb, &icc, Printing::default(), Screen::SRGB)
                    .map(|_| icc)
                    .map_err(|e| format!("{e:#}"))
            });
        match read {
            Ok(icc) => {
                self.printer = Some(Printer {
                    icc,
                    name: file_name(path),
                    baked_for: None,
                })
            }
            Err(e) => self.printer_note = Some(format!("{}: {e}", file_name(path))),
        }
    }

    /// The printer profile the picker answered with.
    pub(crate) fn use_printer_profile(&mut self, path: PathBuf) {
        self.settings.proof_icc = Some(path);
        self.refresh_printer();
        self.save_settings();
        match (&self.printer_note, &self.printer) {
            (Some(problem), _) => self.error = Some(format!("Could not use it: {problem}")),
            (None, Some(p)) => {
                self.proofing = true;
                self.set_status(format!("Proofing as printed through {}", p.name));
            }
            (None, None) => {}
        }
    }

    /// The proof the preview should show, if it is proofing: against the colour space
    /// the export will be written in, and through the printer if one is chosen.
    pub(crate) fn wanted_proof(&self) -> Option<Proof> {
        self.proofing.then(|| Proof {
            space: Screen::of(self.export_settings.color_space),
            warn: self.proof_warn,
            printer: self.printer.is_some(),
        })
    }

    /// Tell the engine about a changed screen or proof, and re-render for it.
    pub(crate) fn sync_display(&mut self, gpu: &Gpu) {
        if self.screen != self.shown_screen {
            self.engine.set_screen(self.screen);
            self.shown_screen = self.screen;
            self.needs_render = true;
        }
        let proof = self.wanted_proof();
        if proof.is_some_and(|p| p.printer) {
            self.sync_printer_table(gpu);
        }
        if proof != self.shown_proof {
            self.engine.set_proof(proof);
            self.shown_proof = proof;
            self.needs_render = true;
        }
    }

    /// Bake the printer's table for the export's space and this screen, if it is not
    /// already baked for them (about 20 ms, so only when one of them changes).
    fn sync_printer_table(&mut self, gpu: &Gpu) {
        let space = self.export_settings.color_space;
        let printing = self.settings.proof_printing;
        let key = (space, self.screen, printing);
        let Some(printer) = &mut self.printer else {
            return;
        };
        if printer.baked_for == Some(key) {
            return;
        }
        match ProofTable::bake(space, &printer.icc, printing, self.screen) {
            Ok(table) => {
                self.engine.set_proof_table(gpu.queue, Some(&table));
                printer.baked_for = Some(key);
            }
            Err(e) => {
                // Proofing carries on against the space alone.
                self.printer_note = Some(format!("{}: {e:#}", printer.name));
                self.printer = None;
                self.engine.set_proof_table(gpu.queue, None);
            }
        }
        self.needs_render = true;
    }

    /// Customize's screen row: what the monitor shows, so the preview's colours are its.
    pub(crate) fn screen_row(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Screen")
                .family(theme::sans_strong())
                .size(12.5)
                .color(theme::TEXT),
        );
        let using_file = self.settings.display_icc.is_some() && self.screen_note.is_none();
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for space in OutputSpace::ALL {
                let on = !using_file && self.settings.display.unwrap_or_default() == space;
                if widgets::chip(ui, on, space.label()).clicked() {
                    self.settings.display = Some(space);
                    self.settings.display_icc = None;
                    changed = true;
                }
            }
            if widgets::chip(ui, using_file, "profile…")
                .on_hover_text("The monitor's own ICC profile, as a calibrator writes it")
                .clicked()
            {
                let from = self
                    .settings
                    .display_icc
                    .as_ref()
                    .and_then(|p| p.parent().map(PathBuf::from));
                self.request(ui.ctx(), platform::Request::DisplayProfile { from });
            }
        });
        let note = match (&self.screen_note, &self.settings.display_icc) {
            (Some(problem), _) => problem.clone(),
            (None, Some(path)) => format!("using {}", file_name(path)),
            (None, None) => "the preview's colours, for a screen of this kind".into(),
        };
        ui.label(theme::tag(note).color(theme::TEXT_FAINT));
        if changed {
            self.refresh_screen();
            self.save_settings();
        }
    }

    /// The display profile the picker answered with.
    pub(crate) fn use_display_profile(&mut self, path: PathBuf) {
        self.settings.display_icc = Some(path);
        self.refresh_screen();
        self.save_settings();
        match &self.screen_note {
            Some(problem) => self.error = Some(format!("Could not use it: {problem}")),
            None => self.set_status("The preview now follows the monitor's profile"),
        }
    }

    /// The stage toolbar's proof chips: on and off (S), and the gamut warning.
    pub(crate) fn proof_chips(&mut self, ui: &mut egui::Ui) {
        let space = self.export_settings.color_space.label();
        if widgets::chip(ui, self.proofing, "proof")
            .on_hover_text(format!(
                "Soft proof (S): the preview as the export, in {space}, will have its colours"
            ))
            .clicked()
        {
            self.proofing = !self.proofing;
        }
        if !self.proofing {
            return;
        }
        let cannot = match &self.printer {
            Some(p) => format!("Grey where printing through {} loses the colour", p.name),
            None => format!("Grey where {space} cannot hold the colour"),
        };
        if widgets::chip(ui, self.proof_warn, "gamut")
            .on_hover_text(cannot)
            .clicked()
        {
            self.proof_warn = !self.proof_warn;
        }
        match &self.printer {
            Some(p) => {
                let hover = format!(
                    "Shown as the {space} export will print through {}. Click to stop",
                    p.name
                );
                if widgets::chip(ui, true, &p.name)
                    .on_hover_text(hover)
                    .clicked()
                {
                    self.settings.proof_icc = None;
                    self.refresh_printer();
                    self.save_settings();
                    return;
                }
                let mut printing = self.settings.proof_printing;
                let perceptual = printing.intent == Intent::Perceptual;
                if widgets::chip(ui, perceptual, "perceptual")
                    .on_hover_text(
                        "Print perceptual: the whole image compressed into what the printer \
                         holds. Off: relative colorimetric, which keeps what it can print \
                         exactly and clips the rest",
                    )
                    .clicked()
                {
                    printing.intent = if perceptual {
                        Intent::RelativeColorimetric
                    } else {
                        Intent::Perceptual
                    };
                }
                if widgets::chip(ui, printing.paper, "paper")
                    .on_hover_text("Show the paper's own white and tint, not the screen's white")
                    .clicked()
                {
                    printing.paper = !printing.paper;
                }
                if printing != self.settings.proof_printing {
                    self.settings.proof_printing = printing;
                    self.save_settings();
                }
            }
            None => {
                let hover = match &self.printer_note {
                    Some(problem) => format!("Could not use the last one: {problem}"),
                    None => "Proof as printed: the printer and paper's ICC profile".into(),
                };
                if widgets::chip(ui, false, "printer…")
                    .on_hover_text(hover)
                    .clicked()
                {
                    let from = self
                        .settings
                        .proof_icc
                        .as_ref()
                        .and_then(|p| p.parent().map(PathBuf::from));
                    self.request(ui.ctx(), platform::Request::PrinterProfile { from });
                }
            }
        }
    }
}
