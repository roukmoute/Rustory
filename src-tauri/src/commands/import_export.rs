use std::path::{Path, PathBuf};

use tauri::{async_runtime, AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::application::import_export::{self, ExportStoryInput};
use crate::application::story::get_story_detail;
use crate::commands::shared::validate_story_id;
use crate::domain::export::RUSTORY_ARTIFACT_EXTENSION;
use crate::domain::shared::AppError;
use crate::ipc::dto::{ExportStoryDialogInputDto, ExportStoryDialogOutcomeDto};
use crate::AppState;

const EXPORT_DIALOG_FILTER_NAME: &str = "Artefact Rustory";
const MAX_DESTINATION_PATH_LEN: usize = 4096;

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

    let detail = {
        // Scoped block so the `MutexGuard` is dropped BEFORE the first
        // `.await`. A `MutexGuard<DbHandle>` is not `Send` on all
        // platforms, and an async command's future must be `Send` for
        // Tauri to spawn it on the runtime.
        let db = state
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        get_story_detail(&db, &input.story_id)?.ok_or_else(|| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;

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
}
