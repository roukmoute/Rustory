use tauri::{AppHandle, State};

use crate::application::library;
use crate::domain::shared::AppError;
use crate::ipc::dto::LibraryOverviewDto;
use crate::AppState;

/// Read the current library overview.
///
/// Thin command: validates the managed local storage is reachable and reads
/// the persisted story projection from SQLite. All logic lives in the
/// application service.
#[tauri::command]
pub fn get_library_overview(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LibraryOverviewDto, AppError> {
    // A prior panic may have poisoned the mutex; recover the inner handle
    // so the next IPC call still works instead of crashing the whole
    // session. SQLite itself is already in a consistent state — only Rust's
    // Mutex wraps the handle, and poisoning here signals "observe with
    // care", not "unrecoverable".
    let db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    library::load_overview(&app, &db)
}
