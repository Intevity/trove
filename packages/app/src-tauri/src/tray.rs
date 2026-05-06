//! Tray icon + menu setup.
//!
//! Sprint 0: left-click toggles the main window, right-click shows a menu
//! with "Open Trove" and "Quit Trove". Sprint 6 layers on dynamic icon
//! retinting (green/amber/red) based on Collector and harness health.

use tauri::menu::{Menu, MenuEvent, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

/// Builds the tray icon, attaches the menu, and wires click handlers.
pub fn setup<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(app, "open", "Open Trove", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Trove", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &quit_item])?;

    TrayIconBuilder::with_id("trove-tray")
        .icon(app.default_window_icon().expect("default icon").clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(handle_tray_event)
        .build(app)?;

    Ok(())
}

#[allow(clippy::needless_pass_by_value)] // Tauri's on_menu_event callback takes MenuEvent by value.
fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id.as_ref() {
        "open" => show_main(app),
        "quit" => app.exit(0),
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
