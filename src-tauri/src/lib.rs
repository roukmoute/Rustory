pub mod application;
pub mod commands;
pub mod domain;
pub mod infrastructure;
pub mod ipc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::library::get_library_overview
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
