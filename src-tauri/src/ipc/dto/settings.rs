//! Wire DTOs of the support profile (the `Profil de support` screen):
//! the device support matrix and the local-artifact registry with
//! their FROZEN labels and per-limit reasons
//! (`docs/architecture/product-language.md`). Every copy is
//! Rust-authoritative: the frontend renders these strings verbatim and
//! never recomposes them. The content sources are NOT duplicated here
//! — `read_content_source_policy` stays their single truth.

use serde::Serialize;

use crate::domain::device::{
    DeviceFamily, DeviceSupportLine, FirmwareCohort, FlamFirmwareCohort, LuniiFirmwareCohort,
    SupportedOperation,
};
use crate::domain::import::{LocalArtifactKind, LocalArtifactLine, LocalArtifactSupport};

/// The stable wire rendering order of the four operations of a device
/// matrix line — the same closed set as `SupportedOperations`, in the
/// documented column order.
const OPERATION_RENDER_ORDER: [SupportedOperation; 4] = [
    SupportedOperation::ReadLibrary,
    SupportedOperation::InspectStory,
    SupportedOperation::ImportStory,
    SupportedOperation::WriteStory,
];

/// Stable camelCase wire tag of a device family (byte-identical to the
/// `SupportedFamilyDto` wire values). Exhaustive match — adding a
/// family without deciding its tag is a compile error (the DTO
/// tripwire pattern).
pub fn device_family_wire_tag(family: DeviceFamily) -> &'static str {
    match family {
        DeviceFamily::Lunii => "lunii",
        DeviceFamily::Flam => "flam",
    }
}

/// The frozen user-facing label of a device family
/// (`product-language.md`). Exhaustive match (tripwire).
pub fn device_family_label(family: DeviceFamily) -> &'static str {
    match family {
        DeviceFamily::Lunii => "Lunii",
        DeviceFamily::Flam => "FLAM",
    }
}

/// Stable camelCase wire tag of a firmware cohort (byte-identical to
/// the `FirmwareCohortDto` wire values). Exhaustive match (tripwire).
pub fn firmware_cohort_wire_tag(cohort: FirmwareCohort) -> &'static str {
    match cohort {
        FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1) => "origineV1",
        FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2) => "midGenV2",
        FirmwareCohort::Lunii(LuniiFirmwareCohort::V3) => "v3",
        FirmwareCohort::Flam(FlamFirmwareCohort::Gen1) => "flamGen1",
    }
}

/// The frozen user-facing label of a firmware cohort, aligned with the
/// documented matrix (`product-language.md`). Exhaustive match
/// (tripwire).
pub fn firmware_cohort_label(cohort: FirmwareCohort) -> &'static str {
    match cohort {
        FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1) => "Origine v1",
        FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2) => "Mid-Gen v2",
        FirmwareCohort::Lunii(LuniiFirmwareCohort::V3) => "V3",
        FirmwareCohort::Flam(FlamFirmwareCohort::Gen1) => "Gen1",
    }
}

/// The frozen metadata-format line derived from the version the MATRIX
/// LINE carries — the registry is the single truth the screen serves,
/// never a parallel per-cohort table. `None` when the line documents
/// no version (FLAM): the key is OMITTED, never invented. A version
/// WITHOUT a frozen copy is omitted too (fail-closed: a label is never
/// composed at runtime, `&'static str` only), and the contract tests
/// prove every OFFICIAL documented version has its copy.
pub fn metadata_format_label(metadata_format_version: Option<u8>) -> Option<&'static str> {
    match metadata_format_version {
        Some(3) => Some("Format métadonnées v3"),
        Some(6) => Some("Format métadonnées v6"),
        Some(7) => Some("Format métadonnées v7"),
        None => None,
        // A documented version with no frozen copy: never invented —
        // adding the version to the official matrix requires deciding
        // its copy here (the exact-serialization contract trips).
        Some(_) => None,
    }
}

/// Stable camelCase wire tag of an operation (byte-identical to the
/// `SupportedOperationsDto` field names). Exhaustive match (tripwire).
pub fn operation_wire_tag(operation: SupportedOperation) -> &'static str {
    match operation {
        SupportedOperation::ReadLibrary => "readLibrary",
        SupportedOperation::InspectStory => "inspectStory",
        SupportedOperation::ImportStory => "importStory",
        SupportedOperation::WriteStory => "writeStory",
    }
}

/// The frozen user-facing label of an operation on a matrix line —
/// REUSED VERBATIM from the detection panel's capability lines (same
/// operations, same words); the write label bifurcates family-correctly
/// by construction (the family is KNOWN on every line — the
/// neutralize-vs-bifurcate rule). Exhaustive match (tripwire).
pub fn device_capability_label(
    family: DeviceFamily,
    operation: SupportedOperation,
) -> &'static str {
    match operation {
        SupportedOperation::ReadLibrary => "Lecture bibliothèque appareil",
        SupportedOperation::InspectStory => "Inspection d'histoire",
        SupportedOperation::ImportStory => "Copie dans la bibliothèque locale",
        SupportedOperation::WriteStory => match family {
            DeviceFamily::Lunii => "Transfert vers la Lunii",
            DeviceFamily::Flam => "Transfert vers l'appareil",
        },
    }
}

/// The frozen user-facing label of a local-artifact kind
/// (`product-language.md`). Exhaustive match (tripwire).
pub fn local_artifact_label(kind: LocalArtifactKind) -> &'static str {
    match kind {
        LocalArtifactKind::RustoryArtifact => "Artefact d'histoire Rustory (.rustory)",
        LocalArtifactKind::StructuredFolder => "Dossier structuré",
        LocalArtifactKind::StructuredArchive => "Archive structurée",
    }
}

/// The frozen format-version line derived from the version the
/// REGISTRY LINE carries (same single-truth discipline as
/// [`metadata_format_label`]) — `None` when the line documents none:
/// the key is OMITTED, never invented; a version without a frozen copy
/// is omitted too.
pub fn local_artifact_format_label(format_version: Option<u8>) -> Option<&'static str> {
    match format_version {
        Some(1) => Some("Format v1"),
        None => None,
        Some(_) => None,
    }
}

/// The frozen capability wording of each DOCUMENTED support bundle —
/// aligned word for word with the documented table; `None` on the
/// deferred state (the reason replaces it). Exhaustive match on the
/// closed bundle set (tripwire): a new bundle cannot ship without
/// deciding its wording.
pub fn local_artifact_capabilities_label(support: LocalArtifactSupport) -> Option<&'static str> {
    match support {
        LocalArtifactSupport::ImportAndExport => Some("Import et export"),
        LocalArtifactSupport::StoryCreation => Some("Création d'une histoire"),
        LocalArtifactSupport::Deferred { .. } => None,
    }
}

/// One serialized capability of a device matrix line: the closed wire
/// tag, the frozen label, the availability and — on a non-available
/// capability only — the frozen reason CARRIED BY THE LINE (`reason`
/// is OMITTED on an available one, and the TS guard refuses any
/// incoherence).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCapabilityDto {
    pub operation: &'static str,
    pub label: &'static str,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// One serialized line of the device support matrix: the closed wire
/// tags, the frozen labels, the frozen metadata-format line (omitted
/// for a family without one) and the four capability lines in the
/// documented column order.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSupportLineDto {
    pub family: &'static str,
    pub family_label: &'static str,
    pub cohort: &'static str,
    pub cohort_label: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_format_label: Option<&'static str>,
    pub capabilities: Vec<DeviceCapabilityDto>,
}

/// One serialized line of the local-artifact registry: the closed wire
/// tag, the frozen label, the frozen format line (omitted when the
/// table documents none), the availability and the coherent
/// capabilities/reason pair (the bundle wording on an available line,
/// the line-carried reason on a deferred one — never both, never
/// neither, guaranteed by the closed `LocalArtifactSupport` shape).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalArtifactLineDto {
    pub kind: &'static str,
    pub label: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_label: Option<&'static str>,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities_label: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// The serialized support profile: every line of the received device
/// matrix and artifact registry, in their stable order
/// (`read_support_profile` hands the official ones; tests may
/// serialize custom distributions).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportProfileDto {
    pub devices: Vec<DeviceSupportLineDto>,
    pub local_artifacts: Vec<LocalArtifactLineDto>,
}

impl SupportProfileDto {
    /// Map a device matrix + artifact registry to their wire profile
    /// (tags, frozen labels, line-carried availability and reasons —
    /// the received lines are the single truth: nothing is recomputed
    /// from a parallel table).
    pub fn from_matrices(
        devices: &[DeviceSupportLine],
        local_artifacts: &[LocalArtifactLine],
    ) -> Self {
        Self {
            devices: devices
                .iter()
                .map(|line| DeviceSupportLineDto {
                    family: device_family_wire_tag(line.family),
                    family_label: device_family_label(line.family),
                    cohort: firmware_cohort_wire_tag(line.cohort),
                    cohort_label: firmware_cohort_label(line.cohort),
                    metadata_format_label: metadata_format_label(line.metadata_format_version),
                    capabilities: OPERATION_RENDER_ORDER
                        .iter()
                        .map(|&operation| {
                            let support = line.support.support_for(operation);
                            DeviceCapabilityDto {
                                operation: operation_wire_tag(operation),
                                label: device_capability_label(line.family, operation),
                                available: support.is_available(),
                                // The reason travels ON the line: a
                                // closed cell always carries one (the
                                // OperationSupport shape guarantees it).
                                reason: support.reason(),
                            }
                        })
                        .collect(),
                })
                .collect(),
            local_artifacts: local_artifacts
                .iter()
                .map(|line| LocalArtifactLineDto {
                    kind: line.kind.wire_tag(),
                    label: local_artifact_label(line.kind),
                    format_label: local_artifact_format_label(line.format_version),
                    available: line.support.is_available(),
                    capabilities_label: local_artifact_capabilities_label(line.support),
                    reason: line.support.reason(),
                })
                .collect(),
        }
    }
}
