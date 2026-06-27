//! Device-story import application service.
//!
//! Owns the full acquisition sequence of "Copier dans ma bibliothèque":
//!
//! 1. duplicate guard (scoped DB lock, released before any device I/O),
//! 2. authoritative re-scan at engagement time (identity + capability
//!    gate — the inspection snapshot is never trusted),
//! 3. pack re-verification against the live index (`.pi` AND
//!    `.pi.hidden` — a hidden story is importable),
//! 4. bounded acquisition into a staging tempdir,
//! 5. atomic promotion (`rename` within the same filesystem),
//! 6. canonical commit (one `stories` row strictly conforming to the
//!    `create_story` model + one `story_imports` provenance row) with
//!    compensation: a commit failure removes the promoted folder.
//!
//! Invariant: FILES FIRST, DB SECOND. A `stories` row must never
//! reference pack files that are not known to exist and be valid; the
//! inverse (a promoted folder without its row) is an orphan removed by
//! compensation or by the boot sweep ([`sweep_import_artifacts`]).
//!
//! The device mount is never written. The DB mutex is never held across
//! device I/O.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::OptionalExtension;

use crate::application::story::now_iso_ms;
use crate::domain::device::pack::imported_story_title;
use crate::domain::device::{DeviceStoryEntry, SupportedOperation};
use crate::domain::shared::AppError;
use crate::domain::story::{
    canonical_structure_json, content_checksum, map_error, normalize_title, validate_title,
    CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::device::{
    AcquiredPack, DeviceLibraryReader, DevicePackReader, DeviceScanner,
};
use crate::infrastructure::filesystem::{ensure_import_store, resolve_imports_staging_dir};
use crate::ipc::dto::StoryCardDto;

use super::{check_operation_allowed, resolve_connected_lunii, ConnectedLuniiOutcome};

/// Input of [`import_device_story`]. Both identifiers are validated at
/// the IPC boundary (32-hex device id, canonical lowercase pack UUID).
#[derive(Debug, Clone)]
pub struct ImportDeviceStoryRequest {
    pub device_identifier: String,
    pub pack_uuid: String,
}

/// Result of a successful import, echoed to the UI so the success
/// surface can name the created draft without a second read. The byte /
/// file counts feed the diagnostic event.
#[derive(Debug, Clone)]
pub struct ImportedDeviceStory {
    pub story: StoryCardDto,
    pub pack_short_id: String,
    pub imported_at: String,
    pub pack_file_count: u32,
    pub pack_total_bytes: u64,
}

/// Run the full import sequence. Synchronous by design: the command
/// layer hands it to `spawn_blocking` whole, so no `MutexGuard` ever
/// lives across an `await`.
pub fn import_device_story(
    db: &Mutex<DbHandle>,
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    pack_reader: &dyn DevicePackReader,
    app_data_dir: &Path,
    request: &ImportDeviceStoryRequest,
    budget: Duration,
) -> Result<ImportedDeviceStory, AppError> {
    let started = Instant::now();
    let remaining = |started: Instant| budget.saturating_sub(started.elapsed());

    // 1. Duplicate guard — scoped lock, released before any device I/O.
    {
        let db = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if find_existing_import(&db, &request.pack_uuid)?.is_some() {
            return Err(already_imported_error());
        }
    }

    // 2. Authoritative re-scan at engagement time. The UI snapshot the
    //    user inspected is NOT trusted: identity and capability are
    //    re-proven against the live device.
    let resolved = resolve_connected_lunii(scanner, remaining(started))?;
    let (profile, mount_path) = match resolved.outcome {
        ConnectedLuniiOutcome::Supported(profile) => {
            if profile.device_identifier != request.device_identifier {
                return Err(device_changed_error("identifier_mismatch"));
            }
            let mount = resolved
                .supported_mount_path
                .ok_or_else(|| device_changed_error("mount_unavailable"))?;
            (profile, mount)
        }
        ConnectedLuniiOutcome::None => return Err(device_changed_error("device_absent")),
        ConnectedLuniiOutcome::Unsupported { .. } => {
            return Err(device_changed_error("device_unsupported"))
        }
        ConnectedLuniiOutcome::Ambiguous { .. } => {
            return Err(device_changed_error("multiple_candidates"))
        }
    };
    // Fail-closed gate BEFORE any acquisition (NFR17/NFR18). V3 refuses
    // here with the existing DEVICE_UNSUPPORTED / capability_gate error.
    check_operation_allowed(&profile, SupportedOperation::ImportStory)?;

    // 3. Pack re-verification against the live index (visible AND
    //    hidden — a `Masquée` story is importable).
    let library = library_reader.read_library(&mount_path, remaining(started))?;
    let entry: &DeviceStoryEntry = library
        .entries
        .iter()
        .find(|e| e.uuid == request.pack_uuid)
        .ok_or_else(pack_missing_error)?;
    if !entry.content_present {
        return Err(pack_missing_error());
    }
    let short_id = entry.short_id.clone();

    // 4. Bounded acquisition into a staging tempdir. Any failure from
    //    here drops the TempDir, which removes the partial copy.
    let (imports_dir, _staging_root) = ensure_import_store(app_data_dir)?;
    let staging = tempfile::tempdir_in(resolve_imports_staging_dir(app_data_dir))
        .map_err(|err| staging_create_error(&err))?;
    let acquired =
        pack_reader.acquire_pack(&mount_path, &short_id, staging.path(), remaining(started))?;

    // 5. Atomic promotion. `imports/.staging` lives inside `imports/`,
    //    so the rename never crosses a filesystem boundary.
    let story_id = uuid::Uuid::now_v7().to_string();
    let target = imports_dir.join(&story_id);
    std::fs::rename(staging.path(), &target).map_err(|err| promote_error(&err))?;
    // The staging path no longer exists; the TempDir drop is a no-op.

    // Durability of the promotion BEFORE the DB references it: the file
    // CONTENTS were fsynced during the copy, but the directory ENTRIES
    // (created subdirs, and the rename in `imports/`) live in their
    // parent directories' data. Fsync the promoted tree and `imports/`
    // itself so a power loss after the SQLite commit cannot resurrect a
    // row pointing at entries the filesystem never persisted. A failure
    // compensates like any promotion failure (remove + explicit error).
    if let Err(err) = fsync_promoted_tree(&imports_dir, &target) {
        let _ = std::fs::remove_dir_all(&target);
        return Err(err);
    }

    // 6. Canonical commit — files are promoted and valid, the DB may now
    //    reference them. A failure here compensates by removing the
    //    promoted folder so no orphan survives.
    let committed = {
        let mut db = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        commit_imported_story(
            &mut db,
            &story_id,
            &short_id,
            &request.pack_uuid,
            &request.device_identifier,
            &acquired,
        )
    };
    match committed {
        Ok(outcome) => Ok(outcome),
        Err(err) => {
            // Compensation: best-effort removal; a leftover is caught by
            // the boot sweep, never by a dangling DB row (none exists).
            let _ = std::fs::remove_dir_all(&target);
            Err(err)
        }
    }
}

/// Fsync the promoted directory tree (directories only — file contents
/// were already fsynced during the copy) and the `imports/` parent that
/// records the rename. Directory fsync is the documented POSIX way to
/// persist directory entries; opening a directory read-only for
/// `sync_all` is supported on every Tauri desktop target.
fn fsync_promoted_tree(imports_dir: &Path, target: &Path) -> Result<(), AppError> {
    fn fsync_dir(path: &Path) -> std::io::Result<()> {
        std::fs::File::open(path)?.sync_all()
    }
    fn walk(dir: &Path) -> std::io::Result<()> {
        fsync_dir(dir)?;
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(&path)?;
            }
        }
        Ok(())
    }
    walk(target)
        .and_then(|_| fsync_dir(imports_dir))
        .map_err(|err| {
            AppError::import_failed(
                "Copie impossible: finalisation locale refusée.",
                "Vérifie l'espace disque et les permissions puis réessaie.",
            )
            .with_details(serde_json::json!({
                "source": "promote",
                "stage": "fsync_dirs",
                "kind": crate::infrastructure::filesystem::io_error_kind_tag(&err),
            }))
        })
}

fn find_existing_import(db: &DbHandle, pack_uuid: &str) -> Result<Option<String>, AppError> {
    db.conn()
        .query_row(
            "SELECT story_id FROM story_imports WHERE pack_uuid = ?1",
            rusqlite::params![pack_uuid],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| db_commit_error(&err, "duplicate_guard"))
}

/// Insert the canonical `stories` row + the `story_imports` provenance
/// row in one `BEGIN IMMEDIATE` transaction. Factored out so the
/// UNIQUE-race branch (two imports of the same pack racing past the
/// duplicate guard) is directly testable.
fn commit_imported_story(
    db: &mut DbHandle,
    story_id: &str,
    short_id: &str,
    pack_uuid: &str,
    device_identifier: &str,
    acquired: &AcquiredPack,
) -> Result<ImportedDeviceStory, AppError> {
    // The default title is re-validated authoritatively, exactly like a
    // user-typed one — a generator drift must fail here, not at a CHECK.
    let title = normalize_title(&imported_story_title(short_id));
    validate_title(&title).map_err(map_error)?;

    let structure = CanonicalStructure::minimal();
    let structure_json = canonical_structure_json(&structure);
    let checksum = content_checksum(&structure_json);
    let now_iso = now_iso_ms()?;

    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| db_commit_error(&err, "begin_transaction"))?;

    tx.execute(
        "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![
            story_id,
            &title,
            CANONICAL_STORY_SCHEMA_VERSION,
            &structure_json,
            &checksum,
            &now_iso,
        ],
    )
    .map_err(|err| db_commit_error(&err, "insert_story"))?;

    tx.execute(
        "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            story_id,
            pack_uuid,
            device_identifier,
            &now_iso,
            acquired.manifest.files.len() as u32,
            acquired.manifest.total_bytes,
            &acquired.checksum,
        ],
    )
    .map_err(|err| {
        // A UNIQUE violation on pack_uuid means another import committed
        // the same pack between the duplicate guard and this commit —
        // surface the same canonical refusal as the guard (fail-closed).
        // Match the EXTENDED code (`SQLITE_CONSTRAINT_UNIQUE`) only: a
        // CHECK/FK failure also carries the generic `ConstraintViolation`
        // code but is a genuine `db_commit` error, never a duplicate.
        if is_unique_violation(&err) {
            already_imported_error()
        } else {
            db_commit_error(&err, "insert_provenance")
        }
    })?;

    tx.commit().map_err(|err| db_commit_error(&err, "commit"))?;

    Ok(ImportedDeviceStory {
        story: StoryCardDto::native(story_id.to_string(), title),
        pack_short_id: short_id.to_string(),
        imported_at: now_iso,
        pack_file_count: acquired.manifest.files.len() as u32,
        pack_total_bytes: acquired.manifest.total_bytes,
    })
}

/// True ONLY for a UNIQUE-constraint violation (SQLite extended code
/// `SQLITE_CONSTRAINT_UNIQUE` = 2067). A CHECK or FK violation also maps
/// to the generic `ConstraintViolation` primary code, so matching the
/// primary code alone would wrongly report e.g. a malformed `pack_uuid`
/// / `pack_checksum` (the `story_imports` CHECKs) as `already_imported`.
/// Those are unreachable through the command boundary (`validate_import_input`
/// guarantees a canonical pack UUID, and counts/checksum are computed
/// internally), so this is defense-in-depth for direct service callers
/// (tests, smoke, future entry points).
fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(code, _)
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}

/// Outcome of the best-effort boot sweep. Counts are diagnostic only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportSweepOutcome {
    pub staging_entries_removed: u32,
    pub orphan_dirs_removed: u32,
}

/// Boot-time sweep of import residues. Removes (a) anything left in
/// `imports/.staging/` (crash mid-acquisition) and (b) any promoted
/// `imports/<id>` folder with no matching `story_imports` row (crash
/// between promotion and commit, or post-compensation leftovers).
/// Best-effort by contract: an unremovable entry is skipped, never a
/// boot failure.
pub fn sweep_import_artifacts(
    db: &DbHandle,
    app_data_dir: &Path,
) -> Result<ImportSweepOutcome, AppError> {
    let mut outcome = ImportSweepOutcome::default();
    let imports_dir = crate::infrastructure::filesystem::resolve_imports_dir(app_data_dir);
    if !imports_dir.is_dir() {
        return Ok(outcome);
    }

    let staging_root = resolve_imports_staging_dir(app_data_dir);
    if staging_root.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&staging_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let removed = if path.is_dir() {
                    std::fs::remove_dir_all(&path).is_ok()
                } else {
                    std::fs::remove_file(&path).is_ok()
                };
                if removed {
                    outcome.staging_entries_removed += 1;
                }
            }
        }
    }

    let known: std::collections::HashSet<String> = {
        let mut stmt = db
            .conn()
            .prepare("SELECT story_id FROM story_imports")
            .map_err(|err| db_commit_error(&err, "sweep_select"))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|err| db_commit_error(&err, "sweep_query"))?;
        rows.filter_map(Result::ok).collect()
    };

    if let Ok(entries) = std::fs::read_dir(&imports_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name == crate::infrastructure::filesystem::IMPORTS_STAGING_DIR_NAME {
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            if !known.contains(name) && std::fs::remove_dir_all(&path).is_ok() {
                outcome.orphan_dirs_removed += 1;
            }
        }
    }

    Ok(outcome)
}

// Closed user-facing copy — sober, no OS message, no path (PII rules).

fn already_imported_error() -> AppError {
    AppError::import_failed(
        "Copie impossible: cette histoire est déjà dans ta bibliothèque.",
        "Retrouve-la dans ta bibliothèque locale ; aucune nouvelle copie n'est nécessaire.",
    )
    .with_details(serde_json::json!({
        "source": "already_imported",
    }))
}

fn device_changed_error(cause: &'static str) -> AppError {
    AppError::import_failed(
        "Copie impossible: l'appareil connecté a changé.",
        "Rebranche la Lunii souhaitée puis réessaie la copie.",
    )
    .with_details(serde_json::json!({
        "source": "device_changed",
        "cause": cause,
    }))
}

fn pack_missing_error() -> AppError {
    AppError::import_failed(
        "Copie impossible: l'histoire est introuvable sur l'appareil.",
        "Vérifie l'appareil puis relance la lecture de sa bibliothèque.",
    )
    .with_details(serde_json::json!({
        "source": "pack_missing",
    }))
}

fn staging_create_error(err: &std::io::Error) -> AppError {
    AppError::import_failed(
        "Copie impossible: écriture locale refusée.",
        "Vérifie l'espace disque et les permissions puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "staging_write",
        "kind": crate::infrastructure::filesystem::io_error_kind_tag(err),
    }))
}

fn promote_error(err: &std::io::Error) -> AppError {
    AppError::import_failed(
        "Copie impossible: finalisation locale refusée.",
        "Vérifie l'espace disque et les permissions puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "promote",
        "kind": crate::infrastructure::filesystem::io_error_kind_tag(err),
    }))
}

fn db_commit_error(err: &rusqlite::Error, stage: &'static str) -> AppError {
    // PII discipline: no raw rusqlite message (it can embed table names
    // or filesystem detail) — a stable stage + coarse kind suffice.
    let kind = match err {
        rusqlite::Error::SqliteFailure(code, _) => match code.code {
            rusqlite::ErrorCode::ConstraintViolation => "constraint_violation",
            rusqlite::ErrorCode::DatabaseBusy => "busy",
            rusqlite::ErrorCode::DatabaseLocked => "locked",
            _ => "other",
        },
        _ => "other",
    };
    AppError::import_failed(
        "Copie impossible: enregistrement local refusé.",
        "Réessaie ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "db_commit",
        "stage": stage,
        "kind": kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::get_story_detail;
    use crate::infrastructure::db;
    use crate::infrastructure::device::fixtures::temp_lunii_mount_with_pack_content;
    use crate::infrastructure::device::{
        compute_device_identifier, DeviceCandidate, DeviceScanReport, MockDeviceLibraryReader,
        MockDevicePackReader, MockDeviceScanner, SystemDevicePackReader,
    };
    use std::path::PathBuf;
    use tempfile::TempDir;

    const PACK_UUID: &str = "abababab-abab-abab-abab-ababfac5562d";
    const SHORT_ID: &str = "FAC5562D";

    fn fresh_db() -> Mutex<DbHandle> {
        let mut handle = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut handle).expect("migrate");
        Mutex::new(handle)
    }

    fn budget() -> Duration {
        Duration::from_secs(30)
    }

    /// Identifier produced by `enqueue_supported_lunii` (`.pi` = MOCK_PI,
    /// serial = MOCK_SERIAL).
    fn mock_identifier() -> String {
        compute_device_identifier(b"MOCK_PI", Some("MOCK_SERIAL"))
    }

    fn request() -> ImportDeviceStoryRequest {
        ImportDeviceStoryRequest {
            device_identifier: mock_identifier(),
            pack_uuid: PACK_UUID.into(),
        }
    }

    fn library_entry(content_present: bool, hidden: bool) -> crate::domain::device::DeviceLibrary {
        crate::domain::device::DeviceLibrary {
            entries: vec![DeviceStoryEntry {
                uuid: PACK_UUID.into(),
                short_id: SHORT_ID.into(),
                hidden,
                content_present,
            }],
            had_trailing_bytes: false,
        }
    }

    struct Harness {
        db: Mutex<DbHandle>,
        scanner: MockDeviceScanner,
        library: MockDeviceLibraryReader,
        packs: MockDevicePackReader,
        app_data: TempDir,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                db: fresh_db(),
                scanner: MockDeviceScanner::new(),
                library: MockDeviceLibraryReader::new(),
                packs: MockDevicePackReader::new(),
                app_data: TempDir::new().expect("app data tempdir"),
            }
        }

        fn run(&self, request: &ImportDeviceStoryRequest) -> Result<ImportedDeviceStory, AppError> {
            import_device_story(
                &self.db,
                &self.scanner,
                &self.library,
                &self.packs,
                self.app_data.path(),
                request,
                budget(),
            )
        }

        fn imports_dir(&self) -> PathBuf {
            crate::infrastructure::filesystem::resolve_imports_dir(self.app_data.path())
        }

        fn staging_dir(&self) -> PathBuf {
            resolve_imports_staging_dir(self.app_data.path())
        }

        fn staging_is_empty(&self) -> bool {
            !self.staging_dir().is_dir()
                || std::fs::read_dir(self.staging_dir())
                    .expect("read staging")
                    .next()
                    .is_none()
        }

        fn story_rows(&self) -> u32 {
            let db = self.db.lock().expect("lock");
            db.conn()
                .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
                .expect("count stories")
        }

        fn import_rows(&self) -> u32 {
            let db = self.db.lock().expect("lock");
            db.conn()
                .query_row("SELECT COUNT(*) FROM story_imports", [], |row| row.get(0))
                .expect("count imports")
        }
    }

    /// Discipline: every import-refusal constructor must be ACTIONABLE —
    /// a non-empty cause AND a non-empty next gesture — so the UI never
    /// surfaces an opaque refusal (AC1, ui-states.md#Device Story Import
    /// Contract → actionability rule). No new error code / source: this
    /// only locks the canonical fr copy the existing constructors carry.
    #[test]
    fn every_import_refusal_constructor_is_actionable() {
        let io_err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let sqlite_err = rusqlite::Error::QueryReturnedNoRows;
        let refusals = [
            already_imported_error(),
            device_changed_error("identifier_mismatch"),
            pack_missing_error(),
            staging_create_error(&io_err),
            promote_error(&io_err),
            db_commit_error(&sqlite_err, "stories"),
        ];
        for err in &refusals {
            assert_eq!(
                err.code,
                crate::domain::shared::AppErrorCode::ImportFailed,
                "{err:?}"
            );
            assert!(!err.message.is_empty(), "refusal needs a cause: {err:?}");
            let action = err.user_action.as_deref().unwrap_or("");
            assert!(!action.is_empty(), "refusal needs a next gesture: {err:?}");
        }
    }

    #[test]
    fn imports_a_device_story_end_to_end() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        h.library.enqueue(Ok(library_entry(true, false)));
        h.packs.enqueue_success();

        let outcome = h.run(&request()).expect("import");

        assert_eq!(outcome.story.title, "Histoire de ma Lunii (FAC5562D)");
        assert_eq!(outcome.pack_short_id, SHORT_ID);
        assert_eq!(outcome.pack_file_count, 5);
        assert!(outcome.imported_at.ends_with('Z'));

        // Canonical row strictly conforming to the create_story model.
        let db = h.db.lock().expect("lock");
        let detail = get_story_detail(&db, &outcome.story.id)
            .expect("read detail")
            .expect("row present");
        assert_eq!(detail.schema_version, 1);
        assert_eq!(detail.structure_json, "{\"schemaVersion\":1,\"nodes\":[]}");
        assert_eq!(detail.content_checksum.len(), 64);
        assert_eq!(detail.created_at, detail.updated_at);

        // Provenance row.
        let (pack_uuid, source_id, file_count, total_bytes, checksum): (String, String, u32, u64, String) = db
            .conn()
            .query_row(
                "SELECT pack_uuid, source_device_identifier, pack_file_count, pack_total_bytes, pack_checksum \
                 FROM story_imports WHERE story_id = ?1",
                rusqlite::params![&outcome.story.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .expect("provenance row");
        assert_eq!(pack_uuid, PACK_UUID);
        assert_eq!(source_id, mock_identifier());
        assert_eq!(file_count, 5);
        assert_eq!(total_bytes, 18);
        assert_eq!(checksum.len(), 64);
        drop(db);

        // Files promoted under imports/<story_id>/, staging clean.
        let promoted = h.imports_dir().join(&outcome.story.id);
        assert!(promoted.join("ni").is_file());
        assert!(promoted.join("rf").join("000").join("AAAAAAAA").is_file());
        assert!(h.staging_is_empty());

        // The canonical row is listed exactly like any local draft (the
        // overview query reads this same table ordered by created_at).
        let db = h.db.lock().expect("lock");
        let (listed_id, listed_title): (String, String) = db
            .conn()
            .query_row(
                "SELECT id, title FROM stories ORDER BY created_at ASC, id ASC",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("listed row");
        assert_eq!(listed_id, outcome.story.id);
        assert_eq!(listed_title, outcome.story.title);
    }

    #[test]
    fn refuses_a_pack_already_imported_without_touching_the_device() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        h.library.enqueue(Ok(library_entry(true, false)));
        h.packs.enqueue_success();
        let first = h.run(&request()).expect("first import");

        // No scanner/library/pack enqueue for the second call: reaching
        // the device would surface as a missing-mock panic or a scan of
        // an empty queue ("no device"), so the already_imported refusal
        // proves the guard fired FIRST.
        let err = h.run(&request()).expect_err("second import must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "IMPORT_FAILED");
        assert_eq!(v["details"]["source"], "already_imported");

        // First import's artifacts are intact.
        assert_eq!(h.story_rows(), 1);
        assert_eq!(h.import_rows(), 1);
        assert!(h.imports_dir().join(&first.story.id).join("ni").is_file());
    }

    #[test]
    fn refuses_when_no_device_answers_the_re_scan() {
        let h = Harness::new();
        h.scanner.enqueue_no_device();
        let err = h.run(&request()).expect_err("must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "device_changed");
        assert_eq!(h.story_rows(), 0);
        assert_eq!(h.import_rows(), 0);
    }

    #[test]
    fn refuses_identifier_mismatch_as_device_changed() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        let err = h
            .run(&ImportDeviceStoryRequest {
                device_identifier: "deadbeefdeadbeefdeadbeefdeadbeef".into(),
                pack_uuid: PACK_UUID.into(),
            })
            .expect_err("mismatch must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "device_changed");
        assert_eq!(v["details"]["cause"], "identifier_mismatch");
    }

    #[test]
    fn gate_blocks_import_for_v3_profile() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(7); // V3 — import_story = false
        let err = h
            .run(&ImportDeviceStoryRequest {
                device_identifier: mock_identifier(),
                pack_uuid: PACK_UUID.into(),
            })
            .expect_err("V3 must be gated");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_UNSUPPORTED");
        assert_eq!(v["details"]["source"], "capability_gate");
        assert_eq!(v["details"]["operation"], "import_story");
        assert_eq!(h.story_rows(), 0);
    }

    #[test]
    fn refuses_a_pack_absent_from_the_live_index() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        h.library.enqueue_empty_library();
        let err = h.run(&request()).expect_err("must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_missing");
    }

    #[test]
    fn refuses_a_pack_whose_content_folder_is_absent() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        h.library.enqueue(Ok(library_entry(false, false)));
        let err = h.run(&request()).expect_err("must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_missing");
    }

    #[test]
    fn imports_a_hidden_pack() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        h.library.enqueue(Ok(library_entry(true, true)));
        h.packs.enqueue_success();
        let outcome = h.run(&request()).expect("hidden pack must be importable");
        assert_eq!(outcome.pack_short_id, SHORT_ID);
    }

    #[test]
    fn mid_copy_failure_cleans_staging_and_leaves_no_db_row() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        h.library.enqueue(Ok(library_entry(true, false)));
        h.packs.enqueue_interrupted_mid_copy();

        let err = h.run(&request()).expect_err("mid-copy must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "fs_read");

        assert!(h.staging_is_empty(), "staging residue must be dropped");
        assert_eq!(h.story_rows(), 0, "no stories row may exist");
        assert_eq!(h.import_rows(), 0, "no provenance row may exist");
        // imports/ holds only the (empty) staging dir — no orphan.
        let leftovers: Vec<_> = std::fs::read_dir(h.imports_dir())
            .expect("read imports")
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name() != crate::infrastructure::filesystem::IMPORTS_STAGING_DIR_NAME
            })
            .collect();
        assert!(leftovers.is_empty(), "no orphan dir: {leftovers:?}");
    }

    #[test]
    fn pack_invalid_refusal_stages_nothing_and_leaves_no_row() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        h.library.enqueue(Ok(library_entry(true, false)));
        h.packs.enqueue_pack_invalid();

        let err = h.run(&request()).expect_err("invalid pack must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert!(h.staging_is_empty());
        assert_eq!(h.story_rows(), 0);
    }

    #[test]
    fn db_commit_failure_compensates_by_removing_the_promoted_folder() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        h.library.enqueue(Ok(library_entry(true, false)));
        h.packs.enqueue_success();

        // Sabotage the stories INSERT (and only it) so the commit fails
        // AFTER the promotion succeeded — the duplicate guard upstream
        // keeps working against an intact story_imports table.
        {
            let db = h.db.lock().expect("lock");
            db.conn()
                .execute_batch(
                    "CREATE TRIGGER sabotage_story BEFORE INSERT ON stories \
                     BEGIN SELECT RAISE(ABORT, 'sabotage'); END;",
                )
                .expect("install sabotage trigger");
        }

        let err = h.run(&request()).expect_err("commit must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "db_commit");

        // Compensation removed the promoted folder; the stories INSERT
        // was rolled back with the transaction.
        let leftovers: Vec<_> = std::fs::read_dir(h.imports_dir())
            .expect("read imports")
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name() != crate::infrastructure::filesystem::IMPORTS_STAGING_DIR_NAME
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "promoted dir must be compensated away"
        );
        assert_eq!(h.story_rows(), 0, "stories insert must be rolled back");
    }

    #[test]
    fn unique_race_on_commit_maps_to_already_imported() {
        let h = Harness::new();
        // First commit through the factored helper.
        let acquired = AcquiredPack {
            manifest: MockDevicePackReader::staged_manifest(),
            checksum: "cd".repeat(32),
        };
        {
            let mut db = h.db.lock().expect("lock");
            commit_imported_story(&mut db, "story-1", SHORT_ID, PACK_UUID, "id-1", &acquired)
                .expect("first commit");
            // Second commit with the SAME pack_uuid models the race where
            // both imports passed the duplicate guard before either
            // committed. The UNIQUE index must close it fail-closed.
            let err =
                commit_imported_story(&mut db, "story-2", SHORT_ID, PACK_UUID, "id-1", &acquired)
                    .expect_err("unique race must fail");
            let v = serde_json::to_value(&err).expect("ser");
            assert_eq!(v["details"]["source"], "already_imported");
        }
        assert_eq!(
            h.story_rows(),
            1,
            "the racing stories row must be rolled back"
        );
        assert_eq!(h.import_rows(), 1);
    }

    #[test]
    fn check_violation_on_provenance_insert_maps_to_db_commit_not_already_imported() {
        // Defense-in-depth: a malformed `pack_checksum` (length != 64)
        // trips the story_imports CHECK, which carries the SAME generic
        // `ConstraintViolation` primary code as a UNIQUE conflict. It must
        // surface as `db_commit`, never be mistaken for `already_imported`.
        let h = Harness::new();
        let malformed = AcquiredPack {
            manifest: MockDevicePackReader::staged_manifest(),
            checksum: "tooshort".into(), // length != 64 → CHECK fails
        };
        let err = {
            let mut db = h.db.lock().expect("lock");
            commit_imported_story(
                &mut db,
                "story-bad",
                SHORT_ID,
                PACK_UUID,
                "id-1",
                &malformed,
            )
            .expect_err("CHECK violation must fail")
        };
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "IMPORT_FAILED");
        assert_eq!(v["details"]["source"], "db_commit");
        // The whole transaction rolled back — no half-written stories row.
        assert_eq!(h.story_rows(), 0);
        assert_eq!(h.import_rows(), 0);
    }

    #[test]
    fn zero_remaining_budget_aborts_the_real_acquisition_with_read_timeout() {
        // Mock scan + index (they ignore the budget), REAL pack reader on
        // a REAL mount with an exhausted budget: the deadline must fire
        // inside the acquisition, leaving no artifact anywhere.
        let pack_uuid_bytes: [u8; 16] = [
            0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xfa, 0xc5,
            0x56, 0x2d,
        ];
        let (_guard, mount) = temp_lunii_mount_with_pack_content(3, pack_uuid_bytes);
        let h = Harness::new();
        let report = DeviceScanReport {
            candidates: vec![DeviceCandidate {
                mount_path: mount.clone(),
                metadata_payload: vec![3],
                pi_payload: b"MOCK_PI".to_vec(),
                has_bt: true,
                volume_serial: Some("MOCK_SERIAL".into()),
            }],
            elapsed: Duration::from_millis(1),
            truncated_due_to_timeout: false,
        };
        h.scanner.enqueue(Ok(report));
        h.library.enqueue(Ok(library_entry(true, false)));

        let real_reader = SystemDevicePackReader;
        let err = import_device_story(
            &h.db,
            &h.scanner,
            &h.library,
            &real_reader,
            h.app_data.path(),
            &request(),
            Duration::ZERO,
        )
        .expect_err("exhausted budget must abort");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "read_timeout");
        assert!(h.staging_is_empty());
        assert_eq!(h.story_rows(), 0);
    }

    // ---------------- sweep_import_artifacts ----------------

    #[test]
    fn sweep_removes_staging_residue_and_orphan_dirs_but_keeps_known_imports() {
        let h = Harness::new();
        h.scanner.enqueue_supported_lunii(3);
        h.library.enqueue(Ok(library_entry(true, false)));
        h.packs.enqueue_success();
        let imported = h.run(&request()).expect("import");

        // Seed residues: a stale staging tempdir + an orphan promoted dir.
        let stale_staging = h.staging_dir().join("stale-acquisition");
        std::fs::create_dir_all(&stale_staging).expect("mk stale staging");
        std::fs::write(stale_staging.join("ni"), b"PART").expect("seed partial");
        let orphan = h.imports_dir().join("0197-orphan-no-row");
        std::fs::create_dir_all(&orphan).expect("mk orphan");
        std::fs::write(orphan.join("ni"), b"ORPHAN").expect("seed orphan");

        let outcome = {
            let db = h.db.lock().expect("lock");
            sweep_import_artifacts(&db, h.app_data.path()).expect("sweep")
        };
        assert_eq!(outcome.staging_entries_removed, 1);
        assert_eq!(outcome.orphan_dirs_removed, 1);

        assert!(!stale_staging.exists(), "stale staging must be removed");
        assert!(!orphan.exists(), "orphan promoted dir must be removed");
        assert!(
            h.imports_dir()
                .join(&imported.story.id)
                .join("ni")
                .is_file(),
            "a known import must survive the sweep"
        );
    }

    #[test]
    fn sweep_on_a_fresh_app_data_dir_is_a_no_op() {
        let h = Harness::new();
        let db = h.db.lock().expect("lock");
        let outcome = sweep_import_artifacts(&db, h.app_data.path()).expect("sweep");
        assert_eq!(outcome, ImportSweepOutcome::default());
    }
}
