use tauri::AppHandle;

use crate::domain::shared::AppError;
use crate::infrastructure::filesystem::ensure_app_data_dir;
use crate::ipc::dto::LibraryOverviewDto;

/// Application service for the `library` flow.
///
/// Confirms the managed local storage is reachable and returns the current
/// overview. Any storage failure bubbles up as a normalized [`AppError`] for
/// the UI to render.
pub fn load_overview(app: &AppHandle) -> Result<LibraryOverviewDto, AppError> {
    ensure_app_data_dir(app)?;
    Ok(LibraryOverviewDto::empty())
}
