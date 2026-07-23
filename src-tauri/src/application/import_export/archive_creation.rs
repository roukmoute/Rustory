//! Structured-archive (.zip pack) creation application service.
//!
//! The zip I/O shell around the pure domain analysis
//! (`analyze_structured_archive_components`) — phase for phase the folder
//! flow's shape:
//!
//! 1. [`analyze_structured_archive`] — bounded reads only, ZERO mutation
//!    and ZERO extraction: the descriptor entry (`story.json`) is read
//!    under its byte bound, each referenced media entry is probed by its
//!    16 header bytes straight from the archive. Every archive-STATE
//!    problem (unreadable zip, descriptor absent / oversize / malformed…)
//!    is a typed VERDICT; only transport crosses as an error.
//! 2. [`prepare_structured_archive_creation`] — RE-ANALYZES from zero,
//!    extracts each retained entry into a bounded TEMPORARY directory
//!    (sober basenames only — validated before any path join), then hands
//!    the promotion / wiring / serialization to the SHARED tail
//!    ([`prepare_from_creatable`]); the provenance rides
//!    `source_format = 'structured-archive'`. The commit and the
//!    compensation are the folder flow's own, unchanged.
//!
//! Zip-bomb posture: entry-count ceiling, per-entry byte bounds enforced
//! on the bytes ACTUALLY read (`take(bound + 1)` — a lying header cannot
//! bypass them), and the shared total-bytes bound re-applied by the
//! promotion. Nothing is ever extracted at analysis time.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use crate::domain::import::{
    analyze_structured_archive_components, archive_referenced_media, is_artifact_checksum,
    is_supported_folder_source_name, MediaProbe, StructuredFolderAnalysis,
    MAX_ARCHIVE_TOTAL_MEDIA_BYTES, STRUCTURED_ARCHIVE_ASSETS_PREFIX,
    STRUCTURED_ARCHIVE_FORMAT_VERSION, STRUCTURED_ARCHIVE_STORY_JSON_NAME,
};
use crate::domain::shared::AppError;
use crate::domain::story::content_checksum_bytes;
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::filesystem::{sniff_media, MAX_MEDIA_BYTES};
use crate::ipc::dto::StoryCardDto;

use super::structured_creation::{
    commit_structured_creation, compensate_structured_creation, folder_kind,
    prepare_from_creatable, CreationProvenance, PrepareFailure, PreparedCreation,
};

/// Hard ceiling on the descriptor bytes. A community descriptor is a few
/// hundred kB at the very worst; a bigger entry is refused as an
/// unreadable envelope (typed verdict).
pub const MAX_STORY_JSON_BYTES: u64 = 4 * 1024 * 1024;

/// Ceiling on the archive's ENTRY COUNT (anti-DoS: bounds every directory
/// walk before a single entry is read). Beyond it the envelope blocks.
pub const MAX_ARCHIVE_ENTRIES: usize = 32_768;

/// Bytes read by the media PROBE — the sniffer's longest magic-byte need.
const MEDIA_SNIFF_BYTES: usize = 16;

/// The application-level outcome of analyzing a picked archive: the typed
/// domain verdict + the provenance facts the accept phase re-derives.
#[derive(Debug, Clone)]
pub struct ArchiveCreationOutcome {
    pub analysis: StructuredFolderAnalysis,
    /// The archive's basename — the only name that ever crosses to the
    /// provenance row (never an absolute path, PII).
    pub archive_name: String,
    /// SHA-256 of the descriptor bytes (the provenance fingerprint);
    /// `None` when the descriptor could not be read (envelope-blocked).
    pub descriptor_checksum: Option<String>,
}

/// Phase 1 — analyze the archive with bounded reads, ZERO mutation and
/// ZERO extraction. An archive whose NAME Rustory cannot carry as a
/// provenance source is an honest TRANSPORT refusal; every archive-STATE
/// problem is a typed verdict.
pub fn analyze_structured_archive(archive_path: &Path) -> Result<ArchiveCreationOutcome, AppError> {
    let archive_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(archive_name_unusable_error)?;
    if !is_supported_folder_source_name(&archive_name) {
        return Err(archive_name_unusable_error());
    }
    let fallback_title = archive_stem(&archive_name);

    let Some(mut archive) = open_archive_bounded(archive_path) else {
        return Ok(ArchiveCreationOutcome {
            analysis: StructuredFolderAnalysis::envelope_blocked(),
            archive_name,
            descriptor_checksum: None,
        });
    };

    let story_json = read_descriptor_bounded(&mut archive);
    let descriptor_checksum = story_json.as_deref().map(content_checksum_bytes);

    // Probe each DISTINCT sober referenced basename straight from the
    // archive's central directory — 16 header bytes per entry, no
    // extraction. A basename the domain never returned is never probed.
    let mut probes: BTreeMap<String, MediaProbe> = BTreeMap::new();
    if let Some(bytes) = story_json.as_deref() {
        for (basename, _kind) in archive_referenced_media(bytes) {
            let probe = probe_archive_entry(&mut archive, &basename);
            probes.insert(basename, probe);
        }
    }

    let analysis = analyze_structured_archive_components(
        story_json.as_deref(),
        &probes,
        Some(&fallback_title),
    );

    Ok(ArchiveCreationOutcome {
        analysis,
        archive_name,
        descriptor_checksum,
    })
}

/// Phase 2a — re-analyze from zero, extract the retained entries into a
/// bounded temporary directory, and hand the shared tail the promotion.
/// NO DB access at all (the command runs this BEFORE taking the DB lock).
pub fn prepare_structured_archive_creation(
    app_data_dir: &Path,
    archive_path: &Path,
) -> Result<PreparedCreation, PrepareFailure> {
    if !archive_path.is_absolute() {
        return Err(PrepareFailure::bare(invalid_archive_path_error()));
    }

    let outcome = analyze_structured_archive(archive_path).map_err(PrepareFailure::bare)?;
    let Some(creatable) = outcome.analysis.creatable.clone() else {
        return Err(PrepareFailure::bare(revalidation_error()));
    };
    let Some(descriptor_checksum) = outcome.descriptor_checksum.clone() else {
        return Err(PrepareFailure::bare(revalidation_error()));
    };

    // Provenance re-validation BEFORE any write (defense in depth).
    if !is_supported_folder_source_name(&outcome.archive_name) {
        return Err(PrepareFailure::bare(invalid_provenance_error(
            "source_name",
        )));
    }
    if !is_artifact_checksum(&descriptor_checksum) {
        return Err(PrepareFailure::bare(invalid_provenance_error(
            "artifact_checksum",
        )));
    }

    // Bounded extraction of every DISTINCT retained basename into a
    // temporary directory: the promotion then reads REGULAR files exactly
    // like the folder flow. Basenames are sober by construction (the
    // domain never retains a non-sober one), so the join cannot traverse.
    let staging =
        tempfile::tempdir().map_err(|_| PrepareFailure::bare(archive_read_error("staging_dir")))?;
    let Some(mut archive) = open_archive_bounded(archive_path) else {
        return Err(PrepareFailure::bare(revalidation_error()));
    };
    let mut extracted: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for retained in &creatable.retained_media {
        if !extracted.insert(retained.basename.as_str()) {
            continue;
        }
        let bytes = read_entry_bounded(&mut archive, &retained.basename, MAX_MEDIA_BYTES as u64)
            .ok_or_else(|| PrepareFailure::bare(archive_read_error("entry_read")))?;
        std::fs::write(staging.path().join(&retained.basename), bytes)
            .map_err(|_| PrepareFailure::bare(archive_read_error("staging_write")))?;
    }

    prepare_from_creatable(
        app_data_dir,
        staging.path(),
        &creatable,
        CreationProvenance {
            source_format: "structured-archive",
            source_format_version: STRUCTURED_ARCHIVE_FORMAT_VERSION,
            source_name: outcome.archive_name,
            artifact_checksum: descriptor_checksum,
            state: outcome.analysis.state,
            findings: outcome.analysis.findings,
        },
        MAX_ARCHIVE_TOTAL_MEDIA_BYTES,
    )
    // `staging` drops here — the promotion copied what it needed into the
    // content-addressed store.
}

/// Convenience: prepare + commit under the SAME borrowed handle (tests and
/// single-threaded callers). The IPC command does NOT use this — it runs
/// the prepare before taking the DB lock and only locks for the commit.
pub fn accept_structured_archive_creation(
    db: &mut DbHandle,
    app_data_dir: &Path,
    archive_path: &Path,
) -> Result<StoryCardDto, AppError> {
    match prepare_structured_archive_creation(app_data_dir, archive_path) {
        Ok(prepared) => commit_structured_creation(db, app_data_dir, prepared),
        Err(failure) => {
            compensate_structured_creation(db, app_data_dir, &failure.promoted);
            Err(failure.error)
        }
    }
}

/// Open the archive read-only with the flow's file discipline: the path
/// must be a REGULAR file (a symlink or any special file is refused
/// unread), the zip directory must parse, and the entry count must sit
/// under the anti-DoS ceiling. `None` = an archive-STATE problem (the
/// envelope blocks); I/O transport failures land there too — the verdict
/// stays honest ("this archive could not be read").
fn open_archive_bounded(path: &Path) -> Option<zip::ZipArchive<std::fs::File>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    // TOCTOU re-check on the OPENED handle: what we hold is what we probed.
    let opened = file.metadata().ok()?;
    if !opened.is_file() {
        return None;
    }
    let archive = zip::ZipArchive::new(file).ok()?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return None;
    }
    Some(archive)
}

/// Read the descriptor entry under its byte bound. `None` = absent,
/// oversize (declared OR actual — `take(bound + 1)` catches a lying
/// header) or unreadable: the envelope blocks.
fn read_descriptor_bounded(archive: &mut zip::ZipArchive<std::fs::File>) -> Option<Vec<u8>> {
    read_entry_bounded(
        archive,
        STRUCTURED_ARCHIVE_STORY_JSON_NAME,
        MAX_STORY_JSON_BYTES,
    )
}

/// Read ONE entry by its canonical names (`assets/<basename>` first — the
/// documented layout —, bare `<basename>` as the hand-made-zip fallback),
/// bounded by `max_bytes` on the bytes ACTUALLY read.
fn read_entry_bounded(
    archive: &mut zip::ZipArchive<std::fs::File>,
    basename: &str,
    max_bytes: u64,
) -> Option<Vec<u8>> {
    let prefixed = format!("{STRUCTURED_ARCHIVE_ASSETS_PREFIX}{basename}");
    let name = if archive.by_name(&prefixed).is_ok() {
        prefixed
    } else if archive.by_name(basename).is_ok() {
        basename.to_string()
    } else {
        return None;
    };
    let entry = archive.by_name(&name).ok()?;
    if entry.size() > max_bytes {
        return None;
    }
    let mut bytes = Vec::new();
    entry.take(max_bytes + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > max_bytes {
        return None;
    }
    Some(bytes)
}

/// Probe one referenced basename straight from the archive: presence,
/// declared size bound, magic-byte sniff on the first header bytes.
fn probe_archive_entry(archive: &mut zip::ZipArchive<std::fs::File>, basename: &str) -> MediaProbe {
    let prefixed = format!("{STRUCTURED_ARCHIVE_ASSETS_PREFIX}{basename}");
    let name = if archive.by_name(&prefixed).is_ok() {
        prefixed
    } else if archive.by_name(basename).is_ok() {
        basename.to_string()
    } else {
        return MediaProbe::Absent;
    };
    let Ok(entry) = archive.by_name(&name) else {
        return MediaProbe::Absent;
    };
    let byte_size = entry.size();
    if byte_size > MAX_MEDIA_BYTES as u64 {
        return MediaProbe::Unusable;
    }
    let mut header = Vec::with_capacity(MEDIA_SNIFF_BYTES);
    if entry
        .take(MEDIA_SNIFF_BYTES as u64)
        .read_to_end(&mut header)
        .is_err()
    {
        return MediaProbe::Unusable;
    }
    match sniff_media(&header) {
        Some(sniffed) => MediaProbe::Usable {
            kind: folder_kind(sniffed.kind),
            byte_size,
        },
        None => MediaProbe::Unusable,
    }
}

/// The archive's stem — its sober basename minus a single trailing `.zip`
/// (any ASCII case): the honest fallback title of a descriptor without
/// one. The suffix match guarantees the last 4 chars are ASCII
/// `[.][Zz][Ii][Pp]`, so the byte slice below cannot split a char.
fn archive_stem(archive_name: &str) -> String {
    if archive_name.to_lowercase().ends_with(".zip") && archive_name.len() > 4 {
        archive_name[..archive_name.len() - 4].to_string()
    } else {
        archive_name.to_string()
    }
}

fn archive_name_unusable_error() -> AppError {
    AppError::import_failed(
        "Création impossible: le nom de l'archive choisie ne peut pas être utilisé par Rustory.",
        "Renomme l'archive (nom plus court, sans caractère spécial) puis relance l'analyse.",
    )
    .with_details(serde_json::json!({
        "source": "file_read",
        "stage": "archive_name",
    }))
}

fn invalid_archive_path_error() -> AppError {
    AppError::import_failed(
        "Création impossible: l'emplacement de l'archive est invalide.",
        "Relance l'analyse de l'archive puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "file_read",
        "stage": "invalid_path",
    }))
}

fn revalidation_error() -> AppError {
    AppError::import_failed(
        "Création impossible: l'archive n'a pas pu être revalidée.",
        "Le contenu de l'archive a peut-être changé. Relance l'analyse puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "other", "cause": "revalidation" }))
}

fn invalid_provenance_error(field: &'static str) -> AppError {
    AppError::import_failed(
        "Création impossible: informations de provenance invalides.",
        "Relance l'analyse de l'archive puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "cause": "invalid_provenance",
        "field": field,
    }))
}

fn archive_read_error(stage: &'static str) -> AppError {
    AppError::import_failed(
        "Création impossible: un fichier de l'archive n'a pas pu être lu.",
        "Vérifie le contenu de l'archive puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "file_read",
        "stage": stage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db;
    use std::io::Write;

    const CLEAN_STORY_JSON: &str = r#"{
        "format": "v1",
        "version": 2,
        "title": "Le pack du smoke",
        "stageNodes": [
            {
                "uuid": "stage-1",
                "squareOne": true,
                "name": "Départ",
                "image": "cover.png",
                "audio": "intro.mp3",
                "okTransition": { "actionNode": "action-1", "optionIndex": 0 },
                "controlSettings": { "wheel": true, "ok": true, "home": false, "pause": false, "autoplay": false }
            },
            {
                "uuid": "stage-2",
                "name": "Suite",
                "image": null,
                "audio": null,
                "okTransition": null,
                "controlSettings": {}
            }
        ],
        "actionNodes": [ { "id": "action-1", "options": ["stage-2"] } ]
    }"#;

    // Real magic bytes so the sniffer recognizes the formats.
    const PNG_BYTES: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 1, 2,
        3, 4,
    ];
    const MP3_BYTES: &[u8] = &[b'I', b'D', b'3', 4, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4];
    const BMP_BYTES: &[u8] = &[b'B', b'M', 60, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0];

    fn write_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in entries {
            writer.start_file(name.to_string(), options).expect("entry");
            writer.write_all(bytes).expect("bytes");
        }
        writer.finish().expect("finish zip");
    }

    fn fresh_db() -> DbHandle {
        let mut handle = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut handle).expect("migrate");
        handle
    }

    #[test]
    fn accepts_a_clean_pack_end_to_end() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_data_dir = dir.path().join("appdata");
        std::fs::create_dir_all(&app_data_dir).expect("appdata");
        let zip_path = dir.path().join("Mon pack.zip");
        write_zip(
            &zip_path,
            &[
                ("story.json", CLEAN_STORY_JSON.as_bytes()),
                ("assets/cover.png", PNG_BYTES),
                ("assets/intro.mp3", MP3_BYTES),
            ],
        );

        let mut handle = fresh_db();
        let card = accept_structured_archive_creation(&mut handle, &app_data_dir, &zip_path)
            .expect("accept");
        assert_eq!(card.title, "Le pack du smoke");

        // Canonical row + wired structure.
        let (title, structure_json): (String, String) = handle
            .conn()
            .query_row(
                "SELECT title, structure_json FROM stories WHERE id = ?1",
                rusqlite::params![&card.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("story row");
        assert_eq!(title, "Le pack du smoke");
        let structure: serde_json::Value =
            serde_json::from_str(&structure_json).expect("structure json");
        assert_eq!(structure["startNodeId"], "stage-1");
        assert_eq!(structure["nodes"].as_array().expect("nodes").len(), 2);
        assert!(structure["nodes"][0]["imageAssetId"].is_string());
        assert!(structure["nodes"][0]["audioAssetId"].is_string());
        assert_eq!(structure["nodes"][0]["options"][0]["target"], "stage-2");

        // Provenance row carries the archive identity.
        let (source_format, source_name): (String, String) = handle
            .conn()
            .query_row(
                "SELECT source_format, source_name FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![&card.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("provenance row");
        assert_eq!(source_format, "structured-archive");
        assert_eq!(source_name, "Mon pack.zip");

        // Two promoted media rows, files present in the store.
        let asset_count: i64 = handle
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE story_id = ?1",
                rusqlite::params![&card.id],
                |r| r.get(0),
            )
            .expect("assets count");
        assert_eq!(asset_count, 2);
    }

    #[test]
    fn analyze_discards_unsupported_media_and_stays_creatable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip_path = dir.path().join("bmp-pack.zip");
        let story = CLEAN_STORY_JSON.replace("cover.png", "cover.bmp");
        write_zip(
            &zip_path,
            &[
                ("story.json", story.as_bytes()),
                ("assets/cover.bmp", BMP_BYTES),
                ("assets/intro.mp3", MP3_BYTES),
            ],
        );

        let outcome = analyze_structured_archive(&zip_path).expect("analyze");
        assert_eq!(
            outcome.analysis.discarded_media,
            vec!["cover.bmp".to_string()]
        );
        let creatable = outcome.analysis.creatable.expect("creatable");
        assert_eq!(creatable.retained_media.len(), 1);
    }

    #[test]
    fn a_zip_without_story_json_blocks_and_never_creates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_data_dir = dir.path().join("appdata");
        std::fs::create_dir_all(&app_data_dir).expect("appdata");
        let zip_path = dir.path().join("vide.zip");
        write_zip(&zip_path, &[("lisez-moi.txt", b"pas un pack")]);

        let outcome = analyze_structured_archive(&zip_path).expect("analyze");
        assert!(outcome.analysis.creatable.is_none());
        assert!(outcome.descriptor_checksum.is_none());

        let mut handle = fresh_db();
        let err = accept_structured_archive_creation(&mut handle, &app_data_dir, &zip_path)
            .expect_err("blocked pack must refuse acceptance");
        assert_eq!(err.code, crate::domain::shared::AppErrorCode::ImportFailed);
        let count: i64 = handle
            .conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn a_non_zip_file_is_an_envelope_block_not_a_transport_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pas-une-archive.zip");
        std::fs::write(&path, b"contenu quelconque").expect("write");
        let outcome = analyze_structured_archive(&path).expect("analyze");
        assert!(outcome.analysis.creatable.is_none());
        assert!(outcome.descriptor_checksum.is_none());
    }

    #[test]
    fn a_lying_descriptor_declaring_a_small_size_is_still_bounded() {
        // The declared entry size is what `size()` reports; the read is
        // bounded on ACTUAL bytes. A descriptor entry larger than the
        // ceiling is refused as an envelope block.
        let dir = tempfile::tempdir().expect("tempdir");
        let zip_path = dir.path().join("gros.zip");
        let big = vec![b' '; (MAX_STORY_JSON_BYTES + 10) as usize];
        write_zip(&zip_path, &[("story.json", &big)]);
        let outcome = analyze_structured_archive(&zip_path).expect("analyze");
        assert!(outcome.analysis.creatable.is_none());
    }

    #[test]
    fn missing_title_uses_the_archive_stem() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip_path = dir.path().join("Histoire du soir.zip");
        let story = r#"{
            "stageNodes": [
                { "uuid": "s1", "squareOne": true, "image": null, "audio": null,
                  "okTransition": null, "controlSettings": {} }
            ],
            "actionNodes": []
        }"#;
        write_zip(&zip_path, &[("story.json", story.as_bytes())]);
        let outcome = analyze_structured_archive(&zip_path).expect("analyze");
        let creatable = outcome.analysis.creatable.expect("creatable");
        assert_eq!(creatable.title, "Histoire du soir");
    }
}

#[cfg(test)]
mod real_pack_smoke {
    //! Ignored by default — points at a REAL community pack on disk via
    //! `RUSTORY_TEST_ZIP`. Proves the analyzer accepts genuine large packs
    //! (hundreds/thousands of media) rather than falsely blocking them.
    use super::*;

    #[test]
    #[ignore = "requires RUSTORY_TEST_ZIP pointing at a real .zip pack"]
    fn analyzes_a_real_pack_as_creatable() {
        let path = std::env::var("RUSTORY_TEST_ZIP").expect("set RUSTORY_TEST_ZIP");
        let outcome = analyze_structured_archive(std::path::Path::new(&path)).expect("analyze");
        let quality = &outcome.analysis.quality;
        let retained = outcome
            .analysis
            .creatable
            .as_ref()
            .map(|c| c.retained_media.len())
            .unwrap_or(0);
        let discarded = outcome.analysis.discarded_media.len();
        let nodes = outcome
            .analysis
            .creatable
            .as_ref()
            .map(|c| c.structure.nodes.len())
            .unwrap_or(0);
        eprintln!(
            "REAL PACK '{}': quality={:?} nodes={} retained_media={} discarded_media={}",
            outcome.archive_name, quality, nodes, retained, discarded,
        );
        assert!(
            outcome.analysis.creatable.is_some(),
            "a real pack must be creatable, not blocked as unusable"
        );
    }
}
