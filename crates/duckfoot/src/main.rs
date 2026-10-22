//! Desktop bootstrap: a logger, the command line, native dialogs. The shell itself is in
//! `lib.rs`, shared with the `duckfoot-android` crate.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::path::PathBuf;

/// Log files kept, one a launch: the one before a hard freeze must survive the relaunch
/// after it.
const KEPT_LOGS: usize = 10;

fn main() -> anyhow::Result<()> {
    let log_path = start_logging();

    // A panic's message goes to the log file too, not only to a terminal nobody opened.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));
    log::info!(
        "duckfoot {} ({}) on {} {}, logging to {}",
        env!("CARGO_PKG_VERSION"),
        option_env!("GITHUB_SHA").map_or("local build", |sha| &sha[..sha.len().min(7)]),
        std::env::consts::OS,
        std::env::consts::ARCH,
        log_path
            .as_ref()
            .map_or("stderr only".into(), |p| p.display().to_string()),
    );

    duckfoot::run(
        duckfoot::event_loop()?,
        duckfoot::Options {
            // Every argument (a file manager's %F passes several), and paths that are
            // not UTF-8 too.
            pending_open: std::env::args_os().skip(1).map(Into::into).collect(),
            platform: Box::new(photo_app::platform::default_platform),
        },
    )
}

/// stderr, as before, and a file of this launch's own in the platform's log folder. wgpu's
/// own chatter stays at warnings, since every line is synced to disk as it is written.
fn start_logging() -> Option<PathBuf> {
    let file = log_dir().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        prune_logs(&dir);
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let path = dir.join(format!("duckfoot-{secs}.log"));
        let file = std::fs::File::create(&path).ok()?;
        Some((path, file))
    });
    let (path, file) = file.unzip();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    .target(env_logger::Target::Pipe(Box::new(Trail { file })))
    .init();
    path
}

/// Where the log files go: ~/Library/Logs on a Mac, %LOCALAPPDATA% on Windows, the XDG
/// state folder elsewhere.
fn log_dir() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        Some(home()?.join("Library/Logs/Duckfoot"))
    } else if cfg!(windows) {
        Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Duckfoot/Logs"))
    } else {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(home()?.join(".local/state")))?;
        Some(state.join("duckfoot"))
    }
}

/// Make room for this launch's file: the oldest go once there are `KEPT_LOGS`.
fn prune_logs(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("duckfoot-") && n.ends_with(".log"))
        })
        .collect();
    // Named by launch time in seconds, so the names sort oldest first.
    logs.sort();
    let excess = (logs.len() + 1).saturating_sub(KEPT_LOGS);
    for old in &logs[..excess] {
        let _ = std::fs::remove_file(old);
    }
}

/// Each record to stderr and to the file, synced before the next. A lockup that needs the
/// power pulled loses whatever the OS still held in memory, and the last lines are the
/// ones that say where it stopped. `sync_all` is `F_FULLFSYNC` on macOS, which also
/// flushes the drive's own cache.
struct Trail {
    file: Option<std::fs::File>,
}

impl Write for Trail {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(file) = &mut self.file {
            if file.write_all(buf).is_ok() {
                let _ = file.sync_all();
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}
