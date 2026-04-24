use std::sync::Mutex;

use tauri::Manager;

pub mod application;
pub mod commands;
pub mod domain;
pub mod infrastructure;
pub mod ipc;

/// Application-wide state managed by Tauri. Holds the long-lived database
/// handle behind a synchronous mutex — SQLite serializes writers internally
/// and reads are short-lived enough that a fine-grained async pool would
/// be over-engineered for the current surface.
pub struct AppState {
    pub db: Mutex<infrastructure::db::DbHandle>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = infrastructure::filesystem::ensure_app_data_dir(app.handle())
                .map_err(|err| err.to_string())?;
            let db_path = infrastructure::filesystem::resolve_db_path(&app_data_dir);
            let mut db = infrastructure::db::open_at(&db_path).map_err(|err| err.to_string())?;
            infrastructure::db::run_migrations(&mut db).map_err(|err| err.to_string())?;
            app.manage(AppState { db: Mutex::new(db) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library::get_library_overview,
            commands::story::create_story,
            commands::story::get_story_detail,
            commands::story::update_story,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
