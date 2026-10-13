//! The one table of the pipeline's modules.
//!
//! Every per-module fact the UI needs — its field in the stack, its names, how it is
//! switched, reset and described in History — comes from here. There used to be seven
//! hand-written tables of these, and they had already drifted: three modules showed as
//! "no change" in History. Adding a module is now one row.

use crate::*;

macro_rules! module_table {
    ($( $id:ident => $field:ident; )*) => {
        /// The modules that are a `Module<T>` in the stack, in pipeline order.
        pub(crate) const PIPELINE_MODULES: &[InspectorSectionId] =
            &[$(InspectorSectionId::$id),*];

        /// Each pipeline module's field name in `PhotoStack`, as serialised.
        #[cfg(test)]
        pub(crate) const PIPELINE_FIELDS: &[&str] = &[$(stringify!($field)),*];

        impl InspectorSectionId {
            /// The name on its card (`module-<field>` in `locales/`).
            pub fn title(&self) -> &'static str {
                crate::i18n::tr(match self {
                    Self::Scope => "module-scope",
                    Self::Geometry => "module-geometry",
                    $(Self::$id => concat!("module-", stringify!($field)),)*
                })
            }

            /// The name on the phone's module strip, where every letter costs width.
            pub fn short_name(&self) -> &'static str {
                crate::i18n::tr(match self {
                    Self::Scope => "module-short-scope",
                    Self::Geometry => "module-short-geometry",
                    $(Self::$id => concat!("module-short-", stringify!($field)),)*
                })
            }

            /// Its on/off switch, if it has one. Scope and geometry do not: there is no
            /// meaningful "off" for either.
            pub(crate) fn enabled_mut(self, stack: &mut PhotoStack) -> Option<&mut bool> {
                match self {
                    Self::Scope | Self::Geometry => None,
                    $(Self::$id => Some(&mut stack.$field.enabled),)*
                }
            }

            /// Whether it is contributing to the image.
            pub(crate) fn is_on(self, stack: &PhotoStack) -> bool {
                #[allow(unreachable_patterns)]
                match self {
                    Self::Scope | Self::Geometry => true,
                    Self::ToneCurve => stack.tone_curve.enabled || stack.rgb_curves.enabled,
                    $(Self::$id => stack.$field.enabled,)*
                }
            }

            /// Put it back as it is in `baseline`.
            pub(crate) fn reset(self, stack: &mut PhotoStack, baseline: &PhotoStack) {
                #[allow(unreachable_patterns)]
                match self {
                    Self::Scope => {}
                    Self::Geometry => stack.geometry = baseline.geometry,
                    Self::ToneCurve => {
                        stack.tone_curve = baseline.tone_curve.clone();
                        stack.rgb_curves = baseline.rgb_curves.clone();
                        stack.curve_domain = baseline.curve_domain;
                    }
                    $(Self::$id => stack.$field = baseline.$field.clone(),)*
                }
            }

            /// How History names a step that changed this module from `a` to `b`, or
            /// `None` if it did not change.
            pub(crate) fn describe(self, a: &PhotoStack, b: &PhotoStack) -> Option<String> {
                #[allow(unreachable_patterns)]
                let (was, is, params_changed) = match self {
                    Self::Scope | Self::Geometry => return None,
                    Self::ToneCurve => (
                        a.tone_curve.enabled || a.rgb_curves.enabled,
                        b.tone_curve.enabled || b.rgb_curves.enabled,
                        a.tone_curve.params != b.tone_curve.params
                            || a.rgb_curves.params != b.rgb_curves.params
                            || a.curve_domain != b.curve_domain,
                    ),
                    $(Self::$id => (
                        a.$field.enabled,
                        b.$field.enabled,
                        a.$field.params != b.$field.params,
                    ),)*
                };
                if was != is {
                    Some(format!("{} {}", self.title(), if is { "on" } else { "off" }))
                } else if params_changed {
                    Some(self.title().to_string())
                } else {
                    None
                }
            }
        }
    };
}

module_table! {
    Lens => lens;
    Denoise => denoise;
    WhiteBalance => white_balance;
    Exposure => exposure;
    Tonal => tonal;
    ToneCurve => tone_curve;
    Clarity => clarity;
    Dehaze => dehaze;
    ColorEq => color_eq;
    Filmic3D => filmic;
    ColorBalance => color_balance;
    Vibrance => vibrance;
    Sharpen => sharpen;
    Vignette => vignette;
    Local => local;
    Heal => heal;
    Look => look;
}

/// What changed between two adjacent undo states, for the History list.
pub(crate) fn describe_change(a: &PhotoStack, b: &PhotoStack) -> String {
    let mut names: Vec<String> = Vec::new();
    let (ga, gb) = (&a.geometry, &b.geometry);
    if ga.quarter_turns != gb.quarter_turns {
        names.push("rotate".into());
    }
    if ga.mirror_h != gb.mirror_h || ga.mirror_v != gb.mirror_v {
        names.push("flip".into());
    }
    if ga.straighten_deg != gb.straighten_deg {
        names.push("straighten".into());
    }
    if ga.perspective_v != gb.perspective_v || ga.perspective_h != gb.perspective_h {
        names.push("keystone".into());
    }
    if ga.crop != gb.crop {
        names.push("crop".into());
    }
    if a.working_space != b.working_space {
        names.push("colour space".into());
    }
    names.extend(PIPELINE_MODULES.iter().filter_map(|id| id.describe(a, b)));

    match names.len() {
        0 => "no change".to_string(),
        1 => names.remove(0),
        2 => names.join(" + "),
        n => format!("{n} modules"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every field of the stack is in the table (or is geometry, or rides with the tone
    /// curve). A module added to the engine without a row here fails this, rather than
    /// quietly missing from History, reset and the switches.
    #[test]
    fn every_stack_field_has_a_row() {
        let json = serde_json::to_value(PhotoStack::default()).unwrap();
        for key in json.as_object().unwrap().keys() {
            let covered = PIPELINE_FIELDS.contains(&key.as_str())
                || key == "geometry"
                || key == "curve_domain"
                || key == "rgb_curves"
                // Not modules: the space every module works in (W-09), and the version
                // of the colour math an edit is drawn with.
                || key == "working_space"
                || key == "rendering";
            assert!(covered, "PhotoStack::{key} has no row in the module table");
        }
        assert_eq!(PIPELINE_FIELDS.len(), PIPELINE_MODULES.len());
    }

    /// History names a change to every module, which is what went missing before.
    #[test]
    fn history_names_a_change_to_any_module() {
        let base = PhotoStack::default();
        for &id in PIPELINE_MODULES {
            let mut on = base.clone();
            *id.enabled_mut(&mut on).unwrap() ^= true;
            let described = describe_change(&base, &on);
            assert!(
                described.starts_with(id.title()),
                "{id:?}: switching it read as {described:?}"
            );
            let mut reset = on.clone();
            id.reset(&mut reset, &base);
            assert_eq!(reset, base, "{id:?}: reset did not restore it");
        }
    }
}
