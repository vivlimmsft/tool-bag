// hide the console window in release builds; keep it during dev so logs are visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;

mod autostart;
mod config;
mod convert;
mod logging;
mod notify;
mod pool;
mod sanitize;
mod watcher;

use config::Config;
use sanitize::{ellipsize_left, AUMID};

fn main() -> Result<()> {
    // Set up file logging FIRST. With #[windows_subsystem = "windows"] there
    // is no console for stderr to land in, so the only way to surface errors
    // is to a log file under %LOCALAPPDATA%.
    let log_dir = logging::init().context("init logging")?;

    // Register our AppUserModelID with the OS so toast notifications and the
    // taskbar group identity attribute to webp-tray (matching the AUMID set
    // on the installer's Start menu shortcut). Must be called before any UI.
    register_aumid();

    let config_path = config::config_path()?;
    config::ensure_default(&config_path)?;
    let initial = Config::load(&config_path).context("loading config")?;
    let downloads = resolve_downloads(&initial)?;
    let config = Arc::new(RwLock::new(initial));

    log::info!("downloads folder: {}", downloads.display());
    log::info!("config file: {}", config_path.display());
    log::info!("log dir: {}", log_dir.display());

    // If autostart is enabled but pointing at a stale path (reinstall into a
    // different dir, exe moved), correct it silently.
    if let Ok(exe) = std::env::current_exe() {
        autostart::refresh_path_if_enabled(&exe);
    }

    watcher::spawn(downloads.clone(), config.clone(), config_path.clone());

    run_tray(config_path, downloads)
}

fn resolve_downloads(cfg: &Config) -> Result<PathBuf> {
    if !cfg.downloads_override.trim().is_empty() {
        let p = PathBuf::from(cfg.downloads_override.trim());
        if !p.is_dir() {
            anyhow::bail!(
                "downloads_override is set to {} but that is not a directory",
                p.display()
            );
        }
        return Ok(p);
    }
    // dirs::download_dir uses SHGetKnownFolderPath(FOLDERID_Downloads) on Windows,
    // which respects user relocation (e.g. moved to D:\Downloads).
    let p = dirs::download_dir().context("could not locate the OS Downloads folder")?;
    if !p.is_dir() {
        anyhow::bail!(
            "Downloads folder reported as {} but does not exist",
            p.display()
        );
    }
    Ok(p)
}

fn run_tray(config_path: PathBuf, downloads: PathBuf) -> Result<()> {
    let tray_menu = Menu::new();
    let edit_item = MenuItem::new("Edit config…", true, None);
    let open_dl_item = MenuItem::new("Open Downloads folder", true, None);
    let open_cfg_dir_item = MenuItem::new("Open config folder", true, None);
    let open_log_item = MenuItem::new("Open log file", true, None);
    let open_log_dir_item = MenuItem::new("Open log folder", true, None);
    // Reflect the registry's current state when the menu is built. Click
    // events flip the underlying state and we re-set checked() to match,
    // so the visible state stays consistent with reality.
    let autostart_item = CheckMenuItem::new("Start at login", true, autostart::is_enabled(), None);
    let quit_item = MenuItem::new("Quit", true, None);
    tray_menu.append(&edit_item)?;
    tray_menu.append(&open_dl_item)?;
    tray_menu.append(&open_cfg_dir_item)?;
    tray_menu.append(&PredefinedMenuItem::separator())?;
    tray_menu.append(&open_log_item)?;
    tray_menu.append(&open_log_dir_item)?;
    tray_menu.append(&PredefinedMenuItem::separator())?;
    tray_menu.append(&autostart_item)?;
    tray_menu.append(&PredefinedMenuItem::separator())?;
    tray_menu.append(&quit_item)?;

    let icon = build_icon();
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip(format!(
            "webp-tray\nWatching: {}",
            ellipsize_left(&downloads.display().to_string(), 60)
        ))
        .with_icon(icon)
        .build()
        .context("create tray icon")?;

    let edit_id = edit_item.id().clone();
    let open_dl_id = open_dl_item.id().clone();
    let open_cfg_dir_id = open_cfg_dir_item.id().clone();
    let open_log_id = open_log_item.id().clone();
    let open_log_dir_id = open_log_dir_item.id().clone();
    let autostart_id = autostart_item.id().clone();
    let quit_id = quit_item.id().clone();
    let cfg_path_for_handler = config_path.clone();
    let downloads_for_handler = downloads.clone();
    // We keep the CheckMenuItem alive (in a leaked-by-binding sense) by
    // holding it in `_autostart_item` until the tray drops. We can't move it
    // into the event handler because muda's CheckMenuItem isn't Sync; the
    // handler instead just consults the registry as the source of truth.
    let _autostart_item = autostart_item;

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            log::info!("quit requested");
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            }
        } else if event.id == edit_id {
            shell_open(&cfg_path_for_handler);
        } else if event.id == open_dl_id {
            shell_open(&downloads_for_handler);
        } else if event.id == open_cfg_dir_id {
            if let Some(parent) = cfg_path_for_handler.parent() {
                shell_open(parent);
            }
        } else if event.id == open_log_id {
            match logging::current_log_file() {
                Some(p) => shell_open(&p),
                None => {
                    if let Some(d) = logging::log_dir() {
                        shell_open(&d);
                    }
                }
            }
        } else if event.id == open_log_dir_id {
            if let Some(d) = logging::log_dir() {
                shell_open(&d);
            }
        } else if event.id == autostart_id {
            // muda toggles the visible check itself on click; we react by
            // making the registry match. The desired new state is the
            // *opposite* of what was in the registry before the click.
            let want = !autostart::is_enabled();
            let exe = match std::env::current_exe() {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("current_exe failed: {e}");
                    notify::error_toast(
                        "Couldn't change autostart",
                        &format!("could not locate the running exe: {e}"),
                    );
                    return;
                }
            };
            match autostart::set_enabled(want, &exe) {
                Ok(()) => {
                    notify::info_toast(
                        if want {
                            "Start at login: ON"
                        } else {
                            "Start at login: OFF"
                        },
                        if want {
                            "webp-tray will start automatically when you sign in."
                        } else {
                            "webp-tray will no longer start at login."
                        },
                    );
                }
                Err(e) => {
                    log::warn!("toggle autostart failed: {e:#}");
                    notify::error_toast(
                        "Couldn't change autostart",
                        &format!(
                            "registry write failed: {e:#}. The menu check may be \
                             out of sync until the app is restarted."
                        ),
                    );
                }
            }
        }
    }));

    // Standard Win32 message pump. Blocks in GetMessageW so the thread is idle
    // (no busy loop, near-zero CPU when nothing is happening).
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG,
    };
    unsafe {
        let mut msg = MSG::default();
        loop {
            let r = GetMessageW(&mut msg, Some(HWND::default()), 0, 0);
            // 0 = WM_QUIT; -1 = error.
            if r.0 == 0 || r.0 == -1 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

fn build_icon() -> tray_icon::Icon {
    // Load the .exe's embedded icon resource (ID 1, see assets/webp-tray.rc).
    // This keeps the icon binary in one place (the exe) instead of carrying
    // a second copy in our data section. tray-icon picks the system tray
    // size automatically.
    match tray_icon::Icon::from_resource(1, None) {
        Ok(icon) => icon,
        Err(e) => {
            // Fallback: synthesised flat colour so we still get *something*
            // in the tray if the resource somehow isn't present.
            log::warn!("Icon::from_resource failed ({e}); using fallback");
            const W: u32 = 16;
            const H: u32 = 16;
            let mut rgba = vec![0u8; (W * H * 4) as usize];
            for y in 0..H {
                for x in 0..W {
                    let i = ((y * W + x) * 4) as usize;
                    let edge = x == 0 || y == 0 || x == W - 1 || y == H - 1;
                    let (r, g, b) = if edge {
                        (0x22, 0x44, 0x77)
                    } else {
                        (0x55, 0x99, 0xff)
                    };
                    rgba[i] = r;
                    rgba[i + 1] = g;
                    rgba[i + 2] = b;
                    rgba[i + 3] = 0xff;
                }
            }
            tray_icon::Icon::from_rgba(rgba, W, H).expect("fallback icon")
        }
    }
}

/// Open a path with its OS-default handler via ShellExecuteW.
///
/// Why not `cmd /C start`:
/// * spawns a transient cmd.exe just to invoke the same shell verb
/// * cmd has its own metacharacter rules (`&`, `^`, `%`) that can mangle
///   paths even when std::process::Command quotes the arg properly
///
/// ShellExecuteW is the same call Explorer makes on double-click, so it
/// just works for both files (opens with associated app) and dirs (opens
/// in Explorer).
fn shell_open(path: &std::path::Path) {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb = HSTRING::from("open");
    let file = HSTRING::from(path.as_os_str());
    let result = unsafe {
        ShellExecuteW(
            Some(HWND::default()),
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns an HINSTANCE > 32 on success. <= 32 indicates an error code.
    if (result.0 as isize) <= 32 {
        log::warn!(
            "ShellExecuteW({}) returned error code {}",
            path.display(),
            result.0 as isize
        );
    }
}

fn register_aumid() {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    let aumid: HSTRING = AUMID.into();
    unsafe {
        if let Err(e) = SetCurrentProcessExplicitAppUserModelID(&aumid) {
            log::warn!("SetCurrentProcessExplicitAppUserModelID failed: {e}");
        }
    }
}
