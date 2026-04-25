use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::application::story::now_iso_ms;
use crate::domain::export::{
    ArtifactEnvelopeV1, ExportedStoryV1, RustoryArtifactV1, RUSTORY_ARTIFACT_FORMAT_VERSION,
};
use crate::domain::shared::AppError;
use crate::ipc::dto::StoryDetailDto;

/// Input accepted by the `export_story` application service. The detail
/// is already loaded from SQLite by the command layer so the DB mutex
/// can be released before the (potentially slow) disk I/O starts.
#[derive(Debug, Clone)]
pub struct ExportStoryInput {
    pub detail: StoryDetailDto,
    pub destination_path: PathBuf,
}

/// Result returned by a successful export. Echoes enough of the committed
/// state back to the UI so the success surface can display the path and
/// the preserved checksum without issuing a second read.
#[derive(Debug, Clone)]
pub struct ExportStoryOutput {
    pub destination_path: String,
    pub bytes_written: u64,
    pub content_checksum: String,
}

/// Build a Rustory v1 artifact from an already-loaded `StoryDetailDto`
/// and write it atomically at `destination_path`. The SQLite row is
/// NEVER mutated — this operation is strictly write-to-disk. The DB
/// handle is intentionally absent from the signature so the caller can
/// (and does) release the `Mutex<DbHandle>` before the disk I/O starts.
///
/// Atomicity is enforced by writing through a co-located `NamedTempFile`
/// and promoting it with `persist()`, which lowers to a single `rename()`
/// syscall on POSIX filesystems. Any failure along the way drops the
/// temp file, so no residual `.tmp*` survives a crash.
pub fn export_story(input: ExportStoryInput) -> Result<ExportStoryOutput, AppError> {
    let ExportStoryInput {
        detail,
        destination_path,
    } = input;

    let parent = destination_path.parent().ok_or_else(|| {
        AppError::export_destination_unavailable(
            "Chemin d'export invalide: le dossier cible est introuvable.",
            "Choisis un emplacement sous un dossier existant et réessaie.",
        )
        .with_details(serde_json::json!({
            "source": "invalid_path",
            "kind": "invalid_input",
            "cause": "no_parent",
        }))
    })?;
    // The parent existence/kind check lives in `NamedTempFile::new_in`
    // below: if the directory is missing at the moment we create the
    // temp file, `io::ErrorKind::NotFound` maps to `source="parent_missing"`.
    // Relying on the syscall itself avoids a TOCTOU race — the previous
    // `parent.is_dir()` pre-check could observe a live directory that
    // another process then deletes before `persist()`.

    let artifact = RustoryArtifactV1 {
        rustory_artifact: ArtifactEnvelopeV1 {
            format_version: RUSTORY_ARTIFACT_FORMAT_VERSION,
            exported_at: now_iso_ms()?,
            exported_by: format!("rustory/{}", env!("CARGO_PKG_VERSION")),
        },
        story: ExportedStoryV1 {
            schema_version: detail.schema_version,
            title: detail.title.clone(),
            structure_json: detail.structure_json.clone(),
            content_checksum: detail.content_checksum.clone(),
            created_at: detail.created_at.clone(),
            updated_at: detail.updated_at.clone(),
        },
    };

    let bytes = artifact.to_canonical_json()?;
    let bytes_written = bytes.len() as u64;

    write_artifact_atomically(parent, &destination_path, &bytes)?;

    // Echo the validated destination path as-is. Canonicalization would
    // resolve symlinks (leaking the real target) and produce the
    // Windows `\\?\` UNC form (confusing UX) — neither is desirable as
    // a confirmation string.
    Ok(ExportStoryOutput {
        destination_path: destination_path.to_string_lossy().into_owned(),
        bytes_written,
        content_checksum: detail.content_checksum,
    })
}

/// Stage the artifact into a co-located temp file, flush + fsync the
/// bytes, then promote via an atomic rename. Any intermediate failure
/// drops the `NamedTempFile`, removing the half-written file.
fn write_artifact_atomically(
    parent: &Path,
    destination: &Path,
    bytes: &[u8],
) -> Result<(), AppError> {
    let temp_parent: &Path = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };

    let mut temp = NamedTempFile::new_in(temp_parent).map_err(|err| {
        // ENOENT at temp-create time means the parent directory did
        // not exist when we reached for it — surface that as
        // `parent_missing` rather than the generic `temp_create`, so
        // the UI can route its canonical message. Any other kind
        // keeps the structural `temp_create` source.
        let source = if err.kind() == std::io::ErrorKind::NotFound {
            "parent_missing"
        } else {
            "temp_create"
        };
        AppError::export_destination_unavailable(
            export_io_message(&err),
            export_io_user_action(&err),
        )
        .with_details(serde_json::json!({
            "source": source,
            "kind": io_error_kind_tag(&err),
        }))
    })?;

    temp.write_all(bytes).map_err(|err| {
        AppError::export_destination_unavailable(
            export_io_message(&err),
            export_io_user_action(&err),
        )
        .with_details(serde_json::json!({
            "source": "write_temp",
            "kind": io_error_kind_tag(&err),
        }))
    })?;

    temp.flush().map_err(|err| {
        AppError::export_destination_unavailable(
            export_io_message(&err),
            export_io_user_action(&err),
        )
        .with_details(serde_json::json!({
            "source": "write_temp",
            "kind": io_error_kind_tag(&err),
            "stage": "flush",
        }))
    })?;

    temp.as_file().sync_all().map_err(|err| {
        AppError::export_destination_unavailable(
            export_io_message(&err),
            export_io_user_action(&err),
        )
        .with_details(serde_json::json!({
            "source": "write_temp",
            "kind": io_error_kind_tag(&err),
            "stage": "sync_all",
        }))
    })?;

    // Final-stage symlink check: even though the command layer rejects
    // a symlink destination upfront, the filesystem could have been
    // manipulated between validation and persist. Refusing here too
    // closes the TOCTOU window.
    if let Ok(meta) = std::fs::symlink_metadata(destination) {
        if meta.file_type().is_symlink() {
            return Err(AppError::export_destination_unavailable(
                "Le chemin d'export pointe sur un lien symbolique.",
                "Choisis un emplacement qui n'est pas un lien symbolique puis réessaie.",
            )
            .with_details(serde_json::json!({
                "source": "rename",
                "kind": "invalid_input",
                "cause": "symlink_destination",
            })));
        }
    }

    temp.persist(destination).map_err(|err| {
        AppError::export_destination_unavailable(
            export_io_message(&err.error),
            export_io_user_action(&err.error),
        )
        .with_details(serde_json::json!({
            "source": "rename",
            "kind": io_error_kind_tag(&err.error),
        }))
    })?;

    Ok(())
}

/// Pick a user-facing message from a canonical, closed table. The raw OS
/// message is NEVER forwarded — it may include the absolute destination
/// path (PII) or locale-specific wording.
fn export_io_message(err: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match err.kind() {
        PermissionDenied => "Écriture refusée par le système pour ce dossier.",
        StorageFull => "Espace disque insuffisant pour écrire l'artefact.",
        ReadOnlyFilesystem => "Le dossier sélectionné est en lecture seule.",
        NotFound => "Le dossier sélectionné n'existe plus.",
        _ => "Une erreur d'écriture est survenue pendant l'export.",
    }
}

fn export_io_user_action(err: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match err.kind() {
        PermissionDenied => "Choisis un dossier où tu as les droits en écriture.",
        StorageFull => "Libère de l'espace disque ou choisis un autre emplacement.",
        ReadOnlyFilesystem => "Sélectionne un dossier autorisé en écriture.",
        NotFound => "Sélectionne un dossier existant puis relance l'export.",
        _ => "Choisis un autre emplacement puis réessaie l'export.",
    }
}

/// Short, PII-free `details.kind` tag. Mirrors the tagging used by the
/// app-data-dir probe so support tooling can triage with the same vocabulary.
fn io_error_kind_tag(err: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match err.kind() {
        PermissionDenied => "permission_denied",
        StorageFull => "no_space",
        ReadOnlyFilesystem => "read_only_filesystem",
        NotFound => "not_found",
        AlreadyExists => "already_exists",
        _ => "io",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::{create_story, get_story_detail, CreateStoryInput};
    use crate::domain::shared::AppErrorCode;
    use crate::infrastructure::db;
    use crate::infrastructure::db::DbHandle;
    use std::fs;
    use tempfile::TempDir;

    fn fresh_db() -> DbHandle {
        let mut handle = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut handle).expect("migrate");
        handle
    }

    fn load_sample_story(db: &mut DbHandle) -> StoryDetailDto {
        let dto = create_story(
            db,
            CreateStoryInput {
                title: "Le Soleil Couchant".into(),
            },
        )
        .expect("create story");
        get_story_detail(db, &dto.id)
            .expect("read detail")
            .expect("detail present")
    }

    fn destination_in(dir: &TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    #[test]
    fn export_writes_canonical_json_at_destination() {
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let target_dir = TempDir::new().expect("tempdir");
        let destination = destination_in(&target_dir, "histoire.rustory");

        let output = export_story(ExportStoryInput {
            detail: detail.clone(),
            destination_path: destination.clone(),
        })
        .expect("export");

        assert!(destination.exists(), "artifact file must exist");
        let bytes = fs::read(&destination).expect("read artifact");
        let parsed: RustoryArtifactV1 =
            serde_json::from_slice(&bytes).expect("artifact must parse");
        assert_eq!(
            parsed.rustory_artifact.format_version,
            RUSTORY_ARTIFACT_FORMAT_VERSION
        );
        assert_eq!(parsed.story.title, "Le Soleil Couchant");
        assert_eq!(output.bytes_written as usize, bytes.len());
    }

    #[test]
    fn export_preserves_structure_json_byte_for_byte() {
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let target_dir = TempDir::new().expect("tempdir");
        let destination = destination_in(&target_dir, "histoire.rustory");

        export_story(ExportStoryInput {
            detail: detail.clone(),
            destination_path: destination.clone(),
        })
        .expect("export");

        let bytes = fs::read(&destination).expect("read artifact");
        let parsed: RustoryArtifactV1 = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(parsed.story.structure_json, detail.structure_json);
    }

    #[test]
    fn export_recopies_content_checksum_without_recomputation() {
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let target_dir = TempDir::new().expect("tempdir");
        let destination = destination_in(&target_dir, "histoire.rustory");

        let output = export_story(ExportStoryInput {
            detail: detail.clone(),
            destination_path: destination.clone(),
        })
        .expect("export");

        assert_eq!(output.content_checksum, detail.content_checksum);

        let bytes = fs::read(&destination).expect("read artifact");
        let parsed: RustoryArtifactV1 = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(parsed.story.content_checksum, detail.content_checksum);
    }

    #[test]
    fn export_sets_exported_at_to_iso8601_z() {
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let target_dir = TempDir::new().expect("tempdir");
        let destination = destination_in(&target_dir, "histoire.rustory");

        export_story(ExportStoryInput {
            detail,
            destination_path: destination.clone(),
        })
        .expect("export");

        let bytes = fs::read(&destination).expect("read artifact");
        let parsed: RustoryArtifactV1 = serde_json::from_slice(&bytes).expect("parse");
        assert!(
            parsed.rustory_artifact.exported_at.ends_with('Z'),
            "exportedAt must be UTC with Z suffix"
        );
        assert_eq!(parsed.rustory_artifact.exported_at.len(), 24);
    }

    #[test]
    fn export_sets_exported_by_to_rustory_cargo_pkg_version() {
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let target_dir = TempDir::new().expect("tempdir");
        let destination = destination_in(&target_dir, "histoire.rustory");

        export_story(ExportStoryInput {
            detail,
            destination_path: destination.clone(),
        })
        .expect("export");

        let bytes = fs::read(&destination).expect("read artifact");
        let parsed: RustoryArtifactV1 = serde_json::from_slice(&bytes).expect("parse");
        let expected = format!("rustory/{}", env!("CARGO_PKG_VERSION"));
        assert_eq!(parsed.rustory_artifact.exported_by, expected);
    }

    #[test]
    fn export_does_not_mutate_stories_row() {
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let before: (String, String, String) = db
            .conn()
            .query_row(
                "SELECT title, updated_at, content_checksum FROM stories WHERE id = ?1",
                rusqlite::params![&detail.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("row");

        let target_dir = TempDir::new().expect("tempdir");
        let destination = destination_in(&target_dir, "histoire.rustory");

        export_story(ExportStoryInput {
            detail: detail.clone(),
            destination_path: destination,
        })
        .expect("export");

        let after: (String, String, String) = db
            .conn()
            .query_row(
                "SELECT title, updated_at, content_checksum FROM stories WHERE id = ?1",
                rusqlite::params![&detail.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("row");

        assert_eq!(
            before, after,
            "canonical row must be invariant under export"
        );
    }

    #[test]
    fn export_returns_destination_unavailable_when_parent_missing() {
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let target_dir = TempDir::new().expect("tempdir");
        let ghost_parent = target_dir.path().join("missing-subdir");
        let destination = ghost_parent.join("histoire.rustory");

        let err = export_story(ExportStoryInput {
            detail,
            destination_path: destination,
        })
        .expect_err("must fail");

        assert_eq!(err.code, AppErrorCode::ExportDestinationUnavailable);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "parent_missing");
    }

    #[test]
    fn export_returns_destination_path_as_input_without_canonicalization() {
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let target_dir = TempDir::new().expect("tempdir");
        let destination = destination_in(&target_dir, "histoire.rustory");

        let output = export_story(ExportStoryInput {
            detail,
            destination_path: destination.clone(),
        })
        .expect("export");

        // Output echoes the input path as-is. Canonicalization would
        // surface the Windows `\\?\` prefix and resolve symlinks —
        // neither is wanted in the confirmation surface.
        assert_eq!(output.destination_path, destination.to_string_lossy());
    }

    /// Check whether the current process actually honors filesystem
    /// permission bits. CI containers often run as UID 0, which bypasses
    /// DAC checks entirely, so a `chmod 500` probe test would silently
    /// succeed there despite our production code doing the right thing.
    /// When this helper returns false we skip the permission-driven
    /// failure branches and trust the corresponding branches of the
    /// `io_error_kind_tag` unit tests.
    #[cfg(unix)]
    fn permissions_are_honored(dir: &std::path::Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        let probe_dir = dir.join("probe");
        if fs::create_dir(&probe_dir).is_err() {
            return false;
        }
        if fs::set_permissions(&probe_dir, fs::Permissions::from_mode(0o500)).is_err() {
            let _ = fs::remove_dir_all(&probe_dir);
            return false;
        }
        let probe_file = probe_dir.join("x");
        let writable = fs::write(&probe_file, b"x").is_ok();
        let _ = fs::set_permissions(&probe_dir, fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&probe_dir);
        !writable
    }

    #[cfg(unix)]
    #[test]
    fn export_returns_destination_unavailable_on_permission_denied_parent() {
        use std::os::unix::fs::PermissionsExt;

        let target_dir = TempDir::new().expect("tempdir");
        if !permissions_are_honored(target_dir.path()) {
            eprintln!("skipping permission probe test — current euid bypasses DAC checks");
            return;
        }

        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let locked = target_dir.path().join("locked");
        fs::create_dir(&locked).expect("mkdir locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500))
            .expect("chmod read+exec only");

        let destination = locked.join("histoire.rustory");
        let err = export_story(ExportStoryInput {
            detail,
            destination_path: destination,
        })
        .expect_err("must fail");

        // Restore permissions so TempDir can clean up on drop.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).ok();

        assert_eq!(err.code, AppErrorCode::ExportDestinationUnavailable);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "temp_create");
        assert_eq!(details["kind"], "permission_denied");
    }

    #[cfg(unix)]
    #[test]
    fn export_cleans_up_temp_file_on_write_failure() {
        use std::os::unix::fs::PermissionsExt;

        let target_dir = TempDir::new().expect("tempdir");
        if !permissions_are_honored(target_dir.path()) {
            eprintln!("skipping permission probe test — current euid bypasses DAC checks");
            return;
        }

        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let locked = target_dir.path().join("locked");
        fs::create_dir(&locked).expect("mkdir");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).expect("chmod read-only");

        let destination = locked.join("histoire.rustory");
        let _ = export_story(ExportStoryInput {
            detail,
            destination_path: destination,
        })
        .expect_err("must fail");

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).ok();

        // Directory must contain NO residual temp file (NamedTempFile::drop
        // removed it on error).
        let leftovers: Vec<_> = fs::read_dir(&locked)
            .expect("read locked dir")
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp file must be cleaned up after export failure: {leftovers:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn export_refuses_symlink_destination_at_persist_time() {
        // Defense-in-depth: even though `commands::import_export`
        // pre-validates and rejects symlinks BEFORE the service runs,
        // the service-level final-stage check inside
        // `write_artifact_atomically` exists to close the TOCTOU
        // window — the symlink could be created between command-side
        // validation and the persist syscall. This test bypasses the
        // command layer to exercise that final-stage guard in
        // isolation.
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let target_dir = TempDir::new().expect("tempdir");
        let real = target_dir.path().join("real.rustory");
        fs::write(&real, b"placeholder").expect("seed real");
        let link = target_dir.path().join("link.rustory");
        std::os::unix::fs::symlink(&real, &link).expect("mklink");

        let err = export_story(ExportStoryInput {
            detail,
            destination_path: link,
        })
        .expect_err("must fail at the persist-time symlink check");

        assert_eq!(err.code, AppErrorCode::ExportDestinationUnavailable);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "rename");
        assert_eq!(details["cause"], "symlink_destination");

        // The original `real` file must remain untouched — the symlink
        // refusal must NOT have followed the link to write through it.
        let real_bytes = fs::read(&real).expect("read real after refusal");
        assert_eq!(real_bytes, b"placeholder");
    }

    #[test]
    fn export_idempotent_on_rewrite_same_destination() {
        let mut db = fresh_db();
        let detail = load_sample_story(&mut db);
        let target_dir = TempDir::new().expect("tempdir");
        let destination = destination_in(&target_dir, "histoire.rustory");

        export_story(ExportStoryInput {
            detail: detail.clone(),
            destination_path: destination.clone(),
        })
        .expect("first export");

        // Mutate the file on disk so we can detect the overwrite.
        fs::write(&destination, b"not-a-rustory-artifact").expect("mutate");

        export_story(ExportStoryInput {
            detail,
            destination_path: destination.clone(),
        })
        .expect("second export");

        let bytes = fs::read(&destination).expect("read");
        let parsed: RustoryArtifactV1 =
            serde_json::from_slice(&bytes).expect("second export must produce valid artifact");
        assert_eq!(parsed.story.title, "Le Soleil Couchant");
    }
}
