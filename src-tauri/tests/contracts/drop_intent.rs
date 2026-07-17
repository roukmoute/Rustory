//! Wire-shape contract for the drop channel: the exact JSON of every
//! `DropAnalysisDto` kind, the frozen multi-element calm-limit copy, the
//! three empty drop signals, and the FORM PARITY of the `artifact` kind
//! with the dialog-import / OS-open `analyzed` verdicts AND of the
//! `folder` kind with the picker folder verdict (the frontend mirror
//! `src/shared/ipc-contracts/import-export.ts` reuses the SAME guards).
//! The locked dialog-import / OS-open / folder-creation contracts stay
//! untouched — this channel COMPOSES their field types, never extends
//! them.

use rustory_lib::ipc::dto::{
    CreatableSummaryDto, DropAnalysisDto, ImportArtifactAnalysisDto, ImportAspectDto,
    ImportCategoryDto, ImportFindingDto, ImportQualityDto, ImportStateDto, ImportableContentDto,
    OsOpenAnalysisDto, StructuredCreationAnalysisDto, DROP_MULTIPLE_ITEMS_MESSAGE,
    OS_OPEN_MULTIPLE_FILES_MESSAGE,
};
use rustory_lib::ipc::events::{
    DropHoverEndedEvent, DropHoverEvent, DropRequestedEvent, EVENT_DROP_HOVER,
    EVENT_DROP_HOVER_ENDED, EVENT_DROP_REQUESTED,
};
use serde_json::json;

fn importable_content() -> ImportableContentDto {
    ImportableContentDto {
        title: "Le Soleil".into(),
        structure_json: "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}".into(),
        content_checksum: "a".repeat(64),
        created_at: "2026-06-20T10:00:00.000Z".into(),
        updated_at: "2026-06-24T14:15:00.000Z".into(),
    }
}

fn finding() -> ImportFindingDto {
    ImportFindingDto {
        aspect: ImportAspectDto::Title,
        category: ImportCategoryDto::Ambiguous,
        message: "msg".into(),
    }
}

fn creatable_summary() -> CreatableSummaryDto {
    CreatableSummaryDto {
        title: "Le voyage de Nour".into(),
        node_count: 2,
        retained_media: vec!["couverture.png".into()],
        discarded_media: vec!["absente.png".into()],
    }
}

#[test]
fn none_is_a_single_kind_key() {
    let v = serde_json::to_value(DropAnalysisDto::None).expect("ser");
    assert_eq!(v, json!({ "kind": "none" }));
}

#[test]
fn multiple_items_carries_the_frozen_copy_byte_for_byte() {
    let v = serde_json::to_value(DropAnalysisDto::multiple_items()).expect("ser");
    assert_eq!(
        v,
        json!({
            "kind": "multipleItems",
            "message": "Rustory traite un seul élément déposé à la fois. Dépose chaque élément séparément.",
        })
    );
    // The constant IS the wire copy — one literal, carried by the DTO.
    assert_eq!(
        DROP_MULTIPLE_ITEMS_MESSAGE,
        "Rustory traite un seul élément déposé à la fois. Dépose chaque élément séparément."
    );
    // IDENTITY ≠ COPY: a SISTER literal of the OS-open multi-file copy,
    // never the same words (reopening ≠ dropping).
    assert_ne!(DROP_MULTIPLE_ITEMS_MESSAGE, OS_OPEN_MULTIPLE_FILES_MESSAGE);
}

#[test]
fn artifact_wire_shape_is_tagged_camel_case() {
    let v = serde_json::to_value(DropAnalysisDto::Artifact {
        quality: ImportQualityDto::Partial,
        state: ImportStateDto::NeedsReview,
        findings: vec![finding()],
        importable_content: Some(importable_content()),
        source_name: "histoire.rustory".into(),
        artifact_checksum: "b".repeat(64),
    })
    .expect("ser");
    assert_eq!(v["kind"], "artifact");
    assert_eq!(v["quality"], "partial");
    assert_eq!(v["state"], "needsReview");
    assert_eq!(v["findings"][0]["aspect"], "title");
    assert_eq!(v["sourceName"], "histoire.rustory");
    assert_eq!(v["artifactChecksum"].as_str().expect("str").len(), 64);
    assert_eq!(v["importableContent"]["title"], "Le Soleil");
    for snake in ["source_name", "artifact_checksum", "importable_content"] {
        assert!(v.get(snake).is_none(), "{snake} must be camelCase");
    }
}

#[test]
fn a_blocked_artifact_verdict_omits_importable_content() {
    let v = serde_json::to_value(DropAnalysisDto::Artifact {
        quality: ImportQualityDto::Unusable,
        state: ImportStateDto::Blocked,
        findings: vec![ImportFindingDto {
            aspect: ImportAspectDto::Envelope,
            category: ImportCategoryDto::Blocking,
            message: "msg".into(),
        }],
        importable_content: None,
        source_name: "photo.png".into(),
        artifact_checksum: "c".repeat(64),
    })
    .expect("ser");
    assert_eq!(v["state"], "blocked");
    assert!(
        v.get("importableContent").is_none(),
        "a blocked verdict carries no importable content"
    );
}

#[test]
fn folder_wire_shape_is_tagged_camel_case() {
    let v = serde_json::to_value(DropAnalysisDto::Folder {
        quality: ImportQualityDto::Partial,
        state: ImportStateDto::Partial,
        findings: vec![finding()],
        creatable_summary: Some(creatable_summary()),
        folder_name: "mon-histoire".into(),
        folder_path: "/home/user/mon-histoire".into(),
    })
    .expect("ser");
    assert_eq!(v["kind"], "folder");
    assert_eq!(v["quality"], "partial");
    assert_eq!(v["state"], "partial");
    assert_eq!(v["folderName"], "mon-histoire");
    assert_eq!(v["folderPath"], "/home/user/mon-histoire");
    assert_eq!(v["creatableSummary"]["title"], "Le voyage de Nour");
    assert_eq!(v["creatableSummary"]["nodeCount"], 2);
    for snake in ["folder_name", "folder_path", "creatable_summary"] {
        assert!(v.get(snake).is_none(), "{snake} must be camelCase");
    }
}

#[test]
fn a_blocked_folder_verdict_omits_the_creatable_summary() {
    let v = serde_json::to_value(DropAnalysisDto::Folder {
        quality: ImportQualityDto::Unusable,
        state: ImportStateDto::Blocked,
        findings: vec![ImportFindingDto {
            aspect: ImportAspectDto::Envelope,
            category: ImportCategoryDto::Blocking,
            message: "msg".into(),
        }],
        creatable_summary: None,
        folder_name: "dossier-vide".into(),
        folder_path: "/home/user/dossier-vide".into(),
    })
    .expect("ser");
    assert_eq!(v["state"], "blocked");
    assert!(
        v.get("creatableSummary").is_none(),
        "a blocked verdict carries nothing creatable"
    );
}

/// The drop `artifact` verdict must expose the EXACT same wire keys as the
/// dialog-import `analyzed` one (the TS mirror validates both with one
/// guard, modulo the `kind` tag). A field drifting on either side breaks
/// this parity assertion first.
#[test]
fn artifact_form_parity_with_the_dialog_import_and_os_open_verdicts() {
    let drop = serde_json::to_value(DropAnalysisDto::Artifact {
        quality: ImportQualityDto::Partial,
        state: ImportStateDto::NeedsReview,
        findings: vec![finding()],
        importable_content: Some(importable_content()),
        source_name: "histoire.rustory".into(),
        artifact_checksum: "b".repeat(64),
    })
    .expect("ser");
    let dialog = serde_json::to_value(ImportArtifactAnalysisDto::Analyzed {
        quality: ImportQualityDto::Partial,
        state: ImportStateDto::NeedsReview,
        findings: vec![finding()],
        importable_content: Some(importable_content()),
        source_name: "histoire.rustory".into(),
        artifact_checksum: "b".repeat(64),
    })
    .expect("ser");
    let os_open = serde_json::to_value(OsOpenAnalysisDto::Analyzed {
        quality: ImportQualityDto::Partial,
        state: ImportStateDto::NeedsReview,
        findings: vec![finding()],
        importable_content: Some(importable_content()),
        source_name: "histoire.rustory".into(),
        artifact_checksum: "b".repeat(64),
    })
    .expect("ser");

    let drop_keys: Vec<&str> = drop
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    let dialog_keys: Vec<&str> = dialog
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(drop_keys, dialog_keys, "same wire keys, same order");

    // Beyond the keys: every field EXCEPT the tag serializes identically
    // across the three channels feeding the same review machine.
    for key in dialog_keys {
        if key == "kind" {
            assert_eq!(drop[key], "artifact");
            assert_eq!(dialog[key], "analyzed");
            continue;
        }
        assert_eq!(drop[key], dialog[key], "field `{key}` must match dialog");
        assert_eq!(drop[key], os_open[key], "field `{key}` must match os-open");
    }
}

/// The drop `folder` verdict must expose the EXACT same wire keys as the
/// picker `analyzed` folder verdict (same guard TS-side, modulo the tag).
#[test]
fn folder_form_parity_with_the_picker_folder_verdict() {
    let drop = serde_json::to_value(DropAnalysisDto::Folder {
        quality: ImportQualityDto::Partial,
        state: ImportStateDto::Partial,
        findings: vec![finding()],
        creatable_summary: Some(creatable_summary()),
        folder_name: "mon-histoire".into(),
        folder_path: "/home/user/mon-histoire".into(),
    })
    .expect("ser");
    let picker = serde_json::to_value(StructuredCreationAnalysisDto::Analyzed {
        quality: ImportQualityDto::Partial,
        state: ImportStateDto::Partial,
        findings: vec![finding()],
        creatable_summary: Some(creatable_summary()),
        folder_name: "mon-histoire".into(),
        folder_path: "/home/user/mon-histoire".into(),
    })
    .expect("ser");

    let drop_keys: Vec<&str> = drop
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    let picker_keys: Vec<&str> = picker
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(drop_keys, picker_keys, "same wire keys, same order");

    for key in picker_keys {
        if key == "kind" {
            assert_eq!(drop[key], "folder");
            assert_eq!(picker[key], "analyzed");
            continue;
        }
        assert_eq!(drop[key], picker[key], "field `{key}` must match picker");
    }
}

#[test]
fn drop_event_names_and_empty_versionable_payloads() {
    assert_eq!(EVENT_DROP_HOVER, "drop:hover");
    assert_eq!(EVENT_DROP_HOVER_ENDED, "drop:hover-ended");
    assert_eq!(EVENT_DROP_REQUESTED, "drop:requested");
    // Pure signals — no path, no count, no kind ever crosses.
    assert_eq!(
        serde_json::to_value(DropHoverEvent {}).expect("ser"),
        json!({})
    );
    assert_eq!(
        serde_json::to_value(DropHoverEndedEvent {}).expect("ser"),
        json!({})
    );
    assert_eq!(
        serde_json::to_value(DropRequestedEvent {}).expect("ser"),
        json!({})
    );
}
