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
    /// The batch itself (not one photo of it) panicked; nothing more will come.
    Crashed(String),
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
                        // Each photo is already caught on its own; this catches the batch
                        // around them (a scoped thread's panic is re-raised here), so the
                        // queue reports an end instead of running forever.
                        let run = std::panic::AssertUnwindSafe(|| {
                            run_batch(&device, &queue, &mut engine, jobs, &progress_tx, &stop)
                        });
                        if let Err(panic) = std::panic::catch_unwind(run) {
                            let reason = panic
                                .downcast_ref::<&str>()
                                .map(|s| s.to_string())
                                .or_else(|| panic.downcast_ref::<String>().cloned())
                                .unwrap_or_else(|| "unknown reason".into());
                            log::error!("batch export crashed: {reason}");
                            let _ = progress_tx.send(ExportProgressMsg::Crashed(reason));
                            // Whatever it was mid-way through is not to be trusted.
                            engine = Engine::new(&device);
                        }
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
                    ExportProgressMsg::Crashed(reason) => {
                        self.stopped(format!("the export crashed ({reason})"))
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
                // The worker is gone: a batch it was running will never report its end.
                Err(TryRecvError::Disconnected) => {
                    self.stopped("the export worker stopped".into());
                    break;
                }
            }
        }
        &self.current_status
    }

    /// Ends a running batch that will not end itself, keeping what was written.
    fn stopped(&mut self, reason: String) {
        if let ExportStatus::Running {
            total, completed, ..
        } = self.current_status
        {
            self.cancelling = false;
            self.current_status = ExportStatus::Finished {
                total,
                success: completed,
                errors: vec![(PathBuf::new(), reason)],
            };
        }
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
    /// With the lens profile the editor would have found for it.
    Raw(
        Box<photo_engine::CfaImage>,
        Option<(String, photo_engine::lens::FileWarp)>,
    ),
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
        let cfa = decode_raw(&file)?;
        let profile = photo_engine::lensfun::for_file(&file, &cfa);
        Ok(Source::Raw(Box::new(cfa), profile))
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
        Source::Raw(cfa, profile) => {
            let mut uploaded =
                photo_engine::upload_cfa(device, queue, &engine.upload_pool(), *cfa)?;
            uploaded.lens_profile = profile;
            engine.load_uploaded(device, queue, uploaded)?
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A queue whose worker has gone, mid-batch.
    fn orphaned() -> ExportQueue {
        let (cmd_tx, _) = mpsc::channel();
        let (_, progress_rx) = mpsc::channel();
        ExportQueue {
            cmd_tx,
            cancel: Arc::default(),
            progress_rx,
            current_status: ExportStatus::Running {
                total: 5,
                completed: 2,
                current_file: "a.dng".into(),
            },
            cancelling: true,
        }
    }

    /// A RAW's lens correction reaches the batch's engine, as it does the editor's: an
    /// export that skipped it would not be what the user looked at (Invariant 2).
    #[test]
    fn a_batch_export_corrects_the_lens_as_the_editor_does() {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::NOOP;
        desc.backend_options.noop.enable = true;
        let instance = wgpu::Instance::new(desc);
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("the no-op adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("a no-op device");

        let db = photo_engine::lensfun::database();
        let camera = db.camera("SONY", "ILCE-7M4");
        let lens = db
            .lens("FE 24-70mm F2.8 GM", camera)
            .expect("in the database");
        let cfa = photo_engine::testkit::flat(64, 48, 0.4);
        let profile = lens.profile(24.0, Some(2.8));
        let name = profile.name.clone();
        let warp = profile
            .warp((cfa.width, cfa.height), lens.crop)
            .expect("a warp");

        let mut recipe = ExportRecipe::default_recipes().remove(0);
        recipe.settings = photo_engine::ExportSettings::new(photo_engine::ExportFormat::Png);
        let job = ExportJob {
            source_path: PathBuf::from("/nowhere/a.ARW"),
            stack: PhotoStack::default(),
            crop: Crop::default(),
            output_path: PathBuf::from("/nowhere/a.png"),
            recipe,
        };
        let mut engine = Engine::new(&device);
        let source = Source::Raw(Box::new(cfa), Some((name.clone(), warp)));
        render(&device, &queue, &mut engine, source, &job).expect("rendered");
        assert_eq!(
            engine.image().and_then(|i| i.lens_profile_name()),
            Some(name.as_str())
        );
    }

    #[test]
    fn a_batch_whose_worker_is_gone_finishes_with_an_error() {
        let mut q = orphaned();
        match q.poll() {
            ExportStatus::Finished { total, errors, .. } => {
                assert_eq!(*total, 5);
                assert_eq!(errors.len(), 1);
            }
            _ => panic!("still running"),
        }
        assert!(!q.is_cancelling());
    }

    #[test]
    fn a_crashed_batch_finishes_with_its_reason() {
        let mut q = orphaned();
        let (tx, rx) = mpsc::channel();
        q.progress_rx = rx;
        tx.send(ExportProgressMsg::Crashed("boom".into())).unwrap();
        match q.poll() {
            ExportStatus::Finished { errors, .. } => assert!(errors[0].1.contains("boom")),
            _ => panic!("still running"),
        }
    }
}
