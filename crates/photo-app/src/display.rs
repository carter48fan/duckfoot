//! The screen the preview is shown on (W-14), and soft proofing (W-10): both change only
//! the preview's last step, never what an export writes.

use crate::*;
use photo_engine::engine::Proof;
use photo_engine::gamut::{OutputSpace, Screen};
use photo_engine::proof::{Intent, Monitor, Printing, ProofTable};
use std::sync::Arc;

/// A printer profile to proof through: read once when chosen, and baked into the engine's
/// table again whenever the export's space or the screen it is baked for changes.
pub(crate) struct Printer {
    icc: Vec<u8>,
    name: String,
    /// The export's space, the screen (`screen_rev`) and the printing it was baked for.
    baked_for: Option<(OutputSpace, u64, Printing)>,
}

/// The monitor's own profile (W-14), when the screen is a profile file that bakes: its
/// bytes, for the printer proof to end on, and its table, for the preview to finish
/// through.
/// What the shell found the system has for the window's monitor: its name and bytes,
/// nothing, or why it could not ask.
pub type SystemDisplay = Result<Option<(String, Vec<u8>)>, String>;

pub(crate) struct ScreenProfile {
    icc: Arc<[u8]>,
    table: ProofTable,
}

impl PhotoApp {
    /// Work out the screen the settings name, once, when they change. A profile file is
    /// baked through moxcms, so any monitor profile (LUT-based included) is shown as it
    /// says; its matrix and curve, where it has them, stand in for it where a table cannot
    /// (the phone). One that does not read falls back to the named space and says why.
    pub(crate) fn refresh_screen(&mut self) {
        let named = self.settings.display.map_or(Screen::SRGB, Screen::of);
        self.screen_note = None;
        self.screen_profile = None;
        self.screen_rev += 1;
        if self.settings.display_system {
            self.screen = match &self.system_display {
                Some(Ok(Some((name, bytes)))) => {
                    let (name, bytes) = (name.clone(), bytes.clone());
                    self.screen_from_icc(bytes, &name, named)
                }
                Some(Ok(None)) => {
                    self.screen_note =
                        Some("the system has no profile for this screen; showing sRGB".into());
                    named
                }
                Some(Err(e)) => {
                    self.screen_note = Some(format!("could not ask the system: {e}"));
                    named
                }
                None => named,
            };
            return;
        }
        self.screen = match &self.settings.display_icc {
            Some(path) => match std::fs::read(path) {
                Ok(bytes) => {
                    let name = file_name(path);
                    self.screen_from_icc(bytes, &name, named)
                }
                Err(e) => {
                    self.screen_note = Some(format!("{}: {e}", file_name(path)));
                    named
                }
            },
            None => named,
        };
    }

    /// The screen a monitor profile describes: baked through moxcms if it bakes, its
    /// matrix if not, else `named`, with the note saying why.
    fn screen_from_icc(&mut self, bytes: Vec<u8>, name: &str, named: Screen) -> Screen {
        let matrix = Screen::from_icc(&bytes);
        match ProofTable::bake_screen(&bytes) {
            Ok(table) => {
                self.screen_profile = Some(ScreenProfile {
                    icc: bytes.into(),
                    table,
                });
                matrix.unwrap_or(named)
            }
            Err(table_e) => match matrix {
                Ok(screen) => screen,
                Err(e) => {
                    self.screen_note = Some(format!("{name}: {e}; {table_e:#}"));
                    named
                }
            },
        }
    }

    /// What the shell found the system has for the window's monitor (looked up again
    /// whenever the window reaches another one).
    pub fn set_system_display(&mut self, found: SystemDisplay) {
        let changed = self.system_display.as_ref() != Some(&found);
        self.system_display = Some(found);
        if changed && self.settings.display_system {
            self.refresh_screen();
        }
    }

    /// The screen a printer proof ends on: the monitor's profile if it has one baked.
    fn monitor(&self) -> Monitor {
        match &self.screen_profile {
            Some(p) => Monitor::Profile(p.icc.clone()),
            None => Monitor::Matrix(self.screen),
        }
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
                let monitor = Monitor::Matrix(Screen::SRGB);
                ProofTable::bake(OutputSpace::Srgb, &icc, Printing::default(), &monitor)
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

    /// The `.cube` the Look module's picker answered with: on, and one undo step.
    pub(crate) fn use_look_file(&mut self, path: PathBuf) {
        if !self.can_edit() {
            return;
        }
        let mut next = self.stack.clone();
        next.look.enabled = true;
        next.look.params.path = Some(path.to_string_lossy().into_owned());
        if next != self.stack {
            self.history.checkpoint(&self.stack);
            self.stack = next;
            self.needs_render = true;
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
            space: Screen::of(self.export_settings.effective_space()),
            warn: self.proof_warn,
            printer: self.printer.is_some(),
        })
    }

    /// Tell the engine about a changed screen or proof, and re-render for it.
    pub(crate) fn sync_display(&mut self, gpu: &Gpu) {
        if self.screen_rev != self.shown_screen_rev {
            self.engine.set_screen(self.screen);
            let table = self.screen_profile.as_ref().map(|p| &p.table);
            self.engine.set_screen_table(gpu.queue, table);
            self.shown_screen_rev = self.screen_rev;
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
        let space = self.export_settings.effective_space();
        let printing = self.settings.proof_printing;
        let key = (space, self.screen_rev, printing);
        let monitor = self.monitor();
        let Some(printer) = &mut self.printer else {
            return;
        };
        if printer.baked_for == Some(key) {
            return;
        }
        match ProofTable::bake(space, &printer.icc, printing, &monitor) {
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
        let system = self.settings.display_system;
        let using_file =
            !system && self.settings.display_icc.is_some() && self.screen_note.is_none();
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for space in OutputSpace::ALL {
                let on =
                    !system && !using_file && self.settings.display.unwrap_or_default() == space;
                if widgets::chip(ui, on, space.label()).clicked() {
                    self.settings.display = Some(space);
                    self.settings.display_icc = None;
                    self.settings.display_system = false;
                    changed = true;
                }
            }
            if self.system_display.is_some()
                && widgets::chip(ui, system, "system")
                    .on_hover_text(
                        "The profile the system has for this monitor (colord, Windows colour \
                         management or ColorSync), followed as the window moves",
                    )
                    .clicked()
            {
                self.settings.display_system = !system;
                changed = true;
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
            (None, _) if system => match &self.system_display {
                Some(Ok(Some((name, _)))) => format!("using the system's {name}"),
                _ => "the system's profile for this monitor".into(),
            },
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
        self.settings.display_system = false;
        self.refresh_screen();
        self.save_settings();
        match &self.screen_note {
            Some(problem) => self.error = Some(format!("Could not use it: {problem}")),
            None => self.set_status("The preview now follows the monitor's profile"),
        }
    }

    /// The stage toolbar's proof chips: on and off (S), and the gamut warning.
    pub(crate) fn proof_chips(&mut self, ui: &mut egui::Ui) {
        let space = self.export_settings.effective_space().label();
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
