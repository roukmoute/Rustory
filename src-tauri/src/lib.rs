use std::sync::Mutex;

use tauri::Manager;

pub mod application;
pub mod commands;
pub mod domain;
pub mod infrastructure;
pub mod ipc;

/// Application-wide state managed by Tauri. Holds the long-lived database
/// handle behind a synchronous mutex — SQLite serializes writers internally
/// and reads are short-lived enough that a fine-grained async pool would
/// be over-engineered for the current surface.
pub struct AppState {
    pub db: Mutex<infrastructure::db::DbHandle>,
}

/// Read every story id that still has a pending draft row. Ordered by
/// `draft_at DESC` so the most recently buffered story shows up first
/// (mostly for human readability when scanning the log).
///
/// Errors propagate as a typed `AppError`: the boot probe needs the
/// support-side trace surface to know WHY it could not read, not just
/// that it failed.
fn collect_pending_drafts(
    db: &infrastructure::db::DbHandle,
) -> Result<Vec<String>, domain::shared::AppError> {
    let mut stmt = db
        .conn()
        .prepare("SELECT story_id FROM story_drafts ORDER BY draft_at DESC")
        .map_err(|_| {
            domain::shared::AppError::recovery_draft_unavailable(
                "Récupération indisponible: vérifie le disque local et réessaie.",
                "Relance Rustory ; si le problème persiste, consulte les traces locales.",
            )
            .with_details(serde_json::json!({
                "source": "boot_probe_prepare",
            }))
        })?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| {
            domain::shared::AppError::recovery_draft_unavailable(
                "Récupération indisponible: vérifie le disque local et réessaie.",
                "Relance Rustory ; si le problème persiste, consulte les traces locales.",
            )
            .with_details(serde_json::json!({
                "source": "boot_probe_query",
            }))
        })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|_| {
        domain::shared::AppError::recovery_draft_unavailable(
            "Récupération indisponible: vérifie le disque local et réessaie.",
            "Relance Rustory ; si le problème persiste, consulte les traces locales.",
        )
        .with_details(serde_json::json!({
            "source": "boot_probe_collect",
        }))
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = infrastructure::filesystem::ensure_app_data_dir(app.handle())
                .map_err(|err| err.to_string())?;
            let db_path = infrastructure::filesystem::resolve_db_path(&app_data_dir);
            let mut db = infrastructure::db::open_at(&db_path).map_err(|err| err.to_string())?;
            infrastructure::db::run_migrations(&mut db).map_err(|err| err.to_string())?;

            // Boot-time interruption probe: any draft surviving the
            // previous session indicates the app was killed mid-edit.
            // Emit a single `interrupted_session_detected` event so the
            // recovery log carries the chronology — the per-story
            // banners are then driven by the route-level reads.
            //
            // Failures to read the table or write the log are non-fatal,
            // but they MUST emit a `recovery_draft_unavailable` event so
            // support sees that the boot-time correlation point was
            // lost. The previous shape silently swallowed the error;
            // P7 restores the diagnostic surface by preserving the
            // upstream `AppError` and routing it to the log.
            match collect_pending_drafts(&db) {
                Ok(pending_drafts) => {
                    if !pending_drafts.is_empty() {
                        // P22: cap the list to keep one log line under
                        // a sane budget. 100 drafts is a generous
                        // upper bound for "user had multiple stories
                        // open mentally"; beyond that we summarize.
                        const MAX_LOGGED_DRAFTS: usize = 100;
                        let total = pending_drafts.len();
                        let mut story_ids = pending_drafts;
                        if total > MAX_LOGGED_DRAFTS {
                            story_ids.truncate(MAX_LOGGED_DRAFTS);
                            story_ids.push(format!(
                                "+{} more",
                                total - MAX_LOGGED_DRAFTS
                            ));
                        }
                        let _ = infrastructure::diagnostics::recovery_log::record_event(
                            app.handle(),
                            infrastructure::diagnostics::recovery_log::Event::InterruptedSessionDetected {
                                story_ids,
                            },
                        );
                    }
                }
                Err(err) => {
                    let _ = infrastructure::diagnostics::recovery_log::record_event(
                        app.handle(),
                        infrastructure::diagnostics::recovery_log::Event::RecoveryDraftUnavailable {
                            story_id: String::from("<boot_probe>"),
                            source: "detect_interruption",
                        },
                    );
                    // Drop the wrapped error: we already logged the
                    // category. Boot must continue — losing the probe
                    // is a support-visible degradation, not a fatal.
                    let _ = err;
                }
            }

            app.manage(AppState { db: Mutex::new(db) });
            Ok(())
        })
        // Handlers listed in flat alphabetical order on the COMMAND
        // name (not the module path) so a "where is foo registered"
        // grep pattern resolves at the obvious place, regardless of
        // which sub-module owns the function. The previous shape
        // grouped by module first which made the relative position
        // of two same-prefix commands non-obvious.
        .invoke_handler(tauri::generate_handler![
            commands::story::apply_recovery,
            commands::story::create_story,
            commands::story::discard_draft,
            commands::import_export::export_story_with_save_dialog,
            commands::library::get_library_overview,
            commands::story::get_story_detail,
            commands::story::read_recoverable_draft,
            commands::story::record_draft,
            commands::story::update_story,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
