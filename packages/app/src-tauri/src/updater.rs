//! Auto-update plumbing for Trove. Mirrors claude-sentinel's updater
//! module (Sentinel leads, Trove follows) minus Sentinel's silent
//! auto-install branch — Trove always asks via the in-app modal.
//!
//! Three entry points share the same check core:
//!   - `spawn_update_timer` — called from `lib.rs` setup; checks shortly
//!     after launch and then every `CHECK_INTERVAL` (4 h, overridable via
//!     `TROVE_UPDATE_CHECK_INTERVAL_SECS` for testing). Gated on the
//!     opt-in `auto_update_enabled` app-state flag: Trove never contacts
//!     the update channel in the background unless the user opted in.
//!   - `check_for_updates` — Tauri command invoked from both the tray's
//!     "Check for updates…" item and the Settings panel's explicit
//!     button. Always runs regardless of the opt-in flag and always
//!     surfaces feedback (panel text, notification, or the modal).
//!   - `install_update` — Tauri command invoked from the in-app update
//!     modal's Install button. Consumes the pending update stashed by a
//!     prior check, downloads + installs, then restarts.
//!
//! A found update is stashed in `PendingUpdate` managed state and an
//! `update_available` event is emitted; the frontend shows a modal with
//! an Install button. The main window is usually hidden (tray app), so a
//! timer-found update also fires one native notification per version and
//! the modal greets the user the next time they open the window. The
//! tray/Settings path shows the window immediately instead.
//!
//! The collector sidecar is a child of the app process, so exiting the
//! app terminates it; the new bundle's sidecar binary spawns fresh on
//! relaunch via the supervisor. No special coordination needed.
//!
//! On macOS the `.app` replacement step requires a signed + notarized
//! bundle; installs on unsigned builds fail at the Gatekeeper check.
//! That's one reason `auto_update_enabled` defaults to `false`.

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::ipc::IpcError;
use crate::ipc::commands::UpdateMetadata;

/// Default cadence for the background check loop.
const CHECK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);
/// First check after launch waits this long so startup (collector spawn,
/// watcher rehydration) settles first.
const INITIAL_DELAY: Duration = Duration::from_secs(120);

/// The update found by the most recent check, awaiting user consent via
/// the modal's Install button. Registered with `app.manage` in lib.rs.
pub struct PendingUpdate(pub Mutex<Option<Update>>);

/// Payload of the `update_available` event the frontend listens for.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAvailablePayload {
    version: String,
    current_version: String,
}

/// The opt-in background-check gate. Reads the persisted app state
/// fresh on every tick so a Settings toggle takes effect without a
/// relaunch. Unreadable state reads as opted-out.
fn auto_update_enabled(app: &AppHandle) -> bool {
    crate::app_state::load(app).is_ok_and(|s| s.auto_update_enabled)
}

/// Background-check cadence, overridable for testing
/// (`TROVE_UPDATE_CHECK_INTERVAL_SECS=60` makes the loop tick every
/// minute).
fn check_interval() -> Duration {
    std::env::var("TROVE_UPDATE_CHECK_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&secs| secs > 0)
        .map_or(CHECK_INTERVAL, Duration::from_secs)
}

fn notify(app: &AppHandle, body: String) {
    let _ = app
        .notification()
        .builder()
        .title("Trove")
        .body(body)
        .show();
}

/// Stash the found update for the modal's Install button and tell the
/// frontend. The modal renders whenever the window is (or becomes)
/// visible.
fn stash_and_emit(app: &AppHandle, update: Update) {
    let payload = UpdateAvailablePayload {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
    };
    let state = app.state::<PendingUpdate>();
    *state.0.lock().expect("pending update lock") = Some(update);
    let _ = app.emit("update_available", payload);
}

/// Called once at startup. First check after `INITIAL_DELAY`, then every
/// `check_interval()` forever. Check failures are silent (offline, S3
/// blip); the next tick retries.
pub fn spawn_update_timer(app: AppHandle) {
    // Dev builds (`pnpm build:app`) opt out of the auto-updater entirely:
    // they carry newer code than the public feed but an equal-or-lower
    // version string, so a later-numbered release would be offered as a
    // bogus "upgrade" that rolls code/state backwards. No timer, no nag.
    if crate::is_dev_build() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let interval = check_interval();
        // Track the last version we fired a notification for, so a
        // 4-hourly re-find of the same release doesn't nag every tick.
        let mut notified_version: Option<String> = None;
        tokio::time::sleep(std::cmp::min(interval, INITIAL_DELAY)).await;
        loop {
            scheduled_check(&app, &mut notified_version).await;
            tokio::time::sleep(interval).await;
        }
    });
}

/// One background tick: bail unless the user opted in, then check and
/// stage the modal + notify. No silent-install path — installing always
/// goes through the modal's Install button.
async fn scheduled_check(app: &AppHandle, notified_version: &mut Option<String>) {
    if !auto_update_enabled(app) {
        return;
    }
    let Ok(updater) = app.updater() else { return };
    // No update / check error: silent. The tray item exists for users
    // who want explicit feedback.
    let Ok(Some(update)) = updater.check().await else {
        return;
    };

    let version = update.version.clone();
    // Do NOT pop the window — the notification is the foreground signal;
    // the modal opens the next time the user shows the window.
    stash_and_emit(app, update);
    if notified_version.as_deref() != Some(version.as_str()) {
        notify(
            app,
            format!("Trove v{version} is available. Open Trove to install."),
        );
        *notified_version = Some(version);
    }
}

/// Explicit "check for updates now" probe, shared by the tray-menu item
/// and the Settings panel button. On a hit it brings the window forward
/// and raises the update modal (via `stash_and_emit`); otherwise it
/// fires a notification so a tray-initiated check always gets feedback.
/// The returned [`UpdateMetadata`] keeps the Settings panel's inline
/// status text working unchanged.
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateMetadata, IpcError> {
    // Dev builds never offer updates — see `spawn_update_timer`. Report
    // "no update" with a clear notification rather than handing the user a
    // downgrade-in-disguise from the public release feed.
    if crate::is_dev_build() {
        notify(
            &app,
            "You're on a dev build — auto-update is disabled.".to_string(),
        );
        return Ok(UpdateMetadata {
            available: false,
            version: None,
            current: crate::app_version().to_string(),
        });
    }
    let updater = app
        .updater()
        .map_err(|e| IpcError::UpdaterCheckFailed { reason: e.to_string() })?;
    match updater.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            stash_and_emit(&app, update);
            Ok(UpdateMetadata {
                available: true,
                version: Some(version),
                current: crate::app_version().to_string(),
            })
        }
        Ok(None) => {
            notify(&app, "You're on the latest version.".to_string());
            Ok(UpdateMetadata {
                available: false,
                version: None,
                current: crate::app_version().to_string(),
            })
        }
        Err(e) => {
            notify(&app, format!("Update check failed: {e}"));
            Err(IpcError::UpdaterCheckFailed { reason: e.to_string() })
        }
    }
}

/// Tauri command backing the update modal's Install button. Consumes the
/// pending update from the last check (re-checks as a fallback so a stale
/// webview can't strand the button), installs, and restarts.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    // Take the stash inside a block so the guard drops before any await.
    let pending = {
        let state = app.state::<PendingUpdate>();
        let taken = state.0.lock().expect("pending update lock").take();
        taken
    };
    let update = if let Some(update) = pending {
        update
    } else {
        let updater = app.updater().map_err(|e| e.to_string())?;
        updater
            .check()
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No update available.".to_string())?
    };
    // download_and_install takes two callbacks (progress + done). We
    // ignore both; the modal shows an indeterminate "Installing…" state
    // and the restart is the completion signal.
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}
