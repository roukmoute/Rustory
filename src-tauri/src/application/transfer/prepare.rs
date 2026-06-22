//! Story-preparation application service (orchestration of the job).
//!
//! Drives the preparation state machine `Preflight → Prepare → {prepared |
//! retryable}` for a selected local story against a connected supported Lunii.
//! Preparation is a LOCAL operation producing DERIVED artifacts: it never writes
//! to the device, never consults the `WriteStory` gate, and never mutates the
//! canonical story (FR18). The transfer-artifact descriptor is EPHEMERAL and
//! re-derivable — nothing is persisted (no migration, no job record).
//!
//! Flow (mirrors [`read_story_validation`](crate::application::device::preflight)
//! and [`read_transfer_preview`](crate::application::device::transfer)): the live
//! device is re-scanned FIRST; the local canonical facts are read AFTER the
//! device I/O under a SCOPED DB lock — never held across the scan or the
//! assembly. The `Prepare` phase is local and does not need the device to stay
//! plugged in.
//!
//! Two entry points:
//! - [`prepare_story`] runs the background job, EMITTING progress + a terminal
//!   event through an injected [`PreparationEventEmitter`] (the runtime stays in
//!   the command layer; this service only sees the trait). It returns a
//!   [`PreparationOutcome`] purely so the command can record a local trace.
//! - [`read_preparation_state`] is the AUTHORITATIVE re-read: it re-derives the
//!   current state on demand (no persistence), returning a
//!   [`PreparationStateView`] the IPC layer maps to the DTO.
//!
//! Stays Tauri-free: tests inject a `MockDeviceScanner`, a
//! `MockTransferArtifactSource`, a capturing emitter and a temp DB.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::OptionalExtension;

use crate::application::device::{
    check_operation_allowed, resolve_connected_lunii, ConnectedLuniiOutcome,
};
use crate::domain::device::SupportedOperation;
use crate::domain::shared::AppError;
use crate::domain::story::{validate_canonical, CanonicalBlocker, CanonicalStoryFacts};
use crate::domain::transfer::{
    ensure_descriptor_coherent, gate_prepare, verify_aggregate, PreparationFailureCause,
    PreparationPhase, PreparedArtifactKind, TransferArtifactDescriptor,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::device::{DeviceLibraryReader, DeviceScanner};
use crate::infrastructure::filesystem::{AssemblyPlan, AssemblySource, TransferArtifactSource};

/// Sink for the typed job events. The application owns the monotonic `sequence`
/// and the phase progression; the Tauri runtime impl (command layer) fills in
/// `jobId` / `jobType` / `targetStoryId` and the event names. Keeping it a trait
/// keeps `application` free of `tauri::*` and lets tests capture the sequence.
pub trait PreparationEventEmitter {
    /// A phase transition. `progress` is `None` unless a RELIABLE fraction is
    /// known (MVP sends no fake percentage).
    fn progress(&self, phase: PreparationPhase, progress: Option<f32>, sequence: u64);
    /// Successful terminal state.
    fn completed(&self, sequence: u64);
    /// Failure terminal state, with the canonical message + next gesture.
    fn failed(&self, message: &str, user_action: &str, sequence: u64);
}

/// What the background job produced — returned to the command for a local trace
/// only (the UI learns the truth from the events + the authoritative re-read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparationOutcome {
    /// Artifacts assembled and verified fresh.
    Prepared {
        descriptor: TransferArtifactDescriptor,
    },
    /// A functional failure — a terminal `retryable` job state (NOT an
    /// `AppError`). The local draft is preserved.
    Retryable { cause: PreparationFailureCause },
    /// A transport failure that prevented producing a terminal job state
    /// (e.g. the local store became unreadable). Surfaced as an `AppError`.
    Transport { error: AppError },
}

/// The authoritative re-read state. Never carries the transient `preflight` /
/// `preparing` phases (those are event-only); a synchronous re-derivation runs
/// to a resting/terminal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparationStateView {
    /// No readable supported device is connected — nothing to prepare.
    Idle,
    /// Artifacts assembled and fresh. NOT a transfer success.
    Prepared {
        device_identifier: String,
        story_id: String,
        story_title: String,
        target_cohort: String,
        /// Whether the assembled descriptor carries a device-format pack (an
        /// imported story) versus a native story with no pack. The send gate
        /// uses it to disable `Envoyer` BEFORE any write attempt on a native
        /// story, rather than failing server-side post-click.
        transferable: bool,
    },
    /// A recoverable failure consultable in context.
    Retryable {
        story_id: String,
        story_title: String,
        cause: PreparationFailureCause,
        blockers: Vec<CanonicalBlocker>,
    },
}

/// A confirmed preflight: the requested device is present + supported + read-
/// authorized, and the local canonical facts have been read.
struct ConfirmedPreflight {
    device_identifier: String,
    target_cohort: String,
    story_title: String,
    blockers: Vec<CanonicalBlocker>,
    plan: AssemblyPlan,
    /// Integrity baseline the assembled aggregate must reproduce.
    expected_aggregate: String,
}

/// Outcome of the read-only preflight. `NotConfirmed` carries the HONEST cause —
/// `DeviceChanged` when no readable supported device can be confirmed (gone /
/// swapped / unreadable library), or `Interrupted` on a budget/timeout — so a
/// timeout is never mislabeled "the device changed". A genuine local-store
/// transport failure (vanished story, unreadable DB) propagates as `Err`.
/// `Confirmed` is boxed: it dwarfs `NotConfirmed`, so boxing keeps the enum small.
enum Preflight {
    Confirmed(Box<ConfirmedPreflight>),
    NotConfirmed(PreparationFailureCause),
}

/// Run the preparation job, emitting progress + a terminal event. Returns the
/// outcome for the command's local trace.
#[allow(clippy::too_many_arguments)]
pub fn prepare_story(
    db: &Mutex<DbHandle>,
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    artifact_source: &dyn TransferArtifactSource,
    app_data_dir: &std::path::Path,
    story_id: &str,
    requested_device_identifier: &str,
    preflight_budget: Duration,
    assembly_budget: Duration,
    emitter: &dyn PreparationEventEmitter,
) -> PreparationOutcome {
    let mut sequence: u64 = 0;
    sequence += 1;
    emitter.progress(PreparationPhase::Preflight, None, sequence);

    let confirmed = match run_preflight(
        db,
        scanner,
        library_reader,
        story_id,
        Some(requested_device_identifier),
        preflight_budget,
    ) {
        Ok(Preflight::Confirmed(confirmed)) => *confirmed,
        Ok(Preflight::NotConfirmed(cause)) => {
            // The requested readable device is no longer confirmable during the
            // preflight (gone / swapped / unreadable library → DeviceChanged) or
            // the scan ran out of budget (Interrupted). Recoverable, draft
            // untouched. NEVER a preparation on stale/changed device data.
            return fail(emitter, &mut sequence, cause);
        }
        Err(error) => {
            // A genuine local-store transport failure (vanished story / unreadable
            // DB). It cannot produce a terminal job state — surface the AppError.
            sequence += 1;
            let action = error.user_action.clone().unwrap_or_default();
            emitter.failed(&error.message, &action, sequence);
            return PreparationOutcome::Transport { error };
        }
    };

    // Fail-closed: only a fully-clear preflight (`présumée transférable`)
    // proceeds. A fixable or blocking validation issue stops here.
    if gate_prepare(&confirmed.blockers).is_err() {
        return fail(
            emitter,
            &mut sequence,
            PreparationFailureCause::PreflightNotPassing,
        );
    }

    sequence += 1;
    emitter.progress(PreparationPhase::Prepare, None, sequence);

    match assemble_and_verify(artifact_source, app_data_dir, &confirmed, assembly_budget) {
        Ok(descriptor) => {
            sequence += 1;
            emitter.completed(sequence);
            PreparationOutcome::Prepared { descriptor }
        }
        Err(cause) => fail(emitter, &mut sequence, cause),
    }
}

/// Emit a functional failure terminal event and return the matching outcome.
fn fail(
    emitter: &dyn PreparationEventEmitter,
    sequence: &mut u64,
    cause: PreparationFailureCause,
) -> PreparationOutcome {
    *sequence += 1;
    let (message, action) = cause.copy();
    emitter.failed(message, action, *sequence);
    PreparationOutcome::Retryable { cause }
}

/// Authoritative re-read: re-derive the current preparation state on demand
/// (nothing is persisted, freshness over caching). Resolves whatever single
/// readable supported device is present (no requested identifier).
#[allow(clippy::too_many_arguments)]
pub fn read_preparation_state(
    db: &Mutex<DbHandle>,
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    artifact_source: &dyn TransferArtifactSource,
    app_data_dir: &std::path::Path,
    story_id: &str,
    preflight_budget: Duration,
    assembly_budget: Duration,
) -> Result<PreparationStateView, AppError> {
    let confirmed = match run_preflight(
        db,
        scanner,
        library_reader,
        story_id,
        None,
        preflight_budget,
    )? {
        Preflight::Confirmed(confirmed) => *confirmed,
        // No readable confirmable device (or a budget timeout) → nothing to
        // prepare right now; both honest causes fold to idle on a re-read.
        Preflight::NotConfirmed(_) => return Ok(PreparationStateView::Idle),
    };

    if gate_prepare(&confirmed.blockers).is_err() {
        return Ok(PreparationStateView::Retryable {
            story_id: story_id.to_string(),
            story_title: confirmed.story_title,
            cause: PreparationFailureCause::PreflightNotPassing,
            blockers: confirmed.blockers,
        });
    }

    match assemble_and_verify(artifact_source, app_data_dir, &confirmed, assembly_budget) {
        Ok(descriptor) => Ok(PreparationStateView::Prepared {
            device_identifier: confirmed.device_identifier,
            story_id: story_id.to_string(),
            story_title: confirmed.story_title,
            target_cohort: confirmed.target_cohort,
            // Transferable only when the descriptor carries device-format pack
            // bytes (an imported story). A native story (canonical structure
            // only) has no pack → the send gate disables `Envoyer` before any
            // write attempt, instead of letting it fail server-side post-click.
            transferable: descriptor
                .artifacts
                .iter()
                .any(|a| matches!(a.kind, PreparedArtifactKind::PackFile)),
        }),
        Err(cause) => Ok(PreparationStateView::Retryable {
            story_id: story_id.to_string(),
            story_title: confirmed.story_title,
            cause,
            // Assembly causes are not validation blockers — the cause copy
            // carries the next gesture.
            blockers: Vec::new(),
        }),
    }
}

/// Assemble the descriptor, verify its integrity against the recorded baseline,
/// and sanity-check its coherence. Read-only; the canonical story is never
/// mutated, so any error leaves the draft intact (FR18).
fn assemble_and_verify(
    artifact_source: &dyn TransferArtifactSource,
    app_data_dir: &std::path::Path,
    confirmed: &ConfirmedPreflight,
    assembly_budget: Duration,
) -> Result<TransferArtifactDescriptor, PreparationFailureCause> {
    let descriptor = artifact_source.assemble(app_data_dir, &confirmed.plan, assembly_budget)?;
    verify_aggregate(&descriptor, &confirmed.expected_aggregate)?;
    ensure_descriptor_coherent(&descriptor)?;
    Ok(descriptor)
}

/// Re-scan the device, confirm the requested identity (when one is given), pass
/// the `ReadLibrary` gate, PROVE the inventory is actually readable, then read
/// the local canonical facts under a scoped lock taken AFTER the device I/O.
/// Returns `Preflight::NotConfirmed(cause)` with an honest cause when no readable
/// device is confirmable, or `Err` for a genuine local-store / scan transport
/// failure.
fn run_preflight(
    db: &Mutex<DbHandle>,
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    story_id: &str,
    requested_device_identifier: Option<&str>,
    budget: Duration,
) -> Result<Preflight, AppError> {
    let started = Instant::now();

    // Re-scan. F2: a scan TIMEOUT is an honest `Interrupted` (budget), NOT
    // "device changed"; any other scan transport failure is surfaced explicitly.
    let resolved = match resolve_connected_lunii(scanner, budget) {
        Ok(resolved) => resolved,
        Err(err) => {
            if details_source(&err) == Some("scan_timeout") {
                return Ok(Preflight::NotConfirmed(
                    PreparationFailureCause::Interrupted,
                ));
            }
            return Err(err);
        }
    };

    let profile = match resolved.outcome {
        ConnectedLuniiOutcome::Supported(profile) => profile,
        // None / Unsupported / Ambiguous: the requested readable supported
        // device cannot be confirmed.
        _ => {
            return Ok(Preflight::NotConfirmed(
                PreparationFailureCause::DeviceChanged,
            ))
        }
    };

    if let Some(requested) = requested_device_identifier {
        if profile.device_identifier != requested {
            return Ok(Preflight::NotConfirmed(
                PreparationFailureCause::DeviceChanged,
            ));
        }
    }

    // Fail-closed gate (read is allowed for every supported cohort, but the
    // policy is enforced in one place). NEVER `WriteStory` — preparation is
    // local and orthogonal to it.
    if check_operation_allowed(&profile, SupportedOperation::ReadLibrary).is_err() {
        return Ok(Preflight::NotConfirmed(
            PreparationFailureCause::DeviceChanged,
        ));
    }

    // F1: PROVE the inventory is actually readable before authorising prepare
    // (reuse the proven inventory read). A Lunii whose markers are detectable but
    // whose library became unreadable after detection must end recoverably, never
    // slip into preparation on a device we cannot read.
    let mount_path = match resolved.supported_mount_path {
        Some(path) => path,
        // Defensive: a `Supported` outcome always carries a mount path.
        None => {
            return Ok(Preflight::NotConfirmed(
                PreparationFailureCause::DeviceChanged,
            ))
        }
    };
    let remaining = budget.saturating_sub(started.elapsed());
    if let Err(err) = library_reader.read_library(&mount_path, remaining) {
        // A read timeout is `Interrupted` (budget); any other read failure means
        // the device is no longer readable → `DeviceChanged`.
        let cause = if details_source(&err) == Some("read_timeout") {
            PreparationFailureCause::Interrupted
        } else {
            PreparationFailureCause::DeviceChanged
        };
        return Ok(Preflight::NotConfirmed(cause));
    }

    let facts = read_prepare_facts(db, story_id)?;
    let blockers = validate_canonical(&facts.facts);

    let (source, expected_aggregate) = match facts.pack_checksum {
        // An imported raw pack: re-checksum the promoted files, baseline is the
        // pack checksum the import recorded.
        Some(pack_checksum) => (AssemblySource::ImportedPack, pack_checksum),
        // A native minimal story: the canonical structure is the artifact, the
        // baseline is its `content_checksum`.
        None => (
            AssemblySource::Native {
                structure_json: facts.facts.structure_json.clone(),
            },
            facts.facts.content_checksum.clone(),
        ),
    };
    let target_cohort = profile.firmware_cohort.diagnostic_tag().to_string();

    Ok(Preflight::Confirmed(Box::new(ConfirmedPreflight {
        device_identifier: profile.device_identifier,
        plan: AssemblyPlan {
            story_id: story_id.to_string(),
            target_cohort: target_cohort.clone(),
            source,
        },
        target_cohort,
        story_title: facts.facts.title.clone(),
        blockers,
        expected_aggregate,
    })))
}

/// Read the closed-set `details.source` token of an `AppError`, when present.
fn details_source(err: &AppError) -> Option<&str> {
    err.details.as_ref()?.get("source")?.as_str()
}

/// The local facts a preparation needs: the canonical facts (for validation +
/// the native baseline) plus the imported-pack checksum when the story came from
/// a device copy. Read under a scoped lock taken here, never held across I/O.
struct PrepareFacts {
    facts: CanonicalStoryFacts,
    pack_checksum: Option<String>,
}

fn read_prepare_facts(db: &Mutex<DbHandle>, story_id: &str) -> Result<PrepareFacts, AppError> {
    let db = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let row: Option<(String, u32, String, String)> = db
        .conn()
        .query_row(
            "SELECT title, schema_version, structure_json, content_checksum \
             FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|_| local_read_error("select_facts"))?;
    let Some((title, schema_version, structure_json, content_checksum)) = row else {
        return Err(story_missing_error());
    };

    let pack_checksum: Option<String> = db
        .conn()
        .query_row(
            "SELECT pack_checksum FROM story_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| local_read_error("select_pack_checksum"))?;

    Ok(PrepareFacts {
        facts: CanonicalStoryFacts {
            title,
            schema_version,
            structure_json,
            content_checksum,
        },
        pack_checksum,
    })
}

fn story_missing_error() -> AppError {
    AppError::library_inconsistent(
        "Préparation impossible: histoire introuvable dans la bibliothèque locale.",
        "Recharge la bibliothèque puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "story_preparation",
        "cause": "story_missing",
    }))
}

fn local_read_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Préparation indisponible: vérifie le disque local et réessaie.",
        "Réessaie ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "story_preparation",
        "stage": stage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::story::content_checksum;
    use crate::domain::transfer::{
        PreparedArtifact, PreparedArtifactKind, PREPARATION_PIPELINE_VERSION,
    };
    use crate::infrastructure::db;
    use crate::infrastructure::device::{
        compute_device_identifier, MockDeviceLibraryReader, MockDeviceScanner,
    };
    use crate::infrastructure::filesystem::MockTransferArtifactSource;

    const HEALTHY_JSON: &str = "{\"schemaVersion\":1,\"nodes\":[]}";

    fn budget() -> Duration {
        Duration::from_secs(5)
    }

    fn mock_identifier() -> String {
        compute_device_identifier(b"MOCK_PI", Some("MOCK_SERIAL"))
    }

    fn fresh_db() -> Mutex<DbHandle> {
        let mut handle = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut handle).expect("migrate");
        Mutex::new(handle)
    }

    fn insert_story(db: &Mutex<DbHandle>, id: &str, structure_json: &str, content_checksum: &str) {
        db.lock()
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES (?1, 'Mon histoire', 1, ?2, ?3, '2026-06-22T00:00:00.000Z', '2026-06-22T00:00:00.000Z')",
                rusqlite::params![id, structure_json, content_checksum],
            )
            .expect("insert story");
    }

    fn insert_healthy(db: &Mutex<DbHandle>, id: &str) {
        insert_story(db, id, HEALTHY_JSON, &content_checksum(HEALTHY_JSON));
    }

    /// A coherent native descriptor whose aggregate matches the healthy story's
    /// `content_checksum` so `verify_aggregate` passes.
    fn healthy_native_descriptor(story_id: &str) -> TransferArtifactDescriptor {
        let checksum = content_checksum(HEALTHY_JSON);
        TransferArtifactDescriptor {
            story_id: story_id.into(),
            target_cohort: "origine_v1".into(),
            pipeline_version: PREPARATION_PIPELINE_VERSION,
            artifacts: vec![PreparedArtifact {
                kind: PreparedArtifactKind::CanonicalStructure,
                relative_ref: "structure.json".into(),
                byte_len: HEALTHY_JSON.len() as u64,
                checksum: checksum.clone(),
            }],
            aggregate_checksum: checksum,
        }
    }

    /// An imported-pack descriptor: a single device-format `PackFile` artifact
    /// whose aggregate matches the recorded `pack_checksum`. Marks the prepared
    /// story TRANSFERABLE (it carries device-format bytes).
    fn imported_pack_descriptor(story_id: &str, pack_checksum: &str) -> TransferArtifactDescriptor {
        TransferArtifactDescriptor {
            story_id: story_id.into(),
            target_cohort: "origine_v1".into(),
            pipeline_version: PREPARATION_PIPELINE_VERSION,
            artifacts: vec![PreparedArtifact {
                kind: PreparedArtifactKind::PackFile,
                relative_ref: "ni".into(),
                byte_len: 16,
                checksum: pack_checksum.into(),
            }],
            aggregate_checksum: pack_checksum.into(),
        }
    }

    /// Seed a `story_imports` row so the preparation reads the story as an
    /// imported pack (its integrity baseline becomes `pack_checksum`).
    fn insert_import(db: &Mutex<DbHandle>, story_id: &str, pack_checksum: &str) {
        db.lock()
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
                 VALUES (?1, '0197a5d0-0000-7000-8000-0000000000aa', 'devhash', '2026-06-22T00:00:00.000Z', 1, 16, ?2)",
                rusqlite::params![story_id, pack_checksum],
            )
            .expect("insert story_imports");
    }

    #[derive(Default)]
    struct CapturingEmitter {
        events: Mutex<Vec<Recorded>>,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Recorded {
        Progress {
            phase: PreparationPhase,
            sequence: u64,
        },
        Completed {
            sequence: u64,
        },
        Failed {
            sequence: u64,
        },
    }

    impl PreparationEventEmitter for CapturingEmitter {
        fn progress(&self, phase: PreparationPhase, _progress: Option<f32>, sequence: u64) {
            self.events
                .lock()
                .unwrap()
                .push(Recorded::Progress { phase, sequence });
        }
        fn completed(&self, sequence: u64) {
            self.events
                .lock()
                .unwrap()
                .push(Recorded::Completed { sequence });
        }
        fn failed(&self, _message: &str, _user_action: &str, sequence: u64) {
            self.events
                .lock()
                .unwrap()
                .push(Recorded::Failed { sequence });
        }
    }

    impl CapturingEmitter {
        fn recorded(&self) -> Vec<Recorded> {
            self.events.lock().unwrap().clone()
        }
        fn sequences(&self) -> Vec<u64> {
            self.recorded()
                .iter()
                .map(|e| match e {
                    Recorded::Progress { sequence, .. } => *sequence,
                    Recorded::Completed { sequence } => *sequence,
                    Recorded::Failed { sequence } => *sequence,
                })
                .collect()
        }
    }

    fn supported_scanner(version: u8) -> MockDeviceScanner {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(version);
        scanner
    }

    /// A reader that confirms a readable inventory — the F1 preflight reads it to
    /// prove the device library is actually readable before authorising prepare.
    fn readable_reader() -> MockDeviceLibraryReader {
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1);
        reader
    }

    fn is_monotonic(seqs: &[u64]) -> bool {
        seqs.windows(2).all(|w| w[1] == w[0] + 1) && seqs.first() == Some(&1)
    }

    #[test]
    fn prepared_for_healthy_native_story_on_supported_device() {
        let dir = tempfile::tempdir().expect("app data");
        for version in [3u8, 6, 7] {
            let db = fresh_db();
            insert_healthy(&db, "s1");
            let scanner = supported_scanner(version);
            let reader = readable_reader();
            let artifacts = MockTransferArtifactSource::new();
            artifacts.enqueue(Ok(healthy_native_descriptor("s1")));
            let emitter = CapturingEmitter::default();

            let outcome = prepare_story(
                &db,
                &scanner,
                &reader,
                &artifacts,
                dir.path(),
                "s1",
                &mock_identifier(),
                budget(),
                budget(),
                &emitter,
            );
            assert!(
                matches!(outcome, PreparationOutcome::Prepared { .. }),
                "md v{version}: {outcome:?}"
            );
            assert_eq!(
                emitter.recorded(),
                vec![
                    Recorded::Progress {
                        phase: PreparationPhase::Preflight,
                        sequence: 1
                    },
                    Recorded::Progress {
                        phase: PreparationPhase::Prepare,
                        sequence: 2
                    },
                    Recorded::Completed { sequence: 3 },
                ],
                "md v{version}"
            );
            assert!(is_monotonic(&emitter.sequences()));
        }
    }

    #[test]
    fn retryable_preflight_not_passing_when_checksum_is_corrupt() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        // Healthy structure but a corrupt stored checksum → ChecksumMismatch.
        insert_story(&db, "s1", HEALTHY_JSON, &"0".repeat(64));
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new(); // never assembled
        let emitter = CapturingEmitter::default();

        let outcome = prepare_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            PreparationOutcome::Retryable {
                cause: PreparationFailureCause::PreflightNotPassing
            }
        );
        // The prepare phase was never entered.
        assert_eq!(
            emitter.recorded(),
            vec![
                Recorded::Progress {
                    phase: PreparationPhase::Preflight,
                    sequence: 1
                },
                Recorded::Failed { sequence: 2 },
            ]
        );
    }

    #[test]
    fn retryable_device_changed_when_identifier_mismatches() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        let emitter = CapturingEmitter::default();

        let outcome = prepare_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            "deadbeefdeadbeefdeadbeefdeadbeef",
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            PreparationOutcome::Retryable {
                cause: PreparationFailureCause::DeviceChanged
            }
        );
    }

    #[test]
    fn retryable_device_changed_when_no_device_present() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_no_device();
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        let emitter = CapturingEmitter::default();

        let outcome = prepare_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            PreparationOutcome::Retryable {
                cause: PreparationFailureCause::DeviceChanged
            }
        );
    }

    #[test]
    fn retryable_device_changed_when_the_library_is_unreadable() {
        // F1: the device is detected + supported, but its library is no longer
        // readable → preparation must end recoverably, never slip into prepare.
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let scanner = supported_scanner(7);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_disconnected_mid_read();
        let artifacts = MockTransferArtifactSource::new(); // never assembled
        let emitter = CapturingEmitter::default();

        let outcome = prepare_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            PreparationOutcome::Retryable {
                cause: PreparationFailureCause::DeviceChanged
            }
        );
    }

    #[test]
    fn retryable_interrupted_when_the_scan_times_out() {
        // F2: a scan timeout/budget is an honest `Interrupted`, NOT "device
        // changed".
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_timeout_truncated();
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        let emitter = CapturingEmitter::default();

        let outcome = prepare_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            PreparationOutcome::Retryable {
                cause: PreparationFailureCause::Interrupted
            }
        );
    }

    #[test]
    fn retryable_artifact_missing_propagates_assembly_failure() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let scanner = supported_scanner(7);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Err(PreparationFailureCause::ArtifactMissing));
        let emitter = CapturingEmitter::default();

        let outcome = prepare_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            PreparationOutcome::Retryable {
                cause: PreparationFailureCause::ArtifactMissing
            }
        );
        // Preflight → Prepare → failed: the prepare phase WAS entered.
        assert_eq!(
            emitter.recorded(),
            vec![
                Recorded::Progress {
                    phase: PreparationPhase::Preflight,
                    sequence: 1
                },
                Recorded::Progress {
                    phase: PreparationPhase::Prepare,
                    sequence: 2
                },
                Recorded::Failed { sequence: 3 },
            ]
        );
    }

    #[test]
    fn retryable_artifact_corrupt_when_aggregate_mismatches_baseline() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let scanner = supported_scanner(7);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        // The assembler returns a descriptor whose aggregate disagrees with the
        // story's content_checksum baseline → ArtifactCorrupt.
        let mut bad = healthy_native_descriptor("s1");
        bad.aggregate_checksum = "f".repeat(64);
        artifacts.enqueue(Ok(bad));
        let emitter = CapturingEmitter::default();

        let outcome = prepare_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            PreparationOutcome::Retryable {
                cause: PreparationFailureCause::ArtifactCorrupt
            }
        );
    }

    #[test]
    fn canonical_story_is_unchanged_after_a_failure() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let scanner = supported_scanner(7);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Err(PreparationFailureCause::ArtifactCorrupt));
        let emitter = CapturingEmitter::default();

        let before: (String, String) = read_story_row(&db, "s1");
        let _ = prepare_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        let after = read_story_row(&db, "s1");
        assert_eq!(
            before, after,
            "preparation must never mutate the canonical row"
        );
    }

    fn read_story_row(db: &Mutex<DbHandle>, id: &str) -> (String, String) {
        db.lock()
            .unwrap()
            .conn()
            .query_row(
                "SELECT structure_json, content_checksum FROM stories WHERE id = ?1",
                rusqlite::params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read row")
    }

    #[test]
    fn read_preparation_state_returns_idle_without_a_device() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_no_device();
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();

        let view = read_preparation_state(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            budget(),
            budget(),
        )
        .expect("read state");
        assert_eq!(view, PreparationStateView::Idle);
    }

    #[test]
    fn read_preparation_state_returns_prepared_for_healthy_story() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(healthy_native_descriptor("s1")));

        let view = read_preparation_state(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            budget(),
            budget(),
        )
        .expect("read state");
        match view {
            PreparationStateView::Prepared {
                story_title,
                target_cohort,
                transferable,
                ..
            } => {
                assert_eq!(story_title, "Mon histoire");
                assert_eq!(target_cohort, "origine_v1");
                assert!(
                    !transferable,
                    "a native story has no device-format pack → not transferable"
                );
            }
            other => panic!("expected Prepared, got {other:?}"),
        }
    }

    #[test]
    fn read_preparation_state_marks_an_imported_story_transferable() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_healthy(&db, "s1");
        let pack_checksum = "c".repeat(64);
        insert_import(&db, "s1", &pack_checksum);
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_pack_descriptor("s1", &pack_checksum)));

        let view = read_preparation_state(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            budget(),
            budget(),
        )
        .expect("read state");
        match view {
            PreparationStateView::Prepared { transferable, .. } => assert!(
                transferable,
                "an imported story carries a device-format pack → transferable"
            ),
            other => panic!("expected Prepared, got {other:?}"),
        }
    }

    #[test]
    fn read_preparation_state_reports_preflight_blockers() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1", HEALTHY_JSON, &"0".repeat(64));
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();

        let view = read_preparation_state(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            budget(),
            budget(),
        )
        .expect("read state");
        match view {
            PreparationStateView::Retryable {
                cause, blockers, ..
            } => {
                assert_eq!(cause, PreparationFailureCause::PreflightNotPassing);
                assert!(!blockers.is_empty(), "preflight blockers must be reported");
            }
            other => panic!("expected Retryable(PreflightNotPassing), got {other:?}"),
        }
    }

    #[test]
    fn missing_story_is_a_transport_error() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        // No story seeded.
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        let emitter = CapturingEmitter::default();

        let outcome = prepare_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "ghost",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        match outcome {
            PreparationOutcome::Transport { error } => {
                assert_eq!(
                    error.code,
                    crate::domain::shared::AppErrorCode::LibraryInconsistent
                );
            }
            other => panic!("expected Transport, got {other:?}"),
        }
    }
}
