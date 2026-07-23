//! Official device support matrix: WHICH device families, firmware
//! cohorts and operations this distribution supports, decided line by
//! line — the enumerable single truth the per-family classifiers AND
//! the in-app `Profil de support` screen both consult (the exact
//! pattern of the content-source registry, `domain::import::content_source`,
//! which itself cloned the historical inline matrix of
//! `domain::device::profile`). Supporting an operation is a
//! DISTRIBUTION decision, never a user setting: no table, no
//! migration, no settings surface, no persistence. An alternative
//! distribution edits THIS matrix; the documented reference lives in
//! `docs/architecture/device-support-profile.md#MVP Phase 1 Matrix`.
//!
//! Pure domain: cohort in, operations out, zero I/O.

use super::family::{DeviceFamily, FirmwareCohort, FlamFirmwareCohort, LuniiFirmwareCohort};
use super::operations::{SupportedOperation, SupportedOperations};

/// Every known firmware cohort, in the stable rendering order of the
/// documented matrix. Tripwire: a new cohort variant fails the
/// exhaustive `match` in the tests below, forcing an explicit matrix
/// decision for it ("both or none, never one without the other" — the
/// `family.rs` invariant, outilled here).
pub const ALL_FIRMWARE_COHORTS: [FirmwareCohort; 4] = [
    FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1),
    FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2),
    FirmwareCohort::Lunii(LuniiFirmwareCohort::V3),
    FirmwareCohort::Flam(FlamFirmwareCohort::Gen1),
];

/// Support state of ONE operation on a matrix line: available, or NOT
/// available WITH its frozen user-facing reason. The couple
/// "not available without a reason" is unrepresentable by construction
/// — a limit can never reach the screen as a bare ✗.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationSupport {
    Available,
    NotAvailable { reason: &'static str },
}

impl OperationSupport {
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }

    /// The frozen reason of a non-available operation — `None` on an
    /// available one (the availability itself replaces it).
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::Available => None,
            Self::NotAvailable { reason } => Some(reason),
        }
    }
}

/// The per-operation support of a matrix line — one field per
/// operation of the closed [`SupportedOperations`] gate map, each
/// carrying its availability AND (when closed) its frozen reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceOperationsSupport {
    pub read_library: OperationSupport,
    pub inspect_story: OperationSupport,
    pub import_story: OperationSupport,
    pub write_story: OperationSupport,
    pub delete_story: OperationSupport,
}

impl DeviceOperationsSupport {
    /// Typed lookup of one operation's support on this line.
    pub const fn support_for(self, operation: SupportedOperation) -> OperationSupport {
        match operation {
            SupportedOperation::ReadLibrary => self.read_library,
            SupportedOperation::InspectStory => self.inspect_story,
            SupportedOperation::ImportStory => self.import_story,
            SupportedOperation::WriteStory => self.write_story,
            SupportedOperation::DeleteStory => self.delete_story,
        }
    }

    /// Derive the authorization map the capability gate consumes — the
    /// classifiers see EXACTLY the availability of this line, reasons
    /// stripped (the gate never needed them).
    pub const fn operations(self) -> SupportedOperations {
        SupportedOperations {
            read_library: self.read_library.is_available(),
            inspect_story: self.inspect_story.is_available(),
            import_story: self.import_story.is_available(),
            write_story: self.write_story.is_available(),
            delete_story: self.delete_story.is_available(),
        }
    }
}

/// One line of the official matrix: a known cohort, its family, the
/// documented metadata format version (`None` for a family whose
/// primary marker carries no documented version byte — no value is
/// ever invented) and the per-operation support the distribution
/// decides on it (availability + frozen reason on every closed cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceSupportLine {
    pub family: DeviceFamily,
    pub cohort: FirmwareCohort,
    pub metadata_format_version: Option<u8>,
    pub support: DeviceOperationsSupport,
}

/// The frozen reason of the Lunii V3 import/write cells — one copy for
/// both (same rationale as the documented matrix).
const V3_REVERSE_ENGINEERING_REASON: &str = "Rétro-ingénierie du format en cours";

/// The frozen reason of the FLAM Gen1 write cell.
const FLAM_WRITE_UNPROVEN_REASON: &str = "Écriture non prouvée sur matériel réel";

/// THE official device support matrix of this distribution — activated
/// line by line, never wholesale (every line carries its own
/// justification, mirroring `device-support-profile.md`).
const OFFICIAL_DEVICE_SUPPORT_MATRIX: &[DeviceSupportLine] = &[
    // Lunii Origine v1 (metadata v3) ✅✅✅✅ — the write gate accepts
    // the round-trip of an imported pack (opaque bytes already in
    // device format), the safest possible write: it reproduces what
    // the device held.
    DeviceSupportLine {
        family: DeviceFamily::Lunii,
        cohort: FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1),
        metadata_format_version: Some(3),
        support: DeviceOperationsSupport {
            read_library: OperationSupport::Available,
            inspect_story: OperationSupport::Available,
            import_story: OperationSupport::Available,
            write_story: OperationSupport::Available,
            delete_story: OperationSupport::Available,
        },
    },
    // Lunii Mid-Gen v2 (metadata v6) ✅✅✅✅ — same round-trip write
    // support as Origine v1.
    DeviceSupportLine {
        family: DeviceFamily::Lunii,
        cohort: FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2),
        metadata_format_version: Some(6),
        support: DeviceOperationsSupport {
            read_library: OperationSupport::Available,
            inspect_story: OperationSupport::Available,
            import_story: OperationSupport::Available,
            write_story: OperationSupport::Available,
            delete_story: OperationSupport::Available,
        },
    },
    // Lunii V3 (metadata v7) ✅✅❌❌ — reverse engineering is still
    // active in 2026; we cannot guarantee a corruption-free import.
    // Read is allowed (file enumeration), inspect is allowed
    // (read-only metadata peek), but import_story and write_story stay
    // closed — each cell carrying the frozen reason — until the V3
    // pipeline is verified end-to-end.
    //
    // NB: a V3 PACK-WRITE ENGINE exists and is HW-proven (cipher +
    // transcode + assemble + on-volume write), but it is a DISTINCT
    // operation from this `write_story` (round-trip of an imported pack).
    // Activating the archive-send belongs on its OWN capability so it never
    // enables the library round-trip "Envoyer" for V3 — see the deferred
    // `send_archive` capability, not this cell.
    DeviceSupportLine {
        family: DeviceFamily::Lunii,
        cohort: FirmwareCohort::Lunii(LuniiFirmwareCohort::V3),
        metadata_format_version: Some(7),
        support: DeviceOperationsSupport {
            read_library: OperationSupport::Available,
            inspect_story: OperationSupport::Available,
            import_story: OperationSupport::NotAvailable {
                reason: V3_REVERSE_ENGINEERING_REASON,
            },
            write_story: OperationSupport::NotAvailable {
                reason: V3_REVERSE_ENGINEERING_REASON,
            },
            delete_story: OperationSupport::Available,
        },
    },
    // FLAM Gen1 (no documented metadata version — none is invented)
    // ✅✅✅❌ — read-side capabilities are activated (library
    // inventory, story inspection, story import); write_story stays
    // closed with its frozen reason until the update flow proves a
    // device write end to end on real hardware (on-device format
    // decisions required, see the deferred-work ledger).
    DeviceSupportLine {
        family: DeviceFamily::Flam,
        cohort: FirmwareCohort::Flam(FlamFirmwareCohort::Gen1),
        metadata_format_version: None,
        support: DeviceOperationsSupport {
            read_library: OperationSupport::Available,
            inspect_story: OperationSupport::Available,
            import_story: OperationSupport::Available,
            write_story: OperationSupport::NotAvailable {
                reason: FLAM_WRITE_UNPROVEN_REASON,
            },
            // Deletion on a real FLAM is unproven (different mount/index
            // specifics than Lunii) — kept closed with the same frozen reason
            // until validated on hardware.
            delete_story: OperationSupport::NotAvailable {
                reason: FLAM_WRITE_UNPROVEN_REASON,
            },
        },
    },
];

/// The official matrix, as a borrowed slice: the classifiers consult
/// it through [`supported_operations_for`], the support-profile wire
/// serializes it line by line, and tests inject custom distributions
/// through [`supported_operations_in`] instead.
pub fn official_device_support_matrix() -> &'static [DeviceSupportLine] {
    OFFICIAL_DEVICE_SUPPORT_MATRIX
}

/// The PURE capability lookup on an arbitrary matrix: which operations
/// does `cohort` have in the given lines? A cohort ABSENT from the
/// matrix is fail-closed [`SupportedOperations::ALL_FALSE`] — never a
/// panic, never an authorization by default. The matrix travels as a
/// parameter so tests can prove the fail-closed refusal with custom
/// distributions (the content-source gate pattern).
pub fn supported_operations_in(
    matrix: &[DeviceSupportLine],
    cohort: FirmwareCohort,
) -> SupportedOperations {
    matrix
        .iter()
        .find(|line| line.cohort == cohort)
        .map(|line| line.support.operations())
        .unwrap_or(SupportedOperations::ALL_FALSE)
}

/// The official capability lookup consumed by the classifiers: the
/// operations the OFFICIAL matrix activates for `cohort`. Same
/// fail-closed contract as [`supported_operations_in`].
pub fn supported_operations_for(cohort: FirmwareCohort) -> SupportedOperations {
    supported_operations_in(official_device_support_matrix(), cohort)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_OPERATIONS: [SupportedOperation; 4] = [
        SupportedOperation::ReadLibrary,
        SupportedOperation::InspectStory,
        SupportedOperation::ImportStory,
        SupportedOperation::WriteStory,
    ];

    // ===== The official matrix — one line = one test, every cell
    // asserted (mirrors `device-support-profile.md#MVP Phase 1
    // Matrix`). =====

    #[test]
    fn official_matrix_lunii_origine_v1_line_is_fully_enabled() {
        let ops = supported_operations_for(FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1));
        assert!(ops.read_library);
        assert!(ops.inspect_story);
        assert!(ops.import_story);
        assert!(ops.write_story);
    }

    #[test]
    fn official_matrix_lunii_mid_gen_v2_line_is_fully_enabled() {
        let ops = supported_operations_for(FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2));
        assert!(ops.read_library);
        assert!(ops.inspect_story);
        assert!(ops.import_story);
        assert!(ops.write_story);
    }

    #[test]
    fn official_matrix_lunii_v3_line_blocks_import_and_write_with_the_frozen_reason() {
        let ops = supported_operations_for(FirmwareCohort::Lunii(LuniiFirmwareCohort::V3));
        assert!(ops.read_library);
        assert!(ops.inspect_story);
        assert!(!ops.import_story);
        assert!(!ops.write_story);
        let line = official_device_support_matrix()
            .iter()
            .find(|line| line.cohort == FirmwareCohort::Lunii(LuniiFirmwareCohort::V3))
            .expect("V3 line");
        assert_eq!(
            line.support.import_story.reason(),
            Some("Rétro-ingénierie du format en cours")
        );
        assert_eq!(
            line.support.write_story.reason(),
            Some("Rétro-ingénierie du format en cours")
        );
    }

    #[test]
    fn official_matrix_flam_gen1_line_blocks_write_only_with_the_frozen_reason() {
        let ops = supported_operations_for(FirmwareCohort::Flam(FlamFirmwareCohort::Gen1));
        assert!(ops.read_library);
        assert!(ops.inspect_story);
        assert!(ops.import_story);
        assert!(!ops.write_story);
        let line = official_device_support_matrix()
            .iter()
            .find(|line| line.cohort == FirmwareCohort::Flam(FlamFirmwareCohort::Gen1))
            .expect("FLAM line");
        assert_eq!(
            line.support.write_story.reason(),
            Some("Écriture non prouvée sur matériel réel")
        );
    }

    #[test]
    fn every_closed_cell_of_the_official_matrix_carries_a_non_empty_reason() {
        // The OperationSupport shape makes "not available without a
        // reason" unrepresentable; this asserts the official copies are
        // also non-empty (a limit never reaches the screen as a bare ✗).
        for line in official_device_support_matrix() {
            for operation in ALL_OPERATIONS {
                let support = line.support.support_for(operation);
                if !support.is_available() {
                    let reason = support.reason().expect("closed cell carries a reason");
                    assert!(
                        !reason.is_empty(),
                        "cohort {:?} operation {:?}: empty reason",
                        line.cohort,
                        operation
                    );
                }
            }
        }
    }

    // ===== Line metadata — family and documented format version stay
    // coherent with the cohort (the wire renders these facts). =====

    #[test]
    fn official_matrix_lines_carry_their_family_and_documented_format_version() {
        for line in official_device_support_matrix() {
            match line.cohort {
                FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1) => {
                    assert_eq!(line.family, DeviceFamily::Lunii);
                    assert_eq!(line.metadata_format_version, Some(3));
                }
                FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2) => {
                    assert_eq!(line.family, DeviceFamily::Lunii);
                    assert_eq!(line.metadata_format_version, Some(6));
                }
                FirmwareCohort::Lunii(LuniiFirmwareCohort::V3) => {
                    assert_eq!(line.family, DeviceFamily::Lunii);
                    assert_eq!(line.metadata_format_version, Some(7));
                }
                FirmwareCohort::Flam(FlamFirmwareCohort::Gen1) => {
                    assert_eq!(line.family, DeviceFamily::Flam);
                    // No version byte is ever invented for `.mdf`.
                    assert_eq!(line.metadata_format_version, None);
                }
            }
        }
    }

    #[test]
    fn official_matrix_preserves_the_documented_rendering_order() {
        let cohorts: Vec<FirmwareCohort> = official_device_support_matrix()
            .iter()
            .map(|line| line.cohort)
            .collect();
        assert_eq!(cohorts, ALL_FIRMWARE_COHORTS.to_vec());
    }

    // ===== Exhaustiveness tripwires ("both or none") =====

    #[test]
    fn official_matrix_carries_every_known_cohort_exactly_once() {
        for cohort in ALL_FIRMWARE_COHORTS {
            let lines = official_device_support_matrix()
                .iter()
                .filter(|line| line.cohort == cohort)
                .count();
            assert_eq!(lines, 1, "cohort {cohort:?} must have exactly one line");
        }
        assert_eq!(
            official_device_support_matrix().len(),
            ALL_FIRMWARE_COHORTS.len(),
            "no line may carry an unknown cohort"
        );
    }

    #[test]
    fn all_firmware_cohorts_tripwire_is_exhaustive_and_distinct() {
        // Compile-time tripwire: adding a cohort variant breaks this
        // exhaustive match, forcing ALL_FIRMWARE_COHORTS (and the
        // official matrix, through the exactly-once test above) to
        // absorb the newcomer explicitly.
        for cohort in ALL_FIRMWARE_COHORTS {
            match cohort {
                FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1)
                | FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2)
                | FirmwareCohort::Lunii(LuniiFirmwareCohort::V3)
                | FirmwareCohort::Flam(FlamFirmwareCohort::Gen1) => {}
            }
        }
        // 4 pairwise-distinct values of a 4-shape sum = every shape is
        // present in the tripwire array.
        for (i, a) in ALL_FIRMWARE_COHORTS.iter().enumerate() {
            for b in &ALL_FIRMWARE_COHORTS[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    // ===== The support ↔ gate derivation =====

    #[test]
    fn operations_derivation_maps_availability_and_strips_reasons() {
        let support = DeviceOperationsSupport {
            read_library: OperationSupport::Available,
            inspect_story: OperationSupport::NotAvailable { reason: "why" },
            import_story: OperationSupport::Available,
            write_story: OperationSupport::NotAvailable { reason: "why" },
            delete_story: OperationSupport::Available,
        };
        let ops = support.operations();
        assert!(ops.read_library);
        assert!(!ops.inspect_story);
        assert!(ops.import_story);
        assert!(!ops.write_story);
        assert!(ops.delete_story);
    }

    #[test]
    fn support_for_returns_each_operation_cell() {
        let support = DeviceOperationsSupport {
            read_library: OperationSupport::Available,
            inspect_story: OperationSupport::Available,
            import_story: OperationSupport::NotAvailable { reason: "closed" },
            write_story: OperationSupport::Available,
            delete_story: OperationSupport::Available,
        };
        assert!(support
            .support_for(SupportedOperation::DeleteStory)
            .is_available());
        assert!(support
            .support_for(SupportedOperation::ReadLibrary)
            .is_available());
        assert_eq!(
            support
                .support_for(SupportedOperation::ImportStory)
                .reason(),
            Some("closed")
        );
        assert!(support
            .support_for(SupportedOperation::WriteStory)
            .is_available());
    }

    // ===== The lookup =====

    #[test]
    fn lookup_returns_the_line_operations_for_a_known_cohort() {
        let matrix = [DeviceSupportLine {
            family: DeviceFamily::Lunii,
            cohort: FirmwareCohort::Lunii(LuniiFirmwareCohort::V3),
            metadata_format_version: Some(7),
            support: DeviceOperationsSupport {
                read_library: OperationSupport::Available,
                inspect_story: OperationSupport::NotAvailable { reason: "custom" },
                import_story: OperationSupport::NotAvailable { reason: "custom" },
                write_story: OperationSupport::NotAvailable { reason: "custom" },
                delete_story: OperationSupport::NotAvailable { reason: "custom" },
            },
        }];
        let ops = supported_operations_in(&matrix, FirmwareCohort::Lunii(LuniiFirmwareCohort::V3));
        assert!(ops.read_library);
        assert!(!ops.inspect_story);
    }

    #[test]
    fn lookup_fails_closed_on_a_cohort_absent_from_the_matrix() {
        // An empty matrix (or a partial custom one) never panics and
        // never authorizes by default.
        assert_eq!(
            supported_operations_in(&[], FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1)),
            SupportedOperations::ALL_FALSE
        );
        let flam_only = [DeviceSupportLine {
            family: DeviceFamily::Flam,
            cohort: FirmwareCohort::Flam(FlamFirmwareCohort::Gen1),
            metadata_format_version: None,
            support: DeviceOperationsSupport {
                read_library: OperationSupport::Available,
                inspect_story: OperationSupport::Available,
                import_story: OperationSupport::Available,
                write_story: OperationSupport::NotAvailable { reason: "unproven" },
                delete_story: OperationSupport::NotAvailable { reason: "unproven" },
            },
        }];
        assert_eq!(
            supported_operations_in(&flam_only, FirmwareCohort::Lunii(LuniiFirmwareCohort::V3)),
            SupportedOperations::ALL_FALSE
        );
    }

    // ===== Classifier ↔ matrix equality — the mechanical proof that
    // making the matrix enumerable changed NO gate: the profile
    // produced by each classifier carries exactly the operations of
    // its matrix line. =====

    #[test]
    fn classifier_and_matrix_agree_for_every_lunii_cohort() {
        for (metadata_version, cohort) in [
            (3u8, LuniiFirmwareCohort::OrigineV1),
            (6, LuniiFirmwareCohort::MidGenV2),
            (7, LuniiFirmwareCohort::V3),
        ] {
            let classified = super::super::profile::classify_lunii(
                metadata_version,
                true,
                true,
                "equality-check",
            );
            let profile = match classified {
                super::super::profile::DeviceProfileClassification::Supported(p) => p,
                other => panic!("expected Supported, got {other:?}"),
            };
            assert_eq!(
                profile.supported_operations,
                supported_operations_for(FirmwareCohort::Lunii(cohort)),
                "classifier and matrix diverged for Lunii cohort {cohort:?}"
            );
        }
    }

    #[test]
    fn classifier_and_matrix_agree_for_the_flam_cohort() {
        let classified = super::super::profile::classify_flam(b"MDF", true, true, "equality-check");
        let profile = match classified {
            super::super::profile::DeviceProfileClassification::Supported(p) => p,
            other => panic!("expected Supported, got {other:?}"),
        };
        assert_eq!(
            profile.supported_operations,
            supported_operations_for(FirmwareCohort::Flam(FlamFirmwareCohort::Gen1)),
            "classifier and matrix diverged for FLAM Gen1"
        );
    }
}
