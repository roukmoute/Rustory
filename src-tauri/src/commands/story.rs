use tauri::{AppHandle, State};

use crate::application::story::{self, CreateStoryInput, UpdateStoryInput};
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

/// Bound the accepted story id length so a hostile or malformed payload
/// can never issue a multi-kilobyte SQL prepared-statement parameter.
/// UUIDv7 canonical form is 36 bytes; a generous ceiling leaves room for
/// future id schemes without becoming an attack surface.
const MAX_STORY_ID_LEN: usize = 256;

fn validate_story_id(raw: &str) -> Result<(), AppError> {
    if raw.is_empty() {
        return Err(AppError::library_inconsistent(
            "Histoire introuvable, recharge la bibliothèque.",
            "Retourne à la bibliothèque et recharge la liste.",
        )
        .with_details(serde_json::json!({
            "source": "story_id_invalid",
            "cause": "empty",
        })));
    }
    if raw.len() > MAX_STORY_ID_LEN {
        return Err(AppError::library_inconsistent(
            "Histoire introuvable, recharge la bibliothèque.",
            "Retourne à la bibliothèque et recharge la liste.",
        )
        .with_details(serde_json::json!({
            "source": "story_id_invalid",
            "cause": "too_long",
            "maxLen": MAX_STORY_ID_LEN,
        })));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;

    #[test]
    fn validate_story_id_accepts_a_canonical_uuid_v7() {
        assert!(validate_story_id("0197a5d0-0000-7000-8000-000000000000").is_ok());
    }

    #[test]
    fn validate_story_id_rejects_empty_string() {
        let err = validate_story_id("").expect_err("must reject empty");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "story_id_invalid");
        assert_eq!(details["cause"], "empty");
    }

    #[test]
    fn validate_story_id_rejects_oversize_string() {
        let huge = "a".repeat(MAX_STORY_ID_LEN + 1);
        let err = validate_story_id(&huge).expect_err("must reject oversize");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["cause"], "too_long");
        assert_eq!(details["maxLen"], MAX_STORY_ID_LEN);
    }
}
