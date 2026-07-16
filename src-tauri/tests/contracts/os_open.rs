//! Wire-shape contract for the OS-open channel: the exact JSON of every
//! `OsOpenAnalysisDto` kind, the frozen multi-file calm-limit copy, the
//! `os-open:requested` signal payload, and the FORM PARITY between the
//! os-open `analyzed` verdict and the dialog-import one (the frontend
//! mirror `src/shared/ipc-contracts/import-export.ts` reuses the SAME
//! guard for both). The locked dialog-import contracts (`import_export.rs`)
//! stay untouched — this channel COMPOSES their field types, never extends
//! them.

use rustory_lib::ipc::dto::{
    ImportArtifactAnalysisDto, ImportAspectDto, ImportCategoryDto, ImportFindingDto,
    ImportQualityDto, ImportStateDto, ImportableContentDto, OsOpenAnalysisDto,
    OS_OPEN_MULTIPLE_FILES_MESSAGE,
};
use rustory_lib::ipc::events::{OsOpenRequestedEvent, EVENT_OS_OPEN_REQUESTED};
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

#[test]
fn none_is_a_single_kind_key() {
    let v = serde_json::to_value(OsOpenAnalysisDto::None).expect("ser");
    assert_eq!(v, json!({ "kind": "none" }));
}

#[test]
fn multiple_files_carries_the_frozen_copy_byte_for_byte() {
    let v = serde_json::to_value(OsOpenAnalysisDto::multiple_files()).expect("ser");
    assert_eq!(
        v,
        json!({
            "kind": "multipleFiles",
            "message": "Rustory ouvre un fichier à la fois. Rouvre chaque fichier séparément.",
        })
    );
    // The constant IS the wire copy — one literal, carried by the DTO.
    assert_eq!(
        OS_OPEN_MULTIPLE_FILES_MESSAGE,
        "Rustory ouvre un fichier à la fois. Rouvre chaque fichier séparément."
    );
}

#[test]
fn analyzed_wire_shape_is_tagged_camel_case() {
    let v = serde_json::to_value(OsOpenAnalysisDto::Analyzed {
        quality: ImportQualityDto::Partial,
        state: ImportStateDto::NeedsReview,
        findings: vec![finding()],
        importable_content: Some(importable_content()),
        source_name: "histoire.rustory".into(),
        artifact_checksum: "b".repeat(64),
    })
    .expect("ser");
    assert_eq!(v["kind"], "analyzed");
    assert_eq!(v["quality"], "partial");
    assert_eq!(v["state"], "needsReview");
    assert_eq!(v["findings"][0]["aspect"], "title");
    assert_eq!(v["findings"][0]["category"], "ambiguous");
    assert_eq!(v["sourceName"], "histoire.rustory");
    assert_eq!(v["artifactChecksum"].as_str().expect("str").len(), 64);
    assert_eq!(
        v["importableContent"]["title"], "Le Soleil",
        "the carried content keeps its camelCase field set"
    );
    for snake in ["source_name", "artifact_checksum", "importable_content"] {
        assert!(v.get(snake).is_none(), "{snake} must be camelCase");
    }
}

#[test]
fn a_blocked_analyzed_verdict_omits_importable_content() {
    let v = serde_json::to_value(OsOpenAnalysisDto::Analyzed {
        quality: ImportQualityDto::Unusable,
        state: ImportStateDto::Blocked,
        findings: vec![ImportFindingDto {
            aspect: ImportAspectDto::Envelope,
            category: ImportCategoryDto::Blocking,
            message: "msg".into(),
        }],
        importable_content: None,
        source_name: "inconnu.txt".into(),
        artifact_checksum: "c".repeat(64),
    })
    .expect("ser");
    assert_eq!(v["state"], "blocked");
    assert!(
        v.get("importableContent").is_none(),
        "a blocked verdict carries no importable content"
    );
}

/// The os-open `analyzed` verdict and the dialog-import one must expose the
/// EXACT same wire keys (the TS mirror validates both with one guard). A
/// field drifting on either side breaks this parity assertion first.
#[test]
fn analyzed_form_parity_with_the_dialog_import_verdict() {
    let os_open = serde_json::to_value(OsOpenAnalysisDto::Analyzed {
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

    let os_open_keys: Vec<&str> = os_open
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
    assert_eq!(os_open_keys, dialog_keys, "same wire keys, same order");

    // Beyond the keys: every shared field serializes identically.
    for key in dialog_keys {
        assert_eq!(os_open[key], dialog[key], "field `{key}` must match");
    }
}

#[test]
fn os_open_requested_event_name_and_empty_versionable_payload() {
    assert_eq!(EVENT_OS_OPEN_REQUESTED, "os-open:requested");
    let v = serde_json::to_value(OsOpenRequestedEvent {}).expect("ser");
    assert_eq!(v, json!({}), "a pure signal — the event carries no data");
}
