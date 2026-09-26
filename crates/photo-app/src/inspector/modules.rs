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
        let d = PhotoStack::default();

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
                            if widgets::chip(ui, false, "reset crop").clicked() {
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
                ui.horizontal(|ui| {
                    let pad = widgets::sized(ui, 3.0, 6.0);
                    ui.spacing_mut().item_spacing = vec2(pad, pad);
                    self.crop_chips(ui);
                });
                track!(widgets::param_slider(
                    ui,
                    "straighten",
                    &mut self.stack.geometry.straighten_deg,
                    -15.0..=15.0,
                    2,
                    d.geometry.straighten_deg,
                ));
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

                // Presets as chips; the one matching the current setting is lit. Adobe's
                // standard lights, so a preset means the same here as in Lightroom.
                let mut preset = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                    let as_shot = (p.tint_r - 1.0).abs() < 1e-4 && (p.tint_b - 1.0).abs() < 1e-4;
                    if widgets::chip(ui, as_shot, "as shot")
                        .on_hover_text(format!("What the camera metered: {home_k:.0} K"))
                        .clicked()
                    {
                        preset = Some(dp);
                    }
                    for (label, tip, pk, pt) in [
                        ("daylight", "5500 K, tint +10", 5500.0, 10.0),
                        ("cloudy", "6500 K, tint +10", 6500.0, 10.0),
                        ("shade", "7500 K, tint +10", 7500.0, 10.0),
                        ("tungsten", "2850 K incandescent", 2850.0, 0.0),
                        ("fluoro", "3800 K, tint +21", 3800.0, 21.0),
                        ("flash", "5500 K", 5500.0, 0.0),
                    ] {
                        let lit = !as_shot && (kelvin - pk).abs() < 5.0 && (tint - pt).abs() < 0.5;
                        let wb = scale.white_balance(pk, pt);
                        let chip =
                            widgets::chip_enabled(ui, wb.is_some(), lit, label).on_hover_text(tip);
                        if chip.clicked() {
                            preset = wb;
                        }
                    }
                });
                if let Some(wb) = preset {
                    *p = wb;
                    discrete!();
                }
            }
            InspectorSectionId::Exposure => {
                let p = &mut self.stack.exposure.params;
                let dp = d.exposure.params;

                ui.horizontal(|ui| {
                    ui.label(theme::tag("mode").color(theme::TEXT_DIM));
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
                        if widgets::chip(ui, sel, label).on_hover_text(tip).clicked() {
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

                ui.add_space(4.0);

                let max_ev = if p.extended_range { 8.0 } else { 4.0 };
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

                ui.horizontal(|ui| {
                    if widgets::chip(
                        ui,
                        p.extended_range,
                        if p.extended_range {
                            "±8 EV (Extended)"
                        } else {
                            "±4 EV (Standard)"
                        },
                    )
                    .on_hover_text(
                        "Toggle between standard ±4 EV range and extended ±8 EV extreme range",
                    )
                    .clicked()
                    {
                        p.extended_range = !p.extended_range;
                        discrete!();
                    }
                });

                // A picture of the shoulder, where there is room for one: on a phone the
                // mode chips say the same in a line.
                if !widgets::is_touch(ui.ctx()) {
                    ui.add_space(4.0);
                    widgets::mini_knee_curve(ui, p.ev, p.rolloff);
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
                track!(curve_editor(ui, &mut self.stack.tone_curve.params));
                let is_linear = self.stack.tone_curve.params == identity_nodes();
                ui.horizontal(|ui| {
                    let hint = if widgets::is_touch(ui.ctx()) {
                        "tap to add · drag · double-tap to remove"
                    } else {
                        "click to add · drag · double-click to remove"
                    };
                    ui.label(theme::tag(hint).color(theme::TEXT_FAINT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_enabled_ui(!is_linear, |ui| {
                            if widgets::chip(ui, false, "reset").clicked() {
                                self.stack.tone_curve.params = identity_nodes();
                                self.stack.curve_domain = photo_engine::CurveDomain::Display;
                                discrete!();
                            }
                        });
                    });
                });
                if self.stack.curve_domain == photo_engine::CurveDomain::Linear {
                    // An edit from before curves moved to display values: it keeps its
                    // look, on axes that do not match the histogram under them.
                    ui.horizontal(|ui| {
                        ui.label(theme::tag("older curve · linear axes").color(theme::TEXT_DIM))
                            .on_hover_text(
                                "Made before the curve's axes matched the histogram. It looks as \
                             it always did; update it to edit on display values.",
                            );
                        if widgets::chip(ui, false, "update").clicked() {
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
                        if widgets::chip(ui, sel, label).clicked() {
                            self.color_eq_tab = tab;
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
                            if widgets::chip(ui, active, name).clicked() {
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
                            if widgets::chip(ui, active, label).clicked() {
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
