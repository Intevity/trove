fn main() {
    // `option_env!("TROVE_BUILD_CHANNEL")` (lib.rs) is read at compile time
    // to decide whether the auto-updater runs. Without this line, Cargo
    // wouldn't recompile when the var flips between a `pnpm build:app` (dev)
    // and a release build on the same machine, baking in a stale channel.
    println!("cargo:rerun-if-env-changed=TROVE_BUILD_CHANNEL");
    tauri_build::build();
}
