//! Tokenized batch renaming engine.
//!
//! Supports flexible pattern strings like `{date}_{camera}_{seq:3}`,
//! regex or literal find/replace, and collision resolution policies.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionPolicy {
    AutoIncrementSuffix, // DSC08450_1.jpg
    Skip,
    Overwrite,
}

#[derive(Debug, Clone)]
pub struct RenameContext<'a> {
    pub source_path: &'a Path,
    pub sequence_index: usize,
    pub camera_model: Option<&'a str>,
    pub rating: u8,
    pub custom_text: &'a str,
    pub target_extension: &'a str,
    /// The capture date, `YYYYMMDD`: from the photo's metadata, else the file's own date.
    pub date: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct Renamer {
    pub pattern: String,
    pub start_index: usize,
    pub collision_policy: CollisionPolicy,
}

impl Default for Renamer {
    fn default() -> Self {
        Self {
            pattern: "{filename}_{seq:3}".to_string(),
            start_index: 1,
            collision_policy: CollisionPolicy::AutoIncrementSuffix,
        }
    }
}

/// Widest `{seq:N}` honoured. Beyond this a typo would allocate a gigabyte of zeros.
const MAX_SEQ_WIDTH: usize = 9;

/// One path component made safe to create on every platform: no separators, nothing a
/// Windows or Android filesystem refuses, no `..`, no Windows device names.
pub fn sanitize_component(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    while out.contains("..") {
        out = out.replace("..", ".");
    }
    let out = out.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    match (out.is_empty(), reserved) {
        (true, _) => "untitled".to_string(),
        (false, true) => format!("_{out}"),
        (false, false) => out.to_string(),
    }
}

impl Renamer {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            ..Default::default()
        }
    }

    /// Evaluates the pattern template against the given context and returns the
    /// destination filename: one component, never a path, whatever the pattern and the
    /// photo's metadata contain.
    pub fn format_filename(&self, ctx: &RenameContext) -> String {
        let stem = ctx
            .source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled");

        let ext = if ctx.target_extension.is_empty() {
            ctx.source_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
        } else {
            ctx.target_extension.trim_start_matches('.')
        };

        let seq_num = self.start_index + ctx.sequence_index;

        let mut out = self.pattern.clone();

        // 1. {filename} and {name}
        out = out.replace("{filename}", stem);
        out = out.replace("{name}", stem);

        // 2. {ext}
        out = out.replace("{ext}", ext);

        // 3. {seq} and {seq:N} (zero-padded)
        while let Some(start) = out.find("{seq") {
            if let Some(end) = out[start..].find('}') {
                let token = &out[start..start + end + 1];
                let formatted_seq = if token == "{seq}" {
                    seq_num.to_string()
                } else if token.starts_with("{seq:") {
                    let digits_str = &token[5..token.len() - 1];
                    let width = digits_str.parse::<usize>().unwrap_or(3).min(MAX_SEQ_WIDTH);
                    format!("{:0width$}", seq_num, width = width)
                } else {
                    seq_num.to_string()
                };
                out.replace_range(start..start + end + 1, &formatted_seq);
            } else {
                break;
            }
        }

        // 4. {date}, {year}, {month}, {day}
        let date_str = ctx.date.unwrap_or("undated");
        out = out.replace("{date}", date_str);
        let dated = date_str.len() == 8 && date_str.bytes().all(|b| b.is_ascii_digit());
        out = out.replace("{year}", if dated { &date_str[0..4] } else { "undated" });
        out = out.replace("{month}", if dated { &date_str[4..6] } else { "undated" });
        out = out.replace("{day}", if dated { &date_str[6..8] } else { "undated" });

        // 5. {camera}
        let camera_clean = ctx
            .camera_model
            .filter(|c| !c.trim().is_empty())
            .unwrap_or("unknown-camera")
            .trim()
            .replace(' ', "_");
        out = out.replace("{camera}", &camera_clean);

        // 6. {rating}
        let rating_str = if ctx.rating > 0 {
            format!("{}star", ctx.rating)
        } else {
            "unrated".to_string()
        };
        out = out.replace("{rating}", &rating_str);

        // 7. {custom}
        out = out.replace("{custom}", ctx.custom_text);

        // If the pattern didn't explicitly include an extension, append it
        if !ext.is_empty() && !out.ends_with(&format!(".{ext}")) {
            out.push('.');
            out.push_str(ext);
        }

        sanitize_component(&out)
    }

    /// Where this photo's export goes. Collisions are checked against the disk *and*
    /// against `claimed` — the names earlier jobs in the same batch will write — so two
    /// sources that format to one name never overwrite each other, whatever the policy.
    pub fn resolve_target_path(
        &self,
        output_dir: &Path,
        ctx: &RenameContext,
        claimed: &mut HashSet<PathBuf>,
    ) -> Option<PathBuf> {
        let target = output_dir.join(self.format_filename(ctx));
        let taken = |p: &Path, claimed: &HashSet<PathBuf>| {
            claimed.contains(p)
                || (p.exists() && self.collision_policy != CollisionPolicy::Overwrite)
        };

        let chosen = if !taken(&target, claimed) {
            target
        } else if self.collision_policy == CollisionPolicy::Skip && !claimed.contains(&target) {
            return None;
        } else {
            let stem = target
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("file")
                .to_string();
            let ext = target
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();
            (1..)
                .map(|counter| {
                    output_dir.join(if ext.is_empty() {
                        format!("{stem}_{counter}")
                    } else {
                        format!("{stem}_{counter}.{ext}")
                    })
                })
                .find(|candidate| !taken(candidate, claimed))?
        };
        claimed.insert(chosen.clone());
        Some(chosen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_renamer_basic_pattern() {
        let renamer = Renamer {
            pattern: "{filename}_{seq:3}".to_string(),
            start_index: 1,
            collision_policy: CollisionPolicy::AutoIncrementSuffix,
        };

        let src = Path::new("/photos/DSC08450.ARW");
        let ctx = RenameContext {
            source_path: src,
            sequence_index: 0,
            camera_model: Some("ILCE-7M3"),
            rating: 5,
            custom_text: "ClientA",
            target_extension: "jpg",
            date: Some("20260924"),
        };

        let result = renamer.format_filename(&ctx);
        assert_eq!(result, "DSC08450_001.jpg");

        let ctx2 = RenameContext {
            sequence_index: 41,
            ..ctx
        };
        assert_eq!(renamer.format_filename(&ctx2), "DSC08450_042.jpg");
    }

    #[test]
    fn batch_renamer_date_and_camera() {
        let renamer = Renamer::new("{date}_{camera}_{seq:4}_{rating}");
        let src = Path::new("/photos/IMG_1020.CR3");
        let ctx = RenameContext {
            source_path: src,
            sequence_index: 9,
            camera_model: Some("Canon EOS R5"),
            rating: 4,
            custom_text: "",
            target_extension: "png",
            date: Some("20260924"),
        };

        let result = renamer.format_filename(&ctx);
        assert_eq!(result, "20260924_Canon_EOS_R5_0010_4star.png");
    }

    fn ctx(source: &Path) -> RenameContext<'_> {
        RenameContext {
            source_path: source,
            sequence_index: 0,
            camera_model: None,
            rating: 0,
            custom_text: "",
            target_extension: "jpg",
            date: None,
        }
    }

    #[test]
    fn two_sources_with_one_name_never_overwrite_each_other() {
        let dir = std::env::temp_dir().join(format!("duckfoot-rename-{}", std::process::id()));
        let renamer = Renamer::new("{filename}");
        let mut claimed = HashSet::new();
        let (a, b) = (Path::new("/one/DSC1.ARW"), Path::new("/two/DSC1.ARW"));
        let first = renamer
            .resolve_target_path(&dir, &ctx(a), &mut claimed)
            .unwrap();
        let second = renamer
            .resolve_target_path(&dir, &ctx(b), &mut claimed)
            .unwrap();
        assert_eq!(first, dir.join("DSC1.jpg"));
        assert_eq!(second, dir.join("DSC1_1.jpg"));
        // Even told to overwrite, a batch does not overwrite its own output.
        let overwrite = Renamer {
            collision_policy: CollisionPolicy::Overwrite,
            ..Renamer::new("{filename}")
        };
        let mut claimed = HashSet::new();
        let first = overwrite
            .resolve_target_path(&dir, &ctx(a), &mut claimed)
            .unwrap();
        let second = overwrite
            .resolve_target_path(&dir, &ctx(b), &mut claimed)
            .unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn names_stay_one_safe_component() {
        let renamer = Renamer::new("{camera}_{custom}");
        let src = Path::new("/p/a.ARW");
        let c = RenameContext {
            camera_model: Some("../../etc\\passwd:x"),
            custom_text: "a/b",
            ..ctx(src)
        };
        let name = renamer.format_filename(&c);
        assert!(
            !name.contains('/') && !name.contains('\\') && !name.contains(".."),
            "{name}"
        );
        assert_eq!(sanitize_component("CON.jpg"), "_CON.jpg");
        assert_eq!(sanitize_component(" .. "), "untitled");
        assert_eq!(
            Renamer::new("{seq:999999999}")
                .format_filename(&ctx(src))
                .len(),
            "000000001.jpg".len()
        );
    }

    #[test]
    fn an_undated_photo_says_so_rather_than_inventing_a_date() {
        let name = Renamer::new("{date}_{camera}").format_filename(&ctx(Path::new("/p/a.ARW")));
        assert_eq!(name, "undated_unknown-camera.jpg");
    }
}
