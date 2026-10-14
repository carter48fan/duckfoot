//! Time a batch export: every photo in a folder, through the app's own batch queue, as
//! full-size JPEGs into an output folder. `cargo run --release -p photo-app --example
//! batch_bench -- <photos dir> <output dir>`.

use photo_app::batch::queue::{ExportJob, ExportQueue, ExportStatus};
use photo_app::batch::recipe::ExportRecipe;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let photos = PathBuf::from(args.next().expect("a folder of photos"));
    let out = PathBuf::from(args.next().expect("an output folder"));
    std::fs::create_dir_all(&out).unwrap();

    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(
        &instance, None,
    ))
    .expect("a GPU");
    eprintln!("gpu: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .unwrap();

    let mut files: Vec<PathBuf> = std::fs::read_dir(&photos)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("arw") || e.eq_ignore_ascii_case("dng"))
        })
        .collect();
    files.sort();
    let recipe = ExportRecipe::default_recipes()
        .into_iter()
        .find(|r| r.settings.format == photo_engine::ExportFormat::Jpeg)
        .expect("a JPEG recipe");
    let jobs: Vec<ExportJob> = files
        .iter()
        .map(|p| {
            let stack = photo_engine::load_sidecar(p)
                .ok()
                .flatten()
                .unwrap_or_default();
            ExportJob {
                source_path: p.clone(),
                crop: stack.geometry.crop,
                stack,
                output_path: out.join(p.file_stem().unwrap()).with_extension("jpg"),
                recipe: ExportRecipe {
                    settings: photo_engine::ExportSettings {
                        sizing: photo_engine::Resize::Original,
                        ..recipe.settings
                    },
                    ..recipe.clone()
                },
            }
        })
        .collect();
    let n = jobs.len();
    let mut q = ExportQueue::new(device, queue);
    let start = Instant::now();
    q.start_batch(jobs);
    loop {
        if let ExportStatus::Finished {
            success, errors, ..
        } = q.poll().clone()
        {
            let s = start.elapsed().as_secs_f32();
            println!("{success}/{n} in {s:.2}s ({:.2}s each)", s / n as f32);
            for (p, e) in errors {
                println!("  {}: {e}", p.display());
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
