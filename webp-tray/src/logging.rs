use std::path::PathBuf;

use anyhow::{Context, Result};
use flexi_logger::{Cleanup, Criterion, FileSpec, Logger, LoggerHandle, Naming, WriteMode};
use parking_lot::Mutex;

/// Holds the `LoggerHandle` (otherwise the background flusher thread is dropped)
/// plus the resolved log directory. We compute the *current* log file path on
/// demand because flexi_logger's rotated names change over time.
pub struct LogState {
    _handle: LoggerHandle,
    log_dir: PathBuf,
    basename: String,
}

static LOG_STATE: Mutex<Option<LogState>> = Mutex::new(None);

const BASENAME: &str = "webp-tray";

/// Initialise file logging:
/// * writes to `%LOCALAPPDATA%\webp-tray\logs\webp-tray_rCURRENT.log`
/// * rotates at 2 MB, keeps the 5 most recent files
/// * `RUST_LOG`-style env override is honoured; default level is `info`
///
/// Without this, a `windows_subsystem="windows"` build has no console for
/// `env_logger` to write to and every log message would be silently dropped.
pub fn init() -> Result<PathBuf> {
    let dir = log_dir().context("resolve log dir")?;
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;

    let spec = FileSpec::default()
        .directory(&dir)
        .basename(BASENAME)
        .suppress_timestamp();

    let handle = Logger::try_with_env_or_str("info")
        .context("init logger")?
        .log_to_file(spec)
        .rotate(
            Criterion::Size(2_000_000),
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        // BufferAndFlush keeps writes off the hot path while still flushing
        // every few hundred ms — good for a low-impact background tool.
        .write_mode(WriteMode::BufferAndFlush)
        .format(flexi_logger::detailed_format)
        .start()
        .context("start logger")?;

    *LOG_STATE.lock() = Some(LogState {
        _handle: handle,
        log_dir: dir.clone(),
        basename: BASENAME.to_string(),
    });
    Ok(dir)
}

/// Best-effort: returns the *most recently written* log file in the log dir.
/// flexi_logger's rotated filename pattern is e.g. `webp-tray_r00001.log`;
/// rather than reverse-engineer the index, we just pick the newest file by
/// mtime that starts with our basename.
pub fn current_log_file() -> Option<PathBuf> {
    let st = LOG_STATE.lock();
    let st = st.as_ref()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let entries = std::fs::read_dir(&st.log_dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        let name = match p.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.starts_with(&st.basename) {
            continue;
        }
        let mtime = entry.metadata().and_then(|m| m.modified()).ok()?;
        if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
            best = Some((mtime, p));
        }
    }
    best.map(|(_, p)| p).or_else(|| Some(st.log_dir.clone()))
}

pub fn log_dir() -> Option<PathBuf> {
    Some(dirs::data_local_dir()?.join("webp-tray").join("logs"))
}
