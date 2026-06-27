//! Official-catalog commands (story 2-6, Phase C).
//!
//! Three EXPLICIT, user-triggered actions — there is no implicit catalog
//! traffic anywhere (offline-first / anti-catalog guardrail):
//!
//! - [`get_official_catalog_status`]: read how many official titles are
//!   cached (a bounded count query, no network).
//! - [`refresh_official_catalog`]: the ONLY networked path — guest auth +
//!   `/v2/packs`, on a `spawn_blocking` worker; the DB mutex is locked only
//!   for the final replace, never across the fetch.
//! - [`import_official_catalog`]: the 100%-offline alternative — the user
//!   picks a catalog file in a native dialog; Rust reads, parses and caches
//!   it. Mirrors `export_story_with_save_dialog`'s dialog discipline.

use std::time::Duration;

use tauri::{async_runtime, AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::application::device::catalog::{
    self, import_official_catalog_from_bytes, DEFAULT_CATALOG_LOCALE, MAX_CATALOG_BYTES,
};
use crate::application::device::title::count_official_catalog;
use crate::commands::shared::base64_encode;
use crate::domain::device::is_canonical_pack_uuid;
use crate::domain::shared::AppError;
use crate::infrastructure::filesystem::{
    ensure_catalog_covers_dir, read_catalog_cover, resolve_catalog_covers_dir,
};
use crate::ipc::dto::{CatalogStatusDto, ImportOfficialCatalogOutcomeDto, PackCoverDto};
use crate::AppState;

/// Wall-clock budget for the whole networked refresh: guest auth + catalog
/// download + the eager cover downloads (~574 images). Generous because the
/// cover phase dominates; a stalled connection still cannot hang forever,
/// and covers are best-effort (a budget exhaustion just leaves later packs
/// cover-less, never failing the refresh).
const CATALOG_FETCH_BUDGET: Duration = Duration::from_secs(300);

const CATALOG_DIALOG_FILTER_NAME: &str = "Catalogue Lunii (JSON)";

/// Read the number of official titles currently cached. Synchronous: a
/// single bounded query under a scoped lock.
#[tauri::command]
pub fn get_official_catalog_status(
    _app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CatalogStatusDto, AppError> {
    let db = state
        .db
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(CatalogStatusDto::new(count_official_catalog(&db)?))
}

/// EXPLICIT network fetch of the official catalog. Runs the guest-auth +
/// download + parse + cache replace on a `spawn_blocking` worker so the
/// async runtime stays free; the DB mutex is locked only for the replace.
#[tauri::command]
pub async fn refresh_official_catalog(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CatalogStatusDto, AppError> {
    let db = state.db.clone();
    let source = state.catalog_source.clone();
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| catalog_storage_error("app_data_dir"))?;
    // Create the cover cache dir up-front so the (blocking) download phase
    // can write into it.
    let covers_dir = ensure_catalog_covers_dir(&app_data_dir)?;

    let count = async_runtime::spawn_blocking(move || {
        catalog::refresh_official_catalog(
            &db,
            source.as_ref(),
            &covers_dir,
            DEFAULT_CATALOG_LOCALE,
            CATALOG_FETCH_BUDGET,
        )
    })
    .await
    .map_err(|_| {
        AppError::official_catalog_unavailable(
            "Récupération du catalogue interrompue.",
            "Réessaie ; si le problème persiste, redémarre Rustory.",
        )
        .with_details(serde_json::json!({ "source": "network", "stage": "spawn_blocking_join" }))
    })??;

    Ok(CatalogStatusDto::new(count))
}

/// Read the cached cover for a pack as a `data:` URL — a LOCAL read of the
/// cover cache populated during the explicit refresh (NO network). Resolves
/// with `null` whenever no cover is available (no row, no file, unreadable):
/// a missing cover is never an error, the UI just shows the title alone.
#[tauri::command]
pub async fn read_pack_cover(
    app: AppHandle,
    state: State<'_, AppState>,
    pack_uuid: String,
) -> Result<Option<PackCoverDto>, AppError> {
    if !is_canonical_pack_uuid(&pack_uuid) {
        return Ok(None);
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| catalog_storage_error("app_data_dir"))?;
    let db = state.db.clone();

    // The DB lookup + (≤4 MiB) file read + base64 run on a blocking worker,
    // off the async runtime — same discipline as the other device commands.
    async_runtime::spawn_blocking(move || -> Result<Option<PackCoverDto>, AppError> {
        // Stored cover file name (covers ride the 'official' source) under a
        // scoped lock; the file read happens off the lock.
        let file_name = {
            let guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            guard
                .conn()
                .query_row(
                    "SELECT thumbnail FROM pack_metadata \
                     WHERE pack_uuid = ?1 AND source = 'official' AND thumbnail IS NOT NULL",
                    rusqlite::params![pack_uuid],
                    |row| row.get::<_, String>(0),
                )
                .ok()
        };
        let Some(file_name) = file_name else {
            return Ok(None);
        };

        let covers_dir = resolve_catalog_covers_dir(&app_data_dir);
        // A missing/corrupt cover file degrades to "no cover", never an error.
        let Ok((bytes, mime)) = read_catalog_cover(&covers_dir, &file_name) else {
            return Ok(None);
        };
        Ok(Some(PackCoverDto {
            data_url: format!("data:{mime};base64,{}", base64_encode(&bytes)),
        }))
    })
    .await
    .map_err(|_| catalog_storage_error("spawn_blocking_join"))?
}

fn catalog_storage_error(stage: &'static str) -> AppError {
    AppError::official_catalog_unavailable(
        "Catalogue indisponible: stockage local introuvable.",
        "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
    )
    .with_details(serde_json::json!({ "source": "storage", "stage": stage }))
}

/// Import the official catalog from a user-picked file (100%-offline path).
/// Opens a native open-file dialog, reads the chosen file (bounded), parses
/// and caches it. A cancelled dialog resolves with `{ kind: "cancelled" }`.
#[tauri::command]
pub async fn import_official_catalog(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ImportOfficialCatalogOutcomeDto, AppError> {
    // Non-blocking dialog + channel (same reason as the export flow: the
    // native GTK dialog must run on the main thread).
    let (tx, mut rx) = async_runtime::channel::<Option<FilePath>>(1);
    app.dialog()
        .file()
        .add_filter(CATALOG_DIALOG_FILTER_NAME, &["json"])
        .pick_file(move |path| {
            let _ = tx.try_send(path);
        });

    let picked = match rx.recv().await {
        Some(inner) => inner,
        None => {
            return Err(AppError::official_catalog_unavailable(
                "La fenêtre de sélection n'a pas pu s'ouvrir.",
                "Relance Rustory ; si le problème persiste, consulte les traces locales.",
            )
            .with_details(serde_json::json!({
                "source": "import",
                "stage": "dialog_failed",
            })));
        }
    };

    let Some(file_path) = picked else {
        return Ok(ImportOfficialCatalogOutcomeDto::Cancelled);
    };
    let path = file_path
        .as_path()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| {
            AppError::official_catalog_unavailable(
                "Chemin de fichier invalide.",
                "Choisis un fichier local classique puis réessaie.",
            )
            .with_details(serde_json::json!({ "source": "import", "stage": "non_filesystem_path" }))
        })?;

    let db = state.db.clone();
    let count = async_runtime::spawn_blocking(move || -> Result<u32, AppError> {
        // Bound the read before loading the whole file into memory.
        let meta = std::fs::metadata(&path).map_err(|_| import_read_error("metadata"))?;
        if meta.len() > MAX_CATALOG_BYTES as u64 {
            return Err(import_read_error("oversize"));
        }
        let bytes = std::fs::read(&path).map_err(|_| import_read_error("read"))?;
        let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        import_official_catalog_from_bytes(&mut guard, &bytes, DEFAULT_CATALOG_LOCALE)
    })
    .await
    .map_err(|_| import_read_error("spawn_blocking_join"))??;

    Ok(ImportOfficialCatalogOutcomeDto::Imported { count })
}

fn import_read_error(stage: &'static str) -> AppError {
    AppError::official_catalog_unavailable(
        "Import du catalogue impossible: fichier illisible.",
        "Vérifie que le fichier existe et qu'il s'agit d'un catalogue Lunii puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "import",
        "stage": stage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;

    #[test]
    fn import_read_errors_are_actionable_catalog_errors() {
        for stage in ["metadata", "oversize", "read", "spawn_blocking_join"] {
            let err = import_read_error(stage);
            assert_eq!(err.code, AppErrorCode::OfficialCatalogUnavailable);
            assert!(!err.message.is_empty());
            assert!(!err.user_action.as_deref().unwrap_or("").is_empty());
            let v = serde_json::to_value(&err).expect("ser");
            assert_eq!(v["details"]["source"], "import");
            assert_eq!(v["details"]["stage"], stage);
        }
    }
}
