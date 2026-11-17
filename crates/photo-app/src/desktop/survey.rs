//! Survey: the selected photos side by side, to choose between them (culling).
//!
//! Drawn from each photo's camera preview (or the file, shrunk), not the edit: the engine
//! holds one photo at a time, and N photos through N pipelines would be N times its
//! memory. So a survey compares what was shot, at up to [`SIDE`] px; Enter opens the
//! focused photo in the editor for a full-resolution look. Ratings, flags and labels act
//! on the focused photo only. A mode, but one that owns nothing: Esc always leaves it.

use crate::*;
use std::sync::mpsc;

/// The long side the previews are read at.
const SIDE: usize = 1200;
/// The most photos a survey shows; more would be too small to judge.
pub(crate) const MAX: usize = 12;

pub(crate) struct Survey {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) focus: usize,
    previews: HashMap<PathBuf, egui::TextureHandle>,
    rx: mpsc::Receiver<(PathBuf, Option<egui::ColorImage>)>,
}

impl PhotoApp {
    /// Survey the selection (in the order the shelf shows it), if it is two photos or
    /// more; the photo open, if it is one of them, has the focus.
    pub(crate) fn start_survey(&mut self, ctx: &egui::Context) {
        let shown = self.displayed_indices();
        let paths: Vec<PathBuf> = shown
            .iter()
            .map(|&i| self.shelf[i].path.clone())
            .filter(|p| self.selection.is_selected(p))
            .take(MAX)
            .collect();
        if paths.len() < 2 {
            self.set_status("Select two photos or more to compare them (N)");
            return;
        }
        let focus = self
            .loaded_path
            .as_ref()
            .and_then(|p| paths.iter().position(|q| q == p))
            .unwrap_or(0);
        let (tx, rx) = mpsc::channel();
        let (todo, ctx) = (paths.clone(), ctx.clone());
        let _ = std::thread::Builder::new()
            .name("duckfoot-survey".into())
            .spawn(move || {
                for p in todo {
                    let image = crate::loader::survey_preview(&p, SIDE);
                    if tx.send((p, image)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        self.survey = Some(Survey {
            paths,
            focus,
            previews: HashMap::new(),
            rx,
        });
    }

    /// The survey's keys, taken before any other; true if it is showing. Esc leaves,
    /// Enter opens the focused photo, ← → and Tab move the focus, and the marks act on
    /// the focused photo alone.
    pub(crate) fn survey_keys(&mut self, ctx: &egui::Context, gpu: &Gpu) -> bool {
        let Some(survey) = &mut self.survey else {
            return false;
        };
        let n = survey.paths.len();
        let (esc, enter, left, right, tab) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::N),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::ArrowLeft)
                    || (i.key_pressed(egui::Key::Tab) && i.modifiers.shift),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Tab) && !i.modifiers.shift,
            )
        });
        if left {
            survey.focus = (survey.focus + n - 1) % n;
        }
        if right || tab {
            survey.focus = (survey.focus + 1) % n;
        }
        let focused = survey.paths[survey.focus].clone();
        if esc {
            self.survey = None;
            return true;
        }
        if enter {
            self.survey = None;
            self.open_path(gpu, focused);
            return true;
        }
        let mark = ctx.input(|i| {
            if i.modifiers.command {
                return None;
            }
            let digits = [
                egui::Key::Num0,
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
            ];
            if let Some(r) = digits.iter().position(|k| i.key_pressed(*k)) {
                return Some(Mark::Rating(r as u8));
            }
            let labels = [
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ];
            if let Some(l) = labels.iter().position(|k| i.key_pressed(*k)) {
                return Some(Mark::Label(l));
            }
            [
                (egui::Key::P, photo_engine::Flag::Pick),
                (egui::Key::X, photo_engine::Flag::Reject),
                (egui::Key::U, photo_engine::Flag::Unflagged),
            ]
            .iter()
            .find(|(k, _)| i.key_pressed(*k))
            .map(|(_, f)| Mark::Flag(*f))
        });
        match mark {
            Some(Mark::Rating(r)) => self.set_rating_for_path(&focused, r),
            Some(Mark::Flag(f)) => self.set_flag_for_path(&focused, f),
            Some(Mark::Label(l)) => self.set_label_for_paths(&[focused], Some(labels::LABELS[l].0)),
            None => {}
        }
        true
    }

    /// The survey in place of the stage.
    pub(crate) fn survey_view(&mut self, ui: &mut egui::Ui, gpu: &Gpu) {
        let Some(survey) = &mut self.survey else {
            return;
        };
        for (path, image) in survey.rx.try_iter() {
            if let Some(image) = image {
                let name = format!("survey:{}", path.display());
                let tex = ui
                    .ctx()
                    .load_texture(name, image, egui::TextureOptions::LINEAR);
                survey.previews.insert(path, tex);
            }
        }
        let n = survey.paths.len();
        let area = ui.available_rect_before_wrap();
        // As square a grid as fits, wider than tall.
        let cols = ((n as f32 * area.width() / area.height().max(1.0))
            .sqrt()
            .ceil() as usize)
            .clamp(1, n);
        let rows = n.div_ceil(cols);
        let gap = 10.0;
        let cell = egui::vec2(
            (area.width() - gap * (cols - 1) as f32) / cols as f32,
            (area.height() - gap * (rows - 1) as f32) / rows as f32,
        );
        let mut clicked = None;
        let mut open = None;
        for (k, path) in survey.paths.iter().enumerate() {
            let (c, r) = (k % cols, k / cols);
            let rect = egui::Rect::from_min_size(
                area.min + egui::vec2(c as f32 * (cell.x + gap), r as f32 * (cell.y + gap)),
                cell,
            );
            let response = ui.interact(rect, ui.id().with(("survey", k)), egui::Sense::click());
            if response.clicked() {
                clicked = Some(k);
            }
            if response.double_clicked() {
                open = Some(path.clone());
            }
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, theme::RADIUS, theme::BG_900);
            let caption = 22.0;
            let photo_area =
                egui::Rect::from_min_max(rect.min, rect.max - egui::vec2(0.0, caption));
            match survey.previews.get(path) {
                Some(tex) => {
                    let size = tex.size_vec2();
                    let fit = (photo_area.width() / size.x).min(photo_area.height() / size.y);
                    let shown = egui::Rect::from_center_size(photo_area.center(), size * fit);
                    painter.image(
                        tex.id(),
                        shown,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                None => {
                    painter.text(
                        photo_area.center(),
                        Align2::CENTER_CENTER,
                        "…",
                        FontId::proportional(18.0),
                        theme::TEXT_FAINT,
                    );
                }
            }
            let item = self.shelf.get(path);
            let stars = item.map_or(0, |it| it.metadata.rating);
            let flag = item.map_or(photo_engine::Flag::Unflagged, |it| it.metadata.flag);
            let mark = match flag {
                photo_engine::Flag::Pick => " · P",
                photo_engine::Flag::Reject => " · X",
                photo_engine::Flag::Unflagged => "",
            };
            painter.text(
                egui::pos2(rect.left() + 8.0, rect.bottom() - caption / 2.0),
                Align2::LEFT_CENTER,
                format!("{}  {}{mark}", file_name(path), "★".repeat(stars as usize)),
                FontId::monospace(11.0),
                theme::TEXT_DIM,
            );
            if k == survey.focus {
                painter.rect_stroke(
                    rect.shrink(1.0),
                    theme::RADIUS,
                    Stroke::new(2.0, theme::ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if let Some(k) = clicked {
            if let Some(s) = &mut self.survey {
                s.focus = k;
            }
        }
        if let Some(path) = open {
            self.survey = None;
            self.open_path(gpu, path);
        }
    }
}

/// A mark a survey key makes.
enum Mark {
    Rating(u8),
    Flag(photo_engine::Flag),
    Label(usize),
}
