//! Story-transfer domain (pure, framework-free).
//!
//! The write counterpart of the preparation model: preparation (story 3.x)
//! assembles LOCALLY "what a transfer would need"; this module owns the PURE
//! rules of writing it back to the device — deriving the `.content/<SHORT_ID>`
//! folder name, turning a prepared descriptor into a write plan, the idempotent
//! `.pi` index mutation, cohort coherence, and the closed set of functional
//! transfer-failure causes. No I/O, no `infrastructure/`, no `tauri::*`: the
//! infrastructure writer performs the safe/atomic write, the application layer
//! orchestrates the job, and the IPC layer maps these types to wire DTOs.
//!
//! Decision reminders (see `docs/architecture/ui-states.md#Story Transfer
//! Contract`): the MVP write is the round-trip of an IMPORTED story — the opaque
//! pack bytes are re-written verbatim, never decrypted, never invented. A native
//! story (canonical structure only, no device-format pack) is NOT transferable.
//! A functional failure is the terminal `retryable` state of the job (NOT an
//! `AppError`); each cause maps to one canonical FR `message` + `userAction`.

use crate::domain::device::{is_canonical_pack_uuid, parse_pack_index, LUNII_PACK_UUID_BYTES};
use crate::domain::story::Severity;

use super::{PreparedArtifactKind, TransferArtifactDescriptor};

/// Closed set of functional transfer-failure causes. A functional failure is the
/// terminal `retryable` state of the job (NOT an `AppError`); each cause maps to
/// one canonical FR `message` + `userAction` at the IPC layer, and to a fixed
/// severity below. Transport failures (mount/`app_data_dir` unreachable) stay
/// `AppError::TransferFailed` and are deliberately absent here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFailureCause {
    /// The connected profile is not authorized to be written (V3 / unsupported).
    /// The capability gate refuses BEFORE any device mutation (fail-closed).
    WriteNotAuthorized,
    /// The story has no fresh prepared descriptor — preparation must run first.
    NotPrepared,
    /// The story has no device-format pack (a native story, or a descriptor
    /// without any pack file) — nothing to write back in MVP.
    NotTransferable,
    /// The live re-scan no longer resolves to the requested device (unplugged /
    /// swapped / unreadable), or the prepared cohort no longer matches.
    DeviceChanged,
    /// The device refused the write (no space, I/O error, read-only volume).
    WriteRejected,
    /// The wall-clock budget was exhausted, or the operation was interrupted
    /// (device yanked mid-write, window close). The local draft is preserved.
    Interrupted,
}

impl TransferFailureCause {
    /// Stable snake_case wire/log tag — the closed identifier support greps on,
    /// never a localized message.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::WriteNotAuthorized => "write_not_authorized",
            Self::NotPrepared => "not_prepared",
            Self::NotTransferable => "not_transferable",
            Self::DeviceChanged => "device_changed",
            Self::WriteRejected => "write_rejected",
            Self::Interrupted => "interrupted",
        }
    }

    /// Frozen severity per cause (reuses the canonical-validity vocabulary). It
    /// does NOT change the UI rendering — every transfer failure surfaces as
    /// `échec récupérable` with `Relancer` — but it labels the cause for traces
    /// and keeps the cause→severity mapping under test. `Blocking` marks a
    /// structural limit or integrity problem (a fresh transfer is needed once
    /// the cause is cleared); `Fixable` marks a problem the user can clear with
    /// a direct gesture (prepare first, re-plug the device, retry).
    pub const fn severity(self) -> Severity {
        match self {
            Self::NotPrepared | Self::DeviceChanged | Self::Interrupted => Severity::Fixable,
            Self::WriteNotAuthorized | Self::NotTransferable | Self::WriteRejected => {
                Severity::Blocking
            }
        }
    }

    /// Single canonical FR copy per cause: `(message, userAction)`. The SAME
    /// pair feeds the `job:failed` event (application layer) AND the `retryable`
    /// transfer DTO (IPC layer) — never two wordings for one cause. The UI
    /// renders both strings verbatim and adds the `Relancer` gesture. No
    /// technical jargon leaks (no `write`, `job`, `staging`, `payload`).
    pub const fn copy(self) -> (&'static str, &'static str) {
        match self {
            Self::WriteNotAuthorized => (
                "Envoi impossible : ce modèle de Lunii n'accepte pas encore l'envoi d'histoires.",
                "Branche une Lunii compatible puis relance l'envoi.",
            ),
            Self::NotPrepared => (
                "Envoi impossible : l'histoire n'est pas encore prête.",
                "Prépare l'histoire puis relance l'envoi.",
            ),
            Self::NotTransferable => (
                "Envoi impossible : cette histoire n'a pas de version compatible avec l'appareil.",
                "Importe une histoire depuis une Lunii pour pouvoir l'y renvoyer.",
            ),
            Self::DeviceChanged => (
                "Envoi interrompu : l'appareil connecté a changé.",
                "Rebranche la Lunii souhaitée puis relance l'envoi.",
            ),
            Self::WriteRejected => (
                "Envoi interrompu : la Lunii a refusé l'enregistrement de l'histoire.",
                "Vérifie l'espace disponible sur la Lunii puis relance l'envoi.",
            ),
            Self::Interrupted => ("Envoi interrompu avant la fin.", "Relance l'envoi."),
        }
    }
}

/// Derive the `.content/<SHORT_ID>` folder name from a canonical pack UUID: the
/// UPPERCASE last 8 hex characters (= the last four UUID bytes), mirroring the
/// device's own folder-naming convention and [`pack_short_id`]. Returns `None`
/// for a non-canonical value — a programming-error guard, since callers pass the
/// value the import recorded, which the schema keeps canonical.
///
/// [`pack_short_id`]: crate::domain::device::pack_short_id
pub fn short_id_from_pack_uuid(pack_uuid: &str) -> Option<String> {
    if !is_canonical_pack_uuid(pack_uuid) {
        return None;
    }
    Some(pack_uuid[pack_uuid.len() - 8..].to_ascii_uppercase())
}

/// Parse a canonical lowercase hyphenated UUID into its 16 raw bytes — the
/// on-device `.pi` representation that [`append_pack_uuid`] writes. `None` for a
/// non-canonical value (the same fail-closed guard as [`short_id_from_pack_uuid`]).
pub fn pack_uuid_bytes(pack_uuid: &str) -> Option<[u8; LUNII_PACK_UUID_BYTES]> {
    if !is_canonical_pack_uuid(pack_uuid) {
        return None;
    }
    let hex: Vec<u8> = pack_uuid.bytes().filter(|b| *b != b'-').collect();
    if hex.len() != LUNII_PACK_UUID_BYTES * 2 {
        return None;
    }
    let mut bytes = [0u8; LUNII_PACK_UUID_BYTES];
    for (i, slot) in bytes.iter_mut().enumerate() {
        let pair = std::str::from_utf8(&hex[i * 2..i * 2 + 2]).ok()?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(bytes)
}

/// One file the device write must reproduce, in its pack-relative location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackWriteFile {
    pub rel_path: String,
    pub byte_len: u64,
    pub checksum: String,
}

/// The plan a device write executes: the target `.content/<SHORT_ID>` folder
/// name plus the files (references + integrity values) to reproduce there. Built
/// purely from a prepared descriptor — never the duplicated bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackWritePlan {
    pub short_id: String,
    pub files: Vec<PackWriteFile>,
}

/// Turn a prepared [`TransferArtifactDescriptor`] into a [`PackWritePlan`] for
/// the `.content/<SHORT_ID>` folder named by `short_id`.
///
/// Only [`PreparedArtifactKind::PackFile`] artifacts are written (the opaque
/// imported pack bytes). A descriptor with NO pack file — a native minimal story
/// carries only its [`PreparedArtifactKind::CanonicalStructure`] — is
/// [`TransferFailureCause::NotTransferable`]: there is no device-format pack to
/// round-trip, and MVP has no media transcoder to synthesize one.
pub fn build_write_plan(
    descriptor: &TransferArtifactDescriptor,
    short_id: &str,
) -> Result<PackWritePlan, TransferFailureCause> {
    let files: Vec<PackWriteFile> = descriptor
        .artifacts
        .iter()
        .filter(|a| a.kind == PreparedArtifactKind::PackFile)
        .map(|a| PackWriteFile {
            rel_path: a.relative_ref.clone(),
            byte_len: a.byte_len,
            checksum: a.checksum.clone(),
        })
        .collect();
    if files.is_empty() {
        return Err(TransferFailureCause::NotTransferable);
    }
    Ok(PackWritePlan {
        short_id: short_id.to_string(),
        files,
    })
}

/// Append a pack UUID's 16 bytes to a `.pi` index payload, IDEMPOTENTLY: a UUID
/// already present (as a clean 16-byte chunk) yields the payload unchanged; an
/// absent one is appended at EOF — the device's own "list of installed packs,
/// 16 bytes each, in reading order" convention. A trailing partial chunk of an
/// already-corrupt index is left untouched (we never rewrite bytes we did not
/// author). Pure: the infrastructure writer persists the returned bytes
/// atomically (temp + rename) only AFTER the pack content is safely promoted.
pub fn append_pack_uuid(pi_bytes: &[u8], uuid_bytes: &[u8; LUNII_PACK_UUID_BYTES]) -> Vec<u8> {
    let index = parse_pack_index(pi_bytes);
    if index.uuids.iter().any(|existing| existing == uuid_bytes) {
        return pi_bytes.to_vec();
    }
    let mut out = pi_bytes.to_vec();
    out.extend_from_slice(uuid_bytes);
    out
}

/// Ensure the prepared descriptor targets the cohort of the connected device.
///
/// A mismatch means the artifacts were prepared for a DIFFERENT device than the
/// one now connected (e.g. a v3-metadata pack about to land on a v6 device).
/// Treated as [`TransferFailureCause::DeviceChanged`] — the write target is not
/// the one the preparation was pinned to, so it is refused rather than written
/// blindly.
pub fn ensure_cohort_coherent(
    descriptor_cohort: &str,
    device_cohort: &str,
) -> Result<(), TransferFailureCause> {
    if descriptor_cohort == device_cohort {
        Ok(())
    } else {
        Err(TransferFailureCause::DeviceChanged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::{format_pack_uuid, pack_short_id};
    use crate::domain::transfer::{PreparedArtifact, PREPARATION_PIPELINE_VERSION};

    const PACK_UUID: &str = "abababab-abab-abab-abab-ababfac5562d";

    fn pack_file_artifact(rel: &str, size: u64) -> PreparedArtifact {
        PreparedArtifact {
            kind: PreparedArtifactKind::PackFile,
            relative_ref: rel.into(),
            byte_len: size,
            checksum: "a".repeat(64),
        }
    }

    fn descriptor(artifacts: Vec<PreparedArtifact>) -> TransferArtifactDescriptor {
        TransferArtifactDescriptor {
            story_id: "0197a5d0-0000-7000-8000-000000000000".into(),
            target_cohort: "origine_v1".into(),
            pipeline_version: PREPARATION_PIPELINE_VERSION,
            artifacts,
            aggregate_checksum: "a".repeat(64),
        }
    }

    #[test]
    fn short_id_is_uppercase_last_eight_hex_of_canonical_uuid() {
        assert_eq!(
            short_id_from_pack_uuid(PACK_UUID).as_deref(),
            Some("FAC5562D")
        );
        // Matches the byte-based derivation used everywhere else.
        let bytes = pack_uuid_bytes(PACK_UUID).expect("canonical");
        assert_eq!(
            short_id_from_pack_uuid(PACK_UUID).unwrap(),
            pack_short_id(&bytes)
        );
    }

    #[test]
    fn short_id_refuses_a_non_canonical_uuid() {
        assert!(short_id_from_pack_uuid("not-a-uuid").is_none());
        assert!(short_id_from_pack_uuid("ABABABAB-ABAB-ABAB-ABAB-ABABFAC5562D").is_none());
        assert!(short_id_from_pack_uuid("").is_none());
    }

    #[test]
    fn pack_uuid_bytes_round_trips_with_format_pack_uuid() {
        let bytes = pack_uuid_bytes(PACK_UUID).expect("canonical");
        assert_eq!(format_pack_uuid(&bytes), PACK_UUID);
    }

    #[test]
    fn pack_uuid_bytes_refuses_a_non_canonical_uuid() {
        assert!(pack_uuid_bytes("nope").is_none());
        assert!(pack_uuid_bytes("ABABABAB-ABAB-ABAB-ABAB-ABABFAC5562D").is_none());
    }

    #[test]
    fn append_pack_uuid_adds_an_absent_uuid_at_eof() {
        let uuid = pack_uuid_bytes(PACK_UUID).unwrap();
        let out = append_pack_uuid(&[], &uuid);
        assert_eq!(out, uuid.to_vec());

        let existing = pack_uuid_bytes("11111111-1111-1111-1111-111111111111").unwrap();
        let out = append_pack_uuid(&existing, &uuid);
        assert_eq!(
            out.len(),
            32,
            "an absent uuid is appended after the existing one"
        );
        assert_eq!(&out[..16], &existing);
        assert_eq!(&out[16..], &uuid);
    }

    #[test]
    fn append_pack_uuid_is_idempotent_when_already_present() {
        let uuid = pack_uuid_bytes(PACK_UUID).unwrap();
        let pi = uuid.to_vec();
        assert_eq!(
            append_pack_uuid(&pi, &uuid),
            pi,
            "a present uuid is a no-op"
        );

        // Present among several entries → still unchanged.
        let other = pack_uuid_bytes("22222222-2222-2222-2222-222222222222").unwrap();
        let mut multi = other.to_vec();
        multi.extend_from_slice(&uuid);
        assert_eq!(append_pack_uuid(&multi, &uuid), multi);
    }

    #[test]
    fn build_write_plan_keeps_pack_files_for_an_imported_story() {
        let d = descriptor(vec![
            pack_file_artifact("ni", 4),
            pack_file_artifact("rf/000/AAAAAAAA", 8),
        ]);
        let plan = build_write_plan(&d, "FAC5562D").expect("imported is transferable");
        assert_eq!(plan.short_id, "FAC5562D");
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.files[0].rel_path, "ni");
        assert_eq!(plan.files[1].rel_path, "rf/000/AAAAAAAA");
    }

    #[test]
    fn build_write_plan_refuses_a_native_story_as_not_transferable() {
        let d = descriptor(vec![PreparedArtifact {
            kind: PreparedArtifactKind::CanonicalStructure,
            relative_ref: "structure.json".into(),
            byte_len: 30,
            checksum: "a".repeat(64),
        }]);
        assert_eq!(
            build_write_plan(&d, "FAC5562D").expect_err("native must refuse"),
            TransferFailureCause::NotTransferable
        );
    }

    #[test]
    fn cohort_coherence_passes_on_match_and_fails_device_changed_on_mismatch() {
        assert!(ensure_cohort_coherent("origine_v1", "origine_v1").is_ok());
        assert_eq!(
            ensure_cohort_coherent("origine_v1", "mid_gen_v2").expect_err("mismatch must refuse"),
            TransferFailureCause::DeviceChanged
        );
    }

    #[test]
    fn failure_cause_severity_mapping_is_frozen() {
        assert_eq!(
            TransferFailureCause::NotPrepared.severity(),
            Severity::Fixable
        );
        assert_eq!(
            TransferFailureCause::DeviceChanged.severity(),
            Severity::Fixable
        );
        assert_eq!(
            TransferFailureCause::Interrupted.severity(),
            Severity::Fixable
        );
        assert_eq!(
            TransferFailureCause::WriteNotAuthorized.severity(),
            Severity::Blocking
        );
        assert_eq!(
            TransferFailureCause::NotTransferable.severity(),
            Severity::Blocking
        );
        assert_eq!(
            TransferFailureCause::WriteRejected.severity(),
            Severity::Blocking
        );
    }

    #[test]
    fn every_failure_cause_has_non_empty_copy() {
        for cause in [
            TransferFailureCause::WriteNotAuthorized,
            TransferFailureCause::NotPrepared,
            TransferFailureCause::NotTransferable,
            TransferFailureCause::DeviceChanged,
            TransferFailureCause::WriteRejected,
            TransferFailureCause::Interrupted,
        ] {
            let (message, action) = cause.copy();
            assert!(!message.is_empty(), "{cause:?} message empty");
            assert!(!action.is_empty(), "{cause:?} userAction empty");
        }
    }

    #[test]
    fn failure_cause_diagnostic_tags_are_stable_and_distinct() {
        let tags = [
            TransferFailureCause::WriteNotAuthorized.diagnostic_tag(),
            TransferFailureCause::NotPrepared.diagnostic_tag(),
            TransferFailureCause::NotTransferable.diagnostic_tag(),
            TransferFailureCause::DeviceChanged.diagnostic_tag(),
            TransferFailureCause::WriteRejected.diagnostic_tag(),
            TransferFailureCause::Interrupted.diagnostic_tag(),
        ];
        let mut unique = tags.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), tags.len(), "tags must be distinct");
        assert!(tags.iter().all(|t| !t.is_empty()));
    }
}
