//! Tokenized batch renaming engine.
//!
//! Supports flexible pattern strings like `{date}_{camera}_{seq:3}`,
//! regex or literal find/replace, and collision resolution policies.

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
    pub date_override: Option<&'a str>, // YYYYMMDD or None for current/source
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

impl Renamer {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            ..Default::default()
        }
    }

    /// Evaluates the pattern template against the given context and returns the destination filename.
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
                    let width = digits_str.parse::<usize>().unwrap_or(3);
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
        let date_str = ctx.date_override.unwrap_or("20260924");
        out = out.replace("{date}", date_str);
        if date_str.len() >= 8 {
            out = out.replace("{year}", &date_str[0..4]);
            out = out.replace("{month}", &date_str[4..6]);
            out = out.replace("{day}", &date_str[6..8]);
        }

        // 5. {camera}
        let camera_clean = ctx
            .camera_model
            .unwrap_or("Camera")
            .replace(' ', "_")
            .replace('/', "-");
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

        out
    }

    /// Resolves target path and checks for collisions based on policy.
    pub fn resolve_target_path(&self, output_dir: &Path, ctx: &RenameContext) -> Option<PathBuf> {
        let base_name = self.format_filename(ctx);
        let target = output_dir.join(&base_name);

        if !target.exists() {
            return Some(target);
        }

        match self.collision_policy {
            CollisionPolicy::Overwrite => Some(target),
            CollisionPolicy::Skip => None,
            CollisionPolicy::AutoIncrementSuffix => {
                let stem = target
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("file");
                let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("");

                let mut counter = 1;
                loop {
                    let candidate_name = if ext.is_empty() {
                        format!("{stem}_{counter}")
                    } else {
                        format!("{stem}_{counter}.{ext}")
                    };
                    let candidate = output_dir.join(&candidate_name);
                    if !candidate.exists() {
                        return Some(candidate);
                    }
                    counter += 1;
                }
            }
        }
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
            date_override: Some("20260924"),
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
            date_override: Some("20260924"),
        };

        let result = renamer.format_filename(&ctx);
        assert_eq!(result, "20260924_Canon_EOS_R5_0010_4star.png");
    }
}
