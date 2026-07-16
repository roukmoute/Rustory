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
    /// I/O. Networked, never invoked implicitly.
    pub catalog_source: std::sync::Arc<dyn infrastructure::device::OfficialCatalogSource>,
    /// Source of a user-provided RSS feed for the EXPLICIT external-source
    /// creation flow. Behind an `Arc` + `spawn_blocking` like the catalog
    /// source (its deliberate neighbor — dedicated client, same
    /// disciplines). Networked, never invoked implicitly.
    pub rss_source: std::sync::Arc<dyn infrastructure::device::RssFeedSource>,
    /// Read-only assembler of the transfer-artifact descriptor used by the
    /// story-preparation flow. Same `Arc` + `spawn_blocking` discipline; the
    /// production impl reads the local canonical structure / promoted pack and
    /// re-checksums it. Never writes, never touches the device.
    pub artifact_source: std::sync::Arc<dyn infrastructure::filesystem::TransferArtifactSource>,
    /// Safe/atomic device writer used by the story-transfer flow to reproduce a
    /// prepared pack on a connected writable Lunii. Same `Arc` + `spawn_blocking`
    /// discipline; the production impl stages → promotes → fsyncs → updates `.pi`.
    /// Reached only AFTER the `WriteStory` capability gate passes.
    pub pack_writer: std::sync::Arc<dyn infrastructure::device::DevicePackWriter>,
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

/// Read the story ids that still have a NON-success durable transfer outcome.
/// Ordered by `recorded_at DESC` so the most recently remembered transfer shows up
/// first and CAPPED in SQL (`LIMIT`) so a runaway table never loads unbounded rows
/// at boot. `verified` rows are EXCLUDED: a successful transfer that was simply
/// quit has nothing pending to acknowledge (it is never re-surfaced by `hydrate`),
/// so it must not raise a false `interrupted_transfer_detected`; the `verified` row
/// is kept only for the "latest wins" overwrite of a failure by a successful
/// relaunch. A surviving non-success row means a transfer reached a recoverable
/// terminal and was never acknowledged (Relancer / Abandonner) — the boot probe
/// surfaces that to the trace; the per-story re-hydration is driven by the
/// route-level reads. On failure it returns the stable stage tag so the caller can
/// log the degradation (it never propagates a fatal — losing the probe must not
/// block boot).
fn collect_pending_transfer_outcomes(
    db: &infrastructure::db::DbHandle,
) -> Result<Vec<String>, &'static str> {
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT story_id FROM transfer_jobs \
             WHERE terminal_kind <> 'verified' \
             ORDER BY recorded_at DESC LIMIT 100",
        )
        .map_err(|_| "boot_probe_prepare")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| "boot_probe_query")?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "boot_probe_collect")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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

            // Boot-time sweep of node-media staging residue: a crash mid-attach
            // can leave a `.staging/*.tmp` file that was never promoted. Best
            // effort — a sweep failure never blocks the boot.
            infrastructure::filesystem::sweep_node_media_staging(
                &infrastructure::filesystem::resolve_node_media_staging_dir(&app_data_dir),
            );

            // Boot-time reconciliation of PROMOTED node media: delete any file
            // referenced by no `assets` row (a crash between commit and the GC,
            // a GC that failed, or a promote-then-refused attach). Best-effort.
            application::story::node::sweep_orphan_node_media(&db, &app_data_dir);

            // Boot-time transfer-resume probe: any durable transfer outcome
            // surviving the previous session indicates a transfer that reached a
            // terminal and was never acknowledged (Relancer / Abandonner). Emit a
            // single `interrupted_transfer_detected` into transfer.jsonl so the
            // trace carries the chronology — the per-story re-hydration is driven by
            // the route-level reads, not this probe. PII-free: each id is hashed to
            // a short `story_ref`. A read failure is a support-visible degradation,
            // not a fatal: boot must continue.
            match collect_pending_transfer_outcomes(&db) {
                Ok(pending_outcomes) => {
                    if !pending_outcomes.is_empty() {
                        // The SQL `LIMIT` already caps the row count; just hash each id.
                        let story_refs: Vec<String> = pending_outcomes
                            .iter()
                            .map(|id| infrastructure::diagnostics::transfer::story_ref(id))
                            .collect();
                        let _ = infrastructure::diagnostics::transfer::record_event(
                            app.handle(),
                            infrastructure::diagnostics::transfer::Event::InterruptedTransferDetected {
                                story_refs,
                            },
                        );
                    }
                }
                Err(source) => {
                    // Log the probe read failure (a support-visible degradation of the
                    // trace surface) instead of swallowing it — boot still continues.
                    let _ = infrastructure::diagnostics::transfer::record_event(
                        app.handle(),
                        infrastructure::diagnostics::transfer::Event::InterruptedTransferProbeFailed {
                            source,
                        },
                    );
                }
            }

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
                rss_source: std::sync::Arc::new(
                    infrastructure::device::HttpRssFeedSource::default(),
                ),
                artifact_source: std::sync::Arc::new(
                    infrastructure::filesystem::SystemTransferArtifactSource,
                ),
                pack_writer: std::sync::Arc::new(
                    infrastructure::device::SystemDevicePackWriter,
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
            commands::import_export::accept_artifact_import,
            commands::import_export::accept_rss_story_creation,
            commands::import_export::accept_structured_creation,
            commands::story::add_node_option,
            commands::story::add_story_node,
            commands::import_export::analyze_artifact_for_import,
            commands::import_export::analyze_structured_folder_for_creation,
            commands::story::apply_recovery,
            commands::story::attach_node_media,
            commands::story::create_story,
            commands::story::delete_story_node,
            commands::story::discard_draft,
            commands::story::discard_node_draft,
            commands::transfer::discard_transfer_outcome,
            commands::import_export::export_story_with_save_dialog,
            commands::import_export::fetch_rss_source_preview,
            commands::library::get_library_overview,
            commands::catalog::get_official_catalog_status,
            commands::story::get_story_detail,
            commands::catalog::import_official_catalog,
            commands::device::import_device_story,
            commands::story::move_story_node,
            commands::device::read_connected_lunii,
            commands::import_export::read_content_source_policy,
            commands::device::read_device_library,
            commands::story::read_node_media,
            commands::catalog::read_pack_cover,
            commands::transfer::read_preparation_state,
            commands::story::read_recoverable_draft,
            commands::story::read_recoverable_node_draft,
            commands::device::read_story_validation,
            commands::settings::read_support_profile,
            commands::transfer::read_transfer_outcome,
            commands::device::read_transfer_preview,
            commands::transfer::read_transfer_state,
            commands::catalog::refresh_official_catalog,
            commands::story::record_draft,
            commands::story::record_node_draft,
            commands::story::remove_node_media,
            commands::story::remove_node_option,
            commands::device::set_device_story_title,
            commands::story::set_node_option_link,
            commands::transfer::start_prepare_story,
            commands::transfer::start_transfer_story,
            commands::story::update_node_content,
            commands::story::update_story,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use infrastructure::db;

    fn seed_story(handle: &db::DbHandle, id: &str) {
        handle
            .conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES (?1, 'Dummy', 1, '{\"schemaVersion\":1,\"nodes\":[]}', \
                 '0000000000000000000000000000000000000000000000000000000000000000', \
                 '2026-06-23T00:00:00.000Z', '2026-06-23T00:00:00.000Z')",
                rusqlite::params![id],
            )
            .expect("seed story");
    }

    fn seed_outcome(handle: &db::DbHandle, story_id: &str, terminal_kind: &str, recorded_at: &str) {
        handle
            .conn()
            .execute(
                "INSERT INTO transfer_jobs (story_id, job_id, terminal_kind, message, user_action, recorded_at) \
                 VALUES (?1, 'job', ?2, 'm', 'a', ?3)",
                rusqlite::params![story_id, terminal_kind, recorded_at],
            )
            .expect("seed outcome");
    }

    #[test]
    fn boot_probe_excludes_verified_and_keeps_non_success_terminals() {
        let mut handle = db::open_in_memory().expect("open");
        db::run_migrations(&mut handle).expect("migrate");
        seed_story(&handle, "s-verified");
        seed_story(&handle, "s-retryable");
        seed_story(&handle, "s-incomplete");
        // A quit-after-success leaves a `verified` row — it must NOT be probed.
        seed_outcome(
            &handle,
            "s-verified",
            "verified",
            "2026-06-23T00:00:03.000Z",
        );
        seed_outcome(
            &handle,
            "s-retryable",
            "retryable",
            "2026-06-23T00:00:02.000Z",
        );
        seed_outcome(
            &handle,
            "s-incomplete",
            "incomplete",
            "2026-06-23T00:00:01.000Z",
        );

        let pending = collect_pending_transfer_outcomes(&handle).expect("probe");
        // Only the NON-success terminals, most-recent first; `verified` is excluded.
        assert_eq!(pending, vec!["s-retryable", "s-incomplete"]);
    }

    #[test]
    fn boot_probe_is_empty_when_only_verified_rows_remain() {
        let mut handle = db::open_in_memory().expect("open");
        db::run_migrations(&mut handle).expect("migrate");
        seed_story(&handle, "s1");
        seed_outcome(&handle, "s1", "verified", "2026-06-23T00:00:00.000Z");

        assert!(collect_pending_transfer_outcomes(&handle)
            .expect("probe")
            .is_empty());
    }
}
