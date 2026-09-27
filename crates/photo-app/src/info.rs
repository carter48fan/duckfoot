//! What the file says about the photo, shown: the desktop inspector's Info band, the
//! phone Library's line under the open photo, and the filmstrip's tooltips.

use crate::*;
use photo_engine::PhotoInfo;

/// "1/250 s · f/2.8 · ISO 400 · 35 mm": the exposure, as far as the file says.
pub(crate) fn exposure_line(info: &PhotoInfo) -> String {
    let mut parts = Vec::new();
    if let Some(s) = info.shutter() {
        parts.push(s);
    }
    if let Some(f) = info.f_number {
        let f = format!("{f:.1}");
        parts.push(format!("f/{}", f.trim_end_matches(".0")));
    }
    if let Some(iso) = info.iso {
        parts.push(format!("ISO {iso}"));
    }
    if let Some(mm) = info.focal_length {
        let mm = format!("{mm:.1}");
        parts.push(format!("{} mm", mm.trim_end_matches(".0")));
    }
    parts.join(" · ")
}

/// The camera's name, without the maker's name said twice ("Canon Canon EOS R5").
pub(crate) fn camera(info: &PhotoInfo) -> String {
    let (make, model) = (info.make.trim(), info.model.trim());
    if model.is_empty() {
        return make.to_string();
    }
    let first = make.split_whitespace().next().unwrap_or("");
    if first.is_empty() || model.to_lowercase().starts_with(&first.to_lowercase()) {
        model.to_string()
    } else {
        format!("{make} {model}")
    }
}

/// "2026-05-04 10:11" from EXIF's "2026:05:04 10:11:12".
pub(crate) fn captured(info: &PhotoInfo) -> Option<String> {
    let c = info.captured.as_deref()?;
    let (date, time) = c.split_once(' ').unwrap_or((c, ""));
    let date = date.replace(':', "-");
    let time: String = time.chars().take(5).collect();
    Some(format!("{date} {time}").trim().to_string())
}

/// "48.85840° N, 2.29450° E": text, never a map; this is a local-first app.
pub(crate) fn place(info: &PhotoInfo) -> Option<String> {
    let (lat, lon) = info.gps?;
    Some(format!(
        "{:.5}° {}, {:.5}° {}",
        lat.abs(),
        if lat >= 0.0 { "N" } else { "S" },
        lon.abs(),
        if lon >= 0.0 { "E" } else { "W" }
    ))
}

/// One line for a tooltip or the phone: camera, then the exposure.
pub(crate) fn one_line(info: &PhotoInfo) -> String {
    [camera(info), exposure_line(info)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

impl PhotoApp {
    /// The open photo's metadata, once read.
    pub(crate) fn open_info(&self) -> Option<&PhotoInfo> {
        let path = self.loaded_path.as_ref()?;
        self.shelf.iter().find(|it| &it.path == path)?.info.as_ref()
    }

    /// The desktop inspector's Info band: folded to the camera and exposure until opened.
    pub(crate) fn info_band(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.loaded_path.clone() else {
            return;
        };
        let info = self.open_info().cloned();
        let head = widgets::band(ui, |ui| {
            ui.label(theme::head("Info"));
            if let Some(info) = &info {
                let line = exposure_line(info);
                if !line.is_empty() {
                    ui.label(theme::tag(line).color(theme::TEXT_DIM));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let tip = if self.info_open {
                    "Fold the info away"
                } else {
                    "How this photo was taken"
                };
                widgets::icon(ui, if self.info_open { "▾" } else { "▸" }, false, tip)
            })
            .inner
        });
        if head.inner.clicked() || head.response.interact(Sense::click()).clicked() {
            self.info_open = !self.info_open;
        }
        if !self.info_open {
            return;
        }
        egui::Frame::new()
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                self.info_rows(ui, &path);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(theme::dim("label").size(11.5));
                    self.label_picker(ui);
                });
                self.keywords_editor(ui);
            });
    }

    /// Everything known about the photo at `path`, as label and value rows: the desktop
    /// Info band's body and the phone Library sheet's detail card.
    pub(crate) fn info_rows(&self, ui: &mut egui::Ui, path: &std::path::Path) {
        let info = self.open_info().cloned();
        let mut rows: Vec<(&str, String)> = vec![("file", file_name(path))];
        if let Some(image) = self.ready_image() {
            let mp = image.sensor_width as f32 * image.sensor_height as f32 / 1e6;
            rows.push((
                "pixels",
                format!(
                    "{} × {} · {mp:.1} MP",
                    image.sensor_width, image.sensor_height
                ),
            ));
        }
        if let Some(item) = self.shelf.iter().find(|it| it.path == path) {
            rows.push(("size", crate::parts::format_bytes(item.bytes)));
        }
        match &info {
            Some(info) => {
                let cam = camera(info);
                if !cam.is_empty() {
                    rows.push(("camera", cam));
                }
                if let Some(lens) = &info.lens {
                    rows.push(("lens", lens.clone()));
                }
                let exposure = exposure_line(info);
                if !exposure.is_empty() {
                    rows.push(("exposure", exposure));
                }
                if let Some(when) = captured(info) {
                    rows.push(("taken", when));
                }
                if let Some(at) = place(info) {
                    rows.push(("location", at));
                }
                if let Some(a) = &info.artist {
                    rows.push(("artist", a.clone()));
                }
                if let Some(c) = &info.copyright {
                    rows.push(("copyright", c.clone()));
                }
            }
            None => rows.push(("", "the file says nothing more".into())),
        }
        egui::Grid::new(("info_rows", path))
            .num_columns(2)
            .spacing(vec2(12.0, 4.0))
            .show(ui, |ui| {
                for (label, value) in rows {
                    ui.label(theme::dim(label).size(11.5));
                    ui.add(
                        egui::Label::new(
                            RichText::new(value)
                                .monospace()
                                .size(11.5)
                                .color(theme::TEXT),
                        )
                        .wrap(),
                    );
                    ui.end_row();
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> PhotoInfo {
        PhotoInfo {
            make: "Canon".into(),
            model: "Canon EOS R5".into(),
            exposure_time: Some((1, 250)),
            f_number: Some(2.8),
            iso: Some(400),
            focal_length: Some(35.0),
            captured: Some("2026:05:04 10:11:12".into()),
            gps: Some((48.8584, -2.2945)),
            ..Default::default()
        }
    }

    #[test]
    fn metadata_reads_as_a_photographer_says_it() {
        let i = info();
        assert_eq!(exposure_line(&i), "1/250 s · f/2.8 · ISO 400 · 35 mm");
        assert_eq!(camera(&i), "Canon EOS R5");
        let sony = PhotoInfo {
            make: "SONY".into(),
            model: "ILCE-7M4".into(),
            ..Default::default()
        };
        assert_eq!(camera(&sony), "SONY ILCE-7M4");
        assert_eq!(captured(&i).as_deref(), Some("2026-05-04 10:11"));
        assert_eq!(place(&i).as_deref(), Some("48.85840° N, 2.29450° W"));
        assert_eq!(one_line(&PhotoInfo::default()), "");
    }
}
