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
/// be over-engineered for the current surface. The mutex itself sits in an
/// [`Arc`] so a `spawn_blocking` worker (which requires `'static`) can own
/// a handle for the duration of a long device import while every other
/// command keeps locking through `state.db` unchanged (Deref).
///
/// `device_scanner` carries the [`infrastructure::device::DeviceScanner`]
/// trait object behind an [`Arc`] so the async `read_connected_lunii`
/// command can hand it to `spawn_blocking` (which requires `'static`)
/// without contention with the DB mutex. Production wires the
/// `sysinfo`-backed system scanner; tests inject a mock.
///
/// `library_reader` is the [`infrastructure::device::DeviceLibraryReader`]
/// used by `read_device_library` to enumerate a connected Lunii's pack
/// inventory at its mount path. Same `Arc` + `spawn_blocking` discipline
/// as `device_scanner`; production wires the stdlib filesystem reader.
///
/// `pack_reader` is the [`infrastructure::device::DevicePackReader`] used
/// by `import_device_story` to acquire a pack's bytes into the local
/// staging area. Same discipline; production wires the stdlib copier.
pub struct AppState {
    pub db: std::sync::Arc<Mutex<infrastructure::db::DbHandle>>,
    pub device_scanner: std::sync::Arc<dyn infrastructure::device::DeviceScanner>,
    pub library_reader: std::sync::Arc<dyn infrastructure::device::DeviceLibraryReader>,
    pub pack_reader: std::sync::Arc<dyn infrastructure::device::DevicePackReader>,
    /// Source of the official catalog for the EXPLICIT fetch (story 2-6,
    /// Phase C). Behind an `Arc` + `spawn_blocking` like the other device
    /// I/O. The only networked component; never invoked implicitly.
    pub catalog_source: std::sync::Arc<dyn infrastructure::device::OfficialCatalogSource>,
    /// Read-only assembler of the transfer-artifact descriptor used by the
    /// story-preparation flow. Same `Arc` + `spawn_blocking` discipline; the
    /// production impl reads the local canonical structure / promoted pack and
    /// re-checksums it. Never writes, never touches the device.
    pub artifact_source: std::sync::Arc<dyn infrastructure::filesystem::TransferArtifactSource>,
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

            // Boot-time sweep of import residues: stale staging entries
            // (crash mid-acquisition) and promoted folders without their
            // provenance row (crash between promotion and commit).
            // Best-effort by contract — a sweep failure never blocks the
            // boot; the residues are retried at the next launch.
            let _ = application::device::import::sweep_import_artifacts(&db, &app_data_dir);

            app.manage(AppState {
                db: std::sync::Arc::new(Mutex::new(db)),
                device_scanner: std::sync::Arc::new(
                    infrastructure::device::SystemDeviceScanner::default(),
                ),
                library_reader: std::sync::Arc::new(
                    infrastructure::device::SystemDeviceLibraryReader,
                ),
                pack_reader: std::sync::Arc::new(
                    infrastructure::device::SystemDevicePackReader,
                ),
                catalog_source: std::sync::Arc::new(
                    infrastructure::device::LuniiHttpCatalogSource::default(),
                ),
                artifact_source: std::sync::Arc::new(
                    infrastructure::filesystem::SystemTransferArtifactSource,
                ),
            });
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
            commands::catalog::get_official_catalog_status,
            commands::story::get_story_detail,
            commands::catalog::import_official_catalog,
            commands::device::import_device_story,
            commands::device::read_connected_lunii,
            commands::device::read_device_library,
            commands::catalog::read_pack_cover,
            commands::transfer::read_preparation_state,
            commands::story::read_recoverable_draft,
            commands::device::read_story_validation,
            commands::device::read_transfer_preview,
            commands::catalog::refresh_official_catalog,
            commands::story::record_draft,
            commands::device::set_device_story_title,
            commands::transfer::start_prepare_story,
            commands::story::update_story,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
