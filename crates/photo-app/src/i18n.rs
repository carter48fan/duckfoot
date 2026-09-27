//! Interface text by message id (W-13), from Fluent files under `locales/`. English is
//! the source and the only language so far; the scaffolding is here so that strings move
//! out of the code as they are touched, rather than all at once, late. A language other
//! than English falls back to English message by message.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Every language there is text for: its tag and its file.
const LOCALES: &[(&str, &str)] = &[("en-US", include_str!("../locales/en-US/duckfoot.ftl"))];

type Bundle = FluentBundle<FluentResource>;

fn bundle(tag: &str, ftl: &str) -> Bundle {
    let lang = tag.parse().unwrap_or_default();
    let mut bundle = FluentBundle::new_concurrent(vec![lang]);
    // Bidi isolation marks around values are for right-to-left text beside left-to-right;
    // English needs none, and egui would draw them as boxes.
    bundle.set_use_isolating(false);
    let resource = FluentResource::try_new(ftl.to_string()).unwrap_or_else(|(res, errors)| {
        log::error!("{tag} text has {} errors: {errors:?}", errors.len());
        res
    });
    if let Err(errors) = bundle.add_resource(resource) {
        log::error!("{tag} text repeats messages: {errors:?}");
    }
    bundle
}

/// The language asked for (`DUCKFOOT_LANG`, else the system's `LANG`), then English.
fn bundles() -> &'static [Bundle] {
    static BUNDLES: OnceLock<Vec<Bundle>> = OnceLock::new();
    BUNDLES.get_or_init(|| {
        let wanted = std::env::var("DUCKFOOT_LANG")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .replace('_', "-");
        let language = |tag: &str| tag.split('-').next().unwrap_or_default().to_lowercase();
        let mut out = Vec::new();
        let wanted = language(&wanted);
        if let Some((tag, ftl)) = LOCALES[1..].iter().find(|(t, _)| language(t) == wanted) {
            out.push(bundle(tag, ftl));
        }
        out.push(bundle(LOCALES[0].0, LOCALES[0].1));
        out
    })
}

fn format(id: &str, args: Option<&FluentArgs>) -> Option<String> {
    for bundle in bundles() {
        let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) else {
            continue;
        };
        let mut errors = Vec::new();
        let text = bundle.format_pattern(pattern, args, &mut errors);
        if !errors.is_empty() {
            log::warn!("text {id}: {errors:?}");
        }
        return Some(text.into_owned());
    }
    None
}

/// The text of message `id`. Kept for the life of the app, so it can stand where a
/// `&'static str` is wanted; a missing id shows as itself, so it is noticed.
pub fn tr(id: &str) -> &'static str {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(text) = cache.get(id) {
        return text;
    }
    let text: &'static str = Box::leak(
        format(id, None)
            .unwrap_or_else(|| id.to_string())
            .into_boxed_str(),
    );
    cache.insert(id.to_string(), text);
    text
}

/// The text of message `id` with its `{ $name }` values filled in.
pub fn tr_args<'a>(id: &str, args: &[(&str, FluentValue<'a>)]) -> String {
    let mut fluent = FluentArgs::new();
    for (k, v) in args {
        fluent.set(*k, v.clone());
    }
    format(id, Some(&fluent)).unwrap_or_else(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every id the code names, read from the sources: `tr("id")`, `tr_args("id"`, and
    /// the ids the module table and the tour build.
    fn ids_in_code() -> Vec<String> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut ids = Vec::new();
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(d).unwrap().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                // This file's own examples are not the interface's.
                if p.extension().is_none_or(|e| e != "rs") || p.ends_with("i18n.rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&p).unwrap();
                for opener in ["tr(\"", "tr_args(\"", "title_id: \"", "text_id: \""] {
                    for (i, _) in src.match_indices(opener) {
                        let rest = &src[i + opener.len()..];
                        if let Some(end) = rest.find('"') {
                            ids.push(rest[..end].to_string());
                        }
                    }
                }
            }
        }
        for field in crate::modules::PIPELINE_FIELDS
            .iter()
            .chain(&["scope", "geometry"])
        {
            ids.push(format!("module-{field}"));
            ids.push(format!("module-short-{field}"));
        }
        ids
    }

    #[test]
    fn every_id_the_code_names_is_in_english() {
        let english = bundle(LOCALES[0].0, LOCALES[0].1);
        let missing: Vec<String> = ids_in_code()
            .into_iter()
            .filter(|id| english.get_message(id).and_then(|m| m.value()).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "missing from en-US/duckfoot.ftl: {missing:?}"
        );
    }

    #[test]
    fn every_file_parses_and_fills_its_values() {
        for (tag, ftl) in LOCALES {
            assert!(
                FluentResource::try_new(ftl.to_string()).is_ok(),
                "{tag} does not parse"
            );
        }
        assert_eq!(tr("tab-edit"), "edit");
        assert_eq!(
            tr_args(
                "exported",
                &[("name", "a.jpg".into()), ("seconds", "1.5".into())]
            ),
            "Exported a.jpg in 1.5s"
        );
        assert_eq!(tr("no-such-message"), "no-such-message");
    }
}
