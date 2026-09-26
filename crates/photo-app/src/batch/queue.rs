//! Background headless export queue.
//!
//! Processes multi-export recipes and batch rendering off the UI thread
//! using an independent Engine instance on cloned wgpu device/queue handles.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use photo_engine::{decode_raster, decode_raw, is_raster_path, Crop, Engine, PhotoStack};

use super::recipe::{ExportImageFormat, ExportRecipe};

#[derive(Debug, Clone)]
pub struct ExportJob {
    pub source_path: PathBuf,
    pub stack: PhotoStack,
    pub crop: Crop,
    pub output_path: PathBuf,
    pub recipe: ExportRecipe,
}

#[derive(Debug, Clone)]
pub enum ExportProgressMsg {
    Started {
        total: usize,
    },
    JobProgress {
        completed: usize,
        total: usize,
        current_file: String,
    },
    JobFailed {
        path: PathBuf,
        error: String,
        completed: usize,
        total: usize,
    },
    AllFinished {
        total: usize,
        success: usize,
        errors: Vec<(PathBuf, String)>,
    },
}

enum WorkerCommand {
    StartBatch(Vec<ExportJob>),
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExportStatus {
    Idle,
    Running {
        total: usize,
        completed: usize,
        current_file: String,
    },
    Finished {
        total: usize,
        success: usize,
        errors: Vec<(PathBuf, String)>,
    },
}

pub struct ExportQueue {
    cmd_tx: Sender<WorkerCommand>,
    progress_rx: Receiver<ExportProgressMsg>,
    current_status: ExportStatus,
}

impl ExportQueue {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
        let (progress_tx, progress_rx) = mpsc::channel::<ExportProgressMsg>();

        thread::Builder::new()
            .name("duckfoot-batch-export".into())
            .spawn(move || {
                let mut engine = Engine::new(&device);

                while let Ok(cmd) = cmd_rx.recv() {
                    match cmd {
                        WorkerCommand::StartBatch(jobs) => {
                            let total = jobs.len();
                            let _ = progress_tx.send(ExportProgressMsg::Started { total });

                            let mut completed = 0;
                            let mut errors = Vec::new();
                            let mut cancelled = false;

                            for job in jobs {
                                // Check if cancelled before each job
                                while let Ok(cmd) = cmd_rx.try_recv() {
                                    if matches!(cmd, WorkerCommand::Cancel) {
                                        cancelled = true;
                                        break;
                                    }
                                }
                                if cancelled {
                                    break;
                                }

                                let file_str = job
                                    .source_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("image")
                                    .to_string();

                                // A decoder panic on one file fails that job, not the queue;
                                // the engine it happened in is rebuilt, since it may be
                                // half-loaded.
                                let outcome =
                                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        process_single_job(&device, &queue, &mut engine, &job)
                                    }))
                                    .unwrap_or_else(|_| {
                                        engine = Engine::new(&device);
                                        Err(anyhow::anyhow!("the decoder crashed on this file"))
                                    });
                                match outcome {
                                    Ok(()) => {
                                        completed += 1;
                                        let _ = progress_tx.send(ExportProgressMsg::JobProgress {
                                            completed,
                                            total,
                                            current_file: file_str,
                                        });
                                    }
                                    Err(e) => {
                                        let err_msg = format!("{e:#}");
                                        log::error!(
                                            "Export error for {}: {err_msg}",
                                            job.source_path.display()
                                        );
                                        errors.push((job.source_path.clone(), err_msg.clone()));
                                        let _ = progress_tx.send(ExportProgressMsg::JobFailed {
                                            path: job.source_path,
                                            error: err_msg,
                                            completed,
                                            total,
                                        });
                                    }
                                }
                            }

                            let _ = progress_tx.send(ExportProgressMsg::AllFinished {
                                total,
                                success: completed,
                                errors,
                            });
                        }
                        WorkerCommand::Cancel => {
                            // Already idle
                        }
                    }
                }
            })
            .expect("failed to spawn batch export thread");

        Self {
            cmd_tx,
            progress_rx,
            current_status: ExportStatus::Idle,
        }
    }

    pub fn start_batch(&mut self, jobs: Vec<ExportJob>) {
        if jobs.is_empty() {
            return;
        }
        self.current_status = ExportStatus::Running {
            total: jobs.len(),
            completed: 0,
            current_file: "Starting...".into(),
        };
        let _ = self.cmd_tx.send(WorkerCommand::StartBatch(jobs));
    }

    pub fn cancel(&mut self) {
        let _ = self.cmd_tx.send(WorkerCommand::Cancel);
        self.current_status = ExportStatus::Idle;
    }

    pub fn poll(&mut self) -> &ExportStatus {
        loop {
            match self.progress_rx.try_recv() {
                Ok(msg) => match msg {
                    ExportProgressMsg::Started { total } => {
                        self.current_status = ExportStatus::Running {
                            total,
                            completed: 0,
                            current_file: "".into(),
                        };
                    }
                    ExportProgressMsg::JobProgress {
                        completed,
                        total,
                        current_file,
                    } => {
                        self.current_status = ExportStatus::Running {
                            total,
                            completed,
                            current_file,
                        };
                    }
                    ExportProgressMsg::JobFailed {
                        completed, total, ..
                    } => {
                        self.current_status = ExportStatus::Running {
                            total,
                            completed,
                            current_file: "Failed image".into(),
                        };
                    }
                    ExportProgressMsg::AllFinished {
                        total,
                        success,
                        errors,
                    } => {
                        self.current_status = ExportStatus::Finished {
                            total,
                            success,
                            errors,
                        };
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        &self.current_status
    }

    /// The result of a finished batch, handed out once; the queue is idle again after.
    pub fn take_finished(&mut self) -> Option<ExportStatus> {
        self.poll();
        if matches!(self.current_status, ExportStatus::Finished { .. }) {
            Some(std::mem::replace(
                &mut self.current_status,
                ExportStatus::Idle,
            ))
        } else {
            None
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.current_status, ExportStatus::Running { .. })
    }
}

fn process_single_job(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    engine: &mut Engine,
    job: &ExportJob,
) -> anyhow::Result<()> {
    // 1. Decode source into engine
    if is_raster_path(&job.source_path) {
        let bytes = fs::read(&job.source_path)?;
        let raster = decode_raster(&bytes)?;
        engine.load_raster(device, queue, raster)?;
    } else {
        let cfa = decode_raw(&job.source_path)?;
        engine.load(device, queue, cfa)?;
    }

    // 2. Full-resolution export from shader pipeline (Invariant 2)
    let exported = engine.export(device, queue, &job.stack, job.crop)?;

    // 3. Convert buffer to RgbaImage
    let mut rgba_img = image::RgbaImage::from_raw(exported.width, exported.height, exported.rgba)
        .ok_or_else(|| anyhow::anyhow!("exported RGBA buffer size mismatch"))?;

    // 4. Handle Sizing
    let (target_w, target_h) = job
        .recipe
        .sizing
        .calculate_size(exported.width, exported.height);

    if (target_w, target_h) != (exported.width, exported.height) {
        rgba_img = image::imageops::resize(
            &rgba_img,
            target_w,
            target_h,
            image::imageops::FilterType::Lanczos3,
        );
    }

    // 5. Ensure output directory exists
    if let Some(parent) = job.output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // 6. Encode according to recipe format
    encode_and_save(&rgba_img, &job.output_path, &job.recipe)?;

    Ok(())
}

fn encode_and_save(
    img: &image::RgbaImage,
    path: &Path,
    recipe: &ExportRecipe,
) -> anyhow::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    match recipe.format {
        ExportImageFormat::Jpeg => {
            let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut writer,
                recipe.quality.clamp(1, 100),
            );
            encoder.encode(
                &rgb,
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )?;
        }
        ExportImageFormat::Png => {
            img.write_to(&mut writer, image::ImageFormat::Png)?;
        }
        ExportImageFormat::Tiff => {
            img.write_to(&mut writer, image::ImageFormat::Tiff)?;
        }
        ExportImageFormat::Webp => {
            // image 0.25 supports WebP encoding
            let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            rgb.write_to(&mut writer, image::ImageFormat::WebP)?;
        }
    }

    Ok(())
}
