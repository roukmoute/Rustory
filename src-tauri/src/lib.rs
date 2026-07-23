use std::sync::Mutex;

use tauri::{Emitter, Manager};

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
    /// Deletes a story from a writable device (delist `.pi` + remove content).
    /// Behind an `Arc` + `spawn_blocking` like the other device I/O; gated by
    /// the `delete_story` capability before any mutation.
    pub pack_deleter: std::sync::Arc<dyn infrastructure::device::DevicePackDeleter>,
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
    /// Source of the latest published official release for the
    /// once-per-launch update-availability read (`Update Availability
    /// Contract`). Behind an `Arc` + `spawn_blocking` like the two other
    /// networked sources (its deliberate neighbors). Networked, gated by
    /// the pure per-launch decision, never required by the core flow.
    pub update_release_source: std::sync::Arc<dyn infrastructure::updates::UpdateReleaseSource>,
    /// Session memo of the launch's update-availability verdict — the
    /// "one check per launch" bound, single-flight under concurrency
    /// (concurrent readers park until the shared verdict settles; no
    /// lock is ever held across the fetch). `Arc` so the
    /// `spawn_blocking` worker can own a handle.
    pub update_availability: std::sync::Arc<application::update::UpdateCheckMemo>,
    /// Gateway of the update-apply GESTURE (`Update Apply Contract`):
    /// the official Tauri updater behind a mockable trait, wired at
    /// setup with the compile-time public key and the canonical feed
    /// endpoint (+ env override). Only ever invoked AFTER the pure plan
    /// decision allowed the start — a keyless copy never reaches it.
    pub update_apply_gateway: std::sync::Arc<dyn infrastructure::updates::UpdateApplyGateway>,
    /// Session state of the update-apply gesture — the single-flight
    /// bound and the authoritative truth of `read_update_apply_state`.
    /// `Arc` so the `spawn_blocking` worker can own a handle. No
    /// persistence by contract.
    pub update_apply_session: std::sync::Arc<application::update::UpdateApplySession>,
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

/// Best-effort wake of the `main` window: a warm-channel OS-open intent
/// must make the living instance visible ("s'ouvre ou se réveille") even
/// when it sits minimized behind other windows. Every step is `let _ =`
/// by contract — a wake failure must never break the intent delivery
/// (the pull still serves the verdict).
fn wake_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Opportunistic frontier normalization of one raw argv candidate: a
/// UTF-8 string that parses as a `file://` URL becomes its filesystem
/// path (some launchers hand `%U`-style URIs instead of plain paths);
/// anything else — a plain path, a non-`file` URI, a non-UTF-8 byte
/// sequence (which cannot be a URL) — travels RAW, so an unsupported
/// input keeps producing its honest verdict instead of a mangled
/// `<cwd>/file:...` pseudo-path.
fn normalize_os_open_candidate(arg: std::ffi::OsString) -> std::ffi::OsString {
    if let Some(utf8) = arg.to_str() {
        if let Ok(url) = tauri::Url::parse(utf8) {
            if url.scheme() == "file" {
                if let Ok(path) = url.to_file_path() {
                    return path.into_os_string();
                }
            }
        }
    }
    arg
}

/// Warm-channel frontier glue shared by the single-instance relay and the
/// macOS `RunEvent::Opened` arm: offer the OS-provided candidates to the
/// intent state and, when an intent results, wake the window and signal
/// the living frontend (`os-open:requested`, empty payload — the verdict
/// is pulled by command). A FAILED emission is traced to the import log
/// (the intent stays pending — the next library-mount pull serves it, but
/// the lost wake-up must be diagnosable). Cold-start seeding does NOT go
/// through here: there is no frontend to signal yet, the library-mount
/// pull covers it.
fn receive_os_open_candidates(
    app: &tauri::AppHandle,
    candidates: &[std::ffi::OsString],
    cwd: &std::path::Path,
) {
    if application::import_export::OS_OPEN_STATE.offer(candidates, cwd) {
        wake_main_window(app);
        if app
            .emit(
                ipc::events::EVENT_OS_OPEN_REQUESTED,
                ipc::events::OsOpenRequestedEvent {},
            )
            .is_err()
        {
            let _ = infrastructure::diagnostics::import_log::record_event(
                app,
                infrastructure::diagnostics::import_log::Event::OsOpenSignalEmitFailed,
            );
        }
    }
}

/// Drop-channel frontier glue (see `ui-states.md#Drop Intent Contract`):
/// relay the window drag-drop lifecycle as EMPTY signals and, on a drop,
/// offer the paths to the DEDICATED drop intent state. Every decision
/// (filtering, replacement, generations, classification, verdicts) lives
/// in the pure application module — this glue only emits and offers.
///
/// - `Enter` with paths → `drop:hover` (the decorative overlay); an empty
///   `Enter` (an exotic non-file drag) signals nothing.
/// - `Leave` → `drop:hover-ended`.
/// - `Drop` → `drop:hover-ended` FIRST (`Leave` is not guaranteed after a
///   `Drop` on every platform), then the offer; if an intent results,
///   `drop:requested`. A FAILED emission is traced to the import log (the
///   intent stays pending — the next library-mount pull serves it).
/// - `Over` → NOTHING (a high-frequency stream, never relayed).
///
/// Hover emissions are best-effort (`let _ =`): a lost hover signal only
/// costs the decorative overlay. NO window wake, unlike the OS-open glue:
/// a drop lands on a window that is visible and frontal by definition of
/// the gesture.
fn receive_drop_event(app: &tauri::AppHandle, event: &tauri::DragDropEvent) {
    match event {
        tauri::DragDropEvent::Enter { paths, .. } if !paths.is_empty() => {
            let _ = app.emit(
                ipc::events::EVENT_DROP_HOVER,
                ipc::events::DropHoverEvent {},
            );
        }
        tauri::DragDropEvent::Leave => {
            let _ = app.emit(
                ipc::events::EVENT_DROP_HOVER_ENDED,
                ipc::events::DropHoverEndedEvent {},
            );
        }
        tauri::DragDropEvent::Drop { paths, .. } => {
            let _ = app.emit(
                ipc::events::EVENT_DROP_HOVER_ENDED,
                ipc::events::DropHoverEndedEvent {},
            );
            if application::import_export::DROP_INTENT_STATE.offer_dropped(paths.clone())
                && app
                    .emit(
                        ipc::events::EVENT_DROP_REQUESTED,
                        ipc::events::DropRequestedEvent {},
                    )
                    .is_err()
            {
                let _ = infrastructure::diagnostics::import_log::record_event(
                    app,
                    infrastructure::diagnostics::import_log::Event::DropSignalEmitFailed,
                );
            }
        }
        // `Over` floods at pointer frequency; an empty `Enter` (an exotic
        // non-file drag) has nothing to hover for; the enum is
        // non_exhaustive.
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // The single-instance plugin MUST be the first registered plugin
        // (documented requirement) so a second launch is intercepted
        // before anything else runs. Beyond the OS-open relay, it closes
        // a real risk: two manual instances on the same app_data_dir
        // would race the single-process recovery journals.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            // A second launch must make the living instance visible even
            // when it carries NO file: a bare relaunch (double-clicking
            // the app icon) is the generic single-instance expectation —
            // "I relaunch, the app appears". Unconditional and
            // best-effort, BEFORE the candidate offer; the offer-driven
            // wake inside `receive_os_open_candidates` stays as the
            // warm-channel contract for the OTHER reception arms. Not
            // unit-testable (a real second process cannot be provoked in
            // headless CI — the untestable-races pattern).
            wake_main_window(app);
            // argv is the SECOND instance's full argv (argv[0] = binary);
            // relative paths must resolve against the second instance's
            // OWN cwd — never this living process's. Known dependency
            // limit, degraded controlled: the plugin's relay API is
            // `Vec<String>` (UTF-8) — a non-UTF-8 argument would fail in
            // the SECOND (throwaway) process before reaching this
            // callback, so the living instance never panics; only the
            // cold-start seed below (our own code, `args_os`-based) is
            // guaranteed lossless.
            let candidates: Vec<std::ffi::OsString> = argv
                .get(1..)
                .unwrap_or(&[])
                .iter()
                .map(|arg| normalize_os_open_candidate(std::ffi::OsString::from(arg)))
                .collect();
            receive_os_open_candidates(app, &candidates, std::path::Path::new(&cwd));
        }))
        .plugin(tauri_plugin_dialog::init())
        // Official Tauri updater (`Update Apply Contract`), Rust-side
        // ONLY: no capability (the renderer never invokes the plugin —
        // the gesture rides our commands), no npm companion. The real
        // endpoint + public key are provided at runtime by the gateway;
        // the committed `plugins.updater` block stays a neutral shell
        // (the crate requires a deserializable config to register).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Cold-start OS-open seed: Windows/Linux hand the opened file
            // as a raw process argument. `args_os` (never `args`): a Unix
            // filename is a byte sequence — a legal non-UTF-8 path must
            // seed an intent, not panic the boot. NO emission here — the
            // frontend does not exist yet; the library-mount pull picks
            // the intent up. Harmless on macOS (gestures arrive as
            // RunEvent::Opened, never argv). A failed current_dir read
            // degrades to an empty base: an absolute argument still
            // resolves, a relative one will surface as the honest
            // transport failure on analysis.
            let cold_args: Vec<std::ffi::OsString> = std::env::args_os()
                .skip(1)
                .map(normalize_os_open_candidate)
                .collect();
            application::import_export::OS_OPEN_STATE.offer(
                &cold_args,
                &std::env::current_dir().unwrap_or_default(),
            );

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
                pack_deleter: std::sync::Arc::new(
                    infrastructure::device::SystemDevicePackDeleter,
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
                update_release_source: std::sync::Arc::new(
                    infrastructure::updates::GithubHttpReleaseSource::default(),
                ),
                update_availability: std::sync::Arc::new(
                    application::update::UpdateCheckMemo::new(),
                ),
                update_apply_gateway: std::sync::Arc::new(
                    // The compile-time trust chain: `option_env!` (never
                    // `env!`) — a keyless local build compiles and stays
                    // manual-guided by the plan decision; the gateway is
                    // then simply never invoked.
                    infrastructure::updates::TauriUpdaterGateway::new(
                        app.handle().clone(),
                        option_env!("RUSTORY_UPDATER_PUBKEY")
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    ),
                ),
                update_apply_session: std::sync::Arc::new(
                    application::update::UpdateApplySession::new(),
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
            commands::import_export::accept_structured_archive_creation,
            commands::import_export::accept_structured_creation,
            commands::story::add_node_option,
            commands::story::add_story_node,
            commands::import_export::analyze_artifact_for_import,
            commands::import_export::analyze_drop_request,
            commands::import_export::analyze_os_open_request,
            commands::import_export::analyze_structured_archive_for_creation,
            commands::import_export::analyze_structured_folder_for_creation,
            commands::story::apply_recovery,
            commands::story::attach_node_media,
            commands::story::create_story,
            commands::story::delete_stories,
            commands::story::delete_story_node,
            commands::story::discard_draft,
            commands::import_export::discard_drop_request,
            commands::story::discard_node_draft,
            commands::import_export::discard_os_open_request,
            commands::transfer::discard_transfer_outcome,
            commands::import_export::export_story_with_save_dialog,
            commands::import_export::fetch_rss_source_preview,
            commands::library::get_library_overview,
            commands::catalog::get_official_catalog_status,
            commands::story::get_story_detail,
            commands::catalog::import_official_catalog,
            commands::device::import_device_story,
            commands::device::delete_device_story,
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
            commands::settings::read_update_apply_plan,
            commands::settings::read_update_apply_state,
            commands::settings::read_update_availability,
            commands::catalog::refresh_official_catalog,
            commands::story::record_draft,
            commands::story::record_node_draft,
            commands::story::remove_node_media,
            commands::story::remove_node_option,
            commands::settings::restart_for_update,
            commands::device::set_device_story_title,
            commands::story::set_node_option_link,
            commands::transfer::start_prepare_story,
            commands::transfer::start_transfer_story,
            commands::settings::start_update_apply,
            commands::story::update_node_content,
            commands::story::update_story,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            // macOS delivers "open this file with Rustory" gestures as
            // RunEvent::Opened { urls } — cold AND warm starts, never
            // argv. The variant only exists under this cfg (docs.rs
            // targets Linux and hides it), and the Linux CI can neither
            // compile-check nor provoke it: the arm stays a THIN frontier
            // (URL→path conversion + the shared receive glue, all of
            // whose decisions are cross-platform unit-tested), documented
            // here rather than simulated (the untestable-races pattern).
            // A cold-start Opened lands before the frontend exists; the
            // emitted signal is then simply unheard and the library-mount
            // pull picks the seeded intent up.
            //
            // The raw-travels discipline of the argv frontier applies
            // here too: every convertible URL becomes its filesystem
            // path, every non-convertible one travels RAW (its full URL
            // text) toward its honest downstream verdict — two URLs with
            // one rotten still form an honest multi-file intent, a
            // single rotten one becomes an artifact intent whose
            // analysis names the refusal. No silent filter_map: a muted
            // no-op would hide the user's gesture.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = &event {
                let candidates: Vec<std::ffi::OsString> = urls
                    .iter()
                    .map(|url| match url.to_file_path() {
                        Ok(path) => path.into_os_string(),
                        Err(()) => std::ffi::OsString::from(url.to_string()),
                    })
                    .collect();
                receive_os_open_candidates(
                    app_handle,
                    &candidates,
                    &std::env::current_dir().unwrap_or_default(),
                );
            }
            // Window drag-drop, all platforms. For a `WindowContent`
            // webview (this project's single full-frame window), wry
            // SYNTHESIZES the webview drag-drop at the WINDOW level
            // (vendored tauri-runtime-wry 2.10.1, fn `create_webview`:
            // `WebviewKind::WindowContent → SynthesizedWindowEvent::DragDrop`)
            // — so this arm is the ONLY reception point; the
            // `RunEvent::WebviewEvent` arm concerns child webviews that
            // do not exist here and is deliberately not matched. If a
            // platform ever emitted both, the one-shot generational take
            // in the intent state would absorb the duplicate (a property
            // already covered by the take_if tests). The physical drag
            // cannot be provoked in headless CI — every decision lives in
            // the unit-tested application module; this arm stays a thin
            // observe-and-relay (the untestable-races pattern).
            if let tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::DragDrop(drag_drop),
                ..
            } = &event
            {
                receive_drop_event(app_handle, drag_drop);
            }
        });
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

    // ---- OS-open argv candidate normalization (cold start AND the
    // single-instance relay share this exact frontier function) ----

    #[test]
    fn normalize_converts_a_file_url_into_its_filesystem_path() {
        let normalized =
            normalize_os_open_candidate(std::ffi::OsString::from("file:///tmp/histoire.rustory"));
        assert_eq!(
            normalized,
            std::ffi::OsString::from("/tmp/histoire.rustory")
        );
    }

    #[test]
    fn normalize_decodes_percent_encoded_file_urls() {
        // `%20` in a `%U`-style URI must land as a real space in the path.
        let normalized = normalize_os_open_candidate(std::ffi::OsString::from(
            "file:///tmp/mon%20histoire.rustory",
        ));
        assert_eq!(
            normalized,
            std::ffi::OsString::from("/tmp/mon histoire.rustory")
        );
    }

    #[test]
    fn normalize_keeps_a_plain_path_raw() {
        let raw = std::ffi::OsString::from("/tmp/histoire.rustory");
        assert_eq!(normalize_os_open_candidate(raw.clone()), raw);
        let relative = std::ffi::OsString::from("histoire.rustory");
        assert_eq!(normalize_os_open_candidate(relative.clone()), relative);
    }

    #[test]
    fn normalize_keeps_a_non_file_or_unparseable_uri_raw() {
        // A non-file scheme stays raw — it becomes an honest verdict, never
        // a mangled pseudo-path.
        let https = std::ffi::OsString::from("https://exemple.fr/histoire.rustory");
        assert_eq!(normalize_os_open_candidate(https.clone()), https);
        // A file URL that cannot map to a local path stays raw too.
        let remote_host = std::ffi::OsString::from("file://autre-machine/partage/h.rustory");
        assert_eq!(
            normalize_os_open_candidate(remote_host.clone()),
            remote_host
        );
    }

    #[cfg(unix)]
    #[test]
    fn normalize_passes_a_non_utf8_argument_through_untouched() {
        use std::os::unix::ffi::OsStringExt;
        // A byte sequence that is not UTF-8 cannot be a URL — it travels
        // raw as a legal path candidate, never a panic, never a loss.
        let raw = std::ffi::OsString::from_vec(vec![b'/', b't', 0xff, b'.', b'r']);
        assert_eq!(normalize_os_open_candidate(raw.clone()), raw);
    }
}
