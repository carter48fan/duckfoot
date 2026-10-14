//! Background headless export queue.
//!
//! Processes multi-export recipes and batch rendering off the UI thread
//! using an independent Engine instance on cloned wgpu device/queue handles.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use photo_engine::{decode_raster, decode_raw, is_raster_path, Crop, Engine, PhotoStack};

use super::recipe::ExportRecipe;

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
    /// Set by [`Self::cancel`], read by every stage of the batch between its steps.
    cancel: Arc<AtomicBool>,
    progress_rx: Receiver<ExportProgressMsg>,
    current_status: ExportStatus,
    /// Cancel was asked for; each stage stops at its next step (no photo is written after
    /// it), and the status stays Running until the worker says it has.
    cancelling: bool,
}

impl ExportQueue {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
        let (progress_tx, progress_rx) = mpsc::channel::<ExportProgressMsg>();

        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        thread::Builder::new()
            .name("duckfoot-batch-export".into())
            .spawn(move || {
                let mut engine = Engine::new(&device);
                while let Ok(cmd) = cmd_rx.recv() {
                    // A Cancel with no batch running has nothing to stop.
                    if let WorkerCommand::StartBatch(jobs) = cmd {
                        run_batch(&device, &queue, &mut engine, jobs, &progress_tx, &stop);
                    }
                }
            })
            .expect("failed to spawn batch export thread");

        Self {
            cmd_tx,
            cancel,
            progress_rx,
            current_status: ExportStatus::Idle,
            cancelling: false,
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
        // Cleared here, before the worker has the batch: cleared there, a cancel made
        // between the two would be forgotten.
        self.cancel.store(false, Ordering::SeqCst);
        let _ = self.cmd_tx.send(WorkerCommand::StartBatch(jobs));
    }

    /// Stop: no photo is started or written after this. Every output is written through a temporary
    /// file, so nothing half-written is left behind; the queue reports Finished, with
    /// fewer written than asked for, once the worker has actually stopped.
    pub fn cancel(&mut self) {
        if self.is_running() {
            self.cancelling = true;
            self.cancel.store(true, Ordering::SeqCst);
            let _ = self.cmd_tx.send(WorkerCommand::Cancel);
        }
    }

    pub fn is_cancelling(&self) -> bool {
        self.cancelling
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
                        self.cancelling = false;
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

/// How many photos are encoded and written at once, while the GPU renders the next: half
/// the cores, up to three (a 12 MP AVIF is seconds of one core's work), and one on a
/// phone, where each waiting photo is a full-size image in memory.
fn encoders() -> usize {
    if cfg!(target_os = "android") {
        return 1;
    }
    thread::available_parallelism().map_or(1, |n| (n.get() / 2).clamp(1, 3))
}

/// A photo as decoded, for the GPU stage to upload.
enum Source {
    Raster(photo_engine::RasterImage),
    Raw(Box<photo_engine::CfaImage>),
}

/// One batch, in three stages that overlap: a thread decodes the next photo while the
/// GPU (this thread, with the one engine) renders the current one and a few threads
/// encode and write the ones before. The channels between them are bounded, so at most one
/// decoded photo waits and each encoder holds one rendered one. A cancel is seen by every
/// stage at its next step; what was written was written whole (each output goes through a
/// temporary file), and nothing is written after it.
fn run_batch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    engine: &mut Engine,
    jobs: Vec<ExportJob>,
    progress_tx: &Sender<ExportProgressMsg>,
    stop: &AtomicBool,
) {
    let total = jobs.len();
    let _ = progress_tx.send(ExportProgressMsg::Started { total });
    let cancelled = || stop.load(Ordering::SeqCst);
    let (decoded_tx, decoded_rx) = mpsc::sync_channel::<(ExportJob, anyhow::Result<Source>)>(0);
    let (rendered_tx, rendered_rx) = mpsc::sync_channel::<(ExportJob, Rendered)>(encoders());
    let rendered_rx = Mutex::new(rendered_rx);
    let (done_tx, done_rx) = mpsc::channel::<(ExportJob, anyhow::Result<()>)>();

    let (completed, errors) = thread::scope(|scope| {
        // Decode, ahead of the GPU.
        scope.spawn(move || {
            for job in jobs {
                if cancelled() {
                    break;
                }
                let source = std::panic::catch_unwind(|| decode(&job))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("the decoder crashed on this file")));
                if decoded_tx.send((job, source)).is_err() {
                    break;
                }
            }
        });
        // Encode and write, behind it.
        for _ in 0..encoders() {
            let (rendered_rx, done_tx) = (&rendered_rx, done_tx.clone());
            scope.spawn(move || loop {
                let next = rendered_rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                let Ok((job, exported)) = next else { break };
                if cancelled() {
                    continue;
                }
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    write(exported, &job)
                }))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("the encoder crashed on this photo")));
                if done_tx.send((job, outcome)).is_err() {
                    break;
                }
            });
        }
        // Count what finishes, in whatever order it does.
        let done_tx_gpu = done_tx.clone();
        drop(done_tx);
        let counter = scope.spawn(move || {
            let (mut completed, mut errors) = (0, Vec::new());
            for (job, outcome) in done_rx {
                let name = job
                    .source_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("image")
                    .to_string();
                match outcome {
                    Ok(()) => {
                        completed += 1;
                        let _ = progress_tx.send(ExportProgressMsg::JobProgress {
                            completed,
                            total,
                            current_file: name,
                        });
                    }
                    Err(e) => {
                        let error = format!("{e:#}");
                        log::error!("Export error for {}: {error}", job.source_path.display());
                        errors.push((job.source_path.clone(), error.clone()));
                        let _ = progress_tx.send(ExportProgressMsg::JobFailed {
                            path: job.source_path,
                            error,
                            completed,
                            total,
                        });
                    }
                }
            }
            (completed, errors)
        });
        // Render, on the GPU, here.
        for (job, source) in decoded_rx {
            if cancelled() {
                break;
            }
            // A panic fails this photo, not the batch; the engine it happened in is
            // rebuilt, since it may be half-loaded.
            let rendered = source.and_then(|source| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    render(device, queue, engine, source, &job)
                }))
                .unwrap_or_else(|_| {
                    *engine = Engine::new(device);
                    Err(anyhow::anyhow!("the renderer crashed on this photo"))
                })
            });
            // Nothing kept between photos: this engine runs beside the interactive one,
            // and on a phone the two cannot both hold a full-size photo's textures while
            // idle (WP5.2).
            engine.unload();
            engine.release_spares();
            let sent = match rendered {
                Ok(exported) => rendered_tx.send((job, exported)).is_ok(),
                Err(e) => done_tx_gpu.send((job, Err(e))).is_ok(),
            };
            if !sent {
                break;
            }
        }
        drop(rendered_tx);
        drop(done_tx_gpu);
        counter.join().unwrap_or_default()
    });
    let _ = progress_tx.send(ExportProgressMsg::AllFinished {
        total,
        success: completed,
        errors,
    });
}

/// Read and decode a job's photo (a virtual copy's bytes are its photo's).
fn decode(job: &ExportJob) -> anyhow::Result<Source> {
    let file = photo_engine::copies::source_file(&job.source_path);
    if is_raster_path(&file) {
        Ok(Source::Raster(decode_raster(&fs::read(&file)?)?))
    } else {
        Ok(Source::Raw(Box::new(decode_raw(&file)?)))
    }
}

/// A rendered export, and its HDR gain map when the recipe asks for one.
type Rendered = (photo_engine::Exported, Option<photo_engine::GainMap>);

/// The full-resolution export from the shader pipeline (Invariant 2), at the recipe's
/// depth and in its space.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    engine: &mut Engine,
    source: Source,
    job: &ExportJob,
) -> anyhow::Result<Rendered> {
    match source {
        Source::Raster(raster) => engine.load_raster(device, queue, raster)?,
        Source::Raw(cfa) => engine.load(device, queue, *cfa)?,
    }
    let settings = &job.recipe.settings;
    let exported = engine.export_in(
        device,
        queue,
        &job.stack,
        settings.effective_depth(),
        settings.effective_space(),
    )?;
    let gain = if settings.wants_gain_map() {
        Some(engine.gain_map(device, queue, &job.stack, settings.effective_space())?)
    } else {
        None
    };
    Ok((exported, gain))
}

/// Size, sharpen, encode and write it, through the same path the single export uses.
fn write((exported, gain): Rendered, job: &ExportJob) -> anyhow::Result<()> {
    if let Some(parent) = job.output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let info = photo_engine::read_export_info(&job.source_path);
    exported.save_with(
        &job.output_path,
        &job.recipe.settings,
        info.as_ref(),
        &[],
        gain.as_ref(),
    )
}
