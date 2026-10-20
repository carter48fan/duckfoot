//! Each module's controls.

use crate::*;

impl PhotoApp {
    /// The controls inside one module card.
    pub(crate) fn module_body(
        &mut self,
        ui: &mut egui::Ui,
        id: InspectorSectionId,
        changed: &mut bool,
        started: &mut bool,
    ) {
        macro_rules! track {
            ($r:expr) => {{
                let r: egui::Response = $r;
                if r.drag_started() || (r.changed() && !r.dragged()) {
                    *started = true;
                }
                if r.changed() {
                    *changed = true;
                }
            }};
        }
        // A click, not a drag: checkpoint and re-render in one go.
        macro_rules! discrete {
            () => {{
                *started = true;
                *changed = true;
            }};
        }

        // Where each control returns to on double-click.
        let d = self.baseline.clone();

        match id {
            InspectorSectionId::Scope => {
                if self.ready_image().is_none() {
                    ui.label(theme::dim("Open a photo to see its scopes.").size(12.0));
                } else if self.layout.is_compact() {
                    // Flat only on a phone: the 3D views need orbiting, which fights the
                    // sheet's own scrolling under a thumb.
                    let mut flat = self.inspector_map_state.clone();
                    flat.mode = inspector_map_3d::InspectorMapMode::Plot2D;
                    inspector_map_3d::render_inspector_map(ui, &mut flat, self.engine.histogram());
                } else {
                    ui.horizontal_wrapped(|ui| {
                        self.scope_mode_chips(ui, false);
                    });
                    inspector_map_3d::render_inspector_map(
                        ui,
                        &mut self.inspector_map_state,
                        self.engine.histogram(),
                    );
                }
            }
            InspectorSectionId::Geometry => {
                if self.ready_image().is_none() {
                    let text = if self.loaded_path.is_some() {
                        "Rotate, flip and crop as soon as the photo has loaded."
                    } else {
                        "Open a photo to rotate, flip or crop it."
                    };
                    ui.label(theme::dim(text).size(12.0));
                    return;
                }
                let g = &mut self.stack.geometry;
                let mut edited = false;
                let mut turned = false;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = widgets::sized(ui, 4.0, 8.0);
                    if geometry_icon_button(
                        ui,
                        GeometryIcon::RotateLeft,
                        false,
                        "Rotate 90° CCW ([)",
                    )
                    .clicked()
                    {
                        g.rotate_left();
                        edited = true;
                        turned = true;
                    }
                    if geometry_icon_button(
                        ui,
                        GeometryIcon::RotateRight,
                        false,
                        "Rotate 90° CW (])",
                    )
                    .clicked()
                    {
                        g.rotate_right();
                        edited = true;
                        turned = true;
                    }
                    if geometry_icon_button(
                        ui,
                        GeometryIcon::MirrorH,
                        g.mirror_h,
                        "Flip horizontally (H)",
                    )
                    .clicked()
                    {
                        g.flip_h();
                        edited = true;
                        turned = true;
                    }
                    if geometry_icon_button(
                        ui,
                        GeometryIcon::MirrorV,
                        g.mirror_v,
                        "Flip vertically (V)",
                    )
                    .clicked()
                    {
                        g.flip_v();
                        edited = true;
                        turned = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_enabled_ui(!g.crop.is_full_frame(), |ui| {
                            if widgets::outlined_chip(ui, true, false, "reset crop").clicked() {
                                g.crop = Crop::default();
                                edited = true;
                            }
                        });
                        if self.cropping {
                            ui.label(theme::tag("framing").color(theme::ACCENT));
                        }
                    });
                });
                if edited {
                    discrete!();
                }
                if turned {
                    self.rebase_crop();
                }
                ui.horizontal_wrapped(|ui| {
                    let pad = widgets::sized(ui, 3.0, 6.0);
                    ui.spacing_mut().item_spacing = vec2(pad, pad);
                    self.crop_chips(ui);
                    if widgets::outlined_chip(
                        ui,
                        true,
                        self.settings.constrain_crop,
                        "no empty corners",
                    )
                    .on_hover_text("When straightened, keep the crop on what was photographed")
                    .clicked()
                    {
                        self.set_constrain_crop(!self.settings.constrain_crop);
                    }
                });
                track!(widgets::param_slider(
                    ui,
                    "straighten",
                    &mut self.stack.geometry.straighten_deg,
                    -15.0..=15.0,
                    2,
                    d.geometry.straighten_deg,
                ));
                // Keystone: converging lines stood up, the frame zoomed to stay on the photo.
                track!(widgets::param_slider(
                    ui,
                    "vertical",
                    &mut self.stack.geometry.perspective_v,
                    -100.0..=100.0,
                    0,
                    0.0,
                )
                .on_hover_text("Stand up lines that lean in toward the top (positive) or bottom"));
                track!(widgets::param_slider(
                    ui,
                    "horizontal",
                    &mut self.stack.geometry.perspective_h,
                    -100.0..=100.0,
                    0,
                    0.0,
                )
                .on_hover_text("Square up a wall shot from one side"));
                // Auto upright: from the straight lines the photo has.
                if self.ready_image().is_some() {
                    use photo_engine::keystone::Upright;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                        ui.label(theme::tag("auto").color(theme::TEXT_DIM));
                        for (mode, label, tip) in [
                            (
                                Upright::Level,
                                "level",
                                "Straighten by the horizon and the verticals",
                            ),
                            (
                                Upright::Vertical,
                                "vertical",
                                "Straighten, and stand leaning verticals up",
                            ),
                            (
                                Upright::Full,
                                "full",
                                "Both keystones and straighten, from every line",
                            ),
                        ] {
                            if widgets::outlined_chip(ui, true, false, label)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                self.pending_sample = Some(crate::Sample::Upright(mode));
                            }
                        }
                    });
                }
            }
            InspectorSectionId::WhiteBalance if self.ready_image().is_none() => {
                // Kelvin and tint are this camera's; until the decode says which camera,
                // there is no scale to read them on.
                ui.label(theme::dim("White balance as soon as the photo has loaded.").size(12.0));
            }
            InspectorSectionId::WhiteBalance => {
                // Kelvin and tint through this camera's own colour matrix, so the numbers
                // name real lights and the as-shot white reads as what the camera metered.
                let scale = self.white_scale();
                let p = &mut self.stack.white_balance.params;
                let dp = d.white_balance.params;
                let (mut kelvin, mut tint) = scale.temperature(p);
                let (home_k, home_t) = scale.temperature(&dp);
                let (k_lo, k_hi) = (WB_KELVIN_MIN, WB_KELVIN_MAX);
                // Evenly spaced in mired, as Lightroom's is: a step from 3000 to 3500 K is
                // as visible as one from 7000 to 10000 K.
                let (m_lo, m_hi) = (1e6 / k_lo, 1e6 / k_hi);
                // Adobe's standard lights, so a preset means the same here as in Lightroom.
                // The one matching the current setting is lit (desktop: named).
                let mut preset = None;
                let as_shot = (p.tint_r - 1.0).abs() < 1e-4 && (p.tint_b - 1.0).abs() < 1e-4;
                let lights = [
                    ("daylight", "5500 K, tint +10", 5500.0, 10.0),
                    ("cloudy", "6500 K, tint +10", 6500.0, 10.0),
                    ("shade", "7500 K, tint +10", 7500.0, 10.0),
                    ("tungsten", "2850 K incandescent", 2850.0, 0.0),
                    ("fluoro", "3800 K, tint +21", 3800.0, 21.0),
                    ("flash", "5500 K", 5500.0, 0.0),
                ];
                let lit = |pk: f32, pt: f32| {
                    !as_shot && (kelvin - pk).abs() < 5.0 && (tint - pt).abs() < 0.5
                };
                if !widgets::is_touch(ui.ctx()) {
                    // The desktop names the setting and keeps the list in a menu: one line,
                    // not two rows of chips.
                    let current = if as_shot {
                        "As shot"
                    } else {
                        lights
                            .iter()
                            .find(|l| lit(l.2, l.3))
                            .map_or("Custom", |l| l.0)
                    };
                    let mut current = current.to_string();
                    if let Some(first) = current.get_mut(0..1) {
                        first.make_ascii_uppercase();
                    }
                    // One midline for the label, the menu and PICK, which all come at
                    // their own heights.
                    widgets::bar_line(ui, 28.0, |ui| {
                        ui.label(RichText::new("White balance").size(12.5).color(theme::TEXT));
                        ui.menu_button(
                            RichText::new(format!("{current} ▾"))
                                .size(13.0)
                                .color(theme::TEXT_HI),
                            |ui| {
                                ui.set_min_width(180.0);
                                let row = |ui: &mut egui::Ui, name: &str, tip: String, on: bool| {
                                    let colour = if on { theme::ACCENT } else { theme::TEXT };
                                    ui.add(
                                        egui::Button::new(RichText::new(name).color(colour))
                                            .shortcut_text(
                                                theme::num(tip).size(11.0).color(theme::TEXT_FAINT),
                                            )
                                            .frame(false)
                                            .min_size(vec2(ui.available_width(), 24.0)),
                                    )
                                    .clicked()
                                };
                                if row(ui, "As shot", format!("{home_k:.0} K"), as_shot) {
                                    preset = Some(dp);
                                    ui.close();
                                }
                                if row(ui, "Auto", "whole photo".into(), false) {
                                    self.pending_sample = Some(crate::Sample::AutoWb);
                                    ui.close();
                                }
                                for (label, tip, pk, pt) in lights {
                                    let wb = scale.white_balance(pk, pt);
                                    let mut name = label.to_string();
                                    if let Some(first) = name.get_mut(0..1) {
                                        first.make_ascii_uppercase();
                                    }
                                    let chosen = ui
                                        .add_enabled_ui(wb.is_some(), |ui| {
                                            row(ui, &name, tip.to_string(), lit(pk, pt))
                                        })
                                        .inner;
                                    if chosen {
                                        preset = wb;
                                        ui.close();
                                    }
                                }
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::outlined_chip(ui, true, self.picking_wb, "pick")
                                .on_hover_text("Click something that should be grey (W)")
                                .clicked()
                            {
                                self.picking_wb = !self.picking_wb;
                            }
                        });
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                        if widgets::outlined_chip(ui, true, self.picking_wb, "pick")
                            .on_hover_text("Click something that should be grey (W)")
                            .clicked()
                        {
                            self.picking_wb = !self.picking_wb;
                        }
                        if widgets::outlined_chip(ui, true, false, "auto")
                            .on_hover_text("Balance the whole photo")
                            .clicked()
                        {
                            self.pending_sample = Some(crate::Sample::AutoWb);
                        }
                        if widgets::outlined_chip(ui, true, as_shot, "as shot")
                            .on_hover_text(format!("What the camera metered: {home_k:.0} K"))
                            .clicked()
                        {
                            preset = Some(dp);
                        }
                        for (label, tip, pk, pt) in lights {
                            let wb = scale.white_balance(pk, pt);
                            let chip = widgets::outlined_chip(ui, wb.is_some(), lit(pk, pt), label)
                                .on_hover_text(tip);
                            if chip.clicked() {
                                preset = wb;
                            }
                        }
                    });
                }

                let k = widgets::scaled_slider(
                    ui,
                    "temp (K)",
                    &mut kelvin,
                    k_lo..=k_hi,
                    0,
                    home_k,
                    widgets::SliderScale {
                        to_rail: &|k| (m_lo - 1e6 / k) / (m_lo - m_hi),
                        from_rail: &|t| 1e6 / (m_lo - t * (m_lo - m_hi)),
                        speed: 10.0,
                        gradient: Some([theme::WB_COOL, theme::WB_WARM]),
                    },
                )
                .on_hover_text("The light the photo was taken under. Higher is warmer.");
                let t = widgets::scaled_slider(
                    ui,
                    "tint",
                    &mut tint,
                    -150.0..=150.0,
                    0,
                    home_t,
                    widgets::SliderScale {
                        to_rail: &|v| (v + 150.0) / 300.0,
                        from_rail: &|t| t * 300.0 - 150.0,
                        speed: 0.5,
                        gradient: Some([theme::WB_GREEN, theme::WB_MAGENTA]),
                    },
                )
                .on_hover_text("Negative is greener, positive more magenta.");
                let both = k | t;
                if both.changed() {
                    // A light the camera cannot neutralise with positive gains is simply
                    // out of reach: the slider stops rather than jumping somewhere else.
                    if let Some(wb) = scale.white_balance(kelvin, tint) {
                        *p = wb;
                    }
                }
                track!(both);

                if let Some(wb) = preset {
                    *p = wb;
                    discrete!();
                }
                // Where every module works (W-09): wide for every new edit; an edit made
                // before keeps sRGB, and its look, until it is moved.
                use photo_engine::gamut::WorkingSpace;
                let mut space = None;
                // The caption over its choices, as tone mapping's is: beside them, the
                // chips start wherever the caption happens to end.
                ui.label(theme::tag("colour space").color(theme::TEXT_DIM));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    let now = self.stack.working_space;
                    for (s, label, tip) in [
                        (
                            WorkingSpace::Rec2020,
                            "wide",
                            "Rec.2020: saturated colours keep their hue and detail",
                        ),
                        (
                            WorkingSpace::Srgb,
                            "sRGB (older)",
                            "How edits were made before: saturated colours clip, and shift hue",
                        ),
                    ] {
                        if widgets::outlined_chip(ui, true, now == s, label)
                            .on_hover_text(tip)
                            .clicked()
                            && now != s
                        {
                            space = Some(s);
                        }
                    }
                });
                if let Some(s) = space {
                    self.stack.working_space = s;
                    discrete!();
                }
            }
            InspectorSectionId::Exposure => {
                let ready = self.ready_image().is_some();
                let p = &mut self.stack.exposure.params;
                let dp = d.exposure.params;

                // The desktop shows the modes as tabs under a caption; the phone as chips.
                let touch = widgets::is_touch(ui.ctx());
                if !touch {
                    ui.label(theme::tag("tone mapping").color(theme::TEXT_DIM));
                }
                ui.horizontal(|ui| {
                    if touch {
                        ui.label(theme::tag("mode").color(theme::TEXT_DIM));
                    }
                    let tab_w =
                        ((ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0).floor();
                    let modes = [
                        (
                            photo_engine::params::ExposureMode::ClassicLinear,
                            "Classic",
                            "Pure linear gain without highlight compression. Matches Lightroom Classic's punchy, direct sensor response.",
                        ),
                        (
                            photo_engine::params::ExposureMode::ModernSoft,
                            "Modern",
                            "Soft highlight shoulder (similar to Lightroom 2026). Compresses extreme highlights to preserve detail in blown areas.",
                        ),
                        (
                            photo_engine::params::ExposureMode::FilmicShoulder,
                            "Filmic",
                            "Cinematic filmic shoulder. Gentle logarithmic knee simulating analog film highlight latitude.",
                        ),
                        (
                            photo_engine::params::ExposureMode::Custom,
                            "Custom",
                            "Fine-tune the compression shoulder rolloff manually.",
                        ),
                    ];
                    for (m, label, tip) in modes {
                        let sel = p.mode == m;
                        let choice = if touch {
                            widgets::outlined_chip(ui,true, sel, label)
                        } else {
                            widgets::tab(ui, sel, label, tab_w)
                        };
                        if choice.on_hover_text(tip).clicked() {
                            p.mode = m;
                            match m {
                                photo_engine::params::ExposureMode::ClassicLinear => p.rolloff = 0.0,
                                photo_engine::params::ExposureMode::ModernSoft => p.rolloff = 0.25,
                                photo_engine::params::ExposureMode::FilmicShoulder => p.rolloff = 0.50,
                                photo_engine::params::ExposureMode::Custom => {}
                            }
                            discrete!();
                        }
                    }
                });

                // A picture of the shoulder the tabs choose, under them, where there is
                // room for one: on a phone the mode chips say the same in a line.
                if !touch {
                    widgets::mini_knee_curve(ui, p.ev, p.rolloff);
                }

                let max_ev = p.effective_max_ev();
                track!(widgets::param_slider(
                    ui,
                    "EV",
                    &mut p.ev,
                    -max_ev..=max_ev,
                    2,
                    dp.ev
                ));
                track!(widgets::param_slider(
                    ui,
                    "black",
                    &mut p.black,
                    -0.1..=0.1,
                    4,
                    dp.black
                ));

                if p.mode == photo_engine::params::ExposureMode::Custom {
                    track!(widgets::param_slider(
                        ui,
                        "rolloff",
                        &mut p.rolloff,
                        0.0..=1.0,
                        2,
                        0.0
                    ));
                }

                // The section's buttons on one line: auto tone at the start, the EV
                // slider's range chips at the end.
                let mut auto = false;
                widgets::bar_line(ui, 28.0, |ui| {
                    if widgets::outlined_chip(ui, ready, false, "auto tone")
                        .on_hover_text(
                            "Exposure to put the middle tones at mid grey, and highlights and \
                             shadows eased back from clipping",
                        )
                        .clicked()
                    {
                        auto = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let cur_max = p.effective_max_ev();
                        for &(ev_val, label, tip) in &[
                            (10.0, "±10 EV", "Studio / technical range: ±10 stops"),
                            (8.0, "±8 EV", "Extended range: ±8 stops"),
                            (5.0, "±5 EV", "Lightroom standard: ±5 stops"),
                        ] {
                            let active = (cur_max - ev_val).abs() < 0.1;
                            if widgets::outlined_chip(ui, true, active, label)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                p.max_ev = ev_val;
                                p.extended_range = ev_val > 5.0;
                                discrete!();
                            }
                        }
                    });
                });
                if auto {
                    self.pending_sample = Some(crate::Sample::AutoTone);
                }
            }
            InspectorSectionId::Tonal => {
                let p = &mut self.stack.tonal.params;
                let dp = d.tonal.params;
                track!(widgets::param_slider(
                    ui,
                    "highlights",
                    &mut p.highlights,
                    -1.0..=1.0,
                    2,
                    dp.highlights
                ));
                track!(widgets::param_slider(
                    ui,
                    "shadows",
                    &mut p.shadows,
                    -1.0..=1.0,
                    2,
                    dp.shadows
                ));
                track!(widgets::param_slider(
                    ui,
                    "whites",
                    &mut p.whites,
                    -1.0..=1.0,
                    2,
                    dp.whites
                ));
                track!(widgets::param_slider(
                    ui,
                    "blacks",
                    &mut p.blacks,
                    -1.0..=1.0,
                    2,
                    dp.blacks
                ));
            }
            InspectorSectionId::ToneCurve => {
                let mut chan: CurveChannel = ui
                    .data(|d| d.get_temp(ui.id().with("curve_chan")))
                    .unwrap_or(CurveChannel::Rgb);
                ui.horizontal(|ui| {
                    for (c, label) in [
                        (CurveChannel::Rgb, "RGB"),
                        (CurveChannel::Red, "Red"),
                        (CurveChannel::Green, "Green"),
                        (CurveChannel::Blue, "Blue"),
                    ] {
                        if widgets::outlined_chip(ui, true, chan == c, label).clicked() {
                            chan = c;
                            ui.data_mut(|d| d.insert_temp(ui.id().with("curve_chan"), chan));
                        }
                    }
                });

                match chan {
                    CurveChannel::Rgb => {
                        track!(curve_editor_channel(
                            ui,
                            &mut self.stack.tone_curve.params,
                            chan
                        ));
                    }
                    CurveChannel::Red => {
                        self.stack.rgb_curves.enabled = true;
                        track!(curve_editor_channel(
                            ui,
                            &mut self.stack.rgb_curves.params.red,
                            chan
                        ));
                    }
                    CurveChannel::Green => {
                        self.stack.rgb_curves.enabled = true;
                        track!(curve_editor_channel(
                            ui,
                            &mut self.stack.rgb_curves.params.green,
                            chan
                        ));
                    }
                    CurveChannel::Blue => {
                        self.stack.rgb_curves.enabled = true;
                        track!(curve_editor_channel(
                            ui,
                            &mut self.stack.rgb_curves.params.blue,
                            chan
                        ));
                    }
                }
                widgets::bar_line(ui, 28.0, |ui| {
                    let hint = if widgets::is_touch(ui.ctx()) {
                        "tap to add · drag · double-tap to remove"
                    } else {
                        "click to add · drag · double-click to remove"
                    };
                    ui.label(theme::tag(hint).color(theme::TEXT_FAINT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_reset = match chan {
                            CurveChannel::Rgb => {
                                self.stack.tone_curve.params != d.tone_curve.params
                            }
                            CurveChannel::Red => {
                                self.stack.rgb_curves.params.red != identity_nodes()
                            }
                            CurveChannel::Green => {
                                self.stack.rgb_curves.params.green != identity_nodes()
                            }
                            CurveChannel::Blue => {
                                self.stack.rgb_curves.params.blue != identity_nodes()
                            }
                        };
                        ui.add_enabled_ui(can_reset, |ui| {
                            if widgets::outlined_chip(ui, true, false, "reset").clicked() {
                                match chan {
                                    CurveChannel::Rgb => {
                                        self.stack.tone_curve.params = d.tone_curve.params.clone()
                                    }
                                    CurveChannel::Red => {
                                        self.stack.rgb_curves.params.red = identity_nodes()
                                    }
                                    CurveChannel::Green => {
                                        self.stack.rgb_curves.params.green = identity_nodes()
                                    }
                                    CurveChannel::Blue => {
                                        self.stack.rgb_curves.params.blue = identity_nodes()
                                    }
                                }
                                self.stack.curve_domain = photo_engine::CurveDomain::Display;
                                discrete!();
                            }
                        });
                    });
                });
                if self.stack.curve_domain == photo_engine::CurveDomain::Linear {
                    // An edit from before curves moved to display values: it keeps its
                    // look, on axes that do not match the histogram under them.
                    widgets::bar_line(ui, 28.0, |ui| {
                        ui.label(theme::tag("older curve · linear axes").color(theme::TEXT_DIM))
                            .on_hover_text(
                                "Made before the curve's axes matched the histogram. It looks as \
                             it always did; update it to edit on display values.",
                            );
                        if widgets::outlined_chip(ui, true, false, "update").clicked() {
                            let nodes = &mut self.stack.tone_curve.params;
                            for n in nodes.iter_mut() {
                                n.x = encode_srgb(n.x);
                                n.y = encode_srgb(n.y);
                            }
                            self.stack.curve_domain = photo_engine::CurveDomain::Display;
                            discrete!();
                        }
                    });
                }
            }
            InspectorSectionId::Clarity => {
                let p = &mut self.stack.clarity.params;
                let dp = d.clarity.params;
                track!(widgets::param_slider(
                    ui,
                    "clarity",
                    &mut p.clarity,
                    -100.0..=100.0,
                    1,
                    dp.clarity
                ));
                track!(widgets::param_slider(
                    ui,
                    "texture",
                    &mut p.texture,
                    -100.0..=100.0,
                    1,
                    dp.texture
                ));
            }
            InspectorSectionId::Dehaze => {
                let p = &mut self.stack.dehaze.params;
                let dp = d.dehaze.params;
                track!(widgets::param_slider(
                    ui,
                    "amount",
                    &mut p.amount,
                    -100.0..=100.0,
                    1,
                    dp.amount
                ));
            }
            InspectorSectionId::ColorEq => {
                let p = &mut self.stack.color_eq.params;
                let dp = d.color_eq.params;

                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (ColorEqTab::Hue, "Hue"),
                        (ColorEqTab::Saturation, "Saturation"),
                        (ColorEqTab::Luminance, "Luminance"),
                    ] {
                        let sel = self.color_eq_tab == tab;
                        if widgets::outlined_chip(ui, true, sel, label).clicked() {
                            self.color_eq_tab = tab;
                            if self.tat_mode.is_some() {
                                self.tat_mode = Some(crate::tat::TatMode::ColorEq(tab));
                            }
                        }
                    }
                    let tat_on =
                        self.tat_mode == Some(crate::tat::TatMode::ColorEq(self.color_eq_tab));
                    if widgets::outlined_chip(ui, true, tat_on, "🎯 on-photo drag")
                        .on_hover_text("Drag directly on colors in the photo to adjust their band")
                        .clicked()
                    {
                        if tat_on {
                            self.tat_mode = None;
                        } else {
                            self.tat_mode = Some(crate::tat::TatMode::ColorEq(self.color_eq_tab));
                        }
                    }
                });
                ui.add_space(4.0);

                for ((name, band), (_, home_band)) in p.bands_mut().into_iter().zip(dp.bands()) {
                    match self.color_eq_tab {
                        ColorEqTab::Hue => {
                            track!(widgets::param_slider(
                                ui,
                                name,
                                &mut band.hue,
                                -30.0..=30.0,
                                1,
                                home_band.hue
                            ));
                        }
                        ColorEqTab::Saturation => {
                            track!(widgets::param_slider(
                                ui,
                                name,
                                &mut band.saturation,
                                -100.0..=100.0,
                                1,
                                home_band.saturation
                            ));
                        }
                        ColorEqTab::Luminance => {
                            track!(widgets::param_slider(
                                ui,
                                name,
                                &mut band.luminance,
                                -100.0..=100.0,
                                1,
                                home_band.luminance
                            ));
                        }
                    }
                }
            }
            InspectorSectionId::Filmic3D => {
                let p = &mut self.stack.filmic.params;
                let dp = d.filmic.params;
                if self.layout.is_compact() {
                    // Flat on a phone, as the scope is: the sliders below set the curve.
                    tonemap_3d::filmic_presets(ui, p, changed, started);
                    tonemap_3d::render_filmic_2d(ui, p);
                } else {
                    tonemap_3d::render_tonemap_3d(
                        ui,
                        &mut self.tonemap_3d_state,
                        p,
                        changed,
                        started,
                    );
                }
                ui.add_space(4.0);
                track!(widgets::param_slider(
                    ui,
                    "white EV",
                    &mut p.white_ev,
                    0.0..=8.0,
                    2,
                    dp.white_ev
                ));
                track!(widgets::param_slider(
                    ui,
                    "black EV",
                    &mut p.black_ev,
                    -16.0..=-1.0,
                    2,
                    dp.black_ev
                ));
                track!(widgets::param_slider(
                    ui,
                    "latitude %",
                    &mut p.latitude,
                    5.0..=80.0,
                    1,
                    dp.latitude
                ));
                track!(widgets::param_slider(
                    ui,
                    "contrast",
                    &mut p.contrast,
                    0.1..=3.0,
                    2,
                    dp.contrast
                ));
            }
            InspectorSectionId::ColorBalance => {
                let p = &mut self.stack.color_balance.params;
                let dp = d.color_balance.params;
                let touch = widgets::is_touch(ui.ctx());

                let mut current_tab = ui.data_mut(|d| {
                    d.get_temp::<usize>(egui::Id::new("duckfoot.cb_tab"))
                        .unwrap_or(0)
                });

                if touch {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (idx, name) in ["lift", "gamma", "gain", "offset", "all"]
                            .into_iter()
                            .enumerate()
                        {
                            let active = current_tab == idx;
                            if widgets::outlined_chip(ui, true, active, name).clicked() {
                                current_tab = idx;
                                ui.data_mut(|d| {
                                    d.insert_temp(egui::Id::new("duckfoot.cb_tab"), idx)
                                });
                            }
                        }
                    });
                    ui.add_space(4.0);
                }

                let sections = [
                    ("lift", &mut p.lift, dp.lift, 0),
                    ("gamma", &mut p.gamma, dp.gamma, 1),
                    ("gain", &mut p.gain, dp.gain, 2),
                    ("offset", &mut p.offset, dp.offset, 3),
                ];

                for (label, rgb, home, idx) in sections {
                    if touch && current_tab != 4 && current_tab != idx {
                        continue;
                    }
                    ui.add_space(2.0);
                    ui.label(theme::tag(label).color(theme::TEXT_DIM));
                    let range = -0.5..=0.5;
                    track!(widgets::param_slider(
                        ui,
                        "red",
                        &mut rgb.r,
                        range.clone(),
                        3,
                        home.r
                    ));
                    track!(widgets::param_slider(
                        ui,
                        "green",
                        &mut rgb.g,
                        range.clone(),
                        3,
                        home.g
                    ));
                    track!(widgets::param_slider(
                        ui, "blue", &mut rgb.b, range, 3, home.b
                    ));
                }
            }
            InspectorSectionId::Vibrance => {
                let p = &mut self.stack.vibrance.params;
                let dp = d.vibrance.params;
                track!(widgets::param_slider(
                    ui,
                    "vibrance",
                    &mut p.vibrance,
                    -1.0..=1.0,
                    2,
                    dp.vibrance
                ));
                track!(widgets::param_slider(
                    ui,
                    "saturation",
                    &mut p.saturation,
                    -1.0..=1.0,
                    2,
                    dp.saturation
                ));
            }
            InspectorSectionId::Sharpen => {
                let p = &mut self.stack.sharpen.params;
                let dp = d.sharpen.params;
                if widgets::is_touch(ui.ctx()) {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (label, val) in [
                            ("0.0", 0.0),
                            ("0.25", 0.25),
                            ("0.50", 0.50),
                            ("0.75", 0.75),
                            ("1.0", 1.0),
                        ] {
                            let active = (p.amount - val).abs() < 0.05;
                            if widgets::outlined_chip(ui, true, active, label).clicked() {
                                p.amount = val;
                                *changed = true;
                                *started = true;
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
                track!(widgets::param_slider(
                    ui,
                    "amount",
                    &mut p.amount,
                    0.0..=2.0,
                    2,
                    dp.amount
                ));
                track!(widgets::param_slider(
                    ui,
                    "radius",
                    &mut p.radius,
                    0.5..=3.0,
                    1,
                    dp.radius
                ));
                track!(widgets::param_slider(
                    ui,
                    "detail",
                    &mut p.detail,
                    0.0..=100.0,
                    0,
                    dp.detail
                ));
                track!(widgets::param_slider(
                    ui,
                    "masking",
                    &mut p.masking,
                    0.0..=100.0,
                    0,
                    dp.masking
                ));
            }
            InspectorSectionId::Denoise => {
                let iso = self.open_info().and_then(|i| i.iso);
                if let Some(iso) = iso {
                    let suggested = photo_engine::params::Denoise::for_iso(iso);
                    let lit = self.stack.denoise.enabled && self.stack.denoise.params == suggested;
                    if widgets::outlined_chip(ui, true, lit, &format!("match ISO {iso}"))
                        .on_hover_text("A starting strength for how much noise this ISO leaves")
                        .clicked()
                    {
                        self.stack.denoise.params = suggested;
                        self.stack.denoise.enabled = true;
                        discrete!();
                    }
                }
                let p = &mut self.stack.denoise.params;
                let dp = d.denoise.params;
                for (label, value, default) in [
                    ("luminance", &mut p.luminance, dp.luminance),
                    ("detail", &mut p.detail, dp.detail),
                    ("colour", &mut p.color, dp.color),
                    ("smoothness", &mut p.smoothness, dp.smoothness),
                ] {
                    track!(widgets::param_slider(
                        ui,
                        label,
                        value,
                        0.0..=100.0,
                        0,
                        default
                    ));
                }
            }
            InspectorSectionId::Lens => {
                // What the photo's lens is corrected from before the sliders, if anything.
                let own = self
                    .ready_image()
                    .is_some_and(|i| i.corrects_its_own_lens());
                let found = self
                    .ready_image()
                    .and_then(|i| i.lens_profile_name())
                    .map(String::from);
                let lens_name = self.open_info().and_then(|i| i.lens.clone());
                ui.horizontal_wrapped(|ui| {
                    if own {
                        ui.label(theme::dim("The file corrects its own lens.").size(12.0));
                    } else if let Some(name) = &found {
                        let on = self.stack.lens.params.profile;
                        if widgets::outlined_chip(ui,true, on, "profile")
                            .on_hover_text("Correct distortion, fringes and fall-off from the lens's profile (lensfun)")
                            .clicked()
                        {
                            self.stack.lens.params.profile = !on;
                            discrete!();
                        }
                        ui.label(theme::dim(name).size(12.0));
                    } else if let Some(lens) = &lens_name {
                        ui.label(theme::dim(format!("No profile for {lens}.")).size(12.0));
                    }
                });
                let p = &mut self.stack.lens.params;
                let dp = d.lens.params;
                for (label, value, default) in [
                    ("distortion", &mut p.distortion, dp.distortion),
                    ("red fringe", &mut p.ca_red, dp.ca_red),
                    ("blue fringe", &mut p.ca_blue, dp.ca_blue),
                    ("vignetting", &mut p.vignetting, dp.vignetting),
                ] {
                    track!(widgets::param_slider(
                        ui,
                        label,
                        value,
                        -100.0..=100.0,
                        0,
                        default
                    ));
                }
            }
            InspectorSectionId::Look => {
                let path = self.stack.look.params.path.clone();
                let (mut pick, mut clear) = (false, false);
                ui.horizontal_wrapped(|ui| {
                    pick = widgets::outlined_chip(ui, true, false, ".cube file…")
                        .on_hover_text(
                            "A 3D LUT look (.cube, as Resolve and most LUT packs write \
                             them), applied to the photo as an sRGB screen shows it",
                        )
                        .clicked();
                    if path.is_some() {
                        clear = widgets::outlined_chip(ui, true, false, "clear").clicked();
                    }
                });
                let note = match &path {
                    None => "no look chosen".to_string(),
                    Some(p) => {
                        let name = file_name(std::path::Path::new(p));
                        if self.stack.look.enabled && !self.engine.look_loaded() {
                            format!("{name} could not be read")
                        } else {
                            name
                        }
                    }
                };
                ui.label(theme::tag(note).color(theme::TEXT_FAINT));
                let p = &mut self.stack.look.params;
                track!(widgets::param_slider(
                    ui,
                    "amount",
                    &mut p.amount,
                    0.0..=1.0,
                    2,
                    1.0
                ));
                if clear {
                    self.stack.look.params.path = None;
                    discrete!();
                }
                if pick {
                    let from = path
                        .as_deref()
                        .and_then(|p| std::path::Path::new(p).parent())
                        .map(std::path::PathBuf::from);
                    self.request(ui.ctx(), platform::Request::LookFile { from });
                }
            }
            InspectorSectionId::Heal => {
                use photo_engine::heal::{Mode, MAX_SPOTS};
                let n = self.stack.heal.params.spots.len();
                let ready = self.ready_image().is_some();
                let touch = widgets::is_touch(ui.ctx());
                let mut toggle_healing = false;
                widgets::bar_line(ui, 28.0, |ui| {
                    let tip = if self.healing {
                        "Stop healing (Esc)"
                    } else {
                        "Tap or click a spot on the photo to heal it"
                    };
                    if widgets::outlined_chip(ui, ready, self.healing, "heal spots")
                        .on_hover_text(tip)
                        .clicked()
                    {
                        toggle_healing = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::outlined_chip(ui, ready, self.dust_view, "dust view")
                            .on_hover_text(
                                "Show only the finest detail, so dust on the sensor stands out",
                            )
                            .clicked()
                        {
                            self.dust_view = !self.dust_view;
                        }
                    });
                });
                if toggle_healing {
                    self.healing = !self.healing;
                    if self.healing {
                        self.selected_mask = None;
                        if self.cropping {
                            self.cancel_crop();
                        }
                    } else {
                        self.selected_spot = None;
                    }
                }
                if self.healing {
                    widgets::param_slider(
                        ui,
                        "new spot size",
                        &mut self.spot_size,
                        0.3..=8.0,
                        1,
                        1.5,
                    );
                }

                if n == 0 {
                    let how = if touch {
                        "Turn on Heal spots, then tap a blemish. Each spot copies from a patch \
                         nearby, linked to it by a line: drag either circle to move it."
                    } else {
                        "Turn on Heal spots, then click a blemish. Each spot copies from a \
                         patch nearby, linked to it by a line: drag either circle to move it, \
                         with Shift held to place it to the pixel."
                    };
                    ui.label(theme::dim(how).size(11.5));
                    return;
                }

                // Every spot, by number: choosing one here is easier than finding a small
                // circle on the photo.
                ui.add_space(2.0);
                ui.label(theme::tag(format!("spots · {n} of {MAX_SPOTS}")).color(theme::TEXT_DIM));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for i in 0..n {
                        let chosen = self.selected_spot == Some(i);
                        if widgets::outlined_chip(ui, true, chosen, &format!("{}", i + 1))
                            .on_hover_text(if chosen {
                                "Deselect it"
                            } else {
                                "Choose it, to move or change it"
                            })
                            .clicked()
                        {
                            self.selected_spot = if chosen { None } else { Some(i) };
                            if !chosen {
                                // Its circles show only while healing.
                                self.healing = true;
                            }
                        }
                    }
                });

                let Some(i) = self.selected_spot.filter(|&i| i < n) else {
                    ui.label(theme::dim("Choose a spot to move or change it.").size(11.5));
                    return;
                };
                ui.add_space(2.0);
                let mut delete = false;
                widgets::bar_line(ui, 28.0, |ui| {
                    ui.label(theme::tag(format!("spot {}", i + 1)).color(theme::TEXT_DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::outlined_chip(ui, true, false, "delete")
                            .on_hover_text("Take this spot away")
                            .clicked()
                        {
                            delete = true;
                        }
                    });
                });
                let mode = self.stack.heal.params.spots[i].mode;
                let mut set_mode = None;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (m, label, tip) in [
                        (
                            Mode::Heal,
                            "heal",
                            "Cover it, matched to the tone around it",
                        ),
                        (
                            Mode::Clone,
                            "clone",
                            "Cover it with the source exactly as it is",
                        ),
                        (
                            Mode::Remove,
                            "remove",
                            "Fill it from the ground around it, with no source: for wires, \
                             signs and small objects. A large spot fills softer",
                        ),
                    ] {
                        if widgets::outlined_chip(ui, true, mode == m, label)
                            .on_hover_text(tip)
                            .clicked()
                            && mode != m
                        {
                            set_mode = Some(m);
                        }
                    }
                });
                let spot = &mut self.stack.heal.params.spots[i];
                let mut size = spot.radius * 100.0;
                let r = widgets::param_slider(ui, "size", &mut size, 0.1..=10.0, 1, 1.5);
                if r.changed() {
                    spot.radius = size / 100.0;
                }
                track!(r);
                let mut feather = spot.feather * 100.0;
                let r = widgets::param_slider(ui, "feather", &mut feather, 1.0..=100.0, 0, 40.0);
                if r.changed() {
                    spot.feather = feather / 100.0;
                }
                track!(r);
                if let Some(m) = set_mode {
                    self.stack.heal.params.spots[i].mode = m;
                    discrete!();
                }
                if delete {
                    self.stack.heal.params.spots.remove(i);
                    self.selected_spot = None;
                    discrete!();
                }
            }
            InspectorSectionId::Local => {
                use photo_engine::local::{Mask, RangeMask, Shape, MAX_MASKS};
                let n = self.stack.local.params.masks.len();
                let mut add: Option<Mask> = None;
                ui.label(theme::tag("add a mask").color(theme::TEXT_DIM));
                widgets::bar_line(ui, 28.0, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let room = n < MAX_MASKS && self.ready_image().is_some();
                    if widgets::outlined_chip(ui, room, false, "+ radial")
                        .on_hover_text("An ellipse: adjust inside it, or outside when inverted")
                        .clicked()
                    {
                        let c = self.crop_point_on_sensor(0.5, 0.5);
                        add = Some(Mask::radial(c, 0.15));
                    }
                    if widgets::outlined_chip(ui, room, false, "+ gradient")
                        .on_hover_text("A graduated filter: full at its start, fading to its end")
                        .clicked()
                    {
                        let (a, b) = (
                            self.crop_point_on_sensor(0.5, 0.0),
                            self.crop_point_on_sensor(0.5, 0.45),
                        );
                        add = Some(Mask::linear(a, b));
                    }
                    if widgets::outlined_chip(ui, room, false, "+ brush")
                        .on_hover_text("Paint where it applies")
                        .clicked()
                    {
                        add = Some(Mask::brush());
                    }
                    if n > 0 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::outlined_chip(ui, true, self.mask_overlay, "show")
                                .on_hover_text(
                                    "Wash the chosen mask red on the photo, to see where it \
                                     applies",
                                )
                                .clicked()
                            {
                                self.mask_overlay = !self.mask_overlay;
                            }
                        });
                    }
                });
                if let Some(mask) = add {
                    self.stack.local.params.masks.push(mask);
                    self.stack.local.enabled = true;
                    self.selected_mask = Some(n);
                    discrete!();
                }
                if self.stack.local.params.masks.is_empty() {
                    let how = if widgets::is_touch(ui.ctx()) {
                        "A mask changes one part of the photo. Radial: an oval, for a face. \
                         Gradient: a fade, for a sky. Brush: paint where it applies."
                    } else {
                        "A mask changes one part of the photo. Radial: an oval, for a face. \
                         Gradient: a fade, for a sky. Brush: paint where it applies. Drag its \
                         handles on the photo, with Shift held to place them to the pixel."
                    };
                    ui.label(theme::dim(how).size(11.5));
                }
                if n > 0 {
                    ui.add_space(2.0);
                    ui.label(
                        theme::tag(format!("masks · {n} of {MAX_MASKS}")).color(theme::TEXT_DIM),
                    );
                }
                let mut remove = None;
                for i in 0..self.stack.local.params.masks.len() {
                    let m = self.stack.local.params.masks[i].clone();
                    let chosen = self.selected_mask == Some(i);
                    widgets::bar_line(ui, 28.0, |ui| {
                        if widgets::outlined_chip(
                            ui,
                            true,
                            chosen,
                            &format!("{} {}", m.kind(), i + 1),
                        )
                        .on_hover_text(if chosen {
                            "Put its handles away (Esc)"
                        } else {
                            "Place it on the photo and adjust it"
                        })
                        .clicked()
                        {
                            self.selected_mask = if chosen { None } else { Some(i) };
                        }
                        let a = m.adjust;
                        let mut parts = Vec::new();
                        if a.exposure != 0.0 {
                            parts.push(format!("{:+.1} EV", a.exposure));
                        }
                        for (name, v) in [
                            ("temp", a.temperature),
                            ("tint", a.tint),
                            ("con", a.contrast),
                            ("sat", a.saturation),
                            ("clar", a.clarity),
                            ("sharp", a.sharpness),
                            ("noise", a.denoise),
                        ] {
                            if v != 0.0 {
                                parts.push(format!("{name} {v:+.0}"));
                            }
                        }
                        let text = if parts.is_empty() {
                            "no change yet".to_string()
                        } else {
                            parts.join(" · ")
                        };
                        ui.add(egui::Label::new(theme::dim(text).size(11.5)).truncate());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::icon(ui, "×", false, "Delete this mask").clicked() {
                                remove = Some(i);
                            }
                            let mut on = m.on;
                            // The panel's own checkbox, not egui's: one look with the
                            // module cards', centred on the row's midline.
                            if widgets::checkbox(ui, &mut on)
                                .on_hover_text("Apply this mask")
                                .changed()
                            {
                                self.stack.local.params.masks[i].on = on;
                                discrete!();
                            }
                        });
                    });
                }
                if let Some(i) = remove {
                    self.stack.local.params.masks.remove(i);
                    self.selected_mask = match self.selected_mask {
                        Some(s) if s == i => None,
                        Some(s) if s > i => Some(s - 1),
                        other => other,
                    };
                    discrete!();
                }
                if let Some(i) = self.selected_mask {
                    let m = &mut self.stack.local.params.masks[i];
                    ui.add_space(2.0);
                    ui.label(
                        theme::tag(format!("adjust {} {}", m.kind(), i + 1)).color(theme::TEXT_DIM),
                    );
                    let a = &mut m.adjust;
                    track!(widgets::param_slider(
                        ui,
                        "exposure",
                        &mut a.exposure,
                        -4.0..=4.0,
                        2,
                        0.0
                    ));
                    for (label, value, lo) in [
                        ("temperature", &mut a.temperature, -100.0),
                        ("tint", &mut a.tint, -100.0),
                        ("contrast", &mut a.contrast, -100.0),
                        ("saturation", &mut a.saturation, -100.0),
                        ("clarity", &mut a.clarity, -100.0),
                        ("sharpness", &mut a.sharpness, 0.0),
                        ("noise", &mut a.denoise, 0.0),
                    ] {
                        track!(widgets::param_slider(ui, label, value, lo..=100.0, 0, 0.0));
                    }
                    ui.add_space(2.0);
                    ui.label(theme::tag("shape").color(theme::TEXT_DIM));
                    if let Shape::Brush { dabs } = &m.shape {
                        let painted = dabs.len();
                        let brush = &mut self.brush;
                        widgets::param_slider(
                            ui,
                            "brush size",
                            &mut brush.size,
                            1.0..=30.0,
                            0,
                            6.0,
                        );
                        widgets::param_slider(
                            ui,
                            "brush feather",
                            &mut brush.feather,
                            0.0..=100.0,
                            0,
                            60.0,
                        );
                        widgets::bar_line(ui, 28.0, |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if widgets::outlined_chip(ui, true, self.brush.erase, "erase")
                                .on_hover_text("Paint the mask away (Alt or right-drag too)")
                                .clicked()
                            {
                                self.brush.erase = !self.brush.erase;
                            }
                            if widgets::outlined_chip(ui, painted > 0, false, "clear")
                                .on_hover_text("Take all the painting away")
                                .clicked()
                            {
                                if let Shape::Brush { dabs } =
                                    &mut self.stack.local.params.masks[i].shape
                                {
                                    dabs.clear();
                                }
                                discrete!();
                            }
                            if painted == 0 {
                                ui.label(theme::dim("Paint on the photo.").size(11.5));
                            }
                        });
                    }
                    let m = &mut self.stack.local.params.masks[i];
                    if let Shape::Radial { feather, .. } = &mut m.shape {
                        let mut pct = *feather * 100.0;
                        let r =
                            widgets::param_slider(ui, "feather", &mut pct, 1.0..=100.0, 0, 50.0);
                        if r.changed() {
                            *feather = pct / 100.0;
                        }
                        track!(r);
                    }
                    let inverted = m.invert;
                    if widgets::outlined_chip(ui, true, inverted, "invert")
                        .on_hover_text("Apply outside the shape instead of inside it")
                        .clicked()
                    {
                        m.invert = !inverted;
                        discrete!();
                    }

                    ui.add_space(2.0);
                    ui.label(theme::tag("only where it is…").color(theme::TEXT_DIM));
                    let cur_mode = match m.range {
                        RangeMask::None => 0,
                        RangeMask::Luminance { .. } => 1,
                        RangeMask::Color { .. } => 2,
                    };
                    let mut new_mode = cur_mode;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (mode, label, tip) in [
                            (0, "anywhere", "No range: the whole shape"),
                            (1, "bright/dark", "Only tones between two brightnesses"),
                            (2, "one colour", "Only near one hue"),
                        ] {
                            if widgets::outlined_chip(ui, true, cur_mode == mode, label)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                new_mode = mode;
                            }
                        }
                    });
                    if new_mode != cur_mode {
                        m.range = match new_mode {
                            0 => RangeMask::None,
                            1 => RangeMask::Luminance {
                                range: [0.0, 1.0],
                                feather: 0.1,
                            },
                            2 => RangeMask::Color {
                                hue: 0.0,
                                spread: 30.0,
                                feather: 15.0,
                            },
                            _ => RangeMask::None,
                        };
                        discrete!();
                    }
                    match &mut m.range {
                        RangeMask::None => {}
                        RangeMask::Luminance { range, feather } => {
                            let mut min_l = range[0] * 100.0;
                            let mut max_l = range[1] * 100.0;
                            let mut f = *feather * 100.0;
                            let r1 = widgets::param_slider(
                                ui,
                                "luma min",
                                &mut min_l,
                                0.0..=100.0,
                                0,
                                0.0,
                            );
                            let r2 = widgets::param_slider(
                                ui,
                                "luma max",
                                &mut max_l,
                                0.0..=100.0,
                                0,
                                100.0,
                            );
                            let r3 =
                                widgets::param_slider(ui, "feather", &mut f, 0.1..=100.0, 0, 10.0);
                            if r1.changed() || r2.changed() || r3.changed() {
                                range[0] = (min_l / 100.0).min(max_l / 100.0);
                                range[1] = (max_l / 100.0).max(min_l / 100.0);
                                *feather = f / 100.0;
                                discrete!();
                            }
                        }
                        RangeMask::Color {
                            hue,
                            spread,
                            feather,
                        } => {
                            let r1 =
                                widgets::param_slider(ui, "target hue", hue, 0.0..=360.0, 0, 0.0);
                            let r2 = widgets::param_slider(
                                ui,
                                "hue spread",
                                spread,
                                1.0..=180.0,
                                0,
                                30.0,
                            );
                            let r3 =
                                widgets::param_slider(ui, "feather", feather, 0.1..=90.0, 0, 15.0);
                            if r1.changed() || r2.changed() || r3.changed() {
                                discrete!();
                            }
                        }
                    }
                } else if n > 0 {
                    ui.label(theme::dim("Choose a mask to place and adjust it.").size(11.5));
                }
            }
            InspectorSectionId::Vignette => {
                let p = &mut self.stack.vignette.params;
                let dp = d.vignette.params;
                track!(widgets::param_slider(
                    ui,
                    "amount",
                    &mut p.amount,
                    -1.0..=1.0,
                    2,
                    dp.amount
                ));
                track!(widgets::param_slider(
                    ui,
                    "radius",
                    &mut p.radius,
                    0.1..=1.5,
                    2,
                    dp.radius
                ));
            }
        }
    }
}
