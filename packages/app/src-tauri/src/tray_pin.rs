//! One-time promotion of the Windows tray icon out of the overflow flyout.
//!
//! Windows deliberately offers no supported API for this — icon visibility
//! is user-controlled. Windows 11 does expose the toggle the Settings UI
//! itself flips: `HKCU\Control Panel\NotifyIconSettings\<id>\IsPromoted`
//! (DWORD, 1 = always visible), where the subkey's `ExecutablePath` value
//! names the owning binary. Writes take effect immediately, no Explorer
//! restart. Undocumented, so treat failure as a soft no-op: a future
//! Windows build that drops the key simply leaves the icon in overflow.
//!
//! Policy is promote ONCE per install (marker file in the app config dir):
//! the first run lifts Trove onto the taskbar, but a user who afterwards
//! drags it back into the overflow keeps that choice — we never re-assert.
//! On Windows 10 the registry key family doesn't exist, so every attempt
//! no-ops without writing the marker (and would start working should the
//! machine ever upgrade to 11).

use std::time::Duration;

use tauri::{AppHandle, Manager};

/// Marker file (in the same dir as `state.json`) recording that promotion
/// already ran. Deliberately NOT part of state.json — adding a field there
/// bumps `schemaVersion`, which forces a full rebuild/migration for a flag
/// nothing else reads.
const MARKER_FILE: &str = "tray-pinned";

const NOTIFY_KEY: &str = r"Control Panel\NotifyIconSettings";

/// Number of 1 s polls waiting for Windows to materialise the
/// `NotifyIconSettings` subkey — it appears asynchronously some time after
/// `Shell_NotifyIcon` first registers the icon, typically well under a
/// second, but a cold first launch can be slower.
const ATTEMPTS: u32 = 10;

/// Spawns the promote-once task. Call after `tray::setup` so the icon is
/// registered (the registry subkey can't exist before that).
pub fn promote_once(app: &AppHandle) {
    let Ok(config_dir) = app.path().app_config_dir() else {
        return;
    };
    let marker = config_dir.join(MARKER_FILE);
    if marker.exists() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        for _ in 0..ATTEMPTS {
            match promote_current_exe() {
                Ok(true) => {
                    // Best-effort: a failed marker write just means one
                    // redundant (idempotent) promote on the next launch.
                    let _ = std::fs::write(&marker, b"");
                    return;
                }
                // Subkey not materialised yet — keep polling.
                Ok(false) => {}
                // Root key absent (Windows 10) or access denied: give up
                // quietly; retrying every launch is a few registry reads.
                Err(_) => return,
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

/// Sets `IsPromoted = 1` on every `NotifyIconSettings` subkey whose
/// `ExecutablePath` matches the running binary (each distinct icon UID
/// gets its own subkey). Returns `Ok(false)` when no subkey matched.
fn promote_current_exe() -> std::io::Result<bool> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;

    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy();
    let root = RegKey::predef(HKEY_CURRENT_USER).open_subkey(NOTIFY_KEY)?;

    let mut promoted = false;
    for name in root.enum_keys().flatten() {
        let Ok(sub) = root.open_subkey_with_flags(&name, KEY_READ | KEY_SET_VALUE) else {
            continue;
        };
        let Ok(path) = sub.get_value::<String, _>("ExecutablePath") else {
            continue;
        };
        // Registry paths come from the shell; compare case-insensitively
        // like NTFS does.
        if path.eq_ignore_ascii_case(&exe) {
            sub.set_value("IsPromoted", &1u32)?;
            promoted = true;
        }
    }
    Ok(promoted)
}
