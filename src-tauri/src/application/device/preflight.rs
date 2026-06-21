//! Story-validation (`preflight`) application service.
//!
//! Composes, in Rust, the read-only validation verdict shown before a
//! transfer: given a selected local story and a connected supported Lunii,
//! is the story canonically valid (Rustory's own data sound) AND is the
//! detected device profile compatible? The verdict is one of
//! `présumée transférable` / `à corriger` / `bloquée`, derived from a closed
//! `axis × cause` blocker list.
//!
//! The flow mirrors [`read_transfer_preview`](super::transfer::read_transfer_preview):
//! the live device is re-scanned through the proven [`read_device_library`]
//! pipeline (re-scan + identity guard + `ReadLibrary` gate), and the local
//! canonical facts (`title`, `schema_version`, `structure_json`,
//! `content_checksum`) are read AFTER the device I/O under a scoped DB lock —
//! never held across the scan.
//!
//! Two orthogonal axes (AC1): the `Structure` / `Media` / `Filesystem` axes
//! carry Rustory canonical validity (composed by
//! [`validate_canonical`](crate::domain::story::validate_canonical)); the
//! `DeviceProfile` axis carries Lunii compatibility. In MVP Phase 1 a verdict
//! is only ever composed for a CONFIRMED readable supported device (the
//! `Readable` outcome, whose identity matched the request), so a supported
//! profile is compatible by construction and emits no `device_profile`
//! blocker. The `DeviceProfile` axis and its causes are DECLARED in the closed
//! taxonomy (wire-ready, like `Media` / `Filesystem`) but have NO live emitter
//! here: a re-scan that no longer resolves to the requested readable device
//! (none / unsupported / ambiguous) cannot prove the present device is the one
//! the UI asked about, so it surfaces a recoverable `device_changed` rather
//! than a compatibility verdict on an unconfirmed device.
//!
//! Read-only by contract: nothing is written, no `validation_status` is
//! persisted (freshness over caching on a decision surface), and the verdict is
//! ORTHOGONAL to the `WriteStory` gate (the send CTA stays disabled in MVP
//! regardless of the verdict). Stays Tauri-free: tests inject a
//! [`MockDeviceScanner`](crate::infrastructure::device::MockDeviceScanner) +
//! [`MockDeviceLibraryReader`](crate::infrastructure::device::MockDeviceLibraryReader)
//! and a temp DB.

use std::sync::Mutex;
use std::time::Duration;

use rusqlite::OptionalExtension;

use crate::domain::device::UnsupportedReason;
use crate::domain::shared::AppError;
use crate::domain::story::{
    validate_canonical, Axis, CanonicalBlocker, CanonicalCause, CanonicalStoryFacts, Severity,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::device::{DeviceLibraryReader, DeviceScanner};

use super::library::{read_device_library, DeviceLibraryOutcome};

/// The composed validation verdict. Derived in Rust from the blocker list —
/// the frontend maps it to a label + chip tone, never recomputes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No real block — canonically valid + a recognized, supported profile.
    /// NOT a transfer success (the write + verification happen later).
    PresumedTransferable,
    /// A repairable block exists (e.g. an invalid title) and no hard block.
    ToFix,
    /// A hard block exists (corrupt/incoherent canonical data, or an
    /// unreadable / unsupported device profile).
    Blocked,
}

/// A composed blocker spanning the two axes. The cause is either a canonical
/// (structure) cause or a device-profile (`UnsupportedReason`) cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocker {
    pub axis: Axis,
    pub cause: BlockerCause,
    pub severity: Severity,
}

/// Closed union of blocker causes across the two axes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockerCause {
    Canonical(CanonicalCause),
    DeviceProfile(UnsupportedReason),
}

impl Blocker {
    fn from_canonical(b: CanonicalBlocker) -> Self {
        Self {
            axis: b.axis,
            cause: BlockerCause::Canonical(b.cause),
            severity: b.severity,
        }
    }
}

/// Result of [`read_story_validation`]. Mapped 1-to-1 by the IPC layer to
/// `StoryValidationDto`. Recoverable failures (device unplugged mid-read,
/// identity changed, local store unavailable, selected story vanished) propagate
/// as `Err(AppError)` — they are transport failures, not verdict states.
///
/// There is NO separate `Unsupported` variant: an unreadable / ambiguous /
/// unsupported profile is a `device_profile` `Blocker` inside `Ready`, so the
/// canonical axis stays visible alongside (AC1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoryValidationOutcome {
    /// No supported device is connected anymore (the requested readable device
    /// folded to nothing — the UI surfaces a recoverable "device changed").
    NoDevice,
    /// The verdict was composed.
    Ready {
        device_identifier: String,
        story_id: String,
        story_title: String,
        verdict: Verdict,
        blockers: Vec<Blocker>,
    },
}

/// Compose the pre-transfer validation verdict for `story_id` against the
/// supported Lunii whose identifier equals `requested_device_identifier`.
pub fn read_story_validation(
    db: &Mutex<DbHandle>,
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    story_id: &str,
    requested_device_identifier: &str,
    budget: Duration,
) -> Result<StoryValidationOutcome, AppError> {
    // Authoritative device read FIRST — re-scan + identity guard + ReadLibrary
    // gate — reusing the proven device-library pipeline. The DB mutex is NOT
    // held during this I/O.
    let outcome =
        read_device_library(scanner, library_reader, requested_device_identifier, budget)?;

    match outcome {
        DeviceLibraryOutcome::None => Ok(StoryValidationOutcome::NoDevice),
        DeviceLibraryOutcome::Unsupported { .. } => {
            // The re-scan no longer resolves to a readable supported device: a
            // DIFFERENT device (unsupported) or more than one (ambiguous) is now
            // present, and the `read_device_library` pipeline only confirms the
            // requested identity in its `Readable` branch. Composing a verdict
            // here would assert a compatibility result about a device whose
            // identity we never proved — exactly the "never a compatibility on
            // stale/changed data" guardrail. Surface a recoverable
            // `device_changed` instead. The `device_profile` axis stays declared
            // in the taxonomy (wire-ready) but has no live emitter in MVP.
            Err(device_changed_error())
        }
        DeviceLibraryOutcome::Readable {
            device_identifier, ..
        } => {
            // The ONLY path that composes a verdict: the live device matched the
            // requested identity (proven in `read_device_library`). A supported
            // profile is compatible by construction (no media or node-format
            // check exists before the media-preparation step) — so the only
            // blockers are canonical.
            let facts = read_canonical_facts(db, story_id)?;
            Ok(compose_ready(device_identifier, story_id, &facts))
        }
    }
}

/// Compose the `Ready` outcome from the canonical blockers and the derived
/// verdict. NEVER consults `WriteStory` (the gate is a phase concern, orthogonal
/// to the verdict) and never emits a `device_profile` blocker (a supported
/// profile is compatible by construction in MVP).
fn compose_ready(
    device_identifier: String,
    story_id: &str,
    facts: &CanonicalStoryFacts,
) -> StoryValidationOutcome {
    let blockers: Vec<Blocker> = validate_canonical(facts)
        .into_iter()
        .map(Blocker::from_canonical)
        .collect();
    let verdict = derive_verdict(&blockers);
    StoryValidationOutcome::Ready {
        device_identifier,
        story_id: story_id.to_string(),
        story_title: facts.title.clone(),
        verdict,
        blockers,
    }
}

/// `bloquée` if any hard block exists, else `à corriger` if any repairable
/// block exists, else `présumée transférable`.
fn derive_verdict(blockers: &[Blocker]) -> Verdict {
    if blockers.iter().any(|b| b.severity == Severity::Blocking) {
        Verdict::Blocked
    } else if blockers.iter().any(|b| b.severity == Severity::Fixable) {
        Verdict::ToFix
    } else {
        Verdict::PresumedTransferable
    }
}

/// Read the selected story's canonical facts (the EXACT stored values, never
/// recomputed) under a scoped DB lock taken here — never held across the scan.
fn read_canonical_facts(
    db: &Mutex<DbHandle>,
    story_id: &str,
) -> Result<CanonicalStoryFacts, AppError> {
    let db = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    resolve_canonical_facts(&db, story_id)
}

fn resolve_canonical_facts(db: &DbHandle, story_id: &str) -> Result<CanonicalStoryFacts, AppError> {
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
    // A vanished story is a race (deleted between selection and this read):
    // recoverable LIBRARY_INCONSISTENT, not a fabricated verdict.
    let Some((title, schema_version, structure_json, content_checksum)) = row else {
        return Err(story_missing_error());
    };
    Ok(CanonicalStoryFacts {
        title,
        schema_version,
        structure_json,
        content_checksum,
    })
}

/// The re-scan no longer resolves to the requested readable supported device
/// (none / unsupported / ambiguous answered the live scan). Recoverable: the UI
/// folds the validation and re-detects, exactly like the comparison's
/// `device_changed`. Mirrors the `read_device_library` identity-mismatch copy
/// so the closed `device_changed` taxonomy stays single-sourced.
fn device_changed_error() -> AppError {
    AppError::device_scan_failed(
        "Validation indisponible: l'appareil connecté a changé.",
        "Rebranche la Lunii souhaitée puis réessaie la validation.",
    )
    .with_details(serde_json::json!({
        "source": "device_changed",
    }))
}

fn story_missing_error() -> AppError {
    AppError::library_inconsistent(
        "Validation impossible: histoire introuvable dans la bibliothèque locale.",
        "Recharge la bibliothèque puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "story_validation",
        "cause": "story_missing",
    }))
}

fn local_read_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Validation indisponible: vérifie le disque local et réessaie.",
        "Réessaie ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "story_validation",
        "stage": stage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::story::content_checksum;
    use crate::infrastructure::db;
    use crate::infrastructure::device::{
        compute_device_identifier, MockDeviceLibraryReader, MockDeviceScanner,
    };

    const HEALTHY_JSON: &str = "{\"schemaVersion\":1,\"nodes\":[]}";

    fn budget() -> Duration {
        Duration::from_secs(5)
    }

    /// The identifier the mock scanner's `enqueue_supported_lunii` volume hashes
    /// to (`.pi` = `MOCK_PI`, serial = `MOCK_SERIAL`).
    fn mock_identifier() -> String {
        compute_device_identifier(b"MOCK_PI", Some("MOCK_SERIAL"))
    }

    fn fresh_db() -> Mutex<DbHandle> {
        let mut handle = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut handle).expect("migrate");
        Mutex::new(handle)
    }

    fn insert_story(
        db: &Mutex<DbHandle>,
        id: &str,
        title: &str,
        schema_version: u32,
        structure_json: &str,
        content_checksum: &str,
    ) {
        db.lock()
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, '2026-06-19T00:00:00.000Z', '2026-06-19T00:00:00.000Z')",
                rusqlite::params![id, title, schema_version, structure_json, content_checksum],
            )
            .expect("insert story");
    }

    fn insert_healthy(db: &Mutex<DbHandle>, id: &str, title: &str) {
        insert_story(
            db,
            id,
            title,
            1,
            HEALTHY_JSON,
            &content_checksum(HEALTHY_JSON),
        );
    }

    fn supported(version: u8) -> (MockDeviceScanner, MockDeviceLibraryReader) {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(version);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue_library_with(2);
        (scanner, reader)
    }

    fn ready(outcome: StoryValidationOutcome) -> (Verdict, Vec<Blocker>, String) {
        match outcome {
            StoryValidationOutcome::Ready {
                verdict,
                blockers,
                device_identifier,
                ..
            } => (verdict, blockers, device_identifier),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn presumed_transferable_for_healthy_story_and_supported_cohort() {
        for version in [3u8, 6, 7] {
            let db = fresh_db();
            insert_healthy(&db, "s1", "Mon histoire");
            let (scanner, reader) = supported(version);
            let outcome =
                read_story_validation(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                    .expect("validation");
            let (verdict, blockers, device_identifier) = ready(outcome);
            assert_eq!(verdict, Verdict::PresumedTransferable, "md v{version}");
            assert!(blockers.is_empty(), "md v{version} must have no blockers");
            assert_eq!(device_identifier, mock_identifier());
        }
    }

    #[test]
    fn blocked_when_checksum_mismatches() {
        let db = fresh_db();
        insert_story(&db, "s1", "Corrompue", 1, HEALTHY_JSON, &"0".repeat(64));
        let (scanner, reader) = supported(3);
        let outcome =
            read_story_validation(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("validation");
        let (verdict, blockers, _) = ready(outcome);
        assert_eq!(verdict, Verdict::Blocked);
        assert_eq!(
            blockers,
            vec![Blocker {
                axis: Axis::Structure,
                cause: BlockerCause::Canonical(CanonicalCause::ChecksumMismatch),
                severity: Severity::Blocking,
            }]
        );
    }

    #[test]
    fn blocked_when_schema_is_too_recent() {
        let db = fresh_db();
        let json = "{\"schemaVersion\":2,\"nodes\":[]}";
        insert_story(&db, "s1", "Trop récente", 2, json, &content_checksum(json));
        let (scanner, reader) = supported(6);
        let outcome =
            read_story_validation(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("validation");
        let (verdict, blockers, _) = ready(outcome);
        assert_eq!(verdict, Verdict::Blocked);
        assert!(blockers.iter().any(|b| matches!(
            b.cause,
            BlockerCause::Canonical(CanonicalCause::SchemaUnsupported)
        )));
    }

    #[test]
    fn blocked_when_structure_is_unreadable() {
        let db = fresh_db();
        let garbage = "not json at all";
        insert_story(
            &db,
            "s1",
            "Illisible",
            1,
            garbage,
            &content_checksum(garbage),
        );
        let (scanner, reader) = supported(7);
        let outcome =
            read_story_validation(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("validation");
        let (verdict, blockers, _) = ready(outcome);
        assert_eq!(verdict, Verdict::Blocked);
        assert!(blockers.iter().any(|b| matches!(
            b.cause,
            BlockerCause::Canonical(CanonicalCause::StructureCorrupt)
        )));
    }

    #[test]
    fn to_fix_when_only_the_title_is_invalid() {
        let db = fresh_db();
        // Internal control char: passes the SQL `length(trim(title)) > 0` CHECK
        // but fails `validate_title` — a purely fixable (renameable) block.
        insert_healthy(&db, "s1", "Ligne1\nLigne2");
        let (scanner, reader) = supported(3);
        let outcome =
            read_story_validation(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("validation");
        let (verdict, blockers, _) = ready(outcome);
        assert_eq!(verdict, Verdict::ToFix);
        assert_eq!(
            blockers,
            vec![Blocker {
                axis: Axis::Structure,
                cause: BlockerCause::Canonical(CanonicalCause::TitleInvalid),
                severity: Severity::Fixable,
            }]
        );
    }

    #[test]
    fn unsupported_rescan_yields_recoverable_device_changed() {
        // The re-scan now finds an UNSUPPORTED device — not the readable one the
        // UI asked about. Its identity is unconfirmed, so we never compose a
        // compatibility verdict: a recoverable `device_changed` is returned and
        // the `device_profile` axis stays declared-but-unemitted in MVP.
        let db = fresh_db();
        insert_healthy(&db, "s1", "Saine");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_unsupported_metadata(99);
        let reader = MockDeviceLibraryReader::new();
        let err = read_story_validation(&db, &scanner, &reader, "s1", "whatever", budget())
            .expect_err("an unsupported re-scan must fail recoverably");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "device_changed");
    }

    #[test]
    fn ambiguous_rescan_yields_recoverable_device_changed() {
        // Two supported Lunii now present: the requested one cannot be bound
        // unambiguously → recoverable `device_changed`, never a verdict.
        let db = fresh_db();
        insert_healthy(&db, "s1", "Saine");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_multiple_candidates();
        let reader = MockDeviceLibraryReader::new();
        let err = read_story_validation(&db, &scanner, &reader, "s1", "whatever", budget())
            .expect_err("an ambiguous re-scan must fail recoverably");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "device_changed");
    }

    #[test]
    fn a_supported_device_never_emits_a_device_profile_blocker() {
        // A corrupt local story on a CONFIRMED supported device → only canonical
        // blockers; the Lunii-compatibility axis is clear (supported = compatible
        // by construction), so no `device_profile` blocker is ever emitted.
        let db = fresh_db();
        insert_story(&db, "s1", "Corrompue", 1, HEALTHY_JSON, &"0".repeat(64));
        let (scanner, reader) = supported(7);
        let outcome =
            read_story_validation(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("validation");
        let (verdict, blockers, _) = ready(outcome);
        assert_eq!(verdict, Verdict::Blocked);
        assert!(blockers.iter().all(|b| b.axis == Axis::Structure));
        assert!(blockers.iter().all(|b| b.axis != Axis::DeviceProfile));
    }

    #[test]
    fn no_device_when_scanner_finds_nothing() {
        let db = fresh_db();
        insert_healthy(&db, "s1", "Saine");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_no_device();
        let reader = MockDeviceLibraryReader::new();
        let outcome =
            read_story_validation(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("validation");
        assert_eq!(outcome, StoryValidationOutcome::NoDevice);
    }

    #[test]
    fn identifier_mismatch_propagates_recoverable_device_changed() {
        let db = fresh_db();
        insert_healthy(&db, "s1", "Saine");
        let (scanner, reader) = supported(3);
        let err = read_story_validation(
            &db,
            &scanner,
            &reader,
            "s1",
            "deadbeefdeadbeefdeadbeefdeadbeef",
            budget(),
        )
        .expect_err("identity mismatch must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "device_changed");
    }

    #[test]
    fn missing_story_yields_recoverable_library_inconsistent() {
        let db = fresh_db();
        // No story seeded — the selection vanished between pick and read.
        let (scanner, reader) = supported(3);
        let err = read_story_validation(
            &db,
            &scanner,
            &reader,
            "ghost",
            &mock_identifier(),
            budget(),
        )
        .expect_err("a vanished story must fail recoverably");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "LIBRARY_INCONSISTENT");
        assert_eq!(v["details"]["source"], "story_validation");
        assert_eq!(v["details"]["cause"], "story_missing");
    }
}
