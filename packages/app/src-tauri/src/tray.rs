//! Tray icon + menu setup.
//!
//! Sprint 0: left-click toggles the main window, right-click shows a
//! menu. Sprint 6 PR 2 layers on dynamic icon retinting (green / amber
//! / red) driven by `derive_overall_health` over the live supervisor
//! and metrics-tap watchers — exact same truth table the dashboard
//! badge uses. The icon is recomputed on every `CollectorState`
//! transition, every metrics-tap publication, and on a 30 s tick (so
//! green→amber on the staleness boundary fires even when no inputs
//! changed).

use std::sync::Mutex;
use std::time::Duration;

use tauri::async_runtime::JoinHandle;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, Wry};
use tokio::time::Instant;

use crate::collector::{
    MetricsTapHandle, OverallHealth, SupervisorChannels, derive_overall_health,
};
use crate::tray_icon_render::{TintColor, tinted};

/// Tray-side handle held in Tauri-managed state. Owns the live
/// `TrayIcon` plus the disabled status `MenuItem` so the background
/// retint task can update both. Wrapped in a Mutex because every field
/// is touched from a single async task; the lock is uncontended.
pub struct TrayHandle {
    icon: TrayIcon<Wry>,
    status_item: MenuItem<Wry>,
    /// Background retint task. Aborted on shutdown via Drop.
    _retint_task: JoinHandle<()>,
}

/// Tauri-managed slot for the tray. Held by `lib.rs` for the lifetime
/// of the app.
pub type TrayState = Mutex<Option<TrayHandle>>;

/// Builds the tray icon, attaches the menu, wires click handlers, and
/// spawns the background task that subscribes to live channels and
/// retints on transitions.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let status_item = MenuItem::with_id(
        app,
        "status",
        format_status_text(OverallHealth::Amber),
        false,
        None::<&str>,
    )?;
    let separator_a = PredefinedMenuItem::separator(app)?;
    let open_item = MenuItem::with_id(app, "open", "Open Trove", true, None::<&str>)?;
    let synthetic_item = MenuItem::with_id(
        app,
        "synthetic_test",
        "Run synthetic test export",
        true,
        None::<&str>,
    )?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Trove", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &separator_a,
            &open_item,
            &synthetic_item,
            &separator_b,
            &quit_item,
        ],
    )?;

    // Seed amber until the real channels publish — never display the
    // bare default-window-icon, which would lie about the colour
    // contract before the supervisor reports state.
    let initial = tinted(TintColor::Amber);
    let initial_image = tauri::image::Image::new(&initial.bytes, initial.width, initial.height);

    let icon = TrayIconBuilder::with_id("trove-tray")
        .icon(initial_image)
        .icon_as_template(false)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Trove")
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(handle_tray_event)
        .build(app)?;

    let retint_task = spawn_retint_task(app.clone(), icon.clone(), status_item.clone());

    let handle = TrayHandle {
        icon,
        status_item,
        _retint_task: retint_task,
    };
    app.manage::<TrayState>(Mutex::new(Some(handle)));

    Ok(())
}

/// Force the tray to a specific colour, bypassing derivation. Returns
/// the rendered colour so the caller (the dev-hatch IPC) can confirm
/// the call landed.
pub fn force_color<R: Runtime>(app: &AppHandle<R>, color: OverallHealth) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    let guard = state.lock().expect("tray state poisoned");
    if let Some(handle) = guard.as_ref() {
        apply_color(&handle.icon, &handle.status_item, color);
    }
}

#[allow(clippy::needless_pass_by_value)] // Tauri's on_menu_event callback takes MenuEvent by value.
fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id.as_ref() {
        "open" => show_main(app),
        "quit" => app.exit(0),
        "synthetic_test" => {
            // Surface the click as an event the WebView listens to (or
            // ignores). The dashboard's own "Test Pipeline" button
            // calls `test_export` directly via IPC; keeping this menu
            // item event-only avoids a Rust-side dependency on the
            // command's async runtime context from inside the Tauri
            // menu handler.
            let _ = app.emit("tray-synthetic-test-clicked", ());
        }
        _ => {}
    }
}

#[allow(clippy::needless_pass_by_value)] // Tauri's on_tray_icon_event callback takes TrayIconEvent by value.
fn handle_tray_event<R: Runtime>(tray: &tauri::tray::TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        toggle_main(tray.app_handle());
    }
}

fn toggle_main<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn show_main<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.show();
    let _ = window.set_focus();
}

/// Subscribe to the supervisor-state and metrics-tap watchers and
/// retint the tray icon on every transition. A 30 s tick triggers a
/// recompute even when nothing changed — the `derive_overall_health`
/// staleness window flips green→amber at 60 s, so without the tick a
/// quiet pipeline would be reported green forever.
fn spawn_retint_task(
    app: AppHandle,
    icon: TrayIcon<Wry>,
    status_item: MenuItem<Wry>,
) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let channels = app.state::<SupervisorChannels>().inner().clone();
        let metrics = app.state::<MetricsTapHandle>().inner().sender();
        let mut state_rx = channels.subscribe_state();
        let mut metrics_rx = metrics.subscribe();

        // Render the seed once so we don't sit on the amber default
        // until the first input arrives.
        recompute_and_apply(&icon, &status_item, &state_rx, &metrics_rx);

        loop {
            tokio::select! {
                state_changed = state_rx.changed() => {
                    if state_changed.is_err() { return; }
                    recompute_and_apply(&icon, &status_item, &state_rx, &metrics_rx);
                }
                metrics_changed = metrics_rx.changed() => {
                    if metrics_changed.is_err() { return; }
                    recompute_and_apply(&icon, &status_item, &state_rx, &metrics_rx);
                }
                () = tokio::time::sleep(Duration::from_secs(30)) => {
                    recompute_and_apply(&icon, &status_item, &state_rx, &metrics_rx);
                }
            }
        }
    })
}

fn recompute_and_apply(
    icon: &TrayIcon<Wry>,
    status_item: &MenuItem<Wry>,
    state_rx: &tokio::sync::watch::Receiver<crate::collector::CollectorState>,
    metrics_rx: &tokio::sync::watch::Receiver<Option<crate::collector::MetricsSnapshot>>,
) {
    let state = state_rx.borrow().clone();
    let snapshot = metrics_rx.borrow().clone();
    // PR 1 deferred per-harness "last signal" so the count of enabled
    // harnesses is irrelevant to the colour decision. Pass 0 — the
    // function then skips the staleness-window branch and just routes
    // on Running + reachable. PR 3 will revisit this when the
    // dashboard surfaces enabled-harness counts.
    let color = derive_overall_health(&state, snapshot.as_ref(), 0, Instant::now());
    apply_color(icon, status_item, color);
}

fn apply_color(icon: &TrayIcon<Wry>, status_item: &MenuItem<Wry>, color: OverallHealth) {
    let tint = derive_tint_color(color);
    let buf = tinted(tint);
    let img = tauri::image::Image::new(&buf.bytes, buf.width, buf.height);
    let _ = icon.set_icon(Some(img));
    let _ = status_item.set_text(format_status_text(color));
}

/// Map the cross-language `OverallHealth` to the tray-only
/// `TintColor` enum. Kept tiny and mechanically tested rather than
/// inlined so PR 3's dashboard parity test can lock the mapping down.
#[must_use]
pub fn derive_tint_color(color: OverallHealth) -> TintColor {
    match color {
        // Healthy → brand teal so the tray carries the Trove mark at
        // rest. Amber/red still use the health palette so problems are
        // visually distinct at-a-glance.
        OverallHealth::Green => TintColor::Brand,
        OverallHealth::Amber => TintColor::Amber,
        OverallHealth::Red => TintColor::Red,
    }
}

fn format_status_text(color: OverallHealth) -> String {
    match color {
        OverallHealth::Green => "Trove · Healthy".into(),
        OverallHealth::Amber => "Trove · Awaiting telemetry".into(),
        OverallHealth::Red => "Trove · Sidecar down".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_tint_color_maps_each_variant() {
        assert_eq!(derive_tint_color(OverallHealth::Green), TintColor::Brand);
        assert_eq!(derive_tint_color(OverallHealth::Amber), TintColor::Amber);
        assert_eq!(derive_tint_color(OverallHealth::Red), TintColor::Red);
    }

    #[test]
    fn status_text_is_distinct_per_state() {
        let g = format_status_text(OverallHealth::Green);
        let a = format_status_text(OverallHealth::Amber);
        let r = format_status_text(OverallHealth::Red);
        assert_ne!(g, a);
        assert_ne!(a, r);
        assert_ne!(g, r);
        assert!(g.contains("Healthy"));
        assert!(a.contains("Awaiting"));
        assert!(r.contains("down"));
    }
}
