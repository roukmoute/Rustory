use tauri::{async_runtime, AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::application::story::node::{
    self, RecordNodeDraftInput, RemoveNodeMediaInput, SaveNodeContentInput,
};
use crate::application::story::recovery::{
    self, ApplyRecoveryInput as RecoveryApplyInput, RecordDraftInput,
};
use crate::application::story::structure::{self, MoveDirection};
use crate::application::story::{self, CreateStoryInput, UpdateStoryInput};
use crate::commands::shared::{base64_encode, parse_media_slot, validate_story_id};
use crate::domain::shared::AppError;
use crate::domain::story::RecoveryDraftDelta;
use crate::infrastructure::diagnostics::recovery_log;
use crate::infrastructure::filesystem::{resolve_node_media_dir, MediaKind, MAX_MEDIA_BYTES};
use crate::ipc::dto::{
    AddNodeOptionInputDto, AddStoryNodeInputDto, ApplyRecoveryInputDto, AttachNodeMediaOutcomeDto,
    CreateStoryInputDto, DeleteStoryNodeInputDto, DiscardDraftInputDto, DiscardNodeDraftInputDto,
    MoveDirectionDto, MoveStoryNodeInputDto, NodeMediaPreviewDto, NodeMediaSlotInputDto,
    NodeWriteOutputDto, RecordDraftInputDto, RecordNodeDraftInputDto, RecoverableDraftDto,
    RecoverableNodeDraftDto, RemoveNodeOptionInputDto, SetNodeOptionLinkInputDto, StoryCardDto,
    StoryDetailDto, StructureWriteOutputDto, UpdateNodeContentInputDto, UpdateStoryInputDto,
    UpdateStoryOutputDto,
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
///
/// `node_id` is optional and targets the selected node's content projection
/// (`None` = the start node) — the invoke stays backward-compatible for
/// callers that omit it.
#[tauri::command]
pub fn get_story_detail(
    app: AppHandle,
    state: State<'_, AppState>,
    story_id: String,
    node_id: Option<String>,
) -> Result<Option<StoryDetailDto>, AppError> {
    validate_story_id(&story_id)?;
    let app_data_dir = resolve_app_data_dir(&app)?;
    let db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    story::get_story_detail(&db, &app_data_dir, &story_id, node_id.as_deref())
}

/// Persist a buffered keystroke value for a story (recovery flow).
///
/// Best-effort logging: a successful record is intentionally NOT logged
/// (one event per keystroke would drown the log). Only failures emit a
/// `recovery_draft_unavailable` event with the operation source so
/// support can correlate.
#[tauri::command]
pub fn record_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    input: RecordDraftInputDto,
) -> Result<(), AppError> {
    validate_story_id(&input.story_id)?;
    let story_id_for_log = input.story_id.clone();
    let outcome = {
        let mut db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recovery::record_draft(
            &mut db,
            RecordDraftInput {
                story_id: input.story_id,
                draft_title: input.draft_title,
            },
        )
    };
    if outcome.is_err() {
        let _ = recovery_log::record_event(
            &app,
            recovery_log::Event::RecoveryDraftUnavailable {
                story_id: story_id_for_log,
                source: "record_draft",
            },
        );
    }
    outcome
}

/// Read the buffered draft for a story and combine it with the persisted
/// title to produce a UI-ready outcome.
///
/// `RecoverableDraftDto::None` covers three cases that the UI treats
/// identically: no draft exists, the draft already matches the persisted
/// title (silently consumed), or the parent story has disappeared.
#[tauri::command]
pub fn read_recoverable_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    story_id: String,
) -> Result<RecoverableDraftDto, AppError> {
    validate_story_id(&story_id)?;
    let app_data_dir = resolve_app_data_dir(&app)?;

    // We hold the SQLite lock continuously across the two reads AND
    // the AlreadyPersisted cleanup DELETE. The previous shape released
    // the read-lock and re-acquired a fresh write-lock for the DELETE,
    // which opened a race window where a concurrent `record_draft`
    // could refresh the row between read and DELETE. The CAS-on-`draft_at`
    // already protects the row state, but holding the lock once also
    // closes the diagnostic-log inconsistency window where the
    // `recovery_draft_proposed` event could log a draft that was
    // already gone. The `recovery_log::record_event` calls happen AFTER
    // the lock is released, so a slow disk for the diag write never
    // blocks SQLite writers.
    enum ReadOutcome {
        None,
        AlreadyPersisted,
        AlreadyPersistedCleanupFailed(AppError),
        Recoverable {
            story_id: String,
            draft_title: String,
            draft_at: String,
            persisted_title: String,
        },
    }

    let outcome: Result<ReadOutcome, AppError> = (|| {
        let mut db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let detail = story::get_story_detail(&db, &app_data_dir, &story_id, None)?;
        let draft = recovery::read_recoverable_draft(&db, &story_id)?;
        Ok(match (draft, detail) {
            (Some(d), Some(detail)) => {
                match RecoveryDraftDelta::classify(&detail.title, &d.draft_title) {
                    RecoveryDraftDelta::AlreadyPersisted => {
                        // CAS on `draft_at` so a concurrent record_draft
                        // that just refreshed the row survives.
                        match recovery::discard_draft(&mut db, &story_id, Some(&d.draft_at)) {
                            Ok(()) => ReadOutcome::AlreadyPersisted,
                            Err(err) => ReadOutcome::AlreadyPersistedCleanupFailed(err),
                        }
                    }
                    RecoveryDraftDelta::Recoverable {
                        persisted_title,
                        draft_title,
                    } => ReadOutcome::Recoverable {
                        story_id: story_id.clone(),
                        draft_title,
                        draft_at: d.draft_at,
                        persisted_title,
                    },
                }
            }
            // FK CASCADE makes a draft without a parent story
            // essentially impossible, but if it ever happens
            // (FK off, hand-crafted DB), the missing parent dominates.
            _ => ReadOutcome::None,
        })
    })();

    match outcome {
        Ok(ReadOutcome::None | ReadOutcome::AlreadyPersisted) => Ok(RecoverableDraftDto::None),
        Ok(ReadOutcome::AlreadyPersistedCleanupFailed(err)) => {
            // The draft was already-persisted but we failed to
            // consume the row. The user-visible state is correct
            // (no banner) but the row will be re-evaluated next mount,
            // which is a buffered keystroke leaking past the autosave
            // confirm — log the failure so support sees it.
            let _ = recovery_log::record_event(
                &app,
                recovery_log::Event::RecoveryDraftUnavailable {
                    story_id: story_id.clone(),
                    source: "read_recoverable_draft_cleanup",
                },
            );
            // The user is still better served with `None` than with
            // an error: the persisted title is the truth, the buffer
            // mismatch is a janitor concern, not a UX-visible failure.
            // We swallow the underlying error code but log it.
            let _ = err;
            Ok(RecoverableDraftDto::None)
        }
        Ok(ReadOutcome::Recoverable {
            story_id: sid,
            draft_title,
            draft_at,
            persisted_title,
        }) => {
            let _ = recovery_log::record_event(
                &app,
                recovery_log::Event::RecoveryDraftProposed {
                    story_id: sid.clone(),
                },
            );
            Ok(RecoverableDraftDto::Recoverable {
                story_id: sid,
                draft_title,
                draft_at,
                persisted_title,
            })
        }
        Err(err) => {
            // Failure on either get_story_detail or read_recoverable_draft.
            // P39/D4: wrap upstream LOCAL_STORAGE_UNAVAILABLE into a
            // RECOVERY_DRAFT_UNAVAILABLE that preserves the original
            // diagnostic discriminants in `details.cause` so support
            // can pinpoint the failure source.
            let wrapped = wrap_recovery_read_error(err, &story_id);
            let _ = recovery_log::record_event(
                &app,
                recovery_log::Event::RecoveryDraftUnavailable {
                    story_id: story_id.clone(),
                    source: "read_recoverable_draft",
                },
            );
            Err(wrapped)
        }
    }
}

/// Wrap an upstream `AppError` from `get_story_detail` /
/// `read_recoverable_draft` into a recovery-flow-tagged
/// `RECOVERY_DRAFT_UNAVAILABLE`. The original error code, source, and
/// details survive under `details.cause` so support keeps the
/// triage signal.
fn wrap_recovery_read_error(upstream: AppError, story_id: &str) -> AppError {
    let cause_code = serde_json::to_value(&upstream.code)
        .ok()
        .unwrap_or(serde_json::Value::Null);
    let cause_details = upstream.details.clone().unwrap_or(serde_json::Value::Null);
    AppError::recovery_draft_unavailable(
        "Récupération indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "read_recoverable_draft",
        "stage": "get_story_detail_or_read_draft",
        "id": story_id,
        "cause": {
            "code": cause_code,
            "details": cause_details,
        },
    }))
}

/// Apply the buffered draft authoritatively: re-validate, UPDATE the
/// canonical row, consume the draft — all in a single transaction.
#[tauri::command]
pub fn apply_recovery(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ApplyRecoveryInputDto,
) -> Result<UpdateStoryOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let story_id_for_log = input.story_id.clone();
    let outcome = {
        let mut db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recovery::apply_recovery(
            &mut db,
            RecoveryApplyInput {
                story_id: input.story_id,
            },
        )
    };
    let event = if outcome.is_ok() {
        recovery_log::Event::RecoveryDraftApplied {
            story_id: story_id_for_log,
        }
    } else {
        recovery_log::Event::RecoveryDraftUnavailable {
            story_id: story_id_for_log,
            source: "apply_recovery",
        }
    };
    let _ = recovery_log::record_event(&app, event);
    outcome
}

/// Drop the buffered draft without modifying canonical state. Idempotent
/// by design — a second call on an already-empty row is a silent no-op.
///
/// The optional `expected_draft_at` is forwarded as a compare-and-swap
/// guard: when the UI passes the timestamp it observed, a concurrent
/// `record_draft` that refreshed the row between observation and click
/// is preserved. When absent (auto-discard from `useStoryEditor` when
/// the user types back to the persisted value), the DELETE runs
/// unconditionally because the caller does not know which `draft_at`
/// to expect — the trade-off is acceptable on that path because the
/// canonical row already matches the persisted truth, so dropping
/// any buffered value is correct.
#[tauri::command]
pub fn discard_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    input: DiscardDraftInputDto,
) -> Result<(), AppError> {
    validate_story_id(&input.story_id)?;
    let story_id_for_log = input.story_id.clone();
    let outcome = {
        let mut db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recovery::discard_draft(&mut db, &input.story_id, input.expected_draft_at.as_deref())
    };
    let event = if outcome.is_ok() {
        recovery_log::Event::RecoveryDraftDiscarded {
            story_id: story_id_for_log,
        }
    } else {
        recovery_log::Event::RecoveryDraftUnavailable {
            story_id: story_id_for_log,
            source: "discard_draft",
        }
    };
    let _ = recovery_log::record_event(&app, event);
    outcome
}

// ===========================================================================
// Node content + media (schema v2)
// ===========================================================================

/// Write the current node's text + metadata label. Thin command: validates the
/// story id, locks the DB, delegates to the node service (which re-serializes
/// `structure_json` and recomputes `content_checksum` in a `BEGIN IMMEDIATE`
/// transaction), and returns the re-projected node.
#[tauri::command]
pub fn update_node_content(
    app: AppHandle,
    state: State<'_, AppState>,
    input: UpdateNodeContentInputDto,
) -> Result<NodeWriteOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let app_data_dir = resolve_app_data_dir(&app)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    node::save_node_content(
        &mut db,
        &app_data_dir,
        SaveNodeContentInput {
            story_id: input.story_id,
            node_id: input.node_id,
            text: input.text,
            label: input.label,
        },
    )
}

/// Attach a source media file to the current node's image / audio slot. Opens
/// a NON-BLOCKING native file picker (the GTK dialog must run on the main
/// thread), validates the chosen file by magic bytes, stores its bytes under
/// the node-media store, and writes the node's asset reference. A cancelled
/// dialog resolves with `{ kind: "cancelled" }`.
#[tauri::command]
pub async fn attach_node_media(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NodeMediaSlotInputDto,
) -> Result<AttachNodeMediaOutcomeDto, AppError> {
    validate_story_id(&input.story_id)?;
    let kind = parse_media_slot(&input.slot)?;

    let app_data_dir = resolve_app_data_dir(&app)?;

    let (filter_name, exts): (&str, &[&str]) = match kind {
        MediaKind::Image => ("Image (PNG, JPEG)", &["png", "jpg", "jpeg"]),
        MediaKind::Audio => ("Audio (MP3, WAV, OGG)", &["mp3", "wav", "ogg"]),
    };
    let (tx, mut rx) = async_runtime::channel::<Option<FilePath>>(1);
    app.dialog()
        .file()
        .add_filter(filter_name, exts)
        .pick_file(move |path| {
            let _ = tx.try_send(path);
        });

    let picked = match rx.recv().await {
        Some(inner) => inner,
        None => return Err(node_media_transport("dialog_failed")),
    };
    let Some(file_path) = picked else {
        return Ok(AttachNodeMediaOutcomeDto::Cancelled);
    };
    let path = file_path
        .as_path()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| {
            AppError::media_invalid(
                "Chemin de fichier invalide.",
                "Choisis un fichier local classique puis réessaie.",
            )
            .with_details(
                serde_json::json!({ "source": "media_invalid", "stage": "non_filesystem_path" }),
            )
        })?;

    let db = state.db.clone();
    let story_id = input.story_id.clone();
    let node_id = input.node_id.clone();
    let output = async_runtime::spawn_blocking(move || -> Result<NodeWriteOutputDto, AppError> {
        // Open ONCE, verify a regular file, and read with an EFFECTIVE bound so a
        // file that grows between a stat and a read — or a special path with
        // deceptive metadata — can never load more than `MAX_MEDIA_BYTES`.
        use std::io::Read;
        let file = std::fs::File::open(&path).map_err(|_| node_media_read_error())?;
        let is_regular = file.metadata().map(|m| m.is_file()).unwrap_or(false);
        if !is_regular {
            return Err(node_media_read_error());
        }
        let mut bytes = Vec::new();
        file.take(MAX_MEDIA_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| node_media_read_error())?;
        if bytes.len() > MAX_MEDIA_BYTES {
            return Err(AppError::media_invalid(
                "Ce média dépasse la taille autorisée.",
                "Choisis un fichier plus léger puis réessaie.",
            )
            .with_details(serde_json::json!({ "source": "media_invalid", "stage": "oversize" })));
        }
        // Validate + promote the file OFF the DB lock (a multi-MiB write must
        // not serialise every other IPC command — NFR5), then take the lock
        // only for the (fast) transaction.
        let prepared = node::store_node_media(&app_data_dir, kind, &bytes)?;
        let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        node::commit_node_media(&mut guard, prepared, &story_id, &node_id, kind)
    })
    .await
    .map_err(|_| node_media_transport("spawn_blocking_join"))??;

    Ok(AttachNodeMediaOutcomeDto::Attached {
        output: Box::new(output),
    })
}

/// Remove the media from a node's image / audio slot.
#[tauri::command]
pub fn remove_node_media(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NodeMediaSlotInputDto,
) -> Result<NodeWriteOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let kind = parse_media_slot(&input.slot)?;
    let app_data_dir = resolve_app_data_dir(&app)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    node::remove_node_media(
        &mut db,
        &app_data_dir,
        RemoveNodeMediaInput {
            story_id: input.story_id,
            node_id: input.node_id,
            kind,
        },
    )
}

/// Read a node media's bytes for a preview, returned as a self-contained
/// `data:` URL (the frontend never owns the raw bytes). Runs on a blocking
/// worker — the DB lookup + bounded file read + base64 must stay off the async
/// runtime.
#[tauri::command]
pub async fn read_node_media(
    app: AppHandle,
    state: State<'_, AppState>,
    story_id: String,
    asset_id: String,
) -> Result<NodeMediaPreviewDto, AppError> {
    validate_story_id(&story_id)?;
    let app_data_dir = resolve_app_data_dir(&app)?;
    let media_dir = resolve_node_media_dir(&app_data_dir);
    let db = state.db.clone();
    async_runtime::spawn_blocking(move || -> Result<NodeMediaPreviewDto, AppError> {
        // Resolve the stored file name UNDER the lock (a short DB read), then
        // release it BEFORE the file read + base64 so a large preview never
        // serialises every other IPC command (NFR5).
        let file_name = {
            let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            node::resolve_node_media_file(&guard, &story_id, &asset_id)?
        };
        let (bytes, mime) = node::read_node_media_file(&media_dir, &file_name)?;
        Ok(NodeMediaPreviewDto {
            data_url: format!("data:{mime};base64,{}", base64_encode(&bytes)),
        })
    })
    .await
    .map_err(|_| node_media_transport("spawn_blocking_join"))?
}

/// Buffer the in-progress node text + label (NFR8 recovery). Best-effort like
/// `record_draft`: callers proceed on failure, the autosave is the durable
/// mechanism.
#[tauri::command]
pub fn record_node_draft(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: RecordNodeDraftInputDto,
) -> Result<(), AppError> {
    validate_story_id(&input.story_id)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    node::record_node_draft(
        &mut db,
        RecordNodeDraftInput {
            story_id: input.story_id,
            node_id: input.node_id,
            draft_text: input.draft_text,
            draft_label: input.draft_label,
        },
    )
}

/// Read the recoverable node draft for a story. Resolves with a tagged union —
/// `kind: "none"` is informational. A buffer that already matches the persisted
/// node is silently consumed (CAS on `draft_at`) and reported as `none`.
#[tauri::command]
pub fn read_recoverable_node_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    story_id: String,
) -> Result<RecoverableNodeDraftDto, AppError> {
    validate_story_id(&story_id)?;
    let app_data_dir = resolve_app_data_dir(&app)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let Some(draft) = node::read_node_draft(&db, &story_id)? else {
        return Ok(RecoverableNodeDraftDto::None);
    };
    // Compare against the persisted node (projected from Rust), TARGETED by
    // the draft's own node id — the untargeted projection would return the
    // START node and silently kill recovery for any other node of the graph.
    // A missing story / unprojectable structure / vanished node (the graceful
    // fallback projects the start node, which the id filter then rejects)
    // means there is nothing to recover against — report `none`.
    let persisted = story::get_story_detail(&db, &app_data_dir, &story_id, Some(&draft.node_id))?
        .and_then(|d| d.node)
        .filter(|n| n.id == draft.node_id);
    let Some(persisted) = persisted else {
        return Ok(RecoverableNodeDraftDto::None);
    };
    if draft.draft_text == persisted.text && draft.draft_label == persisted.label {
        // Already persisted — consume the row (CAS) and surface nothing.
        let _ = node::discard_node_draft(&mut db, &story_id, Some(&draft.draft_at));
        return Ok(RecoverableNodeDraftDto::None);
    }
    Ok(RecoverableNodeDraftDto::Recoverable {
        story_id,
        node_id: draft.node_id,
        draft_text: draft.draft_text,
        draft_label: draft.draft_label,
        draft_at: draft.draft_at,
        persisted_text: persisted.text,
        persisted_label: persisted.label,
    })
}

/// Drop the buffered node draft without modifying canonical state. Idempotent.
#[tauri::command]
pub fn discard_node_draft(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: DiscardNodeDraftInputDto,
) -> Result<(), AppError> {
    validate_story_id(&input.story_id)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    node::discard_node_draft(&mut db, &input.story_id, input.expected_draft_at.as_deref())
}

/// Append a new empty node to the story's structure; with `linkFrom`, link
/// the referenced option to the new node in the same transaction.
#[tauri::command]
pub fn add_story_node(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: AddStoryNodeInputDto,
) -> Result<StructureWriteOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let link_from = input
        .link_from
        .as_ref()
        .map(|l| (l.node_id.as_str(), l.option_index));
    structure::add_node(&mut db, &input.story_id, link_from)
}

/// Delete a node from the story's structure (the start node is refused).
#[tauri::command]
pub fn delete_story_node(
    app: AppHandle,
    state: State<'_, AppState>,
    input: DeleteStoryNodeInputDto,
) -> Result<StructureWriteOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let app_data_dir = resolve_app_data_dir(&app)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    structure::delete_node(&mut db, &app_data_dir, &input.story_id, &input.node_id)
}

/// Swap a node with its neighbor in the display order.
#[tauri::command]
pub fn move_story_node(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: MoveStoryNodeInputDto,
) -> Result<StructureWriteOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let direction = match input.direction {
        MoveDirectionDto::Up => MoveDirection::Up,
        MoveDirectionDto::Down => MoveDirection::Down,
    };
    structure::move_node(&mut db, &input.story_id, &input.node_id, direction)
}

/// Add an option (its label typed at creation) to a node.
#[tauri::command]
pub fn add_node_option(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: AddNodeOptionInputDto,
) -> Result<StructureWriteOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    structure::add_option(&mut db, &input.story_id, &input.node_id, &input.label)
}

/// Set an option's destination (`target` = an existing node id) or unlink it
/// (`target` = null). A missing destination is refused, never written.
#[tauri::command]
pub fn set_node_option_link(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: SetNodeOptionLinkInputDto,
) -> Result<StructureWriteOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    structure::set_option_link(
        &mut db,
        &input.story_id,
        &input.node_id,
        input.option_index,
        input.target.as_deref(),
    )
}

/// Remove an option from a node.
#[tauri::command]
pub fn remove_node_option(
    _app: AppHandle,
    state: State<'_, AppState>,
    input: RemoveNodeOptionInputDto,
) -> Result<StructureWriteOutputDto, AppError> {
    validate_story_id(&input.story_id)?;
    let mut db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    structure::remove_option(&mut db, &input.story_id, &input.node_id, input.option_index)
}

/// Resolve the Tauri `app_data_dir`, mapping a failure to a PII-free
/// `LOCAL_STORAGE_UNAVAILABLE`. Shared by every story command that needs the
/// node-media store location to project a node's media slots.
fn resolve_app_data_dir(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    app.path().app_data_dir().map_err(|_| {
        AppError::local_storage_unavailable(
            "Rustory n'a pas pu localiser son dossier de données.",
            "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
        )
        .with_details(serde_json::json!({ "source": "app_data_unavailable" }))
    })
}

/// Media-store transport failure (`MEDIA_PROCESSING_FAILED`) with a closed
/// `details.stage`. Reserved for I/O of the media store, the dialog, or the
/// blocking worker — never a rejection of the file's content.
fn node_media_transport(stage: &'static str) -> AppError {
    AppError::media_processing_failed(
        "Média indisponible: le stockage local a échoué.",
        "Réessaie dans un instant ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({ "source": "media_processing_failed", "stage": stage }))
}

/// The chosen media file could not be read from disk (permission, vanished,
/// truncated). A transport failure of the media store, not a content rejection.
fn node_media_read_error() -> AppError {
    node_media_transport("read")
}
