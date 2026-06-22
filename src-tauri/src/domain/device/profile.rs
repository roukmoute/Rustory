use super::family::{DeviceFamily, LuniiFirmwareCohort};
use super::operations::SupportedOperations;

/// Canonical description of a recognized device. Built only by
/// [`classify_lunii`] — a `DeviceProfile` value is the proof that the
/// candidate volume passed every required check (markers + metadata
/// version + identifier hashing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceProfile {
    pub family: DeviceFamily,
    pub firmware_cohort: LuniiFirmwareCohort,
    /// Raw metadata format version read from `.md` (3, 6, 7 in MVP).
    pub metadata_format_version: u8,
    /// Hashed device identifier (digest of `.pi` content + volume
    /// serial when available). Stable across reboots, opaque to UI.
    /// NEVER carries the raw `.pi` bytes — those may include a hardware
    /// serial that the user did not consent to expose.
    pub device_identifier: String,
    pub supported_operations: SupportedOperations,
}

/// Outcome of profile classification. The `Unsupported` variant carries
/// a typed `reason` so the UI maps to a stable copy without parsing a
/// free-form string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceProfileClassification {
    Supported(DeviceProfile),
    Unsupported {
        reason: UnsupportedReason,
        family_hint: Option<DeviceFamily>,
        firmware_hint: Option<String>,
    },
}

/// Closed set. ANY new failure mode adds a variant here AND a string in
/// `docs/architecture/ui-states.md#Disabled Actions and Reasons`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedReason {
    FirmwareUnsupported,
    MetadataUnsupported,
    MetadataCorrupt,
    FamilyUnknown,
    OperationNotAuthorized,
    MultipleCandidates,
}

impl UnsupportedReason {
    /// Stable diagnostic tag for logs and error details.
    pub const fn diagnostic_tag(&self) -> &'static str {
        match self {
            Self::FirmwareUnsupported => "firmware_unsupported",
            Self::MetadataUnsupported => "metadata_unsupported",
            Self::MetadataCorrupt => "metadata_corrupt",
            Self::FamilyUnknown => "family_unknown",
            Self::OperationNotAuthorized => "operation_not_authorized",
            Self::MultipleCandidates => "multiple_candidates",
        }
    }
}

/// Classify a candidate Lunii volume into a [`DeviceProfileClassification`].
///
/// Inputs:
/// - `metadata_version`: first byte of `.md` (3, 6, 7 → supported; else
///   unsupported)
/// - `has_pi`: required marker — missing it signals `MetadataCorrupt`
/// - `has_bt`: informational only — kept in the signature so call sites
///   stay symmetric with the future capability matrix, but does NOT
///   gate classification. Real-world Lunii devices (observed: V3
///   firmware 3.3.2) ship without `.bt`; gating on it would produce a
///   false-negative `MetadataCorrupt` for working hardware.
/// - `hashed_id`: opaque BLAKE2/SHA-256 digest of `.pi` + volume serial.
///   Never the raw payload.
///
/// `.pi` is the only universal device-id marker observed across V1 /
/// V2 / V3 in 2026 — `.md` proves "this is a Lunii-shaped volume",
/// `.pi` proves "this Lunii has a device identity Rustory can hash".
/// Missing either is a true corruption signal.
pub fn classify_lunii(
    metadata_version: u8,
    has_pi: bool,
    _has_bt: bool,
    hashed_id: &str,
) -> DeviceProfileClassification {
    if !has_pi {
        return DeviceProfileClassification::Unsupported {
            reason: UnsupportedReason::MetadataCorrupt,
            family_hint: Some(DeviceFamily::Lunii),
            firmware_hint: None,
        };
    }

    let (cohort, ops) = match metadata_version {
        3 => (
            LuniiFirmwareCohort::OrigineV1,
            // Epic 3 wires the write gate: Origine v1 accepts the round-trip
            // of an imported pack (opaque bytes already in device format) —
            // the safest possible write, reproducing what the device held.
            SupportedOperations {
                read_library: true,
                inspect_story: true,
                import_story: true,
                write_story: true,
            },
        ),
        6 => (
            LuniiFirmwareCohort::MidGenV2,
            // Mid-Gen v2: same round-trip write support as Origine v1.
            SupportedOperations {
                read_library: true,
                inspect_story: true,
                import_story: true,
                write_story: true,
            },
        ),
        7 => (
            LuniiFirmwareCohort::V3,
            // V3 reverse engineering is still active in 2026; we cannot
            // guarantee a corruption-free import. Read is allowed (just
            // file enumeration), inspect is allowed (read-only metadata
            // peek), but import_story stays false and write_story stays
            // false until the V3 pipeline is verified end-to-end.
            SupportedOperations {
                read_library: true,
                inspect_story: true,
                import_story: false,
                write_story: false,
            },
        ),
        _ => {
            return DeviceProfileClassification::Unsupported {
                reason: UnsupportedReason::MetadataUnsupported,
                family_hint: Some(DeviceFamily::Lunii),
                firmware_hint: Some(format!("metadata_v{metadata_version}")),
            };
        }
    };

    DeviceProfileClassification::Supported(DeviceProfile {
        family: DeviceFamily::Lunii,
        firmware_cohort: cohort,
        metadata_format_version: metadata_version,
        device_identifier: hashed_id.to_string(),
        supported_operations: ops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported_profile(c: DeviceProfileClassification) -> DeviceProfile {
        match c {
            DeviceProfileClassification::Supported(p) => p,
            other => panic!("expected Supported, got {other:?}"),
        }
    }

    fn unsupported_reason(c: &DeviceProfileClassification) -> &UnsupportedReason {
        match c {
            DeviceProfileClassification::Unsupported { reason, .. } => reason,
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn classify_lunii_v3_metadata_returns_supported_origine_with_write_enabled() {
        let p = supported_profile(classify_lunii(3, true, true, "abc"));
        assert_eq!(p.family, DeviceFamily::Lunii);
        assert_eq!(p.firmware_cohort, LuniiFirmwareCohort::OrigineV1);
        assert_eq!(p.metadata_format_version, 3);
        assert_eq!(p.device_identifier, "abc");
        assert!(p.supported_operations.read_library);
        assert!(p.supported_operations.inspect_story);
        assert!(p.supported_operations.import_story);
        assert!(p.supported_operations.write_story);
    }

    #[test]
    fn classify_lunii_v6_metadata_returns_supported_midgen_v2_with_write_enabled() {
        let p = supported_profile(classify_lunii(6, true, true, "abc"));
        assert_eq!(p.firmware_cohort, LuniiFirmwareCohort::MidGenV2);
        assert_eq!(p.metadata_format_version, 6);
        assert!(p.supported_operations.read_library);
        assert!(p.supported_operations.inspect_story);
        assert!(p.supported_operations.import_story);
        assert!(p.supported_operations.write_story);
    }

    #[test]
    fn classify_lunii_v7_metadata_returns_supported_v3_with_import_disabled() {
        let p = supported_profile(classify_lunii(7, true, true, "abc"));
        assert_eq!(p.firmware_cohort, LuniiFirmwareCohort::V3);
        assert_eq!(p.metadata_format_version, 7);
        assert!(p.supported_operations.read_library);
        assert!(p.supported_operations.inspect_story);
        assert!(!p.supported_operations.import_story);
        assert!(!p.supported_operations.write_story);
    }

    #[test]
    fn write_story_is_enabled_for_v1_v2_and_blocked_for_v3_in_mvp_phase_1() {
        // Epic 3 wires the write gate: Origine v1 / Mid-Gen v2 accept the
        // round-trip of an imported pack; V3 stays write-blocked while its
        // format reverse-engineering is still active (same rationale as import).
        for (v, expected) in [(3u8, true), (6, true), (7, false)] {
            let p = supported_profile(classify_lunii(v, true, true, "id"));
            assert_eq!(
                p.supported_operations.write_story, expected,
                "metadata v{v} write_story must be {expected} in MVP",
            );
        }
    }

    #[test]
    fn v3_profile_has_import_story_false() {
        let p = supported_profile(classify_lunii(7, true, true, "id"));
        assert!(!p.supported_operations.import_story);
    }

    #[test]
    fn origine_v1_profile_has_import_story_true() {
        let p = supported_profile(classify_lunii(3, true, true, "id"));
        assert!(p.supported_operations.import_story);
    }

    #[test]
    fn midgen_v2_profile_has_import_story_true() {
        let p = supported_profile(classify_lunii(6, true, true, "id"));
        assert!(p.supported_operations.import_story);
    }

    #[test]
    fn default_supported_operations_are_all_false() {
        let ops = SupportedOperations::ALL_FALSE;
        assert!(!ops.read_library);
        assert!(!ops.inspect_story);
        assert!(!ops.import_story);
        assert!(!ops.write_story);
    }

    #[test]
    fn classify_lunii_v4_metadata_returns_metadata_unsupported_with_hint_v4() {
        let c = classify_lunii(4, true, true, "id");
        assert_eq!(
            unsupported_reason(&c),
            &UnsupportedReason::MetadataUnsupported
        );
        match c {
            DeviceProfileClassification::Unsupported { firmware_hint, .. } => {
                assert_eq!(firmware_hint.as_deref(), Some("metadata_v4"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn classify_lunii_v99_metadata_returns_metadata_unsupported_with_hint_v99() {
        let c = classify_lunii(99, true, true, "id");
        assert_eq!(
            unsupported_reason(&c),
            &UnsupportedReason::MetadataUnsupported
        );
        match c {
            DeviceProfileClassification::Unsupported { firmware_hint, .. } => {
                assert_eq!(firmware_hint.as_deref(), Some("metadata_v99"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn classify_lunii_missing_pi_returns_metadata_corrupt() {
        let c = classify_lunii(3, false, true, "id");
        assert_eq!(unsupported_reason(&c), &UnsupportedReason::MetadataCorrupt);
    }

    #[test]
    fn classify_lunii_accepts_missing_bt_as_supported() {
        // Real-world Lunii V3 (firmware 3.3.2) ships without `.bt`. The
        // marker is informational only — `.md` + `.pi` are the
        // universal gates.
        let p = supported_profile(classify_lunii(7, true, false, "id"));
        assert_eq!(p.firmware_cohort, LuniiFirmwareCohort::V3);
        assert!(p.supported_operations.read_library);
        assert!(!p.supported_operations.write_story);
    }

    #[test]
    fn classify_lunii_missing_pi_returns_metadata_corrupt_regardless_of_bt() {
        // Marker absence dominates : we report MetadataCorrupt without
        // suggesting another reason. The hint stays `None` because the
        // version byte is unverified when `.pi` is missing.
        let c = classify_lunii(3, false, true, "id");
        assert_eq!(unsupported_reason(&c), &UnsupportedReason::MetadataCorrupt);
        match c {
            DeviceProfileClassification::Unsupported { firmware_hint, .. } => {
                assert!(firmware_hint.is_none());
            }
            _ => unreachable!(),
        }
        // Same outcome when both `.pi` and `.bt` are missing — `.pi` is
        // the only gate that matters.
        let c2 = classify_lunii(3, false, false, "id");
        assert_eq!(unsupported_reason(&c2), &UnsupportedReason::MetadataCorrupt);
    }

    #[test]
    fn classify_lunii_missing_pi_takes_precedence_over_unsupported_metadata() {
        // A volume with `.md` v99 AND missing `.pi`: marker corruption
        // dominates the reason vector because the user must fix the
        // physical/permission issue before the metadata version even
        // becomes meaningful.
        let c = classify_lunii(99, false, true, "id");
        assert_eq!(unsupported_reason(&c), &UnsupportedReason::MetadataCorrupt);
    }

    #[test]
    fn unsupported_reason_round_trips_via_clone_and_eq() {
        let r = UnsupportedReason::FirmwareUnsupported;
        let s = r.clone();
        assert_eq!(r, s);
    }

    #[test]
    fn unsupported_reason_diagnostic_tags_are_stable() {
        assert_eq!(
            UnsupportedReason::FirmwareUnsupported.diagnostic_tag(),
            "firmware_unsupported"
        );
        assert_eq!(
            UnsupportedReason::MetadataUnsupported.diagnostic_tag(),
            "metadata_unsupported"
        );
        assert_eq!(
            UnsupportedReason::MetadataCorrupt.diagnostic_tag(),
            "metadata_corrupt"
        );
        assert_eq!(
            UnsupportedReason::FamilyUnknown.diagnostic_tag(),
            "family_unknown"
        );
        assert_eq!(
            UnsupportedReason::OperationNotAuthorized.diagnostic_tag(),
            "operation_not_authorized"
        );
        assert_eq!(
            UnsupportedReason::MultipleCandidates.diagnostic_tag(),
            "multiple_candidates"
        );
    }

    #[test]
    fn device_profile_carries_supplied_device_identifier_verbatim() {
        let p = supported_profile(classify_lunii(3, true, true, "OPAQUE_HASH_42"));
        assert_eq!(p.device_identifier, "OPAQUE_HASH_42");
    }

    #[test]
    fn device_profile_round_trips_via_clone_and_eq() {
        let p1 = supported_profile(classify_lunii(3, true, true, "id"));
        let p2 = p1.clone();
        assert_eq!(p1, p2);
    }
}
