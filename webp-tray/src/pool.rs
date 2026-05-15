use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{bounded, Sender, TrySendError};

use crate::config::Config;
use crate::convert;

/// Bounded fixed-size pool for image conversion work.
///
/// Why bounded:
/// - decoding a webp + holding a `DynamicImage` can easily be tens of MB;
///   running this once per filesystem event in `std::thread::spawn` would
///   let a folder full of new webps spawn dozens of threads at once.
/// - bounded(256) gives us a safety net: if a flood arrives faster than the
///   workers can drain, we drop new jobs and log rather than OOM the machine.
///
/// Why a fixed worker count:
/// - image decode is CPU-bound; more workers than physical cores hurts.
///   We cap at 4 because more parallelism rarely helps for a background tool
///   and we don't want to monopolise the user's machine.
pub struct Pool {
    tx: Sender<Job>,
    workers: usize,
}

struct Job {
    path: PathBuf,
    cfg: Config,
}

impl Pool {
    pub fn new(workers: usize) -> Self {
        let workers = workers.clamp(1, 8);
        let (tx, rx) = bounded::<Job>(256);
        let rx = Arc::new(rx);
        for n in 0..workers {
            let rx = rx.clone();
            thread::Builder::new()
                .name(format!("webp-tray-worker-{n}"))
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        if !wait_for_stable(&job.path) {
                            log::warn!("file never stabilized after 6s: {}", job.path.display());
                            crate::notify::error_toast(
                                "Couldn't convert webp",
                                &format!(
                                    "{} never finished writing. Open the log for details.",
                                    job.path
                                        .file_name()
                                        .map(|s| s.to_string_lossy().into_owned())
                                        .unwrap_or_default()
                                ),
                            );
                            continue;
                        }
                        if let Err(e) = convert::convert_one(&job.path, &job.cfg) {
                            log::warn!("convert failed for {}: {e:#}", job.path.display());
                            crate::notify::error_toast(
                                "Couldn't convert webp",
                                &format!(
                                    "{}: {}",
                                    job.path
                                        .file_name()
                                        .map(|s| s.to_string_lossy().into_owned())
                                        .unwrap_or_default(),
                                    short_error(&e)
                                ),
                            );
                        }
                    }
                })
                .expect("spawn worker thread");
        }
        Self { tx, workers }
    }

    pub fn worker_count(&self) -> usize {
        self.workers
    }

    pub fn submit(&self, path: PathBuf, cfg: Config) {
        match self.tx.try_send(Job { path, cfg }) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) => {
                log::warn!(
                    "conversion queue full ({} pending), dropping {}",
                    256,
                    job.path.display()
                );
                crate::notify::error_toast(
                    "webp-tray is overloaded",
                    "too many files arriving at once; some webps were skipped. \
                     Re-trigger by saving them again, or restart the app.",
                );
            }
            Err(TrySendError::Disconnected(_)) => {
                log::error!("conversion pool disconnected; workers gone");
            }
        }
    }
}

/// Shorten an anyhow error to a single line for toast display. Keeps the
/// outermost context message and drops the (already-logged) source chain.
fn short_error(e: &anyhow::Error) -> String {
    let s = format!("{e:#}");
    // Toasts can show a couple lines but a wall of text gets cut off.
    let first_line = s.lines().next().unwrap_or("").to_string();
    if first_line.len() > 200 {
        format!("{}…", &first_line[..200])
    } else {
        first_line
    }
}

/// Wait until file size stops growing and the file is openable for read.
/// Browsers typically rename `.crdownload` -> `.webp` atomically, but we still
/// want to guard against the rare partial-write case before decoding.
fn wait_for_stable(p: &std::path::Path) -> bool {
    use std::time::Duration;
    let mut last: Option<u64> = None;
    let mut stable_hits = 0;
    for _ in 0..40 {
        let Ok(meta) = std::fs::metadata(p) else {
            return false;
        };
        let len = meta.len();
        if len == 0 {
            std::thread::sleep(Duration::from_millis(150));
            last = Some(0);
            continue;
        }
        if Some(len) == last {
            stable_hits += 1;
            if stable_hits >= 2 && std::fs::File::open(p).is_ok() {
                return true;
            }
        } else {
            stable_hits = 0;
            last = Some(len);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}
