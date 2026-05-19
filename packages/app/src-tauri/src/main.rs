// Prevent a console window from appearing on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKit2GTK's DMA-BUF renderer triggers EPROTO (Error 71) on some Wayland
    // compositors — the compositor rejects the zwp_linux_dmabuf_v1 buffer-sharing
    // protocol. Falling back to wl_shm transport works universally. Must be set
    // before Tauri initialises the GTK/WebKit runtime. Skipped if the user has
    // already set this env var (so WEBKIT_DISABLE_DMABUF_RENDERER=0 opts out).
    #[cfg(target_os = "linux")]
    {
        let on_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland");
        if on_wayland && std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    trove_app::run();
}
