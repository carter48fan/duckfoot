//! Background headless export queue.
//!
//! Processes multi-export recipes and batch rendering off the UI thread
//! using an independent Engine instance on cloned wgpu device/queue handles.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
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
    progress_rx: Receiver<ExportProgressMsg>,
    current_status: ExportStatus,
    /// Cancel was asked for; the worker stops after the photo it is on, and the status
    /// stays Running until it says it has.
    cancelling: bool,
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
                                // Nothing kept between jobs: this engine runs beside the
                                // interactive one, and on a phone the two cannot both
                                // hold a full-size photo's textures while idle (WP5.2).
                                engine.unload();
                                engine.release_spares();
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
        let _ = self.cmd_tx.send(WorkerCommand::StartBatch(jobs));
    }

    /// Stop after the photo being written. Every output is written through a temporary
    /// file, so nothing half-written is left behind; the queue reports Finished, with
    /// fewer written than asked for, once the worker has actually stopped.
    pub fn cancel(&mut self) {
        if self.is_running() {
            self.cancelling = true;
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

fn process_single_job(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    engine: &mut Engine,
    job: &ExportJob,
) -> anyhow::Result<()> {
    // 1. Decode source into engine (a virtual copy's bytes are its photo's)
    let file = photo_engine::copies::source_file(&job.source_path);
    if is_raster_path(&file) {
        let bytes = fs::read(&file)?;
        let raster = decode_raster(&bytes)?;
        engine.load_raster(device, queue, raster)?;
    } else {
        let cfa = decode_raw(&file)?;
        engine.load(device, queue, cfa)?;
    }

    // 2. Full-resolution export from shader pipeline (Invariant 2), at the recipe's depth.
    let settings = &job.recipe.settings;
    let exported = engine.export_in(
        device,
        queue,
        &job.stack,
        settings.effective_depth(),
        settings.color_space,
    )?;

    // 3. Size, sharpen and encode it, through the same path the single export uses.
    if let Some(parent) = job.output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let info = photo_engine::read_export_info(&job.source_path);
    exported.save(&job.output_path, settings, info.as_ref())
}
