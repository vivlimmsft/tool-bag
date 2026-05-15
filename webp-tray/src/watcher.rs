use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use parking_lot::RwLock;

use crate::config::Config;
use crate::pool::Pool;

pub fn spawn(downloads: PathBuf, config: Arc<RwLock<Config>>, config_path: PathBuf) {
    std::thread::Builder::new()
        .name("webp-tray-watcher".into())
        .spawn(move || {
            if let Err(e) = run(downloads, config, config_path) {
                log::error!("watcher exited: {e:?}");
            }
        })
        .expect("spawn watcher thread");
}

fn run(downloads: PathBuf, config: Arc<RwLock<Config>>, config_path: PathBuf) -> Result<()> {
    let (debounce, recursive) = {
        let c = config.read();
        (Duration::from_millis(c.debounce_ms.max(50)), c.recursive)
    };

    // Conversion thread pool. Sized to a small fixed number; image decode is
    // CPU-bound and a background tool shouldn't saturate the machine. Pool
    // applies its own clamp so even on big-core machines we stay reasonable.
    let workers = std::thread::available_parallelism()
        .map(|n| (n.get() / 2).max(1))
        .unwrap_or(2);
    let pool = Arc::new(Pool::new(workers));
    log::info!("conversion pool: {} workers", pool.worker_count());

    let cfg_for_handler = config.clone();
    let cfg_path_for_handler = config_path.clone();
    let pool_for_handler = pool.clone();

    let mut debouncer = new_debouncer(debounce, None, move |res: DebounceEventResult| match res {
        Ok(events) => {
            for e in events {
                for path in &e.paths {
                    if same_file(path, &cfg_path_for_handler) {
                        match Config::load(&cfg_path_for_handler) {
                            Ok(new) => {
                                log::info!("config reloaded");
                                *cfg_for_handler.write() = new;
                            }
                            Err(err) => {
                                log::warn!("config reload failed: {err:#}");
                                crate::notify::error_toast(
                                    "Config has an error",
                                    &format!("Reverted to last known-good values. {err:#}"),
                                );
                            }
                        }
                        continue;
                    }
                    if !matches!(e.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                        continue;
                    }
                    if !is_webp(path) {
                        continue;
                    }
                    let cfg = cfg_for_handler.read().clone();
                    pool_for_handler.submit(path.clone(), cfg);
                }
            }
        }
        Err(errs) => {
            for e in errs {
                log::warn!("watch error: {e}");
            }
        }
    })
    .context("create file watcher")?;

    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    debouncer
        .watch(&downloads, mode)
        .with_context(|| format!("watch {}", downloads.display()))?;

    if let Some(cfg_dir) = config_path.parent() {
        debouncer
            .watch(cfg_dir, RecursiveMode::NonRecursive)
            .with_context(|| format!("watch config dir {}", cfg_dir.display()))?;
    }

    log::info!(
        "watching {} (recursive={}) and config at {}",
        downloads.display(),
        recursive,
        config_path.display()
    );
    log::info!(
        "note: changes to `recursive` and `debounce_ms` require restart; \
         all other config keys are picked up live"
    );

    // park forever; debouncer + pool own their own threads.
    loop {
        std::thread::park();
    }
}

fn is_webp(p: &Path) -> bool {
    p.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("webp"))
        .unwrap_or(false)
}

fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}
