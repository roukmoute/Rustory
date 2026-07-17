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
use crate::domain::import::{
    official_file_association_lines, FileAssociationChannel, FileAssociationLine,
    FileAssociationRegistration, LinuxInstallKind, LocalArtifactKind, LocalArtifactLine,
    LocalArtifactSupport,
};

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

/// The frozen extension label of the file-association section: the
/// single associated type, rendered verbatim. The packaging contract
/// test proves it stays coherent with `RUSTORY_ARTIFACT_EXTENSION` (the
/// domain constant, dot-less by the save-dialog convention).
pub const FILE_ASSOCIATION_EXTENSION_LABEL: &str = ".rustory";

/// The frozen user-facing label of a file-association channel
/// (`product-language.md`). Exhaustive match (tripwire).
pub fn file_association_channel_label(channel: FileAssociationChannel) -> &'static str {
    match channel {
        FileAssociationChannel::LinuxSystemPackage => "Paquet Linux (.deb / .rpm)",
        FileAssociationChannel::LinuxAppImage => "AppImage (Linux)",
        FileAssociationChannel::WindowsInstaller => "Installeur Windows (.msi / .exe)",
        FileAssociationChannel::MacosAppBundle => "Application macOS (.dmg)",
    }
}

/// The frozen user-facing status label of a registration state — the
/// calm chip wording (`product-language.md`: success on a registered
/// channel, neutral on a non-registered one — the durable-limit
/// regime, NEW literals). Exhaustive match (tripwire).
pub fn file_association_status_label(registration: FileAssociationRegistration) -> &'static str {
    match registration {
        FileAssociationRegistration::InstalledWithPackage => "Enregistrée à l'installation",
        FileAssociationRegistration::RegisteredBySystem => "Enregistrée par le système",
        FileAssociationRegistration::NotRegisteredByDefault { .. } => "Non enregistrée d'office",
    }
}

/// Stable camelCase wire tag of a Linux install kind (byte-identical
/// to the TS mirror's closed set). Exhaustive match (tripwire).
pub fn linux_install_kind_wire_tag(kind: LinuxInstallKind) -> &'static str {
    match kind {
        LinuxInstallKind::AppImage => "appImage",
        LinuxInstallKind::SystemPackage => "systemPackage",
        LinuxInstallKind::LocalBuild => "localBuild",
    }
}

/// The frozen current-install notice of each probed Linux install kind
/// (`product-language.md`) — rendered `role="status"` at the head of
/// the section, NEVER invented: an unprobed install serializes no
/// notice at all. Exhaustive match (tripwire).
pub fn linux_install_notice(kind: LinuxInstallKind) -> &'static str {
    match kind {
        LinuxInstallKind::AppImage => {
            "Ton installation actuelle est une AppImage : l'association n'est pas \
             enregistrée d'office."
        }
        LinuxInstallKind::SystemPackage => {
            "Ton installation actuelle provient d'un paquet système : l'association \
             est enregistrée."
        }
        LinuxInstallKind::LocalBuild => {
            "Cette version de Rustory n'a pas été installée par un paquet officiel : \
             elle n'enregistre pas d'association d'office."
        }
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

/// One serialized line of the file-association registry: the closed
/// wire tag, the frozen label, the registration flag with its frozen
/// status wording, the frozen detail — and, on a non-registered channel
/// only, the frozen reason CARRIED BY THE LINE (`reason` is OMITTED on
/// a registered one; the closed `FileAssociationRegistration` shape
/// guarantees the coherence).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileAssociationChannelDto {
    pub channel: &'static str,
    pub label: &'static str,
    pub registered: bool,
    pub status_label: &'static str,
    pub detail: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// The serialized verdict of the Linux install probe: the closed wire
/// tag and its frozen notice. Only ever present when the probe SPOKE
/// (Linux, determinable executable) — the wire never invents a claim.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentInstallDto {
    pub kind: &'static str,
    pub notice: &'static str,
}

impl CurrentInstallDto {
    /// Map a probed install kind to its wire face (tag + frozen
    /// notice) — the kind the probe decided is the single truth.
    pub fn from_kind(kind: LinuxInstallKind) -> Self {
        Self {
            kind: linux_install_kind_wire_tag(kind),
            notice: linux_install_notice(kind),
        }
    }
}

/// The serialized file-association block of the support profile: the
/// frozen extension label, every channel line of the received registry
/// in its stable order, and the current-install verdict (OMITTED when
/// no probe spoke — Windows/macOS, an indeterminable executable).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileAssociationDto {
    pub extension_label: &'static str,
    pub channels: Vec<FileAssociationChannelDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_install: Option<CurrentInstallDto>,
}

impl FileAssociationDto {
    /// Map a file-association registry (+ the probe's verdict) to its
    /// wire block — the received lines are the single truth: tags,
    /// labels, statuses and reasons all derive from them.
    pub fn from_registry(
        lines: &[FileAssociationLine],
        current_install: Option<LinuxInstallKind>,
    ) -> Self {
        Self {
            extension_label: FILE_ASSOCIATION_EXTENSION_LABEL,
            channels: lines
                .iter()
                .map(|line| FileAssociationChannelDto {
                    channel: line.channel.wire_tag(),
                    label: file_association_channel_label(line.channel),
                    registered: line.registration.is_registered(),
                    status_label: file_association_status_label(line.registration),
                    detail: line.detail,
                    // The reason travels ON the line: a non-registered
                    // channel always carries one (the closed
                    // FileAssociationRegistration shape guarantees it).
                    reason: line.registration.reason(),
                })
                .collect(),
            current_install: current_install.map(CurrentInstallDto::from_kind),
        }
    }
}

/// The serialized support profile: every line of the received device
/// matrix and artifact registry, in their stable order
/// (`read_support_profile` hands the official ones; tests may
/// serialize custom distributions), plus the file-association block
/// (the OFFICIAL registry — an ADDITIVE extension of the profile; the
/// Linux install probe attaches through [`Self::with_linux_install`]).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportProfileDto {
    pub devices: Vec<DeviceSupportLineDto>,
    pub local_artifacts: Vec<LocalArtifactLineDto>,
    pub file_association: FileAssociationDto,
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
            // The file-association block always carries the OFFICIAL
            // registry: the channel table is a distribution fact, not
            // a per-call input (custom registries serialize through
            // `FileAssociationDto::from_registry` directly). No probe
            // verdict here — the frontier attaches it explicitly.
            file_association: FileAssociationDto::from_registry(
                official_file_association_lines(),
                None,
            ),
        }
    }

    /// Attach the Linux install probe's verdict to the profile —
    /// `None` (no probe spoke: Windows/macOS, an indeterminable
    /// executable) leaves the notice ABSENT, never invented. Builder
    /// shape so the existing `from_matrices` call sites stay intact
    /// (the extension is additive).
    pub fn with_linux_install(mut self, kind: Option<LinuxInstallKind>) -> Self {
        self.file_association.current_install = kind.map(CurrentInstallDto::from_kind);
        self
    }
}
