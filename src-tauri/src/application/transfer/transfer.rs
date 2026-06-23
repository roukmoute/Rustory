//! Story-transfer application service (orchestration of the write job).
//!
//! The write counterpart of [`prepare_story`](super::prepare::prepare_story):
//! preparation assembles LOCALLY "what a transfer would need"; this service
//! WRITES it back to a connected writable Lunii, driving the `Preflight →
//! Transfer → {transferred | retryable}` machine.
//!
//! It follows the SAME discipline as every device flow — the live device is
//! re-scanned FIRST, the canonical facts are read AFTER the device I/O under a
//! SCOPED DB lock (never held across the scan or the write), and the
//! capability gate (`WriteStory`) is checked BEFORE any device mutation
//! (fail-closed, AC2/FR34). The canonical story is NEVER mutated (FR18); a
//! functional failure is the terminal `retryable` state of the job (NOT an
//! `AppError`), and NO success is ever claimed — verification belongs to a later
//! story, so the success terminal is the honest non-success "écriture effectuée —
//! vérification à venir".
//!
//! Stays Tauri-free: the runtime (event emission, `AppHandle`) lives in the
//! command layer; this service only sees the
//! [`PreparationEventEmitter`](super::prepare::PreparationEventEmitter) trait
//! (reused — its phase enum already carries `Transfer`). Tests inject a
//! `MockDeviceScanner`, a `MockDeviceLibraryReader`, a `MockTransferArtifactSource`,
//! a `MockDevicePackWriter`, a capturing emitter and a temp DB.

use std::cell::Cell;
use std::path::{Path, PathBuf};
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
    build_write_plan, classify, classify_verify, compose_verified_summary, ensure_cohort_coherent,
    ensure_descriptor_coherent, failure_copy, gate_prepare, short_id_from_pack_uuid,
    verify_aggregate, ChecksumProbe, PreparationFailureCause, PreparationPhase,
    TransferArtifactDescriptor, TransferCompleteness, TransferFailureCause, VerifiedSummary,
    VerifyVerdict,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::device::{
    DeviceLibraryReader, DevicePackWriter, DeviceScanner, WriteProgress,
};
use crate::infrastructure::filesystem::{
    resolve_import_story_dir, AssemblyPlan, AssemblySource, TransferArtifactSource,
};

use super::prepare::PreparationEventEmitter;

/// What the background write+verify job produced — returned to the command for a
/// local trace only (the UI learns the truth from the events + the authoritative
/// re-read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferOutcome {
    /// The write landed AND the `verify` phase confirmed it (indexed + content
    /// present + byte-faithful) — the legitimate success `transférée et vérifiée`.
    /// `summary` carries the AC2/FR15 confirmation lines (composed in Rust).
    Verified {
        device_identifier: String,
        story_id: String,
        story_title: String,
        summary: VerifiedSummary,
    },
    /// The write landed but the `verify` phase did NOT confirm it: `Partial`
    /// (`état partiel`) or `Failed` (`échec récupérable`) — a non-success terminal,
    /// never dressed up as a success. The canonical draft is preserved.
    Unverified { verdict: VerifyVerdict },
    /// A WRITE-phase functional failure — a terminal `retryable` job state (NOT an
    /// `AppError`). The canonical draft is preserved. `completeness` distinguishes
    /// a device left intact (`Failed` → `échec récupérable`) from one that may
    /// hold a partial copy (`Incomplete` → `transfert incomplet`).
    Retryable {
        cause: TransferFailureCause,
        completeness: TransferCompleteness,
    },
    /// A transport failure that prevented producing a terminal job state
    /// (e.g. the local store became unreadable). Surfaced as an `AppError`.
    Transport { error: AppError },
}

/// The authoritative re-read state. Read-only and idempotent, it reports only
/// what the DEVICE proves: whether the selected story's pack is currently present
/// on the connected writable device AND byte-faithful (re-checksum). The
/// transient `transferring` / `verifying` phases and the `partial` / `retryable`
/// terminals are EVENT-driven (the frontend holds them from `job:*`); a passive
/// re-read never reconstructs a non-success verdict (they belong to the live
/// session), so it resolves to `Idle` or `Verified`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferStateView {
    /// No writable device, the story is not transferable, or its pack is not
    /// (yet) present + byte-faithful on the device.
    Idle,
    /// The story's pack is present on the connected writable device AND its bytes
    /// re-checksum to the prepared baseline — `transférée et vérifiée`.
    /// `summary` carries the AC2 confirmation lines (composed in Rust).
    Verified {
        device_identifier: String,
        story_id: String,
        story_title: String,
        summary: VerifiedSummary,
    },
}

/// A confirmed transfer preflight: the requested device is present, supported,
/// WRITE-authorized and readable, and the local facts have been read.
struct ConfirmedTransfer {
    device_identifier: String,
    device_cohort: String,
    story_title: String,
    /// The imported pack's canonical UUID, or `None` for a native story (which
    /// has no device-format pack and is therefore not transferable).
    pack_uuid: Option<String>,
    plan: AssemblyPlan,
    expected_aggregate: String,
    blockers: Vec<CanonicalBlocker>,
    /// Whether the story's pack is already present on the device (UUID indexed +
    /// its `.content` folder there). Used by the read-only re-read.
    pack_present: bool,
    /// The FRESH mount path of the confirmed device (never crosses IPC). The
    /// read-only re-read re-checksums the device pack here without a second scan.
    mount_path: PathBuf,
    /// The confirmed device's USB volume serial (when available) — STABLE across
    /// the write's `.pi` mutation. The `verify` phase uses it to prove it is
    /// re-reading the SAME device it wrote to, not a swapped Lunii.
    volume_serial: Option<String>,
    /// Total device-resident stories — the base for the AC2 `unchanged_count`.
    device_entry_count: usize,
    /// How many inventory entries match this story's pack UUID (≥ 1 ⇒ a send
    /// touches that content; ALL occurrences, incl. a `.pi`/`.pi.hidden`
    /// duplicate, are excluded from the unchanged count — the 3.1 precedent).
    pack_match_count: usize,
}

/// Outcome of the read-only transfer preflight. `NotConfirmed` carries the HONEST
/// cause — `WriteNotAuthorized` when the connected profile cannot be written
/// (V3 / unsupported), `DeviceChanged` when no requested writable device can be
/// confirmed (gone / swapped / unreadable), `Interrupted` on a budget/timeout.
/// A genuine local-store transport failure propagates as `Err`.
enum TransferPreflight {
    Confirmed(Box<ConfirmedTransfer>),
    NotConfirmed(TransferFailureCause),
}

/// Clamp ceiling for an in-flight transfer fraction: the content copy can reach
/// 100 % of the bytes, but the job is not done until durability + indexing, so the
/// emitted progress never reaches 1.0 — that is reserved for the `completed`
/// terminal (honest progress, AC1).
const PROGRESS_CEILING: f32 = 0.99;

/// Advance and return the monotonic event sequence. Interior mutability lets the
/// in-flight progress closure and the terminal share one counter.
fn next_sequence(sequence: &Cell<u64>) -> u64 {
    let next = sequence.get() + 1;
    sequence.set(next);
    next
}

/// Run the transfer job, emitting progress + a terminal event. Returns the
/// outcome for the command's local trace.
#[allow(clippy::too_many_arguments)]
pub fn transfer_story(
    db: &Mutex<DbHandle>,
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    artifact_source: &dyn TransferArtifactSource,
    pack_writer: &dyn DevicePackWriter,
    app_data_dir: &Path,
    story_id: &str,
    requested_device_identifier: &str,
    preflight_budget: Duration,
    write_budget: Duration,
    emitter: &dyn PreparationEventEmitter,
) -> TransferOutcome {
    let sequence = Cell::new(0u64);
    emitter.progress(PreparationPhase::Preflight, None, next_sequence(&sequence));

    let confirmed = match run_transfer_preflight(
        db,
        scanner,
        library_reader,
        story_id,
        Some(requested_device_identifier),
        preflight_budget,
    ) {
        Ok(TransferPreflight::Confirmed(confirmed)) => *confirmed,
        Ok(TransferPreflight::NotConfirmed(cause)) => {
            // The capability gate / identity guard refused BEFORE any device
            // mutation (fail-closed, AC2/FR34): V3 or an unsupported profile →
            // `WriteNotAuthorized`; a changed/unreadable device → `DeviceChanged`;
            // a scan timeout → `Interrupted`. The writer is never reached, so the
            // device was never mutated → `Failed`.
            return fail(emitter, &sequence, cause, TransferCompleteness::Failed);
        }
        Err(error) => {
            // A genuine local-store transport failure (vanished story / unreadable
            // DB). It cannot produce a terminal job state — surface the AppError.
            let action = error.user_action.clone().unwrap_or_default();
            emitter.failed(&error.message, &action, next_sequence(&sequence));
            return TransferOutcome::Transport { error };
        }
    };

    // Fail-closed: the story must have passed validation to be transferable.
    if gate_prepare(&confirmed.blockers).is_err() {
        return fail(
            emitter,
            &sequence,
            TransferFailureCause::NotPrepared,
            TransferCompleteness::Failed,
        );
    }

    // A native story (no imported pack) has no device-format artifacts to write
    // back in MVP — refused BEFORE entering the transfer phase, no false start.
    let Some(pack_uuid) = confirmed.pack_uuid.clone() else {
        return fail(
            emitter,
            &sequence,
            TransferFailureCause::NotTransferable,
            TransferCompleteness::Failed,
        );
    };
    let Some(short_id) = short_id_from_pack_uuid(&pack_uuid) else {
        // Defensive: the schema keeps `pack_uuid` canonical, so this cannot
        // happen for a real import — a non-canonical value yields no target.
        return fail(
            emitter,
            &sequence,
            TransferFailureCause::NotTransferable,
            TransferCompleteness::Failed,
        );
    };

    // PREPARE phase: re-assemble the descriptor FRESH (read-only) + re-verify its
    // integrity. Emitting `Prepare` here makes a relaunch a visible FULL cycle
    // `preflight → prepare → transfer → verify` (AC1), distinct from the device
    // `Transfer` write below — the local assembly is not yet a device mutation.
    emitter.progress(PreparationPhase::Prepare, None, next_sequence(&sequence));

    // Re-assemble the descriptor FRESH (read-only) and re-verify its integrity
    // against the recorded baseline before writing a single byte. A failure here
    // means the story is not in a clean prepared state → `NotPrepared`.
    let descriptor = match assemble_for_transfer(
        artifact_source,
        app_data_dir,
        &confirmed.plan,
        &confirmed.expected_aggregate,
        write_budget,
    ) {
        Ok(descriptor) => descriptor,
        Err(cause) => return fail(emitter, &sequence, cause, TransferCompleteness::Failed),
    };

    let plan = match build_write_plan(&descriptor, &short_id) {
        Ok(plan) => plan,
        // Defensive: an imported descriptor always carries pack files; a
        // descriptor without one is not transferable.
        Err(cause) => return fail(emitter, &sequence, cause, TransferCompleteness::Failed),
    };
    if let Err(cause) = ensure_cohort_coherent(&descriptor.target_cohort, &confirmed.device_cohort)
    {
        return fail(emitter, &sequence, cause, TransferCompleteness::Failed);
    }

    // F5 — re-validate the device identity IMMEDIATELY before the first mutation.
    // The preflight scan ran before the (local) assembly; if the device was
    // unplugged or swapped at the same mount path since, writing now could hit a
    // DIFFERENT Lunii. Re-scan, refuse `DeviceChanged` before a single byte, and
    // use the FRESHLY confirmed mount path for the write.
    let mount_path =
        match revalidate_writable_device(scanner, requested_device_identifier, preflight_budget) {
            Ok(path) => path,
            Err(cause) => return fail(emitter, &sequence, cause, TransferCompleteness::Failed),
        };

    // TRANSFER phase: the actual device write begins now, after the local PREPARE
    // assembly and the fresh identity re-validation. Emitting the transition here
    // keeps `en transfert` honest — named only while bytes are really moving.
    emitter.progress(PreparationPhase::Transfer, None, next_sequence(&sequence));

    // The opaque pack bytes live under the LOCAL imports folder — the writer
    // reproduces them on the device (round-trip, no decryption). The writer
    // reports the content-copy fraction; translate each report to a `job:progress`
    // (phase `Transfer`) with a monotone sequence, clamped below 100 %.
    let source_pack_dir = resolve_import_story_dir(app_data_dir, story_id);
    let report = |p: WriteProgress| {
        if p.bytes_total == 0 {
            return;
        }
        let fraction = ((p.bytes_done as f32) / (p.bytes_total as f32)).min(PROGRESS_CEILING);
        emitter.progress(
            PreparationPhase::Transfer,
            Some(fraction),
            next_sequence(&sequence),
        );
    };
    match pack_writer.write_pack(
        &mount_path,
        &source_pack_dir,
        &pack_uuid,
        &plan,
        write_budget,
        &report,
    ) {
        Ok(()) => {
            // The writer reports success; PROVE it (NFR: "no success without
            // explicit verification of the expected result"). Enter the FINAL
            // `verify` phase of the SAME job: re-read the device and re-checksum
            // what landed, then classify the verdict. "écriture effectuée —
            // vérification à venir" is now the TRANSIENT label of this phase.
            emitter.progress(PreparationPhase::Verify, None, next_sequence(&sequence));
            let facts = verify_written_pack(
                scanner,
                library_reader,
                artifact_source,
                &pack_uuid,
                &short_id,
                &confirmed.expected_aggregate,
                &mount_path,
                confirmed.volume_serial.as_deref(),
                write_budget,
            );
            match classify_verify(
                facts.indexed,
                facts.content_present,
                facts.checksum,
                facts.readable,
            ) {
                VerifyVerdict::Verified => {
                    // Compose the AC2 summary in Rust and carry it ON the terminal
                    // event (F1/F5): the UI renders `verified` straight from the
                    // event, never via a re-read with the now-stale pre-write
                    // identifier, and never re-composes the lines in React.
                    let summary =
                        compose_verified_summary(&confirmed.story_title, facts.unchanged_count);
                    emitter.completed_verified(
                        &summary.changed,
                        &summary.unchanged,
                        next_sequence(&sequence),
                    );
                    TransferOutcome::Verified {
                        device_identifier: confirmed.device_identifier,
                        story_id: story_id.to_string(),
                        story_title: confirmed.story_title,
                        summary,
                    }
                }
                // Honest non-success: NEVER dressed up as a success, NEVER the
                // `transfert incomplet` write-phase wording. Reuse the failure
                // channel carrying the verify-verdict discriminant so the UI
                // renders `état partiel` (partial) vs `échec récupérable` (failed).
                verdict @ (VerifyVerdict::Partial | VerifyVerdict::Failed) => {
                    let (message, action) =
                        verdict.copy().expect("a non-success verdict carries copy");
                    emitter.failed_verify(
                        message,
                        action,
                        verdict.diagnostic_tag(),
                        next_sequence(&sequence),
                    );
                    TransferOutcome::Unverified { verdict }
                }
            }
        }
        // The writer reports whether the device was already mutated; the domain
        // classifies it into `Failed` (device intact) vs `Incomplete` (a possible
        // partial copy a fresh relaunch converges).
        Err(failure) => {
            let completeness = classify(failure.cause, failure.reached_device_mutation);
            fail(emitter, &sequence, failure.cause, completeness)
        }
    }
}

/// Emit a functional failure terminal event and return the matching outcome,
/// carrying the device COMPLETENESS (`Failed` vs `Incomplete`).
fn fail(
    emitter: &dyn PreparationEventEmitter,
    sequence: &Cell<u64>,
    cause: TransferFailureCause,
    completeness: TransferCompleteness,
) -> TransferOutcome {
    let (message, action) = failure_copy(cause, completeness);
    emitter.failed_with_completeness(
        message,
        action,
        Some(completeness.diagnostic_tag()),
        Some(cause.wire_cause()),
        next_sequence(sequence),
    );
    TransferOutcome::Retryable {
        cause,
        completeness,
    }
}

/// Facts the read-only `verify` re-read produces, consumed by [`classify_verify`].
struct VerifyFacts {
    /// The device re-read succeeded AND is the device we wrote to (re-scan +
    /// supported + `ReadLibrary` + continuity). `false` ⇒ device gone / unreadable
    /// / swapped ⇒ cannot confirm ⇒ `Failed`.
    readable: bool,
    /// The pack UUID is listed in the device inventory (`.pi` / `.pi.hidden`).
    indexed: bool,
    /// The pack's `.content/<SHORT_ID>` folder is present on the device — probed
    /// INDEPENDENTLY of the index (so a promoted-but-unindexed pack is detected).
    content_present: bool,
    /// The device-pack re-checksum outcome (match / readable divergence / unable).
    checksum: ChecksumProbe,
    /// How many OTHER device stories a send left untouched (AC2/FR15 summary).
    unchanged_count: u32,
}

/// Read-only `verify` re-read run AFTER a successful write: re-scan the device,
/// confirm the SAME device we wrote to is still present (gate `ReadLibrary` +
/// continuity), read its inventory and re-checksum the written pack against the
/// prepared baseline. Produces the facts [`classify_verify`] turns into a verdict.
///
/// **Continuity, not pre-write identity (F1/F2).** The write itself mutates `.pi`,
/// so the device's derived `device_identifier` legitimately CHANGES across the
/// write — it cannot be re-pinned. Instead the verify binds to the device we wrote
/// to via its STABLE USB `volume_serial` (falling back to the written mount path
/// when no serial is available): a Lunii swapped after the write for ANOTHER
/// supported device — even one that already holds the same pack + bytes — fails the
/// continuity check (`readable == false` ⇒ `Failed`), so `verified` is never
/// attributed to the wrong device. Strictly read-only (FR18).
#[allow(clippy::too_many_arguments)]
fn verify_written_pack(
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    artifact_source: &dyn TransferArtifactSource,
    pack_uuid: &str,
    short_id: &str,
    expected_aggregate: &str,
    written_mount_path: &Path,
    expected_serial: Option<&str>,
    budget: Duration,
) -> VerifyFacts {
    let unreadable = VerifyFacts {
        readable: false,
        indexed: false,
        content_present: false,
        checksum: ChecksumProbe::Unavailable,
        unchanged_count: 0,
    };

    let started = Instant::now();
    let resolved = match resolve_connected_lunii(scanner, budget) {
        Ok(resolved) => resolved,
        Err(_) => return unreadable,
    };
    let profile = match resolved.outcome {
        ConnectedLuniiOutcome::Supported(profile) => profile,
        _ => return unreadable,
    };
    if check_operation_allowed(&profile, SupportedOperation::ReadLibrary).is_err() {
        return unreadable;
    }
    let mount_path = match resolved.supported_mount_path {
        Some(path) => path,
        None => return unreadable,
    };
    // F2 — continuity proof that survives the write's `.pi` mutation: the volume
    // serial (stable) when available, else the written mount path. A swap to a
    // different device (different serial) is NOT the device we wrote to.
    let bound = match (expected_serial, resolved.supported_volume_serial.as_deref()) {
        (Some(expected), Some(seen)) => expected == seen,
        _ => mount_path == written_mount_path,
    };
    if !bound {
        return unreadable;
    }
    let remaining = budget.saturating_sub(started.elapsed());
    let library = match library_reader.read_library(&mount_path, remaining) {
        Ok(library) => library,
        Err(_) => return unreadable,
    };

    let indexed = library.entries.iter().any(|e| e.uuid == pack_uuid);
    let pack_match_count = library
        .entries
        .iter()
        .filter(|e| e.uuid == pack_uuid)
        .count();
    let unchanged_count = (library.entries.len() as u32).saturating_sub(pack_match_count as u32);

    // F3/F4 — probe `.content/<SHORT_ID>` INDEPENDENTLY of the `.pi` index by
    // re-reading the content folder directly, and distinguish a readable DIVERGENCE
    // (present but incoherent ⇒ `Partial`) from an ABSENT pack or an UNCONFIRMABLE
    // re-read (⇒ `Failed`). The re-checksum reads the folder regardless of whether
    // the UUID is indexed, so a promoted-but-unindexed pack reads as present.
    let remaining = budget.saturating_sub(started.elapsed());
    let (content_present, checksum) =
        match artifact_source.reaggregate_device_pack(&mount_path, short_id, remaining) {
            Ok(aggregate) if aggregate == expected_aggregate => (true, ChecksumProbe::Match),
            Ok(_) => (true, ChecksumProbe::Diverged),
            // The content folder is absent — the write provably did not land here.
            Err(PreparationFailureCause::ArtifactMissing) => (false, ChecksumProbe::Unavailable),
            // The folder exists but its bytes could not be re-read (corrupt /
            // interrupted / timed out): present but UNCONFIRMABLE.
            Err(_) => (true, ChecksumProbe::Unavailable),
        };

    VerifyFacts {
        readable: true,
        indexed,
        content_present,
        checksum,
        unchanged_count,
    }
}

/// Authoritative re-read: re-derive the current transfer state on demand
/// (nothing is persisted; the device is the truth). PINNED to the requested
/// device identifier (C1): a pack present on a DIFFERENT writable device is never
/// reported as transferred for this target, so the terminal can be neither a false
/// "écriture effectuée" nor attributed to the wrong Lunii. Returns `Transferred`
/// only when the story's pack is present on the REQUESTED device; otherwise
/// `Idle`. The `retryable` failure terminal is NOT re-derived here — a passive
/// read cannot reproduce a write-time failure — so the frontend keeps the
/// `job:failed` terminal it received.
#[allow(clippy::too_many_arguments)]
pub fn read_transfer_state(
    db: &Mutex<DbHandle>,
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    artifact_source: &dyn TransferArtifactSource,
    app_data_dir: &Path,
    story_id: &str,
    requested_device_identifier: &str,
    preflight_budget: Duration,
    assembly_budget: Duration,
) -> Result<TransferStateView, AppError> {
    let confirmed = match run_transfer_preflight(
        db,
        scanner,
        library_reader,
        story_id,
        Some(requested_device_identifier),
        preflight_budget,
    )? {
        TransferPreflight::Confirmed(confirmed) => *confirmed,
        // No writable confirmable device (or a budget timeout) → nothing to
        // report; the panel's disabled-CTA reason covers the messaging.
        TransferPreflight::NotConfirmed(_) => return Ok(TransferStateView::Idle),
    };

    // The story must have passed validation, assemble cleanly and be a
    // transferable imported pack for a `Transferred` claim to be meaningful.
    if gate_prepare(&confirmed.blockers).is_err() {
        return Ok(TransferStateView::Idle);
    }
    let descriptor = match assemble_for_transfer(
        artifact_source,
        app_data_dir,
        &confirmed.plan,
        &confirmed.expected_aggregate,
        assembly_budget,
    ) {
        Ok(descriptor) => descriptor,
        Err(_) => return Ok(TransferStateView::Idle),
    };
    let short_id = confirmed
        .pack_uuid
        .as_deref()
        .and_then(short_id_from_pack_uuid);
    let transferable = short_id
        .as_deref()
        .map(|sid| build_write_plan(&descriptor, sid).is_ok())
        .unwrap_or(false);

    // Authoritative `Verified` re-derivation: the pack must be PRESENT (uuid
    // indexed + `.content` there) AND its bytes must re-checksum to the prepared
    // baseline. Presence alone is not enough — that would be the old, weaker
    // "écriture effectuée" claim; `verified` requires proven byte fidelity (NFR).
    if transferable && confirmed.pack_present {
        let short_id = short_id.expect("a transferable pack has a short id");
        let byte_faithful = matches!(
            artifact_source.reaggregate_device_pack(
                &confirmed.mount_path,
                &short_id,
                assembly_budget,
            ),
            Ok(aggregate) if aggregate == confirmed.expected_aggregate
        );
        if byte_faithful {
            let unchanged_count = (confirmed.device_entry_count as u32)
                .saturating_sub(confirmed.pack_match_count as u32);
            let summary = compose_verified_summary(&confirmed.story_title, unchanged_count);
            return Ok(TransferStateView::Verified {
                device_identifier: confirmed.device_identifier,
                story_id: story_id.to_string(),
                story_title: confirmed.story_title,
                summary,
            });
        }
        // Present but NOT byte-faithful: a `partial` / `failed` verdict belongs to
        // the LIVE session (the hook holds the `job:*` terminal), never a passive
        // re-read — fold to idle here (the open-question default).
    }
    Ok(TransferStateView::Idle)
}

/// Assemble the descriptor, verify its integrity against the recorded baseline,
/// and sanity-check its coherence. Read-only; the canonical story is never
/// mutated (FR18). Any failure means the story is not in a clean prepared state.
fn assemble_for_transfer(
    artifact_source: &dyn TransferArtifactSource,
    app_data_dir: &Path,
    plan: &AssemblyPlan,
    expected_aggregate: &str,
    budget: Duration,
) -> Result<TransferArtifactDescriptor, TransferFailureCause> {
    let descriptor = artifact_source
        .assemble(app_data_dir, plan, budget)
        .map_err(|_| TransferFailureCause::NotPrepared)?;
    verify_aggregate(&descriptor, expected_aggregate)
        .map_err(|_| TransferFailureCause::NotPrepared)?;
    ensure_descriptor_coherent(&descriptor).map_err(|_| TransferFailureCause::NotPrepared)?;
    Ok(descriptor)
}

/// Re-scan the device, confirm the requested identity (when one is given), pass
/// the `WriteStory` gate, PROVE the inventory is readable, then read the local
/// facts under a scoped lock taken AFTER the device I/O. Returns
/// `NotConfirmed(cause)` with an honest cause when no writable device is
/// confirmable, or `Err` for a genuine local-store / scan transport failure.
fn run_transfer_preflight(
    db: &Mutex<DbHandle>,
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    story_id: &str,
    requested_device_identifier: Option<&str>,
    budget: Duration,
) -> Result<TransferPreflight, AppError> {
    let started = Instant::now();

    // Re-scan. A scan TIMEOUT is an honest `Interrupted` (budget), NOT "device
    // changed"; any other scan transport failure is surfaced explicitly.
    let resolved = match resolve_connected_lunii(scanner, budget) {
        Ok(resolved) => resolved,
        Err(err) => {
            if details_source(&err) == Some("scan_timeout") {
                return Ok(TransferPreflight::NotConfirmed(
                    TransferFailureCause::Interrupted,
                ));
            }
            return Err(err);
        }
    };

    let profile = match resolved.outcome {
        ConnectedLuniiOutcome::Supported(profile) => profile,
        _ => {
            return Ok(TransferPreflight::NotConfirmed(
                TransferFailureCause::DeviceChanged,
            ))
        }
    };

    if let Some(requested) = requested_device_identifier {
        if profile.device_identifier != requested {
            return Ok(TransferPreflight::NotConfirmed(
                TransferFailureCause::DeviceChanged,
            ));
        }
    }

    // GATE BEFORE MUTATION (AC2/FR34): only a WRITE-authorized profile proceeds.
    // V3 and any unsupported cohort are refused here, before a single byte is
    // written. Fail-closed.
    if check_operation_allowed(&profile, SupportedOperation::WriteStory).is_err() {
        return Ok(TransferPreflight::NotConfirmed(
            TransferFailureCause::WriteNotAuthorized,
        ));
    }

    let mount_path = match resolved.supported_mount_path {
        Some(path) => path,
        // Defensive: a `Supported` outcome always carries a mount path.
        None => {
            return Ok(TransferPreflight::NotConfirmed(
                TransferFailureCause::DeviceChanged,
            ))
        }
    };

    // PROVE the inventory is readable (a writable volume whose library became
    // unreadable must end recoverably, never written blindly), and capture it to
    // resolve whether the pack is already present.
    let remaining = budget.saturating_sub(started.elapsed());
    let library = match library_reader.read_library(&mount_path, remaining) {
        Ok(library) => library,
        Err(err) => {
            let cause = if details_source(&err) == Some("read_timeout") {
                TransferFailureCause::Interrupted
            } else {
                TransferFailureCause::DeviceChanged
            };
            return Ok(TransferPreflight::NotConfirmed(cause));
        }
    };

    let facts = read_transfer_facts(db, story_id)?;
    let blockers = validate_canonical(&facts.facts);

    let (source, expected_aggregate) = match &facts.pack_checksum {
        Some(pack_checksum) => (AssemblySource::ImportedPack, pack_checksum.clone()),
        None => (
            AssemblySource::Native {
                structure_json: facts.facts.structure_json.clone(),
            },
            facts.facts.content_checksum.clone(),
        ),
    };
    let target_cohort = profile.firmware_cohort.diagnostic_tag().to_string();

    let pack_present = facts
        .pack_uuid
        .as_deref()
        .map(|uuid| {
            library
                .entries
                .iter()
                .any(|entry| entry.uuid == uuid && entry.content_present)
        })
        .unwrap_or(false);
    // Count EVERY entry matching the pack (a `.pi` + `.pi.hidden` duplicate yields
    // two): all are excluded from the unchanged count, mirroring 3.1.
    let pack_match_count = facts
        .pack_uuid
        .as_deref()
        .map(|uuid| library.entries.iter().filter(|e| e.uuid == uuid).count())
        .unwrap_or(0);
    let device_entry_count = library.entries.len();

    Ok(TransferPreflight::Confirmed(Box::new(ConfirmedTransfer {
        device_identifier: profile.device_identifier,
        device_cohort: target_cohort.clone(),
        story_title: facts.facts.title.clone(),
        pack_uuid: facts.pack_uuid,
        plan: AssemblyPlan {
            story_id: story_id.to_string(),
            target_cohort,
            source,
        },
        expected_aggregate,
        blockers,
        pack_present,
        mount_path,
        volume_serial: resolved.supported_volume_serial,
        device_entry_count,
        pack_match_count,
    })))
}

/// F5 — re-scan and confirm the requested device is STILL the connected,
/// supported, WRITE-authorized volume, returning its FRESH mount path. Run
/// immediately before the first device mutation so a swap at the same mount path
/// cannot redirect the write to another Lunii. Any mismatch is `DeviceChanged`
/// (a scan timeout is `Interrupted`) — refused BEFORE a single byte is written.
fn revalidate_writable_device(
    scanner: &dyn DeviceScanner,
    requested_device_identifier: &str,
    budget: Duration,
) -> Result<PathBuf, TransferFailureCause> {
    let resolved = match resolve_connected_lunii(scanner, budget) {
        Ok(resolved) => resolved,
        Err(err) => {
            return Err(if details_source(&err) == Some("scan_timeout") {
                TransferFailureCause::Interrupted
            } else {
                TransferFailureCause::DeviceChanged
            });
        }
    };
    let profile = match resolved.outcome {
        ConnectedLuniiOutcome::Supported(profile) => profile,
        _ => return Err(TransferFailureCause::DeviceChanged),
    };
    if profile.device_identifier != requested_device_identifier {
        return Err(TransferFailureCause::DeviceChanged);
    }
    if check_operation_allowed(&profile, SupportedOperation::WriteStory).is_err() {
        return Err(TransferFailureCause::DeviceChanged);
    }
    resolved
        .supported_mount_path
        .ok_or(TransferFailureCause::DeviceChanged)
}

/// Read the closed-set `details.source` token of an `AppError`, when present.
fn details_source(err: &AppError) -> Option<&str> {
    err.details.as_ref()?.get("source")?.as_str()
}

/// The local facts a transfer needs: the canonical facts (for validation + the
/// native baseline) plus the imported-pack UUID + checksum when the story came
/// from a device copy. Read under a scoped lock taken here, never held across I/O.
struct TransferFacts {
    facts: CanonicalStoryFacts,
    pack_uuid: Option<String>,
    pack_checksum: Option<String>,
}

fn read_transfer_facts(db: &Mutex<DbHandle>, story_id: &str) -> Result<TransferFacts, AppError> {
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

    let import_row: Option<(String, String)> = db
        .conn()
        .query_row(
            "SELECT pack_uuid, pack_checksum FROM story_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| local_read_error("select_pack"))?;
    let (pack_uuid, pack_checksum) = match import_row {
        Some((uuid, checksum)) => (Some(uuid), Some(checksum)),
        None => (None, None),
    };

    Ok(TransferFacts {
        facts: CanonicalStoryFacts {
            title,
            schema_version,
            structure_json,
            content_checksum,
        },
        pack_uuid,
        pack_checksum,
    })
}

fn story_missing_error() -> AppError {
    AppError::library_inconsistent(
        "Envoi impossible: histoire introuvable dans la bibliothèque locale.",
        "Recharge la bibliothèque puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "story_transfer",
        "cause": "story_missing",
    }))
}

fn local_read_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Envoi indisponible: vérifie le disque local et réessaie.",
        "Réessaie ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "story_transfer",
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
        compute_device_identifier, MockDeviceLibraryReader, MockDevicePackWriter, MockDeviceScanner,
    };
    use crate::infrastructure::filesystem::MockTransferArtifactSource;

    const HEALTHY_JSON: &str = "{\"schemaVersion\":1,\"nodes\":[]}";
    const PACK_UUID: &str = "abababab-abab-abab-abab-ababfac5562d";
    const PACK_CHECKSUM: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

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

    fn insert_story(db: &Mutex<DbHandle>, id: &str) {
        db.lock()
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES (?1, 'Mon histoire', 1, ?2, ?3, '2026-06-22T00:00:00.000Z', '2026-06-22T00:00:00.000Z')",
                rusqlite::params![id, HEALTHY_JSON, content_checksum(HEALTHY_JSON)],
            )
            .expect("insert story");
    }

    /// Mark a story as imported so the transfer path treats it as a writable
    /// pack (and the assembler baseline becomes the recorded `pack_checksum`).
    fn insert_import(db: &Mutex<DbHandle>, id: &str) {
        db.lock()
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
                 VALUES (?1, ?2, 'dev', '2026-06-22T00:00:00.000Z', 7, 1024, ?3)",
                rusqlite::params![id, PACK_UUID, PACK_CHECKSUM],
            )
            .expect("insert import");
    }

    /// A descriptor for an imported pack whose aggregate matches the recorded
    /// `pack_checksum`, with one `PackFile` so it is transferable.
    fn imported_descriptor(story_id: &str, cohort: &str) -> TransferArtifactDescriptor {
        TransferArtifactDescriptor {
            story_id: story_id.into(),
            target_cohort: cohort.into(),
            pipeline_version: PREPARATION_PIPELINE_VERSION,
            artifacts: vec![PreparedArtifact {
                kind: PreparedArtifactKind::PackFile,
                relative_ref: "ni".into(),
                byte_len: 4,
                checksum: "a".repeat(64),
            }],
            aggregate_checksum: PACK_CHECKSUM.into(),
        }
    }

    fn native_descriptor(story_id: &str) -> TransferArtifactDescriptor {
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

    fn supported_scanner(version: u8) -> MockDeviceScanner {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(version);
        scanner
    }

    fn readable_reader() -> MockDeviceLibraryReader {
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1);
        reader
    }

    /// The inventory the `verify` re-read sees after a successful write: the
    /// imported pack (uuid + content present) ALONGSIDE two other untouched
    /// stories — so `unchanged_count` is a meaningful 2.
    fn verify_library_with_pack() -> crate::domain::device::DeviceLibrary {
        use crate::domain::device::{DeviceLibrary, DeviceStoryEntry};
        DeviceLibrary {
            entries: vec![
                DeviceStoryEntry {
                    uuid: PACK_UUID.into(),
                    short_id: "FAC5562D".into(),
                    hidden: false,
                    content_present: true,
                },
                DeviceStoryEntry {
                    uuid: "11111111-1111-1111-1111-111111111111".into(),
                    short_id: "11111111".into(),
                    hidden: false,
                    content_present: true,
                },
                DeviceStoryEntry {
                    uuid: "22222222-2222-2222-2222-222222222222".into(),
                    short_id: "22222222".into(),
                    hidden: false,
                    content_present: true,
                },
            ],
            had_trailing_bytes: false,
        }
    }

    #[derive(Default)]
    struct CapturingEmitter {
        events: Mutex<Vec<Recorded>>,
        progress_values: Mutex<Vec<Option<f32>>>,
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
        FailedVerify {
            verdict: String,
            sequence: u64,
        },
    }

    impl PreparationEventEmitter for CapturingEmitter {
        fn progress(&self, phase: PreparationPhase, progress: Option<f32>, sequence: u64) {
            self.events
                .lock()
                .unwrap()
                .push(Recorded::Progress { phase, sequence });
            self.progress_values.lock().unwrap().push(progress);
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
        fn failed_verify(&self, _message: &str, _user_action: &str, verdict: &str, sequence: u64) {
            self.events.lock().unwrap().push(Recorded::FailedVerify {
                verdict: verdict.to_string(),
                sequence,
            });
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
                    Recorded::FailedVerify { sequence, .. } => *sequence,
                })
                .collect()
        }
        /// The non-null in-flight fractions actually emitted (honest progress).
        fn transfer_fractions(&self) -> Vec<f32> {
            self.progress_values
                .lock()
                .unwrap()
                .iter()
                .filter_map(|p| *p)
                .collect()
        }
    }

    fn is_monotonic(seqs: &[u64]) -> bool {
        seqs.windows(2).all(|w| w[1] == w[0] + 1) && seqs.first() == Some(&1)
    }

    #[test]
    fn verified_for_imported_story_on_writable_device() {
        let dir = tempfile::tempdir().expect("app data");
        // V1 (md v3) and V2 (md v6) are writable.
        for (version, cohort) in [(3u8, "origine_v1"), (6u8, "mid_gen_v2")] {
            let db = fresh_db();
            insert_story(&db, "s1");
            insert_import(&db, "s1");
            let scanner = supported_scanner(version);
            // F5 re-validates the device identity again right before the write.
            scanner.enqueue_supported_lunii(version);
            // The verify phase re-scans the device after the write.
            scanner.enqueue_supported_lunii(version);
            let reader = MockDeviceLibraryReader::new();
            reader.enqueue_library_with(1); // preflight readability proof
            reader.enqueue(Ok(verify_library_with_pack())); // verify sees the landed pack
            let artifacts = MockTransferArtifactSource::new();
            artifacts.enqueue(Ok(imported_descriptor("s1", cohort)));
            // The device bytes re-checksum to the prepared baseline (byte fidelity).
            artifacts.enqueue_reaggregate(Ok(PACK_CHECKSUM.to_string()));
            let writer = MockDevicePackWriter::new();
            writer.enqueue_success();
            let emitter = CapturingEmitter::default();

            let outcome = transfer_story(
                &db,
                &scanner,
                &reader,
                &artifacts,
                &writer,
                dir.path(),
                "s1",
                &mock_identifier(),
                budget(),
                budget(),
                &emitter,
            );
            match outcome {
                TransferOutcome::Verified {
                    story_title,
                    summary,
                    ..
                } => {
                    assert_eq!(story_title, "Mon histoire", "md v{version}");
                    assert!(
                        summary.changed.contains("Mon histoire"),
                        "md v{version}: the changed line names the story"
                    );
                    assert!(
                        summary.unchanged.starts_with("2 autres histoires"),
                        "md v{version}: the two other device stories stay unchanged"
                    );
                }
                other => panic!("md v{version}: expected Verified, got {other:?}"),
            }
            assert_eq!(
                writer.call_count(),
                1,
                "the writer must run exactly once (verify never writes)"
            );
            // The job emits preflight → transfer → verify, THEN the verified terminal.
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
                    Recorded::Progress {
                        phase: PreparationPhase::Transfer,
                        sequence: 3
                    },
                    Recorded::Progress {
                        phase: PreparationPhase::Verify,
                        sequence: 4
                    },
                    Recorded::Completed { sequence: 5 },
                ],
                "md v{version}"
            );
            assert!(is_monotonic(&emitter.sequences()));
        }
    }

    #[test]
    fn verify_partial_when_the_device_bytes_diverge() {
        // The write lands, the pack is present + indexed, but the device re-checksum
        // disagrees with the prepared baseline → the honest `Partial` (état partiel),
        // never a silent success, never the `transfert incomplet` write wording.
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        scanner.enqueue_supported_lunii(3); // F5
        scanner.enqueue_supported_lunii(3); // verify re-scan
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1);
        reader.enqueue(Ok(verify_library_with_pack()));
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        // The bytes on the device DIVERGE from the baseline.
        artifacts.enqueue_reaggregate(Ok("f".repeat(64)));
        let writer = MockDevicePackWriter::new();
        writer.enqueue_success();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Unverified {
                verdict: VerifyVerdict::Partial
            }
        );
        // verify was entered, then the non-success verify terminal (not a plain
        // `failed`, not `completed`).
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
                Recorded::Progress {
                    phase: PreparationPhase::Transfer,
                    sequence: 3
                },
                Recorded::Progress {
                    phase: PreparationPhase::Verify,
                    sequence: 4
                },
                Recorded::FailedVerify {
                    verdict: "partial".to_string(),
                    sequence: 5
                },
            ]
        );
    }

    #[test]
    fn verify_failed_when_the_device_is_gone_during_verify() {
        // The write lands, but the device vanishes before verify can re-read it →
        // `Failed` (échec récupérable): the write MAY have succeeded but it cannot
        // be PROVEN, so no false success — a reconnected relaunch re-verifies.
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        scanner.enqueue_supported_lunii(3); // F5
        scanner.enqueue_no_device(); // verify re-scan: the device is gone
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1); // preflight only (verify never reads)
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        // No reaggregate scripted — verify never reaches the re-checksum.
        let writer = MockDevicePackWriter::new();
        writer.enqueue_success();
        let emitter = CapturingEmitter::default();

        let before = read_story_row(&db, "s1");
        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Unverified {
                verdict: VerifyVerdict::Failed
            }
        );
        assert_eq!(
            emitter.recorded().last(),
            Some(&Recorded::FailedVerify {
                verdict: "failed".to_string(),
                sequence: 5
            })
        );
        assert_eq!(
            read_story_row(&db, "s1"),
            before,
            "verify never mutates the canonical draft (FR18)"
        );
    }

    #[test]
    fn verify_failed_when_the_device_is_swapped_after_write() {
        // F2: a Lunii swapped after the write for ANOTHER supported device fails the
        // continuity check (different volume serial), so verify never attributes a
        // `verified` to the wrong device — even one already holding the same pack.
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        scanner.enqueue_supported_lunii(3); // F5 (still the target)
        scanner.enqueue_supported_lunii_swapped(3); // verify re-scan: a DIFFERENT device
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1); // preflight readability proof
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        // The continuity check refuses BEFORE the verify read/recheck — neither the
        // verify library nor the re-checksum is consulted.
        let writer = MockDevicePackWriter::new();
        writer.enqueue_success();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Unverified {
                verdict: VerifyVerdict::Failed
            }
        );
    }

    #[test]
    fn verify_partial_when_content_present_but_not_indexed() {
        // F3: `.content/<short>` is promoted (byte-faithful) but the UUID is NOT in
        // `.pi` — the device is mutated + present but incoherent ⇒ `Partial`, not the
        // `Failed` the old library-only `content_present` produced.
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        scanner.enqueue_supported_lunii(3); // F5
        scanner.enqueue_supported_lunii(3); // verify re-scan
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1); // preflight
        reader.enqueue_library_with(1); // verify: generic packs, NO PACK_UUID → not indexed
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        // The content folder reads back byte-faithful (present + Match), but it is
        // not indexed in `.pi` above.
        artifacts.enqueue_reaggregate(Ok(PACK_CHECKSUM.to_string()));
        let writer = MockDevicePackWriter::new();
        writer.enqueue_success();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Unverified {
                verdict: VerifyVerdict::Partial
            }
        );
    }

    #[test]
    fn verify_failed_when_indexed_but_content_absent() {
        // F4: the UUID lingers in `.pi` but `.content/<short>` is absent — the pack
        // did NOT land ⇒ `Failed` (pack absent), never `Partial`.
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        scanner.enqueue_supported_lunii(3); // F5
        scanner.enqueue_supported_lunii(3); // verify re-scan
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1); // preflight
        reader.enqueue(Ok(verify_library_with_pack())); // verify: UUID indexed in `.pi`
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        // The content folder is gone → the re-read proves the pack did not land.
        artifacts.enqueue_reaggregate(Err(PreparationFailureCause::ArtifactMissing));
        let writer = MockDevicePackWriter::new();
        writer.enqueue_success();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Unverified {
                verdict: VerifyVerdict::Failed
            }
        );
    }

    #[test]
    fn verify_failed_when_the_recheck_cannot_run() {
        // F4: the content folder is present but its bytes cannot be re-read (corrupt
        // / interrupted) — the result is UNCONFIRMABLE ⇒ `Failed`, never a guessed
        // `Partial`.
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        scanner.enqueue_supported_lunii(3); // F5
        scanner.enqueue_supported_lunii(3); // verify re-scan
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1); // preflight
        reader.enqueue(Ok(verify_library_with_pack())); // verify: UUID indexed
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        // Present but unreadable bytes → the re-checksum cannot run.
        artifacts.enqueue_reaggregate(Err(PreparationFailureCause::ArtifactCorrupt));
        let writer = MockDevicePackWriter::new();
        writer.enqueue_success();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Unverified {
                verdict: VerifyVerdict::Failed
            }
        );
    }

    #[test]
    fn write_not_authorized_on_v3_blocks_before_any_mutation() {
        // AC2/FR34: V3 is not write-authorized — the writer must NEVER be reached.
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(7); // V3
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new(); // never assembled
        let writer = MockDevicePackWriter::new();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Retryable {
                cause: TransferFailureCause::WriteNotAuthorized,
                completeness: TransferCompleteness::Failed,
            }
        );
        assert_eq!(
            writer.call_count(),
            0,
            "block-before-mutation: the writer is never reached on V3"
        );
        // The transfer phase was never entered.
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
    fn not_transferable_for_a_native_story() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1"); // no story_imports row → native
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new(); // never assembled (early refusal)
        let writer = MockDevicePackWriter::new();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Retryable {
                cause: TransferFailureCause::NotTransferable,
                completeness: TransferCompleteness::Failed,
            }
        );
        assert_eq!(writer.call_count(), 0);
    }

    #[test]
    fn device_changed_when_identifier_mismatches() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        let writer = MockDevicePackWriter::new();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            "deadbeefdeadbeefdeadbeefdeadbeef",
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Retryable {
                cause: TransferFailureCause::DeviceChanged,
                completeness: TransferCompleteness::Failed,
            }
        );
        assert_eq!(writer.call_count(), 0);
    }

    #[test]
    fn device_identity_revalidated_just_before_write_blocks_a_swap() {
        // F5: the requested device passes the preflight, but the live re-scan run
        // immediately before the write no longer resolves it (unplugged / swapped
        // at the same mount path) → terminal `DeviceChanged`, the writer NEVER
        // reached (no mutation on a device that is no longer the confirmed target).
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3); // preflight: requested device present
        scanner.enqueue_no_device(); // re-validation before the write: gone
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        let writer = MockDevicePackWriter::new();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Retryable {
                cause: TransferFailureCause::DeviceChanged,
                completeness: TransferCompleteness::Failed,
            }
        );
        assert_eq!(
            writer.call_count(),
            0,
            "a device that changed before the write must never be written to"
        );
    }

    #[test]
    fn retryable_interrupted_when_the_writer_is_interrupted() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        // F5 re-validates the device identity again right before the write.
        scanner.enqueue_supported_lunii(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        let writer = MockDevicePackWriter::new();
        writer.enqueue_failure(TransferFailureCause::Interrupted);
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Retryable {
                cause: TransferFailureCause::Interrupted,
                completeness: TransferCompleteness::Failed,
            }
        );
        assert_eq!(
            writer.call_count(),
            1,
            "the writer ran and reported failure"
        );
        // Preflight → Prepare → Transfer → failed: the write phase WAS entered.
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
                Recorded::Progress {
                    phase: PreparationPhase::Transfer,
                    sequence: 3
                },
                Recorded::Failed { sequence: 4 },
            ]
        );
    }

    #[test]
    fn not_prepared_when_assembly_fails() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Err(
            crate::domain::transfer::PreparationFailureCause::ArtifactMissing,
        ));
        let writer = MockDevicePackWriter::new();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Retryable {
                cause: TransferFailureCause::NotPrepared,
                completeness: TransferCompleteness::Failed,
            }
        );
        assert_eq!(writer.call_count(), 0, "no write when assembly fails");
    }

    #[test]
    fn canonical_story_is_unchanged_after_a_failure() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        // F5 re-validates the device identity again right before the write.
        scanner.enqueue_supported_lunii(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        let writer = MockDevicePackWriter::new();
        writer.enqueue_failure(TransferFailureCause::WriteRejected);
        let emitter = CapturingEmitter::default();

        let before = read_story_row(&db, "s1");
        let _ = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
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
            "transfer must never mutate the canonical row"
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
    fn missing_story_is_a_transport_error() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        // No story seeded.
        let scanner = supported_scanner(3);
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        let writer = MockDevicePackWriter::new();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "ghost",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        match outcome {
            TransferOutcome::Transport { error } => {
                assert_eq!(
                    error.code,
                    crate::domain::shared::AppErrorCode::LibraryInconsistent
                );
            }
            other => panic!("expected Transport, got {other:?}"),
        }
        assert_eq!(writer.call_count(), 0);
    }

    #[test]
    fn read_transfer_state_returns_idle_without_a_device() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_no_device();
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();

        let view = read_transfer_state(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
        )
        .expect("read state");
        assert_eq!(view, TransferStateView::Idle);
    }

    #[test]
    fn read_transfer_state_reports_verified_when_pack_present_and_byte_faithful() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        // A library whose inventory contains THIS story's pack, content present.
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(crate::domain::device::DeviceLibrary {
            entries: vec![crate::domain::device::DeviceStoryEntry {
                uuid: PACK_UUID.into(),
                short_id: "FAC5562D".into(),
                hidden: false,
                content_present: true,
            }],
            had_trailing_bytes: false,
        }));
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        // The device bytes re-checksum to the baseline → upgrade presence to verified.
        artifacts.enqueue_reaggregate(Ok(PACK_CHECKSUM.to_string()));

        let view = read_transfer_state(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
        )
        .expect("read state");
        match view {
            TransferStateView::Verified {
                story_title,
                summary,
                ..
            } => {
                assert_eq!(story_title, "Mon histoire");
                assert!(
                    summary.unchanged.to_lowercase().contains("aucune autre"),
                    "the only device pack is the verified one → none others changed"
                );
            }
            other => panic!("expected Verified, got {other:?}"),
        }
    }

    #[test]
    fn read_transfer_state_is_idle_when_present_but_not_byte_faithful() {
        // A passive re-read never claims a non-success verdict: a present-but-
        // divergent pack folds to idle (the `partial` verdict lives in the live
        // session, not a passive read).
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(crate::domain::device::DeviceLibrary {
            entries: vec![crate::domain::device::DeviceStoryEntry {
                uuid: PACK_UUID.into(),
                short_id: "FAC5562D".into(),
                hidden: false,
                content_present: true,
            }],
            had_trailing_bytes: false,
        }));
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        artifacts.enqueue_reaggregate(Ok("f".repeat(64))); // diverges from baseline

        let view = read_transfer_state(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
        )
        .expect("read state");
        assert_eq!(view, TransferStateView::Idle);
    }

    #[test]
    fn read_transfer_state_is_idle_when_pack_absent_from_device() {
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        // Inventory without this story's pack → not yet transferred.
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));

        let view = read_transfer_state(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
        )
        .expect("read state");
        assert_eq!(view, TransferStateView::Idle);
    }

    #[test]
    fn read_transfer_state_is_idle_when_requested_device_is_not_the_connected_one() {
        // C1 — the re-read is pinned to the REQUESTED device. A different device is
        // connected, so the identity guard refuses BEFORE any inventory read: the
        // re-read never confirms a transfer on a device we did not target, so a
        // pack sitting on another Lunii can never read as a false "écriture
        // effectuée" (nor be attributed to the wrong device).
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3); // connected device = mock_identifier()
        let reader = MockDeviceLibraryReader::new(); // never read — identity mismatches first
        let artifacts = MockTransferArtifactSource::new();

        let view = read_transfer_state(
            &db,
            &scanner,
            &reader,
            &artifacts,
            dir.path(),
            "s1",
            "deadbeefdeadbeefdeadbeefdeadbeef", // a DIFFERENT device than connected
            budget(),
            budget(),
        )
        .expect("read state");
        assert_eq!(view, TransferStateView::Idle);
    }

    #[test]
    fn native_descriptor_is_not_transferable_via_build_plan() {
        // Guards the helper that backs `read_transfer_state` transferability.
        let d = native_descriptor("s1");
        assert!(build_write_plan(&d, "FAC5562D").is_err());
    }

    #[test]
    fn emits_monotone_transfer_progress() {
        // AC1: the writer's content-copy progress surfaces as honest in-flight
        // fractions — monotone, strictly below 100 % (reserved for `completed`).
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        scanner.enqueue_supported_lunii(3); // F5 re-validation before the write
        scanner.enqueue_supported_lunii(3); // verify re-scan after the write
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(1); // preflight readability proof
        reader.enqueue(Ok(verify_library_with_pack())); // verify sees the landed pack
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        artifacts.enqueue_reaggregate(Ok(PACK_CHECKSUM.to_string()));
        let writer = MockDevicePackWriter::new();
        writer.enqueue_success_with_progress();
        let emitter = CapturingEmitter::default();

        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert!(matches!(outcome, TransferOutcome::Verified { .. }));
        let fractions = emitter.transfer_fractions();
        assert_eq!(fractions.len(), 2, "two progress steps were reported");
        assert!(
            fractions.windows(2).all(|w| w[1] >= w[0]),
            "progress is monotone"
        );
        assert!(
            fractions.iter().all(|f| *f > 0.0 && *f < 1.0),
            "honest fraction: never 100 % before the completed terminal"
        );
        assert!(
            is_monotonic(&emitter.sequences()),
            "the sequence stays monotone across progress events"
        );
    }

    #[test]
    fn retryable_incomplete_when_the_writer_fails_after_mutation() {
        // AC2: a durability/index failure AFTER the content promotion is the
        // honest `transfert incomplet` (the device may hold a partial copy), and
        // the canonical draft is still never mutated (FR18).
        let dir = tempfile::tempdir().expect("app data");
        let db = fresh_db();
        insert_story(&db, "s1");
        insert_import(&db, "s1");
        let scanner = supported_scanner(3);
        scanner.enqueue_supported_lunii(3); // F5 re-validation before the write
        let reader = readable_reader();
        let artifacts = MockTransferArtifactSource::new();
        artifacts.enqueue(Ok(imported_descriptor("s1", "origine_v1")));
        let writer = MockDevicePackWriter::new();
        writer.enqueue_failure_after_mutation(TransferFailureCause::WriteRejected);
        let emitter = CapturingEmitter::default();

        let before = read_story_row(&db, "s1");
        let outcome = transfer_story(
            &db,
            &scanner,
            &reader,
            &artifacts,
            &writer,
            dir.path(),
            "s1",
            &mock_identifier(),
            budget(),
            budget(),
            &emitter,
        );
        assert_eq!(
            outcome,
            TransferOutcome::Retryable {
                cause: TransferFailureCause::WriteRejected,
                completeness: TransferCompleteness::Incomplete,
            }
        );
        assert_eq!(
            writer.call_count(),
            1,
            "the writer ran and reported failure"
        );
        assert_eq!(
            read_story_row(&db, "s1"),
            before,
            "the canonical draft is preserved even after an incomplete transfer"
        );
    }
}
