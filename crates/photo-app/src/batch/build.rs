//! Turning a batch's photos and recipes into jobs.
//!
//! Each photo not open in the editor needs its sidecar read, its EXIF read for the name
//! pattern, and every output name checked against the disk: a stat or three per photo
//! per recipe. For a few thousand photos on a card or a network share that is seconds,
//! so it runs on the export queue's thread, not the UI's.

use std::collections::HashSet;
use std::path::PathBuf;

use photo_engine::PhotoStack;

use super::{ExportJob, ExportRecipe, RenameContext, Renamer};

/// What the UI hands over to build a batch: the photos in the order they are numbered,
/// the photo open in the editor (whose stack and rating are in memory, newer than its
/// sidecar), and how outputs are named and where they go.
pub struct BuildRequest {
    pub paths: Vec<PathBuf>,
    /// The open photo's path, its stack and its rating.
    pub open: Option<(PathBuf, PhotoStack, u8)>,
    pub recipes: Vec<ExportRecipe>,
    pub renamer: Renamer,
    pub out_dir: PathBuf,
}

pub fn build_jobs(request: BuildRequest) -> Vec<ExportJob> {
    let BuildRequest {
        paths,
        open,
        recipes,
        renamer,
        out_dir,
    } = request;
    let mut jobs = Vec::new();
    let mut claimed = HashSet::new();

    for (seq_idx, src_path) in paths.iter().enumerate() {
        let file = photo_engine::copies::source_file(src_path);
        let info = photo_engine::read_info(&file);
        let date = info
            .as_ref()
            .and_then(|i| i.capture_date_compact())
            .or_else(|| photo_engine::file_date_compact(&file));
        let (stack, rating) = match &open {
            Some((path, stack, rating)) if path == src_path => (stack.clone(), *rating),
            _ => {
                let (stack, meta) = match photo_engine::load_sidecar_full(src_path) {
                    Ok(Some(sc)) => (sc.stack, sc.metadata),
                    _ => (None, photo_engine::AssetMetadata::default()),
                };
                // Never opened, or only rated: exported as shot, which includes the way
                // the camera was held.
                let stack = stack.unwrap_or_else(|| PhotoStack {
                    geometry: photo_engine::file_orientation(&file),
                    ..PhotoStack::for_path(src_path)
                });
                (stack, meta.rating)
            }
        };
        let crop = stack.geometry.crop;

        for recipe in &recipes {
            let target_dir = match &recipe.subfolder {
                Some(sub) => out_dir.join(super::renamer::sanitize_component(sub)),
                None => out_dir.clone(),
            };
            let ctx = RenameContext {
                source_path: src_path,
                sequence_index: seq_idx,
                camera_model: info.as_ref().map(|i| i.model.as_str()),
                rating,
                custom_text: "",
                target_extension: recipe.settings.format.extension(),
                date: date.as_deref(),
            };
            if let Some(target_path) = renamer.resolve_target_path(&target_dir, &ctx, &mut claimed)
            {
                jobs.push(ExportJob {
                    source_path: src_path.clone(),
                    stack: stack.clone(),
                    crop,
                    output_path: target_path,
                    recipe: recipe.clone(),
                    touchups: Vec::new(),
                });
            }
        }
    }
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each photo gets a job per recipe, numbered in the order given; the open photo is
    /// exported with its stack as it is in the editor, not as its sidecar last said.
    #[test]
    fn a_batch_is_built_from_the_editor_and_the_sidecars() {
        let dir = std::env::temp_dir().join(format!("duckfoot-build-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.png"), dir.join("b.png"));
        for p in [&a, &b] {
            image::RgbImage::new(8, 8).save(p).unwrap();
        }
        let mut open = PhotoStack::default();
        open.exposure.params.ev = 1.5;
        let recipes: Vec<ExportRecipe> = ExportRecipe::default_recipes()
            .into_iter()
            .take(2)
            .collect();
        let jobs = build_jobs(BuildRequest {
            paths: vec![b.clone(), a.clone()],
            open: Some((a.clone(), open.clone(), 4)),
            recipes: recipes.clone(),
            renamer: Renamer::default(),
            out_dir: dir.join("out"),
        });
        assert_eq!(jobs.len(), 4);
        assert_eq!(jobs[0].source_path, b);
        assert_eq!(jobs[3].source_path, a);
        assert_eq!(jobs[3].stack.exposure.params.ev, 1.5);
        assert_ne!(jobs[0].stack.exposure.params.ev, 1.5);
        let outputs: HashSet<_> = jobs.iter().map(|j| j.output_path.clone()).collect();
        assert_eq!(outputs.len(), 4, "every output has a name of its own");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
