use serde::{Deserialize, Serialize};

use crate::domain::shared::AppError;

/// Canonical extension for a Rustory exported story artifact. The literal
/// `"rustory"` (no leading dot) is the form expected by the Tauri save
/// dialog's `extensions` filter and by any future OS file-association
/// registration.
pub const RUSTORY_ARTIFACT_EXTENSION: &str = "rustory";

/// Current format version for the `RustoryArtifactV1` envelope. A future
/// importer must refuse any value different from this constant until a
/// `V2` variant is explicitly introduced.
pub const RUSTORY_ARTIFACT_FORMAT_VERSION: u32 = 1;

/// Envelope metadata describing *when*, *by what tool* and *in what format*
/// the artifact was produced. Never mutated in MVP — every field is
/// populated at export time and read-only afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactEnvelopeV1 {
    pub format_version: u32,
    pub exported_at: String,
    pub exported_by: String,
}

/// Projection of a single persisted story into the exported artifact. The
/// `structure_json` string is a byte-for-byte copy of the SQLite column —
/// never reserialized, never reformatted, so the checksum contract holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportedStoryV1 {
    pub schema_version: u32,
    pub title: String,
    pub structure_json: String,
    pub content_checksum: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Root envelope for a Rustory v1 artifact on disk. The field order
/// (`rustoryArtifact` before `story`) is load-bearing for human-readable
/// diffs — the canonical JSON serializer must preserve it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RustoryArtifactV1 {
    pub rustory_artifact: ArtifactEnvelopeV1,
    pub story: ExportedStoryV1,
}

impl RustoryArtifactV1 {
    /// Serialize the artifact to the canonical on-disk representation:
    /// UTF-8 JSON, pretty-printed with a **two-space indent** nailed
    /// down explicitly via [`serde_json::ser::PrettyFormatter`], with a
    /// trailing `\n` per POSIX convention. The buffer is ready to be
    /// written to disk as-is.
    ///
    /// In practice `serde_json::to_vec_pretty` on data already validated
    /// upstream (all strings come from Rust-owned UTF-8, all integers are
    /// `u32`) cannot fail. The `Result` shape is kept to keep the call
    /// site uniform with the rest of the `AppError`-returning API.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, AppError> {
        // Explicit 2-space indent keeps the on-disk shape stable if
        // `serde_json` ever changes its pretty-printer default.
        let mut buffer = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
        self.serialize(&mut serializer).map_err(|_| {
            AppError::local_storage_unavailable(
                "Impossible de sérialiser l'artefact Rustory.",
                "Merci de signaler ce problème à l'équipe Rustory.",
            )
            .with_details(serde_json::json!({
                "source": "artifact_serialization",
            }))
        })?;
        buffer.push(b'\n');
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_artifact() -> RustoryArtifactV1 {
        RustoryArtifactV1 {
            rustory_artifact: ArtifactEnvelopeV1 {
                format_version: RUSTORY_ARTIFACT_FORMAT_VERSION,
                exported_at: "2026-04-24T14:30:00.000Z".into(),
                exported_by: "rustory/0.1.0".into(),
            },
            story: ExportedStoryV1 {
                schema_version: 1,
                title: "Le Soleil Couchant".into(),
                structure_json: "{\"schemaVersion\":1,\"nodes\":[]}".into(),
                content_checksum: "a".repeat(64),
                created_at: "2026-04-20T10:00:00.000Z".into(),
                updated_at: "2026-04-24T14:15:00.000Z".into(),
            },
        }
    }

    #[test]
    fn canonical_json_is_valid_utf8_with_trailing_newline() {
        let bytes = sample_artifact().to_canonical_json().expect("serialize");
        assert_eq!(bytes.last(), Some(&b'\n'), "must end with LF");
        std::str::from_utf8(&bytes).expect("must be UTF-8");
    }

    #[test]
    fn canonical_json_round_trips_via_serde_json_from_slice() {
        let original = sample_artifact();
        let bytes = original.to_canonical_json().expect("serialize");
        let parsed: RustoryArtifactV1 = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(parsed, original);
    }

    #[test]
    fn envelope_serializes_format_version_exported_at_exported_by_in_camel_case() {
        let bytes = sample_artifact().to_canonical_json().expect("serialize");
        let text = std::str::from_utf8(&bytes).expect("utf8");
        assert!(text.contains("\"rustoryArtifact\""), "camel top-level key");
        assert!(text.contains("\"formatVersion\""), "camel formatVersion");
        assert!(text.contains("\"exportedAt\""), "camel exportedAt");
        assert!(text.contains("\"exportedBy\""), "camel exportedBy");
        for snake in [
            "rustory_artifact",
            "format_version",
            "exported_at",
            "exported_by",
        ] {
            assert!(!text.contains(snake), "{snake} must not appear");
        }
    }

    #[test]
    fn exported_story_preserves_structure_json_byte_for_byte() {
        let canonical = "{\"schemaVersion\":1,\"nodes\":[]}";
        let mut artifact = sample_artifact();
        artifact.story.structure_json = canonical.into();
        let bytes = artifact.to_canonical_json().expect("serialize");
        let parsed: RustoryArtifactV1 = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(
            parsed.story.structure_json.as_bytes(),
            canonical.as_bytes(),
            "structure_json must survive round-trip byte-for-byte"
        );
    }

    #[test]
    fn content_checksum_is_copied_as_is_not_recomputed() {
        // An intentionally non-SHA256 checksum value proves the domain
        // layer never recomputes — export is a pure copy of the row.
        let bogus = "z".repeat(64);
        let mut artifact = sample_artifact();
        artifact.story.content_checksum = bogus.clone();
        let bytes = artifact.to_canonical_json().expect("serialize");
        let parsed: RustoryArtifactV1 = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(parsed.story.content_checksum, bogus);
    }

    #[test]
    fn format_version_constant_is_1() {
        assert_eq!(RUSTORY_ARTIFACT_FORMAT_VERSION, 1);
    }

    #[test]
    fn extension_constant_is_rustory_lowercase_no_dot() {
        assert_eq!(RUSTORY_ARTIFACT_EXTENSION, "rustory");
    }

    /// Forward-compatibility guard for a future importer: a payload
    /// declaring `formatVersion: 0` (or any value other than 1) must
    /// be refused. The importer itself is Post-MVP, so this test only
    /// asserts the shape fact it needs. `#[ignore]` keeps it out of
    /// the default run but documents the contract.
    #[test]
    #[ignore = "importer is Post-MVP; guards the wire shape for later"]
    fn rejects_deserialization_of_format_version_zero() {
        let json = r#"{
            "rustoryArtifact": {
                "formatVersion": 0,
                "exportedAt": "2026-04-24T14:30:00.000Z",
                "exportedBy": "rustory/0.1.0"
            },
            "story": {
                "schemaVersion": 1,
                "title": "t",
                "structureJson": "{}",
                "contentChecksum": "0000000000000000000000000000000000000000000000000000000000000000",
                "createdAt": "2026-04-20T10:00:00.000Z",
                "updatedAt": "2026-04-24T14:15:00.000Z"
            }
        }"#;
        let parsed =
            serde_json::from_str::<RustoryArtifactV1>(json).expect("shape valid for v1 struct");
        assert_ne!(
            parsed.rustory_artifact.format_version, RUSTORY_ARTIFACT_FORMAT_VERSION,
            "a Post-MVP importer MUST refuse this payload: formatVersion=0 != V1"
        );
    }

    #[test]
    fn rejects_deserialization_of_unknown_envelope_field() {
        // Wire drift guard: an unknown top-level field must fail parsing
        // so a future v2 payload cannot be silently accepted as v1.
        let json = r#"{
            "rustoryArtifact": {
                "formatVersion": 1,
                "exportedAt": "2026-04-24T14:30:00.000Z",
                "exportedBy": "rustory/0.1.0",
                "surprise": "field"
            },
            "story": {
                "schemaVersion": 1,
                "title": "t",
                "structureJson": "{}",
                "contentChecksum": "0000000000000000000000000000000000000000000000000000000000000000",
                "createdAt": "2026-04-20T10:00:00.000Z",
                "updatedAt": "2026-04-24T14:15:00.000Z"
            }
        }"#;
        let err = serde_json::from_str::<RustoryArtifactV1>(json)
            .expect_err("unknown field must be refused");
        assert!(
            err.to_string().contains("surprise"),
            "error must name the unknown field: {err}"
        );
    }

    #[test]
    fn envelope_uses_format_version_constant() {
        let bytes = sample_artifact().to_canonical_json().expect("serialize");
        let parsed: RustoryArtifactV1 = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(
            parsed.rustory_artifact.format_version,
            RUSTORY_ARTIFACT_FORMAT_VERSION
        );
    }

    #[test]
    fn canonical_json_key_order_is_envelope_then_story() {
        let bytes = sample_artifact().to_canonical_json().expect("serialize");
        let text = std::str::from_utf8(&bytes).expect("utf8");
        let envelope_pos = text
            .find("\"rustoryArtifact\"")
            .expect("envelope key present");
        let story_pos = text.find("\"story\"").expect("story key present");
        assert!(
            envelope_pos < story_pos,
            "rustoryArtifact must precede story in canonical output"
        );
    }
}
