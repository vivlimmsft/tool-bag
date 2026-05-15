//! Thin wrapper around `tauri-winrt-notification` so errors and successes
//! across the codebase share a consistent toast format and AUMID.

use crate::sanitize::AUMID;

pub fn info_toast(title: &str, body: &str) {
    show(title, body, false);
}

pub fn error_toast(title: &str, body: &str) {
    show(title, body, true);
}

fn show(title: &str, body: &str, is_error: bool) {
    let mut toast = tauri_winrt_notification::Toast::new(AUMID)
        .title(title)
        .text1(body);
    if is_error {
        // Long duration so the user has time to read what went wrong; default
        // toasts disappear quickly.
        toast = toast.duration(tauri_winrt_notification::Duration::Long);
    }
    if let Err(e) = toast.show() {
        log::debug!("toast failed ({title}): {e}");
    }
}
