use tauri::{AppHandle, State};

use crate::application::story::{self, CreateStoryInput};
use crate::domain::shared::AppError;
use crate::ipc::dto::{CreateStoryInputDto, StoryCardDto};
use crate::AppState;

/// Create a new story draft and return its library card projection.
///
/// Thin command: locks the shared database handle, delegates to the
/// application service, and lets the normalized [`AppError`] bubble up
/// untouched.
#[tauri::command]
pub fn create_story(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: CreateStoryInputDto,
) -> Result<StoryCardDto, AppError> {
    // See `commands::library::get_library_overview` for the rationale on
    // mutex-poison recovery — the same reasoning applies here.
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    story::create_story(&mut db, CreateStoryInput { title: input.title })
}
