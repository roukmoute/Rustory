//! Wire contracts of the support profile (the `Profil de support`
//! screen): the frozen labels and per-limit reasons (byte-for-byte),
//! the profile DTO shape — and the EXACT serialization of the CURRENT
//! official distribution (one assertion per matrix line, like the
//! content-source policy).

use rustory_lib::domain::device::{
    official_device_support_matrix, DeviceFamily, DeviceOperationsSupport, DeviceSupportLine,
    FirmwareCohort, FlamFirmwareCohort, LuniiFirmwareCohort, OperationSupport, SupportedOperation,
    ALL_FIRMWARE_COHORTS,
};
use rustory_lib::domain::export::RUSTORY_ARTIFACT_EXTENSION;
use rustory_lib::domain::import::{
    official_file_association_lines, official_local_artifacts, FileAssociationChannel,
    FileAssociationRegistration, LinuxInstallKind, LocalArtifactKind, LocalArtifactLine,
    LocalArtifactSupport, ALL_FILE_ASSOCIATION_CHANNELS, ALL_LOCAL_ARTIFACT_KINDS,
};
use rustory_lib::ipc::dto::settings::{
    device_capability_label, device_family_label, device_family_wire_tag,
    file_association_channel_label, file_association_status_label, firmware_cohort_label,
    firmware_cohort_wire_tag, linux_install_kind_wire_tag, linux_install_notice,
    local_artifact_capabilities_label, local_artifact_format_label, local_artifact_label,
    metadata_format_label, operation_wire_tag, SupportProfileDto, FILE_ASSOCIATION_EXTENSION_LABEL,
};

const ALL_OPERATIONS: [SupportedOperation; 4] = [
    SupportedOperation::ReadLibrary,
    SupportedOperation::InspectStory,
    SupportedOperation::ImportStory,
    SupportedOperation::WriteStory,
];

// ===== Frozen copies (product-language.md — byte-for-byte) =====

#[test]
fn family_labels_are_frozen() {
    assert_eq!(device_family_label(DeviceFamily::Lunii), "Lunii");
    assert_eq!(device_family_label(DeviceFamily::Flam), "FLAM");
}

#[test]
fn cohort_labels_are_frozen() {
    assert_eq!(
        firmware_cohort_label(FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1)),
        "Origine v1"
    );
    assert_eq!(
        firmware_cohort_label(FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2)),
        "Mid-Gen v2"
    );
    assert_eq!(
        firmware_cohort_label(FirmwareCohort::Lunii(LuniiFirmwareCohort::V3)),
        "V3"
    );
    assert_eq!(
        firmware_cohort_label(FirmwareCohort::Flam(FlamFirmwareCohort::Gen1)),
        "Gen1"
    );
}

#[test]
fn metadata_format_labels_are_frozen_per_documented_version_and_never_invented() {
    // The label derives from the VERSION THE LINE CARRIES (the single
    // truth), never from a parallel per-cohort table.
    assert_eq!(
        metadata_format_label(Some(3)),
        Some("Format métadonnées v3")
    );
    assert_eq!(
        metadata_format_label(Some(6)),
        Some("Format métadonnées v6")
    );
    assert_eq!(
        metadata_format_label(Some(7)),
        Some("Format métadonnées v7")
    );
    // No documented version → the key is omitted, never invented.
    assert_eq!(metadata_format_label(None), None);
    // A version WITHOUT a frozen copy is omitted too — a label is
    // never composed at runtime.
    assert_eq!(metadata_format_label(Some(99)), None);
}

#[test]
fn every_official_documented_version_has_its_frozen_label() {
    // Tripwire for the omit-on-unknown fallback: adding a version to
    // the OFFICIAL matrix without deciding its copy fails here (the
    // documented line would silently lose its format line otherwise).
    for line in official_device_support_matrix() {
        assert_eq!(
            metadata_format_label(line.metadata_format_version).is_some(),
            line.metadata_format_version.is_some(),
            "cohort {:?}: a documented version must carry a frozen label",
            line.cohort
        );
    }
    for line in official_local_artifacts() {
        assert_eq!(
            local_artifact_format_label(line.format_version).is_some(),
            line.format_version.is_some(),
            "kind {:?}: a documented version must carry a frozen label",
            line.kind
        );
    }
}

#[test]
fn device_capability_labels_are_frozen_and_reuse_the_panel_wording() {
    // The three family-invariant labels.
    for family in [DeviceFamily::Lunii, DeviceFamily::Flam] {
        assert_eq!(
            device_capability_label(family, SupportedOperation::ReadLibrary),
            "Lecture bibliothèque appareil"
        );
        assert_eq!(
            device_capability_label(family, SupportedOperation::InspectStory),
            "Inspection d'histoire"
        );
        assert_eq!(
            device_capability_label(family, SupportedOperation::ImportStory),
            "Copie dans la bibliothèque locale"
        );
    }
    // The write label bifurcates family-correctly by construction.
    assert_eq!(
        device_capability_label(DeviceFamily::Lunii, SupportedOperation::WriteStory),
        "Transfert vers la Lunii"
    );
    assert_eq!(
        device_capability_label(DeviceFamily::Flam, SupportedOperation::WriteStory),
        "Transfert vers l'appareil"
    );
}

#[test]
fn device_limit_reasons_are_frozen_on_the_official_lines() {
    // The reasons live ON the matrix lines (the closed OperationSupport
    // shape) — asserted byte-for-byte on the official distribution.
    let v3 = official_device_support_matrix()
        .iter()
        .find(|line| line.cohort == FirmwareCohort::Lunii(LuniiFirmwareCohort::V3))
        .expect("V3 line");
    assert_eq!(
        v3.support.import_story.reason(),
        Some("Rétro-ingénierie du format en cours")
    );
    assert_eq!(
        v3.support.write_story.reason(),
        Some("Rétro-ingénierie du format en cours")
    );
    let flam = official_device_support_matrix()
        .iter()
        .find(|line| line.cohort == FirmwareCohort::Flam(FlamFirmwareCohort::Gen1))
        .expect("FLAM line");
    assert_eq!(
        flam.support.write_story.reason(),
        Some("Écriture non prouvée sur matériel réel")
    );
}

#[test]
fn local_artifact_labels_are_frozen() {
    assert_eq!(
        local_artifact_label(LocalArtifactKind::RustoryArtifact),
        "Artefact d'histoire Rustory (.rustory)"
    );
    assert_eq!(
        local_artifact_label(LocalArtifactKind::StructuredFolder),
        "Dossier structuré"
    );
    assert_eq!(
        local_artifact_label(LocalArtifactKind::StructuredArchive),
        "Archive structurée"
    );
}

#[test]
fn local_artifact_capability_and_format_copies_are_frozen() {
    // The bundle wording derives from the SUPPORT THE LINE CARRIES —
    // one frozen copy per documented bundle, none for the deferral.
    assert_eq!(
        local_artifact_capabilities_label(LocalArtifactSupport::ImportAndExport),
        Some("Import et export")
    );
    assert_eq!(
        local_artifact_capabilities_label(LocalArtifactSupport::StoryCreation),
        Some("Création d'une histoire")
    );
    assert_eq!(
        local_artifact_capabilities_label(LocalArtifactSupport::Deferred { reason: "why" }),
        None
    );
    // The format label derives from the VERSION THE LINE CARRIES.
    assert_eq!(local_artifact_format_label(Some(1)), Some("Format v1"));
    assert_eq!(local_artifact_format_label(None), None);
    assert_eq!(local_artifact_format_label(Some(9)), None);
}

// ===== The CURRENT official profile, serialized EXACTLY (one
// assertion per matrix line) =====

#[test]
fn the_official_device_matrix_serializes_exactly() {
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v["devices"][0],
        serde_json::json!({
            "family": "lunii",
            "familyLabel": "Lunii",
            "cohort": "origineV1",
            "cohortLabel": "Origine v1",
            "metadataFormatLabel": "Format métadonnées v3",
            "capabilities": [
                { "operation": "readLibrary", "label": "Lecture bibliothèque appareil", "available": true },
                { "operation": "inspectStory", "label": "Inspection d'histoire", "available": true },
                { "operation": "importStory", "label": "Copie dans la bibliothèque locale", "available": true },
                { "operation": "writeStory", "label": "Transfert vers la Lunii", "available": true },
            ],
        })
    );
    assert_eq!(
        v["devices"][1],
        serde_json::json!({
            "family": "lunii",
            "familyLabel": "Lunii",
            "cohort": "midGenV2",
            "cohortLabel": "Mid-Gen v2",
            "metadataFormatLabel": "Format métadonnées v6",
            "capabilities": [
                { "operation": "readLibrary", "label": "Lecture bibliothèque appareil", "available": true },
                { "operation": "inspectStory", "label": "Inspection d'histoire", "available": true },
                { "operation": "importStory", "label": "Copie dans la bibliothèque locale", "available": true },
                { "operation": "writeStory", "label": "Transfert vers la Lunii", "available": true },
            ],
        })
    );
    assert_eq!(
        v["devices"][2],
        serde_json::json!({
            "family": "lunii",
            "familyLabel": "Lunii",
            "cohort": "v3",
            "cohortLabel": "V3",
            "metadataFormatLabel": "Format métadonnées v7",
            "capabilities": [
                { "operation": "readLibrary", "label": "Lecture bibliothèque appareil", "available": true },
                { "operation": "inspectStory", "label": "Inspection d'histoire", "available": true },
                { "operation": "importStory", "label": "Copie dans la bibliothèque locale", "available": false, "reason": "Rétro-ingénierie du format en cours" },
                { "operation": "writeStory", "label": "Transfert vers la Lunii", "available": false, "reason": "Rétro-ingénierie du format en cours" },
            ],
        })
    );
    assert_eq!(
        v["devices"][3],
        serde_json::json!({
            "family": "flam",
            "familyLabel": "FLAM",
            "cohort": "flamGen1",
            "cohortLabel": "Gen1",
            "capabilities": [
                { "operation": "readLibrary", "label": "Lecture bibliothèque appareil", "available": true },
                { "operation": "inspectStory", "label": "Inspection d'histoire", "available": true },
                { "operation": "importStory", "label": "Copie dans la bibliothèque locale", "available": true },
                { "operation": "writeStory", "label": "Transfert vers l'appareil", "available": false, "reason": "Écriture non prouvée sur matériel réel" },
            ],
        })
    );
    assert_eq!(v["devices"].as_array().expect("devices").len(), 4);
}

#[test]
fn the_official_artifact_registry_serializes_exactly() {
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v["localArtifacts"],
        serde_json::json!([
            {
                "kind": "rustoryArtifact",
                "label": "Artefact d'histoire Rustory (.rustory)",
                "formatLabel": "Format v1",
                "available": true,
                "capabilitiesLabel": "Import et export",
            },
            {
                "kind": "structuredFolder",
                "label": "Dossier structuré",
                "formatLabel": "Format v1",
                "available": true,
                "capabilitiesLabel": "Création d'une histoire",
            },
            {
                "kind": "structuredArchive",
                "label": "Archive structurée",
                "formatLabel": "Format v1",
                "available": true,
                "capabilitiesLabel": "Création d'une histoire",
            },
        ])
    );
}

// ===== The registry lines PILOT the DTO (single-truth proofs) =====

#[test]
fn the_dto_format_labels_follow_the_received_line_not_the_cohort_or_kind() {
    // A custom line carrying a DIFFERENT documented version proves the
    // DTO reads the line — evolving the registry updates the screen.
    let custom_devices = [DeviceSupportLine {
        family: DeviceFamily::Lunii,
        cohort: FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1),
        // Origine v1 officially documents v3 — hand it v6 instead.
        metadata_format_version: Some(6),
        support: DeviceOperationsSupport {
            read_library: OperationSupport::Available,
            inspect_story: OperationSupport::Available,
            import_story: OperationSupport::Available,
            write_story: OperationSupport::Available,
        },
    }];
    let custom_artifacts = [LocalArtifactLine {
        kind: LocalArtifactKind::StructuredArchive,
        // The archive officially documents NO version — hand it v1.
        format_version: Some(1),
        support: LocalArtifactSupport::Deferred {
            reason: "Lecture d'archives non prise en charge",
        },
    }];
    let dto = SupportProfileDto::from_matrices(&custom_devices, &custom_artifacts);
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v["devices"][0]["metadataFormatLabel"], "Format métadonnées v6",
        "the label must follow the LINE version, not the cohort"
    );
    assert_eq!(
        v["localArtifacts"][0]["formatLabel"], "Format v1",
        "the label must follow the LINE version, not the kind"
    );
}

#[test]
fn a_line_version_without_a_frozen_copy_omits_the_format_key_never_invents_one() {
    let custom_devices = [DeviceSupportLine {
        family: DeviceFamily::Lunii,
        cohort: FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1),
        metadata_format_version: Some(99),
        support: DeviceOperationsSupport {
            read_library: OperationSupport::Available,
            inspect_story: OperationSupport::Available,
            import_story: OperationSupport::Available,
            write_story: OperationSupport::Available,
        },
    }];
    let dto = SupportProfileDto::from_matrices(&custom_devices, &[]);
    let v = serde_json::to_value(&dto).expect("ser");
    assert!(
        v["devices"][0]
            .as_object()
            .expect("object")
            .get("metadataFormatLabel")
            .is_none(),
        "a version without a frozen copy is omitted — never composed at runtime"
    );
}

#[test]
fn every_closed_cell_serializes_a_non_empty_reason_even_on_a_custom_distribution() {
    // The OperationSupport shape carries the reason ON the line, so a
    // custom distribution closing ANY cell serializes an honest reason
    // — `available: false` without a reason is unrepresentable.
    let custom = [DeviceSupportLine {
        family: DeviceFamily::Lunii,
        cohort: FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1),
        metadata_format_version: Some(3),
        support: DeviceOperationsSupport {
            read_library: OperationSupport::NotAvailable {
                reason: "Distribution personnalisée",
            },
            inspect_story: OperationSupport::NotAvailable {
                reason: "Distribution personnalisée",
            },
            import_story: OperationSupport::NotAvailable {
                reason: "Distribution personnalisée",
            },
            write_story: OperationSupport::NotAvailable {
                reason: "Distribution personnalisée",
            },
        },
    }];
    let dto = SupportProfileDto::from_matrices(&custom, &[]);
    let v = serde_json::to_value(&dto).expect("ser");
    for capability in v["devices"][0]["capabilities"].as_array().expect("caps") {
        assert_eq!(capability["available"], false);
        assert_eq!(capability["reason"], "Distribution personnalisée");
    }
}

#[test]
fn every_closed_official_cell_and_deferred_artifact_serializes_a_non_empty_reason() {
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    for device in v["devices"].as_array().expect("devices") {
        for capability in device["capabilities"].as_array().expect("caps") {
            if capability["available"] == false {
                let reason = capability["reason"].as_str().expect("closed cell reason");
                assert!(!reason.is_empty(), "a limit never renders as a bare ✗");
            } else {
                assert!(
                    capability.get("reason").is_none(),
                    "an available capability carries NO reason key"
                );
            }
        }
    }
    for artifact in v["localArtifacts"].as_array().expect("artifacts") {
        if artifact["available"] == false {
            let reason = artifact["reason"].as_str().expect("deferred reason");
            assert!(!reason.is_empty());
            assert!(artifact.get("capabilitiesLabel").is_none());
        } else {
            assert!(artifact.get("reason").is_none());
            assert!(
                artifact["capabilitiesLabel"].as_str().is_some(),
                "an available line always names its documented bundle"
            );
        }
    }
}

#[test]
fn artifact_bundles_serialize_their_own_wording_never_another_bundle() {
    // The support bundles are a CLOSED sum (a partial combination is
    // unrepresentable in domain); each serializes EXACTLY its wording.
    let lines = [
        LocalArtifactLine {
            kind: LocalArtifactKind::RustoryArtifact,
            format_version: Some(1),
            support: LocalArtifactSupport::StoryCreation,
        },
        LocalArtifactLine {
            kind: LocalArtifactKind::StructuredFolder,
            format_version: Some(1),
            support: LocalArtifactSupport::ImportAndExport,
        },
    ];
    let dto = SupportProfileDto::from_matrices(&[], &lines);
    let v = serde_json::to_value(&dto).expect("ser");
    // A custom distribution swapping the bundles swaps the wordings
    // with them — the wording follows the LINE support, never the kind.
    assert_eq!(
        v["localArtifacts"][0]["capabilitiesLabel"],
        "Création d'une histoire"
    );
    assert_eq!(
        v["localArtifacts"][1]["capabilitiesLabel"],
        "Import et export"
    );
}

// ===== Optional-key omission discipline =====

#[test]
fn a_fully_available_device_line_omits_every_reason_key() {
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    for capability in v["devices"][0]["capabilities"].as_array().expect("caps") {
        assert!(
            capability.get("reason").is_none(),
            "an available capability carries NO reason key (the chip replaces it)"
        );
    }
}

#[test]
fn the_flam_line_omits_the_metadata_format_key_entirely() {
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert!(
        v["devices"][3]
            .as_object()
            .expect("object")
            .get("metadataFormatLabel")
            .is_none(),
        "metadataFormatLabel must be omitted for FLAM — never null, never invented"
    );
}

#[test]
fn an_available_artifact_line_omits_the_reason_and_a_deferred_one_omits_the_capabilities() {
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    let rustory = v["localArtifacts"][0].as_object().expect("object");
    assert!(rustory.get("reason").is_none());
    assert!(rustory.get("capabilitiesLabel").is_some());
    // Since the archive reader shipped, no OFFICIAL line is deferred
    // anymore: prove the shape rule on a hand-built deferred line — the
    // wire face of a future deferred kind stays locked.
    let deferred = [LocalArtifactLine {
        kind: LocalArtifactKind::StructuredArchive,
        format_version: None,
        support: LocalArtifactSupport::Deferred {
            reason: "Lecture d'archives non prise en charge",
        },
    }];
    let dto = SupportProfileDto::from_matrices(official_device_support_matrix(), &deferred);
    let v = serde_json::to_value(&dto).expect("ser");
    let archive = v["localArtifacts"][0].as_object().expect("object");
    assert!(archive.get("capabilitiesLabel").is_none());
    assert!(archive.get("formatLabel").is_none());
    assert!(archive.get("reason").is_some());
}

// ===== Exhaustiveness (tripwire round-trip: every domain value has
// its wire face, and the reason is coherent with the availability) =====

#[test]
fn every_cohort_serializes_a_tag_a_label_and_a_family_face() {
    for cohort in ALL_FIRMWARE_COHORTS {
        assert!(!firmware_cohort_wire_tag(cohort).is_empty());
        assert!(!firmware_cohort_label(cohort).is_empty());
    }
    for family in [DeviceFamily::Lunii, DeviceFamily::Flam] {
        assert!(!device_family_wire_tag(family).is_empty());
        assert!(!device_family_label(family).is_empty());
        for operation in ALL_OPERATIONS {
            assert!(!device_capability_label(family, operation).is_empty());
        }
    }
    for operation in ALL_OPERATIONS {
        assert!(!operation_wire_tag(operation).is_empty());
    }
}

#[test]
fn every_official_capability_reason_is_coherent_with_its_availability() {
    // Reason present IFF the official line does NOT activate the
    // operation — carried by the line itself (the OperationSupport
    // shape), so the wire can never render a bare ✗ nor justify an
    // available capability.
    for line in official_device_support_matrix() {
        for operation in ALL_OPERATIONS {
            let support = line.support.support_for(operation);
            assert_eq!(
                support.reason().is_none(),
                support.is_available(),
                "cohort {:?} operation {:?}: reason present IFF not available",
                line.cohort,
                operation
            );
        }
    }
}

#[test]
fn every_artifact_kind_serializes_a_tag_a_label_and_a_coherent_pair() {
    for kind in ALL_LOCAL_ARTIFACT_KINDS {
        assert!(!kind.wire_tag().is_empty());
        assert!(!local_artifact_label(kind).is_empty());
    }
    // Bundle wording present IFF the official line offers a bundle;
    // reason present IFF it is deferred — both carried by the line.
    for line in official_local_artifacts() {
        let offers = line.support.is_available();
        assert_eq!(
            local_artifact_capabilities_label(line.support).is_some(),
            offers,
            "kind {:?}: bundle wording present IFF available",
            line.kind
        );
        assert_eq!(
            line.support.reason().is_none(),
            offers,
            "kind {:?}: reason present IFF deferred",
            line.kind
        );
    }
}

#[test]
fn wire_tags_stay_byte_identical_to_the_existing_device_wire() {
    // The settings wire reuses the EXACT tags the detection wire
    // already serializes (`SupportedFamilyDto` / `FirmwareCohortDto` /
    // `SupportedOperationsDto` field names) — one wire vocabulary per
    // fact, never two.
    assert_eq!(device_family_wire_tag(DeviceFamily::Lunii), "lunii");
    assert_eq!(device_family_wire_tag(DeviceFamily::Flam), "flam");
    assert_eq!(
        firmware_cohort_wire_tag(FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1)),
        "origineV1"
    );
    assert_eq!(
        firmware_cohort_wire_tag(FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2)),
        "midGenV2"
    );
    assert_eq!(
        firmware_cohort_wire_tag(FirmwareCohort::Lunii(LuniiFirmwareCohort::V3)),
        "v3"
    );
    assert_eq!(
        firmware_cohort_wire_tag(FirmwareCohort::Flam(FlamFirmwareCohort::Gen1)),
        "flamGen1"
    );
    assert_eq!(
        operation_wire_tag(SupportedOperation::ReadLibrary),
        "readLibrary"
    );
    assert_eq!(
        operation_wire_tag(SupportedOperation::InspectStory),
        "inspectStory"
    );
    assert_eq!(
        operation_wire_tag(SupportedOperation::ImportStory),
        "importStory"
    );
    assert_eq!(
        operation_wire_tag(SupportedOperation::WriteStory),
        "writeStory"
    );
}

// ===== File association — frozen copies (product-language.md,
// byte-for-byte) =====

#[test]
fn file_association_channel_labels_are_frozen() {
    assert_eq!(
        file_association_channel_label(FileAssociationChannel::LinuxSystemPackage),
        "Paquet Linux (.deb / .rpm)"
    );
    assert_eq!(
        file_association_channel_label(FileAssociationChannel::LinuxAppImage),
        "AppImage (Linux)"
    );
    assert_eq!(
        file_association_channel_label(FileAssociationChannel::WindowsInstaller),
        "Installeur Windows (.msi / .exe)"
    );
    assert_eq!(
        file_association_channel_label(FileAssociationChannel::MacosAppBundle),
        "Application macOS (.dmg)"
    );
}

#[test]
fn file_association_status_labels_are_frozen() {
    assert_eq!(
        file_association_status_label(FileAssociationRegistration::InstalledWithPackage),
        "Enregistrée à l'installation"
    );
    assert_eq!(
        file_association_status_label(FileAssociationRegistration::RegisteredBySystem),
        "Enregistrée par le système"
    );
    assert_eq!(
        file_association_status_label(FileAssociationRegistration::NotRegisteredByDefault {
            reason: "why"
        }),
        "Non enregistrée d'office"
    );
}

#[test]
fn the_appimage_limit_reason_is_frozen_on_the_official_line() {
    // The reason lives ON the registry line (the closed
    // FileAssociationRegistration shape) — asserted byte-for-byte on
    // the official distribution.
    let appimage = official_file_association_lines()
        .iter()
        .find(|line| line.channel == FileAssociationChannel::LinuxAppImage)
        .expect("appimage line");
    assert_eq!(
        appimage.registration.reason(),
        Some(
            "Tu peux ajouter l'association avec un outil d'intégration AppImage \
             ou une entrée d'application manuelle."
        )
    );
}

#[test]
fn linux_install_notices_are_frozen_per_kind_and_tags_are_stable() {
    assert_eq!(
        linux_install_kind_wire_tag(LinuxInstallKind::AppImage),
        "appImage"
    );
    assert_eq!(
        linux_install_kind_wire_tag(LinuxInstallKind::SystemPackage),
        "systemPackage"
    );
    assert_eq!(
        linux_install_kind_wire_tag(LinuxInstallKind::LocalBuild),
        "localBuild"
    );
    assert_eq!(
        linux_install_notice(LinuxInstallKind::AppImage),
        "Ton installation actuelle est une AppImage : l'association n'est pas \
         enregistrée d'office."
    );
    assert_eq!(
        linux_install_notice(LinuxInstallKind::SystemPackage),
        "Ton installation actuelle provient d'un paquet système : l'association \
         est enregistrée."
    );
    assert_eq!(
        linux_install_notice(LinuxInstallKind::LocalBuild),
        "Cette version de Rustory n'a pas été installée par un paquet officiel : \
         elle n'enregistre pas d'association d'office."
    );
}

#[test]
fn the_extension_label_stays_coherent_with_the_domain_extension_constant() {
    // The wire label is a frozen literal (never composed at runtime);
    // this tripwire proves it can never silently diverge from the
    // domain's single extension truth.
    assert_eq!(
        FILE_ASSOCIATION_EXTENSION_LABEL,
        format!(".{RUSTORY_ARTIFACT_EXTENSION}")
    );
}

// ===== File association — the CURRENT official block, serialized
// EXACTLY (one registry line = one assertion) =====

#[test]
fn the_official_file_association_block_serializes_exactly() {
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v["fileAssociation"]["extensionLabel"], ".rustory");
    let channels = v["fileAssociation"]["channels"]
        .as_array()
        .expect("channels");
    assert_eq!(channels.len(), 4);
    assert_eq!(
        channels[0],
        serde_json::json!({
            "channel": "linuxSystemPackage",
            "label": "Paquet Linux (.deb / .rpm)",
            "registered": true,
            "statusLabel": "Enregistrée à l'installation",
            "detail": "L'association est déclarée par le paquet et active dès l'installation.",
        })
    );
    assert_eq!(
        channels[1],
        serde_json::json!({
            "channel": "linuxAppImage",
            "label": "AppImage (Linux)",
            "registered": false,
            "statusLabel": "Non enregistrée d'office",
            "detail": "Une AppImage ne modifie pas ton système : rien n'est enregistré automatiquement.",
            "reason": "Tu peux ajouter l'association avec un outil d'intégration AppImage ou une entrée d'application manuelle.",
        })
    );
    assert_eq!(
        channels[2],
        serde_json::json!({
            "channel": "windowsInstaller",
            "label": "Installeur Windows (.msi / .exe)",
            "registered": true,
            "statusLabel": "Enregistrée à l'installation",
            "detail": "L'installeur déclare l'association. Windows peut te demander de confirmer et respecte ton choix existant.",
        })
    );
    assert_eq!(
        channels[3],
        serde_json::json!({
            "channel": "macosAppBundle",
            "label": "Application macOS (.dmg)",
            "registered": true,
            "statusLabel": "Enregistrée par le système",
            "detail": "macOS enregistre l'association quand l'application est déposée dans Applications.",
        })
    );
}

#[test]
fn a_registered_channel_omits_the_reason_key_and_a_non_registered_one_carries_it() {
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    for channel in v["fileAssociation"]["channels"].as_array().expect("chs") {
        if channel["registered"] == true {
            assert!(
                channel.get("reason").is_none(),
                "a registered channel carries NO reason key (the status replaces it)"
            );
        } else {
            let reason = channel["reason"].as_str().expect("limit reason");
            assert!(!reason.is_empty(), "a limit never renders as a bare ✗");
        }
    }
}

#[test]
fn the_profile_without_a_probe_verdict_omits_the_current_install_key_entirely() {
    // Windows/macOS and an indeterminable executable: NO claim is ever
    // serialized — the key is ABSENT, never null, never invented.
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert!(
        v["fileAssociation"]
            .as_object()
            .expect("object")
            .get("currentInstall")
            .is_none(),
        "currentInstall must be omitted when no probe spoke"
    );
    // The explicit no-verdict attach keeps the same shape.
    let dto = SupportProfileDto::from_matrices(
        official_device_support_matrix(),
        official_local_artifacts(),
    )
    .with_linux_install(None);
    let v = serde_json::to_value(&dto).expect("ser");
    assert!(v["fileAssociation"]
        .as_object()
        .expect("object")
        .get("currentInstall")
        .is_none());
}

#[test]
fn each_probed_install_kind_serializes_its_frozen_tag_and_notice() {
    for (kind, tag, notice) in [
        (
            LinuxInstallKind::AppImage,
            "appImage",
            "Ton installation actuelle est une AppImage : l'association n'est pas enregistrée d'office.",
        ),
        (
            LinuxInstallKind::SystemPackage,
            "systemPackage",
            "Ton installation actuelle provient d'un paquet système : l'association est enregistrée.",
        ),
        (
            LinuxInstallKind::LocalBuild,
            "localBuild",
            "Cette version de Rustory n'a pas été installée par un paquet officiel : elle n'enregistre pas d'association d'office.",
        ),
    ] {
        let dto = SupportProfileDto::from_matrices(
            official_device_support_matrix(),
            official_local_artifacts(),
        )
        .with_linux_install(Some(kind));
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(
            v["fileAssociation"]["currentInstall"],
            serde_json::json!({ "kind": tag, "notice": notice }),
            "kind {kind:?}"
        );
    }
}

// ===== File association — exhaustiveness (tripwire round-trip) =====

#[test]
fn every_channel_serializes_a_tag_a_label_and_a_status_face() {
    for channel in ALL_FILE_ASSOCIATION_CHANNELS {
        assert!(!channel.wire_tag().is_empty());
        assert!(!file_association_channel_label(channel).is_empty());
    }
    // Status wording present for every registration shape (the status
    // label never depends on the carried reason).
    for registration in [
        FileAssociationRegistration::InstalledWithPackage,
        FileAssociationRegistration::RegisteredBySystem,
        FileAssociationRegistration::NotRegisteredByDefault { reason: "why" },
    ] {
        assert!(!file_association_status_label(registration).is_empty());
    }
}

#[test]
fn every_official_channel_reason_is_coherent_with_its_registration() {
    // Reason present IFF the official line does NOT register — carried
    // by the line itself (the FileAssociationRegistration shape), so
    // the wire can never render a bare ✗ nor justify a registered
    // channel.
    for line in official_file_association_lines() {
        assert_eq!(
            line.registration.reason().is_none(),
            line.registration.is_registered(),
            "channel {:?}: reason present IFF not registered",
            line.channel
        );
        assert!(
            !line.detail.is_empty(),
            "channel {:?}: every line carries its detail",
            line.channel
        );
    }
}
