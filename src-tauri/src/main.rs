// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod appimage_runtime;

fn main() {
    #[cfg(target_os = "linux")]
    // SAFETY: this is the first application initialization, before GTK,
    // rustory_lib::run(), or any Rustory-created thread can read the environment.
    if let Err(error) = unsafe { appimage_runtime::initialize() } {
        eprintln!("Rustory: {error}");
        std::process::exit(1);
    }

    rustory_lib::run()
}
