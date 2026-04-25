use tauri::{AppHandle, State};

use crate::application::story::{self, CreateStoryInput, UpdateStoryInput};
use crate::commands::shared::validate_story_id;
use crate::domain::shared::AppError;
use crate::ipc::dto::{
    CreateStoryInputDto, StoryCardDto, StoryDetailDto, UpdateStoryInputDto, UpdateStoryOutputDto,
};
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

/// Update an existing story's metadata and return the freshly persisted
/// values so the UI can reconcile its draft with the source of truth.
#[tauri::command]
pub fn update_story(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: UpdateStoryInputDto,
) -> Result<UpdateStoryOutputDto, AppError> {
    validate_story_id(&input.id)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    story::update_story(
        &mut db,
        UpdateStoryInput {
            id: input.id,
            title: input.title,
        },
    )
}

/// Read a single story detail by id for the edit surface. Returns `null`
/// when the row is missing — the UI treats that case as "Histoire
/// introuvable" without needing to parse an error.
#[tauri::command]
pub fn get_story_detail(
    _app: AppHandle,
    state: State<'_, AppState>,
    story_id: String,
) -> Result<Option<StoryDetailDto>, AppError> {
    validate_story_id(&story_id)?;
    let db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    story::get_story_detail(&db, &story_id)
}
