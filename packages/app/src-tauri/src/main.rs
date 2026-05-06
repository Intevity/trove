// Prevent a console window from appearing on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    trove_app::run();
}
