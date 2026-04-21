use tauri::AppHandle;

use crate::application::library;
use crate::domain::shared::AppError;
use crate::ipc::dto::LibraryOverviewDto;

/// Read the current library overview.
///
/// Thin command: validates the managed local storage is reachable and returns
/// the current projection — delegates all logic to the application service.
#[tauri::command]
pub fn get_library_overview(app: AppHandle) -> Result<LibraryOverviewDto, AppError> {
    library::load_overview(&app)
}
