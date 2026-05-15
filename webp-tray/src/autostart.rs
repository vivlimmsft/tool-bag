//! "Start at login" via HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
//!
//! Why the registry Run key (and not a Startup folder .lnk):
//! * One source of truth — easy to query and toggle from a single place.
//! * No file-system litter; uninstall via MSI removes the value cleanly.
//! * Path tracking — if the user reinstalls into a different directory,
//!   we re-point the value to the current exe automatically on startup.

use std::path::Path;

use anyhow::{Context, Result};
use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE};
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "webp-tray";

/// Returns true if our value exists under HKCU Run.
pub fn is_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(run) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_QUERY_VALUE) else {
        return false;
    };
    run.get_value::<String, _>(VALUE_NAME).is_ok()
}

/// Returns the path currently registered for autostart, if any.
pub fn registered_path() -> Option<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu.open_subkey_with_flags(RUN_KEY, KEY_QUERY_VALUE).ok()?;
    run.get_value::<String, _>(VALUE_NAME).ok()
}

/// Set or clear the autostart entry. When enabling, the value is the current
/// exe path quoted to survive paths with spaces.
pub fn set_enabled(enabled: bool, exe_path: &Path) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(RUN_KEY).context("open HKCU Run key")?;
    if enabled {
        // Quote the path so spaces in C:\Program Files\... or elsewhere don't
        // confuse the shell when Windows launches the value at login.
        let quoted = format!("\"{}\"", exe_path.display());
        run.set_value(VALUE_NAME, &quoted)
            .context("write Run value")?;
        log::info!("autostart enabled: {quoted}");
    } else {
        // delete_value returns Err if the value doesn't exist; that's fine — we
        // wanted it gone and it already is.
        let _ = run.delete_value(VALUE_NAME);
        log::info!("autostart disabled");
    }
    Ok(())
}

/// On startup: if autostart is enabled but points at a stale exe path
/// (because the user reinstalled into a different directory, or moved the
/// app), silently fix it to point at the current exe. No-op if disabled or
/// already correct.
pub fn refresh_path_if_enabled(current_exe: &Path) {
    if !is_enabled() {
        return;
    }
    let want = format!("\"{}\"", current_exe.display());
    if registered_path().as_deref() == Some(want.as_str()) {
        return;
    }
    if let Err(e) = set_enabled(true, current_exe) {
        log::warn!("failed to refresh autostart path: {e:#}");
    } else {
        log::info!("autostart path updated to current exe");
    }
}
