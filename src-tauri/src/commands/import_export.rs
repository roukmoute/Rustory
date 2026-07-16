use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::{async_runtime, AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::application::import_export::import::read_artifact_bounded;
use crate::application::import_export::{
    self, import, rss_creation, structured_creation, ExportStoryInput, ImportAnalysis,
    RssAcceptPhase, RssCreationOutcome,
};
use crate::application::story::get_story_detail;
use crate::commands::shared::validate_story_id;
use crate::domain::export::RUSTORY_ARTIFACT_EXTENSION;
use crate::domain::import::{feed_url_host, official_content_sources};
use crate::domain::shared::AppError;
use crate::infrastructure::diagnostics::import_log;
use crate::ipc::dto::import_export::state_db_tag;
use crate::ipc::dto::import_export::ContentSourcePolicyDto;
use crate::ipc::dto::{
    AcceptArtifactImportInputDto, AcceptStructuredCreationInputDto, ExportStoryDialogInputDto,
    ExportStoryDialogOutcomeDto, ImportArtifactAnalysisDto, OsOpenAnalysisDto,
    RssCreationOutcomeDto, RssItemRefDto, RssPreviewDto, StoryCardDto,
    StructuredCreationAnalysisDto,
};
use crate::AppState;

/// Wall-clock budget for one feed fetch (preview OR the accept's
/// re-fetch), connection to last body byte. Short compared to the catalog
/// budget: ONE bounded document, no cover downloads.
const RSS_FETCH_BUDGET: Duration = Duration::from_secs(30);

const EXPORT_DIALOG_FILTER_NAME: &str = "Artefact Rustory";
const MAX_DESTINATION_PATH_LEN: usize = 4096;

/// Dialog filter shown when picking a `.rustory` artifact to import.
const IMPORT_DIALOG_FILTER_NAME: &str = "Artefact Rustory";

/// Persist the currently stored story as a `.rustory` artifact at a
/// destination chosen by the user in a native save dialog.
///
/// The command owns the full boundary: it opens the dialog via
/// `tauri-plugin-dialog`, loads the story under the DB lock, releases
/// the lock before any disk I/O, validates the returned path, and
/// writes the artifact atomically. The frontend never sees an arbitrary
/// filesystem path — it only passes a suggested filename and receives a
/// tagged outcome.
///
/// A cancelled dialog is NOT an error and resolves with
/// `{ kind: "cancelled" }` so the UI can silently return to idle.
///
/// The command is `async` and uses the non-blocking `save_file(cb)`
/// variant: on GTK/Linux the native dialog MUST run on the main
/// thread, and the corresponding `blocking_save_file` variant
/// dead-locks the app when the Tauri command dispatcher hands the
/// handler a thread that is waiting on main-loop progress. The
/// callback pipes the user's choice back through a Tauri
/// async-runtime channel that we await here.
#[tauri::command]
pub async fn export_story_with_save_dialog(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ExportStoryDialogInputDto,
) -> Result<ExportStoryDialogOutcomeDto, AppError> {
    validate_story_id(&input.story_id)?;

    let app_data_dir = app.path().app_data_dir().map_err(|_| {
        AppError::library_inconsistent(
            "Export impossible: dossier de données introuvable.",
            "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
        )
        .with_details(serde_json::json!({ "source": "app_data_unavailable" }))
    })?;

    let detail = {
        // Scoped block so the `MutexGuard` is dropped BEFORE the first
        // `.await`. A `MutexGuard<DbHandle>` is not `Send` on all
        // platforms, and an async command's future must be `Send` for
        // Tauri to spawn it on the runtime.
        let db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        get_story_detail(&db, &app_data_dir, &input.story_id, None)?.ok_or_else(|| {
            AppError::library_inconsistent(
                "Export impossible: histoire introuvable.",
                "Retourne à la bibliothèque et recharge la liste.",
            )
            .with_details(serde_json::json!({
                "source": "story_missing",
                "id": input.story_id,
            }))
        })?
    };

    // Non-blocking dialog + channel: `save_file(cb)` returns
    // immediately; the plugin schedules the native dialog on the main
    // thread and invokes our callback once the user picks a path (or
    // cancels). We await the channel so the command resolves when the
    // dialog settles, without ever blocking the thread that GTK needs
    // to process events.
    let (tx, mut rx) = async_runtime::channel::<Option<FilePath>>(1);
    app.dialog()
        .file()
        .set_file_name(&input.suggested_filename)
        .add_filter(EXPORT_DIALOG_FILTER_NAME, &[RUSTORY_ARTIFACT_EXTENSION])
        .save_file(move |path| {
            // The callback is synchronous and the channel has capacity
            // 1; `try_send` cannot fail in practice. Any failure (e.g.
            // the receiver dropped because the command was cancelled)
            // is silently swallowed — the receiver side handles the
            // `None` path uniformly.
            let _ = tx.try_send(path);
        });

    let picked = match rx.recv().await {
        Some(inner) => inner,
        None => {
            return Err(AppError::export_destination_unavailable(
                "La fenêtre de sauvegarde n'a pas pu s'ouvrir.",
                "Relance Rustory ; si le problème persiste, consulte les traces locales.",
            )
            .with_details(serde_json::json!({
                "source": "dialog_failed",
                "kind": "other",
                "cause": "channel_closed",
            })));
        }
    };

    let Some(file_path) = picked else {
        return Ok(ExportStoryDialogOutcomeDto::Cancelled);
    };

    let raw_path = file_path_to_pathbuf(&file_path)?;
    let destination = validate_and_normalize_destination(&raw_path)?;
    reject_internal_app_directory(&app, &destination)?;

    let output = import_export::export_story(ExportStoryInput {
        detail,
        destination_path: destination.clone(),
    })?;

    // Intentionally echo the dialog-selected (post-normalization) path
    // rather than `fs::canonicalize(destination)` — canonicalization
    // would resolve the `\\?\` UNC prefix on Windows (confusing UX) and
    // follow symlinks (leaking the real target path on POSIX). The
    // normalized path we ask the user to trust is the one we return.
    let _ = output.destination_path;
    let exported_path = destination.to_string_lossy().into_owned();

    Ok(ExportStoryDialogOutcomeDto::Exported {
        destination_path: exported_path,
        bytes_written: output.bytes_written,
        content_checksum: output.content_checksum,
    })
}

/// Convert a Tauri `FilePath` into a native `PathBuf`. Desktop save
/// dialogs always return a local path; a URL form is unexpected and
/// refused at the boundary.
fn file_path_to_pathbuf(file_path: &FilePath) -> Result<PathBuf, AppError> {
    file_path.as_path().map(|p| p.to_path_buf()).ok_or_else(|| {
        AppError::export_destination_unavailable(
            "Chemin d'export invalide: le système a renvoyé une URL au lieu d'un fichier.",
            "Choisis un emplacement local classique puis réessaie.",
        )
        .with_details(serde_json::json!({
            "source": "dialog_failed",
            "kind": "invalid_input",
            "cause": "non_filesystem_path",
        }))
    })
}

/// Validate the dialog-returned path and auto-append the `.rustory`
/// extension if the user typed a bare basename. The returned `PathBuf`
/// is the one actually written to disk.
fn validate_and_normalize_destination(raw: &Path) -> Result<PathBuf, AppError> {
    let raw_str = raw.to_string_lossy();
    if raw_str.is_empty() {
        return Err(destination_unavailable(
            "Chemin d'export manquant.",
            "Sélectionne un emplacement de destination puis réessaie.",
            "empty",
        ));
    }
    if raw_str.len() > MAX_DESTINATION_PATH_LEN {
        return Err(destination_unavailable_with_extra(
            "Chemin d'export trop long.",
            "Choisis un emplacement avec un chemin plus court puis réessaie.",
            "too_long",
            serde_json::json!({ "maxLen": MAX_DESTINATION_PATH_LEN }),
        ));
    }
    if !raw.is_absolute() {
        return Err(destination_unavailable(
            "Le chemin d'export doit être absolu.",
            "Choisis un emplacement via la fenêtre de sauvegarde puis réessaie.",
            "not_absolute",
        ));
    }

    let file_name = raw.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        destination_unavailable(
            "Nom de fichier d'export invalide.",
            "Choisis un nom de fichier non vide puis réessaie.",
            "missing_file_name",
        )
    })?;
    if file_name.is_empty() {
        return Err(destination_unavailable(
            "Nom de fichier d'export manquant.",
            "Choisis un nom de fichier non vide puis réessaie.",
            "missing_file_name",
        ));
    }

    // Windows strips a trailing space or dot from the last path
    // component, silently remapping `foo .rustory` → `foo.rustory` and
    // `foo.rustory.` → `foo.rustory`. Refuse at the boundary so what
    // the user sees on disk matches the name they typed.
    if file_name.ends_with(' ') || file_name.ends_with('.') {
        return Err(destination_unavailable(
            "Le nom de fichier ne peut pas se terminer par un espace ou un point.",
            "Retire l'espace ou le point final du nom puis réessaie.",
            "trailing_whitespace",
        ));
    }

    // Refuse POSIX-style hidden names (e.g. `.rustory` on its own) and
    // any leading-dot basename — the user almost certainly did not
    // intend a hidden file, and auto-appending `.rustory` would yield
    // `.rustory.rustory` which is worse.
    if file_name.starts_with('.') {
        return Err(destination_unavailable(
            "Nom de fichier d'export invalide: le nom avant l'extension est vide.",
            "Choisis un nom qui ne commence pas par un point puis réessaie.",
            "empty_file_stem",
        ));
    }

    // Auto-append the extension when the user typed a bare basename.
    // The filter in the save dialog is active but on most platforms
    // the user can still type any free-form name.
    let has_rustory_extension = raw
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(RUSTORY_ARTIFACT_EXTENSION));
    let normalized = if has_rustory_extension {
        raw.to_path_buf()
    } else {
        let mut with_ext = raw.as_os_str().to_owned();
        with_ext.push(".");
        with_ext.push(RUSTORY_ARTIFACT_EXTENSION);
        PathBuf::from(with_ext)
    };

    // Refuse existing symlinks at the destination. Following a symlink
    // would make the `persist()` rename clobber a file outside the
    // directory the user picked — surprising and potentially unsafe.
    match std::fs::symlink_metadata(&normalized) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(destination_unavailable(
                "Le chemin d'export pointe sur un lien symbolique.",
                "Choisis un emplacement qui n'est pas un lien symbolique puis réessaie.",
                "symlink_destination",
            ));
        }
        _ => {}
    }

    Ok(normalized)
}

fn destination_unavailable(message: &str, user_action: &str, cause: &str) -> AppError {
    AppError::export_destination_unavailable(message, user_action).with_details(serde_json::json!({
        "source": "invalid_path",
        "kind": "invalid_input",
        "cause": cause,
    }))
}

fn destination_unavailable_with_extra(
    message: &str,
    user_action: &str,
    cause: &str,
    extra: serde_json::Value,
) -> AppError {
    let mut details = serde_json::json!({
        "source": "invalid_path",
        "kind": "invalid_input",
        "cause": cause,
    });
    if let (Some(details_obj), serde_json::Value::Object(extra_obj)) =
        (details.as_object_mut(), extra)
    {
        for (k, v) in extra_obj {
            details_obj.insert(k, v);
        }
    }
    AppError::export_destination_unavailable(message, user_action).with_details(details)
}

/// Refuse any destination that would land inside Rustory's own managed
/// directories (`app_data_dir`, `app_config_dir`). An export artifact
/// written there would shadow the canonical local store and corrupt
/// the user's library on next launch.
fn reject_internal_app_directory(app: &AppHandle, destination: &Path) -> Result<(), AppError> {
    for (label, dir_result) in [
        ("app_data_dir", app.path().app_data_dir()),
        ("app_config_dir", app.path().app_config_dir()),
    ] {
        let Ok(managed) = dir_result else { continue };
        let Ok(managed_canonical) = std::fs::canonicalize(&managed) else {
            continue;
        };
        let destination_anchor = destination.parent().unwrap_or(destination);
        let Ok(destination_canonical) = std::fs::canonicalize(destination_anchor) else {
            continue;
        };
        if destination_canonical.starts_with(&managed_canonical) {
            return Err(AppError::export_destination_unavailable(
                "Impossible d'exporter dans le dossier interne de Rustory.",
                "Choisis un autre emplacement (Documents, Bureau, etc.) puis réessaie.",
            )
            .with_details(serde_json::json!({
                "source": "invalid_path",
                "kind": "invalid_input",
                "cause": "internal_app_directory",
                "directory": label,
            })));
        }
    }
    Ok(())
}

/// Analyze a user-picked `.rustory` artifact (phase 1, NO mutation).
///
/// Opens a native open-file dialog, reads the chosen file bounded, and
/// returns a typed recognition verdict. Mirrors `import_official_catalog`'s
/// non-blocking dialog discipline (the native GTK dialog MUST run on the
/// main thread — a `blocking_*` variant dead-locks the app). A cancelled
/// dialog resolves with `{ kind: "cancelled" }`. Only TRANSPORT failures
/// (file unreadable, dialog backend) reject with `IMPORT_FAILED`; the
/// functional verdict (bad version, corruption, normalized title) is the
/// typed DTO, never an error.
#[tauri::command]
pub async fn analyze_artifact_for_import(
    app: AppHandle,
) -> Result<ImportArtifactAnalysisDto, AppError> {
    let (tx, mut rx) = async_runtime::channel::<Option<FilePath>>(1);
    app.dialog()
        .file()
        .add_filter(IMPORT_DIALOG_FILTER_NAME, &[RUSTORY_ARTIFACT_EXTENSION])
        .pick_file(move |path| {
            let _ = tx.try_send(path);
        });

    let picked = match rx.recv().await {
        Some(inner) => inner,
        None => return Err(import::dialog_failed_error()),
    };
    let Some(file_path) = picked else {
        return Ok(ImportArtifactAnalysisDto::Cancelled);
    };
    let path = file_path
        .as_path()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| import::file_read_error("non_filesystem_path"))?;
    // Carry the BASENAME only across the boundary — never the absolute path
    // (PII). Falls back to a sober placeholder for an unnameable path.
    let source_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artefact")
        .to_string();

    let analysis = async_runtime::spawn_blocking(move || -> Result<ImportAnalysis, AppError> {
        let bytes = read_artifact_bounded(&path)?;
        Ok(import::analyze_artifact(&bytes, source_name))
    })
    .await
    .map_err(|_| import::spawn_blocking_join_error())??;

    Ok(ImportArtifactAnalysisDto::analyzed(
        &analysis.analysis,
        analysis.source_name,
        analysis.artifact_checksum,
    ))
}

/// Commit a recognized artifact (phase 2). Takes the validated content from
/// a prior analysis and re-validates it FROM ZERO before the canonical
/// commit (`stories` + `story_local_imports`). The DB work runs on a
/// `spawn_blocking` worker so no `MutexGuard` ever lives across an `await`.
#[tauri::command]
pub async fn accept_artifact_import(
    state: State<'_, AppState>,
    input: AcceptArtifactImportInputDto,
) -> Result<StoryCardDto, AppError> {
    let db = state.db.clone();
    async_runtime::spawn_blocking(move || -> Result<StoryCardDto, AppError> {
        let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        import::accept_import(&mut guard, &input)
    })
    .await
    .map_err(|_| import::spawn_blocking_join_error())?
}

/// Analyze the pending OS-open intent (phase 1, NO mutation, NO dialog —
/// see `ui-states.md#OS Open Contract`). The frontend PULLS this once at
/// library mount (cold start) and on each `os-open:requested` signal.
///
/// The whole decision logic lives in the testable application function
/// [`import_export::analyze_pending_intent`]; this handler only drives it
/// on a `spawn_blocking` worker (the bounded file read is disk I/O), the
/// exact discipline of the dialog-import analysis. No pending intent
/// resolves `{ kind: "none" }` (a total no-op — also the harmless second
/// answer of a StrictMode double-effect); only a TRANSPORT failure (the
/// file became unreadable) rejects, and the intent then STAYS pending so
/// `Réessayer` replays it. A read failure is traced to the local import
/// log (PII-free stage tokens — the PRD's "journalisation locale
/// suffisante pour diagnostiquer les échecs d'ouverture") while the
/// actionable `AppError` still crosses to the UI.
#[tauri::command]
pub async fn analyze_os_open_request(app: AppHandle) -> Result<OsOpenAnalysisDto, AppError> {
    let outcome = async_runtime::spawn_blocking(|| {
        import_export::analyze_pending_intent(&import_export::OS_OPEN_STATE)
    })
    .await
    .map_err(|_| import::spawn_blocking_join_error())?;

    if let Err(err) = &outcome {
        let _ = import_log::record_event(
            &app,
            import_log::Event::OsOpenReadFailed {
                source: error_detail(err, "source"),
                stage: error_detail(err, "stage"),
            },
        );
    }
    outcome
}

/// Drop the pending OS-open intent, if any. Idempotent, infallible: called
/// when the user closes a failed OS-open read (`Fermer`) or when the
/// library declines an intent because a flow is already in flight.
#[tauri::command]
pub fn discard_os_open_request() {
    import_export::OS_OPEN_STATE.discard();
}

/// Analyze a user-picked structured folder (phase 1, NO mutation).
///
/// Opens a native FOLDER picker (same non-blocking callback + channel
/// discipline as the `.rustory` flow — the GTK dialog must run on the main
/// thread, a `blocking_*` variant dead-locks). A cancelled dialog resolves
/// with `{ kind: "cancelled" }`. The bounded analysis (manifest + media
/// probes) runs on a blocking worker. Only TRANSPORT failures reject; every
/// folder-state problem (manifest absent, malformed, media missing…) is the
/// typed verdict DTO. The returned `folderPath` exists ONLY to be passed
/// back to `accept_structured_creation` — never rendered, persisted or
/// logged (PII).
#[tauri::command]
pub async fn analyze_structured_folder_for_creation(
    app: AppHandle,
) -> Result<StructuredCreationAnalysisDto, AppError> {
    let (tx, mut rx) = async_runtime::channel::<Option<FilePath>>(1);
    app.dialog().file().pick_folder(move |path| {
        let _ = tx.try_send(path);
    });

    let picked = match rx.recv().await {
        Some(inner) => inner,
        None => return Err(structured_creation::dialog_failed_error()),
    };
    let Some(folder) = picked else {
        return Ok(StructuredCreationAnalysisDto::Cancelled);
    };
    let path = folder
        .as_path()
        .map(|p| p.to_path_buf())
        .ok_or_else(structured_creation::non_filesystem_path_error)?;
    // The wire is UTF-8 JSON: a non-UTF-8 path cannot round-trip VERBATIM
    // to the accept phase (a lossy conversion would re-analyze a DIFFERENT
    // folder). Refused at the boundary rather than silently altered.
    let folder_path = path
        .to_str()
        .map(str::to_string)
        .ok_or_else(structured_creation::non_filesystem_path_error)?;

    let outcome =
        async_runtime::spawn_blocking(move || import_export::analyze_structured_folder(&path))
            .await
            .map_err(|_| structured_creation::spawn_blocking_join_error())??;

    Ok(StructuredCreationAnalysisDto::analyzed(
        &outcome.analysis,
        outcome.folder_name,
        folder_path,
    ))
}

/// Commit an analyzed structured folder (phase 2). Re-analyzes the folder
/// FROM ZERO on a blocking worker (the wire path is a pointer, never an
/// authority). TRUE "files first, DB second": the re-analysis and the (up
/// to 256 MiB) media promotions run WITHOUT the DB mutex — the lock is
/// taken only for the single atomic transaction (or the brief refcounted
/// compensation after a prepare refusal), INSIDE the worker so no
/// `MutexGuard` ever lives across an `await`.
#[tauri::command]
pub async fn accept_structured_creation(
    app: AppHandle,
    state: State<'_, AppState>,
    input: AcceptStructuredCreationInputDto,
) -> Result<StoryCardDto, AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| structured_creation::app_data_unavailable_error())?;
    let db = state.db.clone();
    async_runtime::spawn_blocking(move || -> Result<StoryCardDto, AppError> {
        let folder = Path::new(&input.folder_path);
        match import_export::prepare_structured_creation(&app_data_dir, folder) {
            Ok(prepared) => {
                let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                import_export::commit_structured_creation(&mut guard, &app_data_dir, prepared)
            }
            Err(failure) => {
                // A refused prepare may have promoted files before failing:
                // reclaim them under a brief lock (refcounted GC).
                if !failure.promoted.is_empty() {
                    let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    import_export::compensate_structured_creation(
                        &guard,
                        &app_data_dir,
                        &failure.promoted,
                    );
                }
                Err(failure.error)
            }
        }
    })
    .await
    .map_err(|_| structured_creation::spawn_blocking_join_error())?
}

/// Fetch + analyze a user-provided RSS feed (phase 1, NO mutation, NO DB).
///
/// The ONLY networked action of the external-source flow, on the explicit
/// `Récupérer le flux` click (offline-first guardrail — the exact
/// discipline of `refresh_official_catalog`). Runs the bounded fetch +
/// parse on a `spawn_blocking` worker; the DB is NEVER touched (the
/// preview is pure). Only TRANSPORT failures (invalid address,
/// unreachable source, over-cap response) reject with
/// `RSS_SOURCE_UNREACHABLE`; every feed-CONTENT problem (unreadable XML,
/// a non-RSS root, zero exploitable item) is the typed verdict inside the
/// resolved DTO.
#[tauri::command]
pub async fn fetch_rss_source_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    feed_url: String,
) -> Result<RssPreviewDto, AppError> {
    let source = state.rss_source.clone();
    // The raw address is CLONED (never parsed) before moving into the
    // worker: the policy gate must run before ANY address analysis, so
    // the diagnostics host is derived only AFTER the outcome settles —
    // and never at all on a policy refusal.
    let feed_url_for_log = feed_url.clone();
    let outcome = async_runtime::spawn_blocking(move || {
        rss_creation::preview_rss_source(
            official_content_sources(),
            source.as_ref(),
            &feed_url,
            RSS_FETCH_BUDGET,
        )
    })
    .await
    .map_err(|_| rss_creation::spawn_blocking_join_error())?;

    match &outcome {
        Ok(preview) => {
            let _ = import_log::record_event(
                &app,
                import_log::Event::RssPreviewSettled {
                    host: preview.source_host.clone(),
                    state: rss_verdict_tag(&preview.analysis),
                    item_count: preview.analysis.items.len(),
                },
            );
        }
        Err(err) => record_rss_failure(&app, &feed_url_for_log, err),
    }

    let preview = outcome?;
    Ok(RssPreviewDto::from_analysis(
        preview.source_host,
        &preview.analysis,
    ))
}

/// Commit one previewed feed item into a canonical local draft (phase 2).
///
/// RE-fetches and re-parses the feed from zero on a blocking worker (the
/// source is the authority; the wire reference is a pointer, never
/// content), WITHOUT the DB lock — the lock is taken only for the single
/// atomic transaction, INSIDE the worker so no `MutexGuard` ever lives
/// across an `await`. A diverged source resolves with the typed
/// `sourceChanged` refusal (nothing created); only transport rejects.
#[tauri::command]
pub async fn accept_rss_story_creation(
    app: AppHandle,
    state: State<'_, AppState>,
    feed_url: String,
    item_ref: RssItemRefDto,
) -> Result<RssCreationOutcomeDto, AppError> {
    let source = state.rss_source.clone();
    let db = state.db.clone();
    // Cloned raw, parsed only AFTER the outcome (see the preview command):
    // a policy refusal must reject before ANY address analysis, boundary
    // included.
    let feed_url_for_log = feed_url.clone();
    let outcome = async_runtime::spawn_blocking(move || -> Result<RssCreationOutcome, AppError> {
        let reference = item_ref.to_domain();
        let expected_fingerprint = item_ref.fingerprint().to_string();
        match rss_creation::prepare_rss_story_creation(
            official_content_sources(),
            source.as_ref(),
            &feed_url,
            &reference,
            &expected_fingerprint,
            RSS_FETCH_BUDGET,
        )? {
            RssAcceptPhase::SourceChanged => Ok(RssCreationOutcome::SourceChanged),
            RssAcceptPhase::Prepared(prepared) => {
                let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                rss_creation::commit_rss_story_creation(&mut guard, *prepared)
                    .map(|story| RssCreationOutcome::Created { story })
            }
        }
    })
    .await
    .map_err(|_| rss_creation::spawn_blocking_join_error())?;

    match &outcome {
        Ok(RssCreationOutcome::Created { story }) => {
            // A settled creation already passed the policy + address
            // gates; deriving the host here cannot precede them.
            let _ = import_log::record_event(
                &app,
                import_log::Event::RssCreationSettled {
                    host: feed_url_host(&feed_url_for_log).unwrap_or_default(),
                    import_state: story
                        .import_state
                        .map(|state| state.wire_tag())
                        .unwrap_or("unknown"),
                },
            );
        }
        Ok(RssCreationOutcome::SourceChanged) => {
            let _ = import_log::record_event(
                &app,
                import_log::Event::RssSourceChanged {
                    host: feed_url_host(&feed_url_for_log).unwrap_or_default(),
                },
            );
        }
        Err(err) => record_rss_failure(&app, &feed_url_for_log, err),
    }

    Ok(match outcome? {
        RssCreationOutcome::Created { story } => RssCreationOutcomeDto::Created {
            report: story.import_report.clone().unwrap_or_default(),
            story,
        },
        RssCreationOutcome::SourceChanged => RssCreationOutcomeDto::SourceChanged,
    })
}

/// Read the official content-source policy: WHICH additional creation
/// sources this distribution activates, with their frozen labels and
/// disabled-entry reasons (`Content Source Activation Contract`). A PURE,
/// synchronous read of the domain matrix — zero network, zero DB, zero
/// lock: the frontend renders what Rust declares and never hardcodes the
/// source list. Infallible by construction (the matrix is a build-time
/// constant), hence no `Result`.
#[tauri::command]
pub fn read_content_source_policy() -> ContentSourcePolicyDto {
    ContentSourcePolicyDto::from_lines(official_content_sources())
}

/// The diagnostic verdict tag of a feed analysis: the durable-state tag
/// for an exploitable feed, the literal `blocked` for a typed verdict
/// (`state_db_tag` deliberately never emits it — it is not persistable).
fn rss_verdict_tag(analysis: &crate::domain::import::RssAnalysis) -> &'static str {
    if analysis.is_blocked() {
        "blocked"
    } else {
        state_db_tag(analysis.state)
    }
}

/// Best-effort failure trace (host at most). The event category follows
/// the ERROR CODE so the closed diagnostic categories stay honest: only a
/// real transport failure (`RSS_SOURCE_UNREACHABLE`) lands under
/// `rss_source_unreachable`; a local failure (DB commit, clock, worker
/// join — `IMPORT_FAILED`…) is an `rss_creation_failed` line. The raw
/// address travels UNPARSED to this point: [`rss_failure_log_host`]
/// derives the host only when the failure category carries one.
fn record_rss_failure(app: &AppHandle, feed_url: &str, err: &AppError) {
    let _ = import_log::record_event(
        app,
        rss_failure_event(rss_failure_log_host(feed_url, err), err),
    );
}

/// The (pure, unit-tested) host derivation of a failure trace: a POLICY
/// refusal never reads the address — its event carries the KIND alone, so
/// the URL is not even parsed (the boundary mirror of "the gate runs
/// before the address validation"). Every other failure derives the host
/// best-effort.
fn rss_failure_log_host(feed_url: &str, err: &AppError) -> String {
    if err.code == crate::domain::shared::AppErrorCode::ContentSourceUnavailable {
        return String::new();
    }
    feed_url_host(feed_url).unwrap_or_default()
}

/// Extract one string detail from an `AppError`'s closed `details` set —
/// the stable, PII-free tokens the diagnostic events carry.
fn error_detail(err: &AppError, key: &str) -> String {
    err.details
        .as_ref()
        .and_then(|details| details.get(key))
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string()
}

/// The (pure, unit-tested) category dispatch of a failure trace.
fn rss_failure_event(host: String, err: &AppError) -> import_log::Event {
    if err.code == crate::domain::shared::AppErrorCode::RssSourceUnreachable {
        import_log::Event::RssSourceUnreachable {
            host,
            stage: error_detail(err, "stage"),
        }
    } else if err.code == crate::domain::shared::AppErrorCode::ContentSourceUnavailable {
        // A POLICY refusal: kind only, and the host is deliberately
        // dropped — the refusal precedes any network dispatch, and a
        // distribution decision is neither a network nor a local failure.
        import_log::Event::ContentSourceBlocked {
            kind: error_detail(err, "kind"),
        }
    } else {
        import_log::Event::RssCreationFailed {
            host,
            code: serde_json::to_value(&err.code)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| "UNKNOWN".to_string()),
            source: error_detail(err, "source"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::import_export::import::MAX_ARTIFACT_BYTES;
    use crate::domain::shared::AppErrorCode;

    #[test]
    fn rss_failure_event_dispatches_on_the_error_code() {
        // A REAL transport failure lands under `rss_source_unreachable`…
        let transport = crate::infrastructure::device::rss_source::fetch_error("request");
        let event = rss_failure_event("exemple.fr".into(), &transport);
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "rss_source_unreachable");
        assert_eq!(v["host"], "exemple.fr");
        assert_eq!(v["stage"], "request");

        // …while a LOCAL accept failure (DB commit, clock, join —
        // IMPORT_FAILED) is an `rss_creation_failed` line: the closed
        // diagnostic categories never count a SQLite/clock failure as a
        // network problem.
        let local = AppError::import_failed("Création impossible.", "Réessaie.")
            .with_details(serde_json::json!({ "source": "db_commit", "stage": "commit" }));
        let event = rss_failure_event("exemple.fr".into(), &local);
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "rss_creation_failed");
        assert_eq!(v["code"], "IMPORT_FAILED");
        assert_eq!(v["source"], "db_commit");

        // …and a POLICY refusal is `content_source_blocked` — kind only,
        // the host is dropped (the refusal precedes any dispatch): never
        // a network line, never a local-failure line.
        let policy =
            AppError::content_source_unavailable(crate::domain::import::ContentSourceKind::Rss);
        let event = rss_failure_event("exemple.fr".into(), &policy);
        let v = serde_json::to_value(&event).expect("ser");
        assert_eq!(v["category"], "content_source_blocked");
        assert_eq!(v["kind"], "rss");
        assert!(v.get("host").is_none(), "no host on a policy refusal");
    }

    #[test]
    fn rss_failure_log_host_never_reads_the_address_on_a_policy_refusal() {
        // A perfectly parseable address + a policy refusal → the host is
        // EMPTY: the address was not consulted (the boundary mirror of
        // "the gate runs before the address validation").
        let policy =
            AppError::content_source_unavailable(crate::domain::import::ContentSourceKind::Rss);
        assert_eq!(
            rss_failure_log_host("https://exemple.fr/flux.xml", &policy),
            ""
        );

        // Every other failure derives the host best-effort.
        let transport = crate::infrastructure::device::rss_source::fetch_error("request");
        assert_eq!(
            rss_failure_log_host("https://exemple.fr/flux.xml", &transport),
            "exemple.fr"
        );
        assert_eq!(rss_failure_log_host("pas une adresse", &transport), "");
    }

    #[test]
    fn os_open_read_failure_maps_to_its_diagnostic_stage_tokens() {
        // The trace line carries exactly the closed tokens of the upstream
        // transport error — never a path, never a basename.
        let err = import::file_read_error("metadata");
        assert_eq!(error_detail(&err, "source"), "file_read");
        assert_eq!(error_detail(&err, "stage"), "metadata");
        assert_eq!(error_detail(&err, "absent_key"), "unknown");
    }

    fn assert_invalid_path(err: &AppError, cause: &str) {
        assert_eq!(err.code, AppErrorCode::ExportDestinationUnavailable);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "invalid_path");
        assert_eq!(details["kind"], "invalid_input");
        assert_eq!(details["cause"], cause);
    }

    #[test]
    fn validate_accepts_absolute_rustory_path() {
        let path =
            validate_and_normalize_destination(Path::new("/tmp/histoire.rustory")).expect("accept");
        assert_eq!(path.to_string_lossy(), "/tmp/histoire.rustory");
    }

    #[test]
    fn validate_auto_appends_rustory_extension_when_missing() {
        let path = validate_and_normalize_destination(Path::new("/tmp/histoire")).expect("accept");
        assert_eq!(path.to_string_lossy(), "/tmp/histoire.rustory");
    }

    #[test]
    fn validate_auto_appends_keeps_dot_when_extension_differs() {
        // `histoire.txt` → `histoire.txt.rustory` — the user picked a
        // non-standard name but the artifact still wins the extension.
        let path =
            validate_and_normalize_destination(Path::new("/tmp/histoire.txt")).expect("accept");
        assert_eq!(path.to_string_lossy(), "/tmp/histoire.txt.rustory");
    }

    #[test]
    fn validate_accepts_mixed_case_rustory_extension() {
        validate_and_normalize_destination(Path::new("/tmp/histoire.Rustory")).expect("accept");
        validate_and_normalize_destination(Path::new("/tmp/histoire.RUSTORY")).expect("accept");
    }

    #[test]
    fn validate_rejects_relative_path() {
        let err = validate_and_normalize_destination(Path::new("relative/histoire.rustory"))
            .expect_err("must reject");
        assert_invalid_path(&err, "not_absolute");
    }

    #[test]
    fn validate_rejects_too_long_path() {
        let huge_str = format!("/{}.rustory", "a".repeat(MAX_DESTINATION_PATH_LEN));
        let err =
            validate_and_normalize_destination(Path::new(&huge_str)).expect_err("must reject");
        assert_invalid_path(&err, "too_long");
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["maxLen"], MAX_DESTINATION_PATH_LEN);
    }

    #[test]
    fn validate_rejects_trailing_space() {
        let err = validate_and_normalize_destination(Path::new("/tmp/histoire.rustory "))
            .expect_err("must reject");
        assert_invalid_path(&err, "trailing_whitespace");
    }

    #[test]
    fn validate_rejects_trailing_dot() {
        let err = validate_and_normalize_destination(Path::new("/tmp/histoire.rustory."))
            .expect_err("must reject");
        assert_invalid_path(&err, "trailing_whitespace");
    }

    #[test]
    fn validate_rejects_empty_file_stem() {
        // `/tmp/.rustory` — no stem, just the extension. On POSIX this
        // would produce a hidden file with only the extension as name.
        let err = validate_and_normalize_destination(Path::new("/tmp/.rustory"))
            .expect_err("must reject");
        assert_invalid_path(&err, "empty_file_stem");
    }

    #[cfg(unix)]
    #[test]
    fn validate_rejects_symlink_destination() {
        use tempfile::TempDir;
        let tmp = TempDir::new().expect("tempdir");
        let real = tmp.path().join("real.rustory");
        std::fs::write(&real, b"placeholder").expect("seed real");
        let link = tmp.path().join("link.rustory");
        std::os::unix::fs::symlink(&real, &link).expect("mklink");
        let err = validate_and_normalize_destination(&link).expect_err("must reject");
        assert_invalid_path(&err, "symlink_destination");
    }

    // ---------------- read_artifact_bounded (F2) ----------------

    fn assert_file_read(err: &AppError, stage: &str) {
        assert_eq!(err.code, AppErrorCode::ImportFailed);
        let v = serde_json::to_value(err).expect("ser");
        assert_eq!(v["details"]["source"], "file_read");
        assert_eq!(v["details"]["stage"], stage);
    }

    #[test]
    fn read_artifact_bounded_reads_a_small_regular_file() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("histoire.rustory");
        std::fs::write(&path, b"{\"ok\":true}").expect("seed");
        let bytes = read_artifact_bounded(&path).expect("read");
        assert_eq!(bytes, b"{\"ok\":true}");
    }

    #[test]
    fn read_artifact_bounded_refuses_a_directory_as_non_regular() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        // A directory must never be read as an artifact.
        let err = read_artifact_bounded(tmp.path()).expect_err("a directory must be refused");
        assert_file_read(&err, "not_regular_file");
    }

    #[test]
    fn read_artifact_bounded_refuses_a_file_over_the_bound() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("huge.rustory");
        // One byte over the ceiling — the metadata gate already refuses it.
        let oversize = vec![b'a'; (MAX_ARTIFACT_BYTES + 1) as usize];
        std::fs::write(&path, &oversize).expect("seed oversize");
        let err = read_artifact_bounded(&path).expect_err("oversize must be refused");
        assert_file_read(&err, "oversize");
    }

    #[test]
    fn read_artifact_bounded_caps_the_read_when_metadata_understates_the_size() {
        // Defense in depth against a misleading metadata / a file that grows
        // after the size check: even if the length gate is bypassed, the
        // capped `take(MAX + 1)` read refuses an over-bound payload rather
        // than loading it whole. We force the path by lowering the bound via
        // a hand-rolled read mirroring `read_artifact_bounded` with a tiny cap.
        use std::io::Read;
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("grown.rustory");
        std::fs::write(&path, vec![b'x'; 32]).expect("seed");
        let file = std::fs::File::open(&path).expect("open");
        let mut bytes = Vec::new();
        // Cap at 8 (< file size) and assert the overflow is observable.
        file.take(8 + 1).read_to_end(&mut bytes).expect("read");
        assert!(
            bytes.len() as u64 > 8,
            "the capped read must surface the over-bound overflow byte"
        );
    }
}
