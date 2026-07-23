//! Structured-folder creation application service (the FR30 flow).
//!
//! Two phases, NO mutation before acceptance (AC4):
//!
//! 1. [`analyze_structured_folder`] — bounded reads only: the manifest
//!    (`histoire.json`) and a cheap header probe of each referenced media,
//!    every one through the same no-follow regular-file open (a symlink is
//!    refused unread, the opened handle is TOCTOU-re-checked). The folder
//!    is NEVER listed: only the manifest and the files it references are
//!    touched, after basename sobriety validation. A folder-state problem
//!    (manifest absent, malformed, media missing…) is a typed VERDICT,
//!    never an `AppError`; only transport crosses as an error.
//! 2. [`prepare_structured_creation`] — RE-ANALYZES from zero (the disk is
//!    the authority; a verdict turned blocking refuses) and promotes each
//!    retained media into the content-addressed store, with NO DB access
//!    at all (the command runs it BEFORE taking the DB lock, so the media
//!    I/O never serializes other commands); then
//!    [`commit_structured_creation`] — the ONLY lock-holding part — runs
//!    ONE `BEGIN IMMEDIATE` transaction inserting the canonical `stories`
//!    row (fresh UUIDv7, `created_at = updated_at = now` — a BIRTH, unlike
//!    the `.rustory` import which preserves history), the provenance row
//!    (`source_format = 'structured-folder'`) and the `assets` rows. EVERY
//!    post-promotion failure (prepare refusal or transaction rollback)
//!    compensates the promoted files (refcounted GC,
//!    [`compensate_structured_creation`]). [`accept_structured_creation`]
//!    chains the two under one handle for tests and simple callers.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use crate::application::story::node::gc_unreferenced_media_file;
use crate::application::story::now_iso_ms;
use crate::domain::import::{
    analyze_structured_folder_components, is_artifact_checksum, is_supported_folder_source_name,
    referenced_media, FolderMediaKind, MediaProbe, RetainedMediaRef, StructuredFolderAnalysis,
    MAX_FOLDER_TOTAL_MEDIA_BYTES, STRUCTURED_FOLDER_FORMAT_VERSION,
    STRUCTURED_FOLDER_MANIFEST_NAME,
};
use crate::domain::shared::AppError;
use crate::domain::story::{
    canonical_structure_json, content_checksum, content_checksum_bytes, normalize_title,
    validate_title, CANONICAL_STORY_SCHEMA_VERSION,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::filesystem::{
    ensure_node_media_store, resolve_node_media_dir, sniff_media, store_media, MediaKind,
    StoredMedia, MAX_MEDIA_BYTES,
};
use crate::ipc::dto::import_export::{
    folder_import_report_dto, serialize_findings_summary, state_db_tag, state_dto,
};
use crate::ipc::dto::StoryCardDto;

/// Hard ceiling on the manifest bytes. An author manifest is a few kB; a
/// bigger file is refused as an unreadable envelope (typed verdict).
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Bytes read by the media PROBE: enough for every magic-byte pattern the
/// sniffer knows (longest need: RIFF/WAVE at 12 bytes). The analysis never
/// loads a media file whole — only the accept phase does, bounded.
const MEDIA_SNIFF_BYTES: usize = 16;

/// The application-level outcome of analyzing a picked folder: the typed
/// domain verdict + the provenance facts the accept phase re-derives.
#[derive(Debug, Clone)]
pub struct StructuredCreationOutcome {
    pub analysis: StructuredFolderAnalysis,
    /// The folder's basename — the only name that ever crosses to the
    /// provenance row (never an absolute path, PII).
    pub folder_name: String,
    /// SHA-256 of the manifest bytes (the provenance fingerprint); `None`
    /// when the manifest could not be read (envelope-blocked verdict).
    pub manifest_checksum: Option<String>,
}

/// Phase 1 — analyze the folder with bounded reads and ZERO mutation
/// (AC4). Every folder-STATE problem (manifest absent, malformed, media
/// missing…) is a typed verdict; a folder whose NAME Rustory cannot carry
/// as a provenance source (no real UTF-8 basename, a name outside the
/// sobriety rules) is an honest TRANSPORT refusal — never disguised as a
/// manifest problem (the manifest may be perfectly readable).
pub fn analyze_structured_folder(
    folder_path: &Path,
) -> Result<StructuredCreationOutcome, AppError> {
    let folder_name = folder_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(folder_name_unusable_error)?;
    if !is_supported_folder_source_name(&folder_name) {
        return Err(folder_name_unusable_error());
    }

    let Some(manifest_bytes) =
        read_manifest_bounded(&folder_path.join(STRUCTURED_FOLDER_MANIFEST_NAME))
    else {
        // Manifest absent / irregular / over the bound / unreadable: a
        // folder STATE, not a transport failure — the envelope blocks.
        return Ok(StructuredCreationOutcome {
            analysis: StructuredFolderAnalysis::envelope_blocked(),
            folder_name,
            manifest_checksum: None,
        });
    };
    let manifest_checksum = content_checksum_bytes(&manifest_bytes);

    // Probe each referenced media (sober basenames only, already bounded in
    // COUNT by the domain — the list is EMPTY for an unlisted format or a
    // bounds-breaking manifest, so no I/O ever runs for those).
    let mut probes: BTreeMap<String, MediaProbe> = BTreeMap::new();
    for basename in referenced_media(&manifest_bytes) {
        let probe = probe_media(&folder_path.join(&basename));
        probes.insert(basename, probe);
    }

    Ok(StructuredCreationOutcome {
        analysis: analyze_structured_folder_components(&manifest_bytes, &probes),
        folder_name,
        manifest_checksum: Some(manifest_checksum),
    })
}

/// The fully validated, files-already-promoted creation, ready for its
/// atomic DB commit. Produced by [`prepare_structured_creation`] with NO
/// DB access at all, so the (up to 256 MiB) media I/O never serializes
/// other commands behind the DB lock — the true "files first, DB second".
#[derive(Debug)]
pub struct PreparedCreation {
    title: String,
    structure_json: String,
    checksum: String,
    now_iso: String,
    /// Provenance identity of the creation source: the DB row's
    /// `source_format` / `source_format_version` — parameterized since the
    /// structured ARCHIVE became the second creation source sharing this
    /// acceptance machinery.
    source_format: &'static str,
    source_format_version: u64,
    source_name: String,
    artifact_checksum: String,
    state: crate::domain::import::ImportState,
    findings: Vec<crate::domain::import::RecognitionFinding>,
    asset_rows: Vec<AssetRow>,
    /// `(content_hash, file_name)` of every file this prepare promoted —
    /// what [`compensate_structured_creation`] reclaims if the commit
    /// never lands.
    promoted: Vec<(String, String)>,
}

/// The provenance facts a creation source hands to
/// [`prepare_from_creatable`] — everything the shared tail cannot derive
/// itself (it only owns the media promotion and the asset wiring).
pub(crate) struct CreationProvenance {
    pub source_format: &'static str,
    pub source_format_version: u64,
    pub source_name: String,
    pub artifact_checksum: String,
    pub state: crate::domain::import::ImportState,
    pub findings: Vec<crate::domain::import::RecognitionFinding>,
}

/// A prepare refusal: the typed error + the files ALREADY promoted when it
/// struck. The caller MUST hand `promoted` to
/// [`compensate_structured_creation`] (under a brief DB lock) so no
/// row-less file ever leaks — whatever the failure point (AC4).
#[derive(Debug)]
pub struct PrepareFailure {
    pub error: AppError,
    pub promoted: Vec<(String, String)>,
}

impl PrepareFailure {
    pub(crate) fn bare(error: AppError) -> Self {
        Self {
            error,
            promoted: Vec::new(),
        }
    }
}

/// Phase 2a — RE-ANALYZE from zero and prepare everything the commit
/// needs, WITHOUT any DB access: provenance + title validation, birth
/// timestamp (computed BEFORE any promotion so no failure can strike
/// after files landed without passing through [`PrepareFailure`]), media
/// promotions into the content-addressed store, asset wiring, canonical
/// serialization. The disk is the authority — a verdict turned blocking
/// refuses and promotes nothing.
pub fn prepare_structured_creation(
    app_data_dir: &Path,
    folder_path: &Path,
) -> Result<PreparedCreation, PrepareFailure> {
    // A forged pointer must at least be an absolute path — the system
    // dialog never returns anything else (empty/relative strings are a
    // hand-crafted call, refused before any I/O).
    if !folder_path.is_absolute() {
        return Err(PrepareFailure::bare(invalid_folder_path_error()));
    }

    let outcome = analyze_structured_folder(folder_path).map_err(PrepareFailure::bare)?;
    let Some(creatable) = outcome.analysis.creatable.clone() else {
        return Err(PrepareFailure::bare(revalidation_error()));
    };
    let Some(manifest_checksum) = outcome.manifest_checksum.clone() else {
        return Err(PrepareFailure::bare(revalidation_error()));
    };

    // Provenance re-validation BEFORE any write (defense in depth — the
    // CHECK constraints are the last net, never the first).
    if !is_supported_folder_source_name(&outcome.folder_name) {
        return Err(PrepareFailure::bare(invalid_provenance_error(
            "source_name",
        )));
    }
    if !is_artifact_checksum(&manifest_checksum) {
        return Err(PrepareFailure::bare(invalid_provenance_error(
            "artifact_checksum",
        )));
    }

    prepare_from_creatable(
        app_data_dir,
        folder_path,
        &creatable,
        CreationProvenance {
            source_format: "structured-folder",
            source_format_version: STRUCTURED_FOLDER_FORMAT_VERSION,
            source_name: outcome.folder_name,
            artifact_checksum: manifest_checksum,
            state: outcome.analysis.state,
            findings: outcome.analysis.findings,
        },
        MAX_FOLDER_TOTAL_MEDIA_BYTES,
    )
}

/// The SHARED tail of every structured-creation prepare (folder and
/// archive): defensive title re-check, birth timestamp, media promotion
/// from `media_base_dir` (the picked folder, or the archive's bounded
/// extraction directory), asset wiring, canonical serialization. The
/// caller owns the source-specific analysis and provenance validation.
pub(crate) fn prepare_from_creatable(
    app_data_dir: &Path,
    media_base_dir: &Path,
    creatable: &crate::domain::import::CreatableStory,
    provenance: CreationProvenance,
    max_total_media_bytes: u64,
) -> Result<PreparedCreation, PrepareFailure> {
    // Defensive title re-check. The re-analysis already blocks an invalid
    // title through the `Title` aspect, so this is unreachable in practice
    // — kept inside the closed `IMPORT_FAILED` taxonomy (never the creation
    // dialog's title error, which does not belong to this flow's contract).
    let title = normalize_title(&creatable.title);
    if validate_title(&title).is_err() {
        return Err(PrepareFailure::bare(revalidation_error()));
    }

    // The birth timestamp is computed BEFORE the promotions: after this
    // point every failure path carries the promoted list for compensation.
    let now_iso = match now_iso_ms() {
        Ok(now) => now,
        Err(_) => return Err(PrepareFailure::bare(clock_unavailable_error())),
    };

    // FILES FIRST — promote each DISTINCT retained basename into the
    // content-addressed store, re-applying the TOTAL byte bound on the
    // bytes actually read. No DB handle exists here by construction.
    let promoted = promote_retained_media(
        app_data_dir,
        media_base_dir,
        &creatable.retained_media,
        max_total_media_bytes,
    )?;

    // Wire the asset ids into the transcoded structure: one asset row per
    // retained slot (content-addressed file sharing happens underneath).
    let mut structure = creatable.structure.clone();
    let mut asset_rows: Vec<AssetRow> = Vec::new();
    for retained in &creatable.retained_media {
        let Some(stored) = promoted.get(&retained.basename) else {
            // Unreachable by construction (every retained basename was
            // promoted above); fail closed rather than invent a slot.
            return Err(PrepareFailure {
                error: revalidation_error(),
                promoted: promoted_pairs(&promoted),
            });
        };
        let Some(node) = structure
            .nodes
            .iter_mut()
            .find(|n| n.id == retained.node_id)
        else {
            return Err(PrepareFailure {
                error: revalidation_error(),
                promoted: promoted_pairs(&promoted),
            });
        };
        let asset_id = uuid::Uuid::now_v7().to_string();
        match retained.kind {
            FolderMediaKind::Image => node.image_asset_id = Some(asset_id.clone()),
            FolderMediaKind::Audio => node.audio_asset_id = Some(asset_id.clone()),
        }
        asset_rows.push(AssetRow {
            asset_id,
            content_hash: stored.content_hash.clone(),
            media_type: stored.kind.as_str(),
            media_format: stored.format,
            byte_size: stored.byte_size,
            file_name: stored.file_name.clone(),
        });
    }

    let structure_json = canonical_structure_json(&structure);
    let checksum = content_checksum(&structure_json);
    Ok(PreparedCreation {
        title,
        structure_json,
        checksum,
        now_iso,
        source_format: provenance.source_format,
        source_format_version: provenance.source_format_version,
        source_name: provenance.source_name,
        artifact_checksum: provenance.artifact_checksum,
        state: provenance.state,
        findings: provenance.findings,
        asset_rows,
        promoted: promoted_pairs(&promoted),
    })
}

/// Promote each DISTINCT retained basename, RE-APPLYING the total byte
/// bound on the bytes actually read — the probe sizes may be stale (files
/// can grow between the re-analysis and the promotion). Beyond
/// `max_total_bytes` the accept refuses with EVERYTHING promoted so far
/// (current file included) handed to the compensation.
fn promote_retained_media(
    app_data_dir: &Path,
    folder_path: &Path,
    retained_media: &[RetainedMediaRef],
    max_total_bytes: u64,
) -> Result<BTreeMap<String, StoredMedia>, PrepareFailure> {
    let mut promoted: BTreeMap<String, StoredMedia> = BTreeMap::new();
    let mut total_bytes: u64 = 0;
    for retained in retained_media {
        if promoted.contains_key(&retained.basename) {
            continue;
        }
        let stored = match promote_media(
            app_data_dir,
            &folder_path.join(&retained.basename),
            retained.kind,
        ) {
            Ok(stored) => stored,
            Err(err) => {
                // The disk moved between the re-analysis and this
                // promotion: refuse — the caller compensates everything
                // promoted so far.
                return Err(PrepareFailure {
                    error: err,
                    promoted: promoted_pairs(&promoted),
                });
            }
        };
        total_bytes = total_bytes.saturating_add(stored.byte_size);
        if total_bytes > max_total_bytes {
            // The current file IS promoted — it must be compensated too.
            let mut all = promoted_pairs(&promoted);
            all.push((stored.content_hash.clone(), stored.file_name.clone()));
            return Err(PrepareFailure {
                error: media_read_error("oversize_total"),
                promoted: all,
            });
        }
        promoted.insert(retained.basename.clone(), stored);
    }
    Ok(promoted)
}

fn promoted_pairs(map: &BTreeMap<String, StoredMedia>) -> Vec<(String, String)> {
    map.values()
        .map(|stored| (stored.content_hash.clone(), stored.file_name.clone()))
        .collect()
}

/// Phase 2b — the single atomic transaction (`stories` + provenance +
/// `assets`). This is the ONLY part of the accept that needs the DB lock.
/// A failed transaction compensates the promoted files before returning.
pub fn commit_structured_creation(
    db: &mut DbHandle,
    app_data_dir: &Path,
    prepared: PreparedCreation,
) -> Result<StoryCardDto, AppError> {
    let story_id = uuid::Uuid::now_v7().to_string();
    let findings_summary = serialize_findings_summary(&prepared.findings);

    let commit = (|| -> Result<(), AppError> {
        let tx = db
            .conn_mut()
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| db_commit_error(&err, "begin_transaction"))?;
        tx.execute(
            "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![
                &story_id,
                &prepared.title,
                CANONICAL_STORY_SCHEMA_VERSION,
                &prepared.structure_json,
                &prepared.checksum,
                &prepared.now_iso,
            ],
        )
        .map_err(|err| db_commit_error(&err, "insert_story"))?;
        tx.execute(
            "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                &story_id,
                prepared.source_format,
                prepared.source_format_version,
                &prepared.source_name,
                &prepared.artifact_checksum,
                state_db_tag(prepared.state),
                &findings_summary,
                &prepared.now_iso,
            ],
        )
        .map_err(|err| db_commit_error(&err, "insert_provenance"))?;
        for row in &prepared.asset_rows {
            tx.execute(
                "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    &row.asset_id,
                    &story_id,
                    &row.content_hash,
                    row.media_type,
                    row.media_format,
                    row.byte_size,
                    &row.file_name,
                    &prepared.now_iso,
                ],
            )
            .map_err(|err| db_commit_error(&err, "insert_assets"))?;
        }
        tx.commit().map_err(|err| db_commit_error(&err, "commit"))
    })();

    if let Err(err) = commit {
        // The transaction rolled back: every promoted file whose content
        // no asset row references anymore is compensated (the boot sweep
        // stays the net for a crash between the two).
        compensate_structured_creation(db, app_data_dir, &prepared.promoted);
        return Err(err);
    }

    let import_report = folder_import_report_dto(&prepared.findings);
    Ok(StoryCardDto {
        id: story_id,
        title: prepared.title,
        import_state: Some(state_dto(prepared.state)),
        import_report: if import_report.is_empty() {
            None
        } else {
            Some(import_report)
        },
        transferable: false,
    })
}

/// Best-effort compensation of promoted files after a refused prepare or
/// a failed commit: each file is removed IF no `assets` row references its
/// content (the exact `commit_node_media` compensation rule). Needs the DB
/// briefly — the refcount check is what keeps a content-shared file safe.
pub fn compensate_structured_creation(
    db: &DbHandle,
    app_data_dir: &Path,
    promoted: &[(String, String)],
) {
    let media_dir = resolve_node_media_dir(app_data_dir);
    for (content_hash, file_name) in promoted {
        gc_unreferenced_media_file(
            db,
            &media_dir,
            Some((content_hash.clone(), file_name.clone())),
        );
    }
}

/// Convenience: prepare + commit under the SAME borrowed handle (tests and
/// single-threaded callers). The IPC command does NOT use this — it runs
/// [`prepare_structured_creation`] before taking the DB lock and only
/// locks for [`commit_structured_creation`] (or the brief compensation).
pub fn accept_structured_creation(
    db: &mut DbHandle,
    app_data_dir: &Path,
    folder_path: &Path,
) -> Result<StoryCardDto, AppError> {
    match prepare_structured_creation(app_data_dir, folder_path) {
        Ok(prepared) => commit_structured_creation(db, app_data_dir, prepared),
        Err(failure) => {
            compensate_structured_creation(db, app_data_dir, &failure.promoted);
            Err(failure.error)
        }
    }
}

#[derive(Debug)]
struct AssetRow {
    asset_id: String,
    content_hash: String,
    media_type: &'static str,
    media_format: &'static str,
    byte_size: u64,
    file_name: String,
}

/// The outcome of the no-follow regular-file open shared by every folder
/// read (manifest, media probe, media promotion).
enum RegularOpen {
    /// The handle IS the regular, in-bound directory entry the lstat saw.
    Open(std::fs::File, u64),
    /// Nothing at this path.
    Absent,
    /// A regular file whose size exceeds the byte bound — kept DISTINCT so
    /// the diagnostic taxonomy names the right cause (`oversize`, never
    /// `not_regular_file`).
    Oversize,
    /// A symlink, an irregular file, an unreadable entry, or an entry
    /// SWAPPED between the lstat and the open (TOCTOU).
    Unusable,
}

/// Open the file WITHOUT following a symlink at its final component and
/// WITHOUT blocking on a special file. On Unix the open itself carries
/// `O_NOFOLLOW | O_NONBLOCK`: a symlink swapped in after the lstat FAILS
/// the open (`ELOOP`) instead of being followed, and a swapped-in FIFO
/// opens non-blocking (then fails the handle re-check) instead of
/// suspending the blocking worker forever. On other platforms the
/// lstat + handle re-check below remain the only guard — the swap window
/// stays theoretical there (documented limit).
fn open_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // ABI-frozen per-OS flag values (`libc` is not a direct
        // dependency of this crate): O_NOFOLLOW | O_NONBLOCK.
        #[cfg(target_os = "linux")]
        const NO_FOLLOW_NON_BLOCK: i32 = 0o400000 | 0o4000;
        #[cfg(not(target_os = "linux"))]
        const NO_FOLLOW_NON_BLOCK: i32 = 0x0100 | 0x0004;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(NO_FOLLOW_NON_BLOCK)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::File::open(path)
    }
}

/// Open `path` as a REGULAR file without ever following a symlink at its
/// final component. `symlink_metadata` gates the common case up-front,
/// [`open_no_follow`] closes the swap window at the open itself (Unix),
/// then the OPENED handle is re-checked: it must still be a regular
/// in-bound file and — on Unix — its `(dev, ino)` must match the lstat'ed
/// entry, so a swap between the two calls is refused instead of read.
fn open_bounded_regular(path: &Path, max_bytes: u64) -> RegularOpen {
    let Ok(pre) = std::fs::symlink_metadata(path) else {
        return RegularOpen::Absent;
    };
    if pre.file_type().is_symlink() || !pre.is_file() {
        return RegularOpen::Unusable;
    }
    if pre.len() > max_bytes {
        return RegularOpen::Oversize;
    }
    let Ok(file) = open_no_follow(path) else {
        return RegularOpen::Unusable;
    };
    let Ok(meta) = file.metadata() else {
        return RegularOpen::Unusable;
    };
    if !meta.is_file() {
        return RegularOpen::Unusable;
    }
    if meta.len() > max_bytes {
        return RegularOpen::Oversize;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // The handle must be the very entry the lstat classified — a
        // symlink/file swapped in between changes (dev, ino).
        if meta.dev() != pre.dev() || meta.ino() != pre.ino() {
            return RegularOpen::Unusable;
        }
    }
    RegularOpen::Open(file, meta.len())
}

/// Read the manifest bounded, never following a symlink (`histoire.json`
/// must be a REGULAR file OF the folder — a symlinked manifest could reach
/// outside it). Returns `None` on ANY failure — the caller maps it to the
/// envelope-blocked VERDICT (a folder state, not a transport error).
fn read_manifest_bounded(path: &Path) -> Option<Vec<u8>> {
    let RegularOpen::Open(file, _) = open_bounded_regular(path, MAX_MANIFEST_BYTES) else {
        return None;
    };
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return None;
    }
    Some(bytes)
}

/// Probe ONE referenced media without loading it: the no-follow open gates
/// symlinks / irregular files / oversize (TOCTOU-checked), and a bounded
/// header read feeds the magic-byte sniff. All failures are folder STATES
/// (`Absent` / `Unusable`), never errors.
fn probe_media(path: &Path) -> MediaProbe {
    let (file, byte_size) = match open_bounded_regular(path, MAX_MEDIA_BYTES as u64) {
        RegularOpen::Open(file, byte_size) => (file, byte_size),
        RegularOpen::Absent => return MediaProbe::Absent,
        // Functionally the same discarded state; the DISTINCT variant only
        // matters to the promotion-path diagnostics.
        RegularOpen::Oversize | RegularOpen::Unusable => return MediaProbe::Unusable,
    };
    let mut header = Vec::with_capacity(MEDIA_SNIFF_BYTES);
    if file
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

/// Read + validate + PROMOTE one retained media into the store (prepare
/// phase). The slot kind is validated on the full bytes BEFORE the store
/// promotion, so a kind drift since the re-analysis refuses WITHOUT having
/// promoted anything for this file (nothing new to compensate); the store
/// re-sniffs the same bytes underneath (defense in depth).
fn promote_media(
    app_data_dir: &Path,
    path: &Path,
    expected: FolderMediaKind,
) -> Result<StoredMedia, AppError> {
    let (file, _) = match open_bounded_regular(path, MAX_MEDIA_BYTES as u64) {
        RegularOpen::Open(file, byte_size) => (file, byte_size),
        RegularOpen::Absent => return Err(media_read_error("metadata")),
        RegularOpen::Oversize => return Err(media_read_error("oversize")),
        RegularOpen::Unusable => return Err(media_read_error("not_regular_file")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_MEDIA_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| media_read_error("read"))?;
    if bytes.len() > MAX_MEDIA_BYTES {
        return Err(media_read_error("oversize"));
    }

    // Validate the slot kind on the bytes we just read, BEFORE promoting:
    // a mismatch (the file changed since the re-analysis) must not leave a
    // freshly promoted file behind.
    let sniffed = sniff_media(&bytes).ok_or_else(media_promotion_error)?;
    if folder_kind(sniffed.kind) != expected {
        return Err(media_promotion_error());
    }

    let (media_dir, staging_dir) =
        ensure_node_media_store(app_data_dir).map_err(|_| app_data_unavailable_error())?;
    store_media(&media_dir, &staging_dir, &bytes).map_err(|_| media_promotion_error())
}

pub(crate) const fn folder_kind(kind: MediaKind) -> FolderMediaKind {
    match kind {
        MediaKind::Image => FolderMediaKind::Image,
        MediaKind::Audio => FolderMediaKind::Audio,
    }
}

// ===== Closed user-facing copy — sober, no OS message, no path (PII). =====
// Same `IMPORT_FAILED` closed `details.source` taxonomy as the `.rustory`
// import flow; the visible copy speaks of a CREATION.

/// Defensive: the folder failed the from-zero re-validation at accept time
/// (turned blocking, or an internal invariant broke). Nothing is committed.
fn revalidation_error() -> AppError {
    AppError::import_failed(
        "Création impossible: le dossier n'a pas pu être revalidé.",
        "Le contenu du dossier a peut-être changé. Relance l'analyse du dossier puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "other", "cause": "revalidation" }))
}

/// Defensive: a provenance field is not sober (a forged call). `field`
/// names WHICH field — never the value (PII).
fn invalid_provenance_error(field: &'static str) -> AppError {
    AppError::import_failed(
        "Création impossible: informations de provenance invalides.",
        "Relance l'analyse du dossier puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "cause": "invalid_provenance",
        "field": field,
    }))
}

/// A retained media could not be read back at accept time (moved,
/// permission, grew past the bound…).
fn media_read_error(stage: &'static str) -> AppError {
    AppError::import_failed(
        "Création impossible: un fichier du dossier n'a pas pu être lu.",
        "Vérifie le contenu du dossier puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "file_read",
        "stage": stage,
    }))
}

/// A retained media could not be promoted into the managed store (store
/// I/O, or the bytes changed kind since the re-analysis).
fn media_promotion_error() -> AppError {
    AppError::import_failed(
        "Création impossible: un média du dossier n'a pas pu être préparé.",
        "Vérifie le contenu du dossier puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "cause": "media_promotion",
    }))
}

/// The managed local store has no resolvable home.
pub fn app_data_unavailable_error() -> AppError {
    AppError::import_failed(
        "Création impossible: stockage local introuvable.",
        "Vérifie les permissions de ton dossier utilisateur puis relance Rustory.",
    )
    .with_details(serde_json::json!({ "source": "app_data_unavailable" }))
}

/// The chosen folder's NAME cannot be carried as a provenance source name
/// (no real UTF-8 basename — e.g. a filesystem root —, or a name outside
/// the sobriety rules: too long, control characters, `\\`, `:`…). An
/// honest transport refusal — never disguised as a manifest problem (the
/// manifest may be perfectly present and readable).
fn folder_name_unusable_error() -> AppError {
    AppError::import_failed(
        "Création impossible: le nom du dossier choisi ne peut pas être utilisé par Rustory.",
        "Renomme le dossier (nom plus court, sans caractère spécial) puis relance l'analyse.",
    )
    .with_details(serde_json::json!({
        "source": "file_read",
        "stage": "folder_name",
    }))
}

/// The accept received a folder pointer the system dialog could never have
/// produced (empty / relative) — refused before any I/O.
fn invalid_folder_path_error() -> AppError {
    AppError::import_failed(
        "Création impossible: l'emplacement du dossier est invalide.",
        "Relance l'analyse du dossier puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "file_read",
        "stage": "invalid_path",
    }))
}

/// The system dialog returned a non-filesystem location (a URL) instead of
/// a local folder.
pub fn non_filesystem_path_error() -> AppError {
    AppError::import_failed(
        "Création impossible: le système a renvoyé un emplacement non local.",
        "Choisis un dossier local classique puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "file_read",
        "stage": "non_filesystem_path",
    }))
}

/// The native folder dialog backend could not open.
pub fn dialog_failed_error() -> AppError {
    AppError::import_failed(
        "Création impossible: la fenêtre de sélection n'a pas pu s'ouvrir.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({ "source": "dialog_failed" }))
}

/// The blocking worker task could not be joined.
pub fn spawn_blocking_join_error() -> AppError {
    AppError::import_failed(
        "Création interrompue de façon inattendue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({ "source": "spawn_blocking_join" }))
}

/// The system clock could not produce the birth timestamp.
fn clock_unavailable_error() -> AppError {
    AppError::import_failed(
        "Création impossible: l'horloge système est indisponible.",
        "Vérifie la date et l'heure de ton ordinateur puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "cause": "system_clock_invalid",
    }))
}

fn db_commit_error(err: &rusqlite::Error, stage: &'static str) -> AppError {
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
        "Création impossible: enregistrement local refusé.",
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
    use crate::domain::import::{ImportState, RecognitionQuality};
    use crate::domain::shared::AppErrorCode;
    use crate::infrastructure::db;
    use tempfile::TempDir;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    const MP3: &[u8] = b"ID3\x03\x00\x00\x00rustory";

    fn fresh_db() -> DbHandle {
        let mut handle = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut handle).expect("migrate");
        handle
    }

    fn write_manifest(folder: &Path, manifest: &str) {
        std::fs::write(folder.join(STRUCTURED_FOLDER_MANIFEST_NAME), manifest).expect("manifest");
    }

    fn clean_folder(tmp: &TempDir) -> std::path::PathBuf {
        let folder = tmp.path().join("mon-histoire");
        std::fs::create_dir(&folder).expect("mkdir");
        write_manifest(
            &folder,
            r#"{
                "formatVersion": 1,
                "title": "Le voyage de Nour",
                "nodes": [
                    { "id": "debut", "text": "Il était une fois…", "image": "couverture.png", "audio": "intro.mp3",
                      "options": [ { "label": "Continuer", "target": "mer" } ] },
                    { "id": "mer", "text": "La mer" }
                ]
            }"#,
        );
        std::fs::write(folder.join("couverture.png"), PNG).expect("png");
        std::fs::write(folder.join("intro.mp3"), MP3).expect("mp3");
        folder
    }

    fn count(db: &DbHandle, table: &str) -> u32 {
        db.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .expect("count")
    }

    fn store_file_count(app_data_dir: &Path) -> usize {
        let media_dir = resolve_node_media_dir(app_data_dir);
        match std::fs::read_dir(&media_dir) {
            Ok(entries) => entries.flatten().filter(|e| e.path().is_file()).count(),
            Err(_) => 0,
        }
    }

    // ---- Phase 1: analysis ---------------------------------------------------

    #[test]
    fn analyze_recognizes_a_clean_folder_with_media() {
        let tmp = TempDir::new().expect("tmp");
        let folder = clean_folder(&tmp);
        let outcome = analyze_structured_folder(&folder).expect("analyze");
        assert_eq!(outcome.analysis.quality, RecognitionQuality::Clean);
        assert_eq!(outcome.analysis.state, ImportState::Recognized);
        assert_eq!(outcome.folder_name, "mon-histoire");
        assert_eq!(outcome.manifest_checksum.as_deref().map(str::len), Some(64));
        let creatable = outcome.analysis.creatable.expect("creatable");
        assert_eq!(creatable.retained_media.len(), 2);
    }

    #[test]
    fn analyze_alone_never_mutates_the_library_nor_the_store() {
        // AC4's twin of `analyze_alone_never_mutates_the_library`: analysis
        // is bounded READS only — zero row, zero promoted file.
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = clean_folder(&tmp);
        let db = fresh_db();

        let _ = analyze_structured_folder(&folder).expect("analyze");

        assert_eq!(count(&db, "stories"), 0);
        assert_eq!(count(&db, "story_local_imports"), 0);
        assert_eq!(count(&db, "assets"), 0);
        assert_eq!(
            store_file_count(app_data.path()),
            0,
            "no file may be promoted by the analysis"
        );
    }

    #[test]
    fn analyze_blocks_a_folder_without_manifest() {
        let tmp = TempDir::new().expect("tmp");
        let folder = tmp.path().join("vide");
        std::fs::create_dir(&folder).expect("mkdir");
        let outcome = analyze_structured_folder(&folder).expect("analyze");
        assert_eq!(outcome.analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(outcome.analysis.state, ImportState::Blocked);
        assert!(outcome.manifest_checksum.is_none());
    }

    #[test]
    fn analyze_blocks_an_oversize_manifest_as_a_verdict() {
        let tmp = TempDir::new().expect("tmp");
        let folder = tmp.path().join("gros");
        std::fs::create_dir(&folder).expect("mkdir");
        let oversize = vec![b' '; (MAX_MANIFEST_BYTES + 1) as usize];
        std::fs::write(folder.join(STRUCTURED_FOLDER_MANIFEST_NAME), oversize).expect("manifest");
        let outcome = analyze_structured_folder(&folder).expect("analyze");
        assert_eq!(outcome.analysis.quality, RecognitionQuality::Unusable);
    }

    #[test]
    fn analyze_marks_an_absent_referenced_media_missing_partial() {
        let tmp = TempDir::new().expect("tmp");
        let folder = tmp.path().join("manque");
        std::fs::create_dir(&folder).expect("mkdir");
        write_manifest(
            &folder,
            r#"{ "formatVersion": 1, "title": "Sans image", "nodes": [ { "id": "n1", "image": "absente.png" } ] }"#,
        );
        let outcome = analyze_structured_folder(&folder).expect("analyze");
        assert_eq!(outcome.analysis.quality, RecognitionQuality::Partial);
        assert_eq!(outcome.analysis.state, ImportState::Partial);
        assert_eq!(
            outcome.analysis.discarded_media,
            vec!["absente.png".to_string()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn analyze_discards_a_symlinked_media_as_unusable() {
        let tmp = TempDir::new().expect("tmp");
        let folder = tmp.path().join("lien");
        std::fs::create_dir(&folder).expect("mkdir");
        write_manifest(
            &folder,
            r#"{ "formatVersion": 1, "title": "Lien", "nodes": [ { "id": "n1", "image": "lien.png" } ] }"#,
        );
        let real = tmp.path().join("vraie.png");
        std::fs::write(&real, PNG).expect("png");
        std::os::unix::fs::symlink(&real, folder.join("lien.png")).expect("symlink");
        let outcome = analyze_structured_folder(&folder).expect("analyze");
        assert_eq!(outcome.analysis.state, ImportState::NeedsReview);
        assert_eq!(
            outcome.analysis.discarded_media,
            vec!["lien.png".to_string()]
        );
    }

    #[test]
    fn analyze_discards_a_non_media_file_as_unusable() {
        let tmp = TempDir::new().expect("tmp");
        let folder = tmp.path().join("texte");
        std::fs::create_dir(&folder).expect("mkdir");
        write_manifest(
            &folder,
            r#"{ "formatVersion": 1, "title": "Texte", "nodes": [ { "id": "n1", "audio": "notes.txt" } ] }"#,
        );
        std::fs::write(folder.join("notes.txt"), b"pas un media").expect("txt");
        let outcome = analyze_structured_folder(&folder).expect("analyze");
        assert_eq!(outcome.analysis.state, ImportState::NeedsReview);
        assert_eq!(
            outcome.analysis.discarded_media,
            vec!["notes.txt".to_string()]
        );
    }

    // ---- Phase 2: accept -------------------------------------------------------

    #[test]
    fn accept_commits_story_provenance_and_assets_atomically() {
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = clean_folder(&tmp);
        let mut db = fresh_db();

        let card = accept_structured_creation(&mut db, app_data.path(), &folder).expect("accept");
        assert_eq!(card.title, "Le voyage de Nour");
        assert_eq!(
            card.import_state.map(|s| s.wire_tag().to_string()),
            Some("recognized".to_string())
        );
        assert!(card.import_report.is_none(), "clean creation has no report");

        assert_eq!(count(&db, "stories"), 1);
        assert_eq!(count(&db, "story_local_imports"), 1);
        assert_eq!(count(&db, "assets"), 2);
        assert_eq!(store_file_count(app_data.path()), 2);

        // The provenance row carries the folder facts.
        let (format, version, name, checksum, state): (String, i64, String, String, String) = db
            .conn()
            .query_row(
                "SELECT source_format, source_format_version, source_name, artifact_checksum, import_state \
                 FROM story_local_imports",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("provenance");
        assert_eq!(format, "structured-folder");
        assert_eq!(version, 1);
        assert_eq!(name, "mon-histoire");
        assert_eq!(checksum.len(), 64);
        assert_eq!(state, "recognized");

        // The canonical row is a BIRTH: created_at == updated_at.
        let (created, updated): (String, String) = db
            .conn()
            .query_row(
                "SELECT created_at, updated_at FROM stories WHERE id = ?1",
                rusqlite::params![card.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("story row");
        assert_eq!(created, updated, "a creation is born, not imported");

        // The structure carries the wired asset ids.
        let structure_json: String = db
            .conn()
            .query_row(
                "SELECT structure_json FROM stories WHERE id = ?1",
                rusqlite::params![card.id],
                |r| r.get(0),
            )
            .expect("structure");
        let structure: crate::domain::story::CanonicalStructure =
            serde_json::from_str(&structure_json).expect("v3 parse");
        assert!(structure.nodes[0].image_asset_id.is_some());
        assert!(structure.nodes[0].audio_asset_id.is_some());
        assert!(structure.nodes[1].image_asset_id.is_none());
    }

    #[test]
    fn accept_a_partial_folder_persists_partial_with_a_findings_summary() {
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = tmp.path().join("partiel");
        std::fs::create_dir(&folder).expect("mkdir");
        write_manifest(
            &folder,
            r#"{ "formatVersion": 1, "title": "Partiel", "nodes": [ { "id": "n1", "image": "absente.png" } ] }"#,
        );
        let mut db = fresh_db();

        let card = accept_structured_creation(&mut db, app_data.path(), &folder).expect("accept");
        assert_eq!(
            card.import_state.map(|s| s.wire_tag().to_string()),
            Some("partial".to_string())
        );
        let report = card.import_report.expect("a partial carries its report");
        assert!(report.iter().any(|f| matches!(
            f.category,
            crate::ipc::dto::import_export::ImportCategoryDto::Missing
        )));

        let (state, summary): (String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT import_state, findings_summary FROM story_local_imports",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("provenance");
        assert_eq!(state, "partial");
        assert!(summary.is_some(), "the durable report backs the marker");
        // The node is born with the empty slot (repairable in the editor).
        assert_eq!(count(&db, "assets"), 0);
    }

    #[test]
    fn accept_refuses_a_blocked_folder_and_creates_nothing() {
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = tmp.path().join("bloque");
        std::fs::create_dir(&folder).expect("mkdir");
        write_manifest(
            &folder,
            r#"{ "formatVersion": 2, "title": "Futur", "nodes": [ { "id": "n1" } ] }"#,
        );
        let mut db = fresh_db();

        let err = accept_structured_creation(&mut db, app_data.path(), &folder)
            .expect_err("a blocked folder must refuse");
        assert_eq!(err.code, AppErrorCode::ImportFailed);
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["cause"], "revalidation");
        assert_eq!(count(&db, "stories"), 0);
        assert_eq!(count(&db, "story_local_imports"), 0);
        assert_eq!(count(&db, "assets"), 0);
        assert_eq!(store_file_count(app_data.path()), 0);
    }

    #[test]
    fn accept_reanalyzes_from_zero_a_media_gone_since_analysis() {
        // The disk changed between the two phases: the re-analysis is the
        // authority — the accept persists the RECOMPUTED partial state.
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = clean_folder(&tmp);
        let mut db = fresh_db();

        let before = analyze_structured_folder(&folder).expect("analyze");
        assert_eq!(before.analysis.state, ImportState::Recognized);

        std::fs::remove_file(folder.join("couverture.png")).expect("remove media");

        let card = accept_structured_creation(&mut db, app_data.path(), &folder).expect("accept");
        assert_eq!(
            card.import_state.map(|s| s.wire_tag().to_string()),
            Some("partial".to_string()),
            "the re-analysis recomputed the state from the changed disk"
        );
        // Only the surviving media was promoted + wired.
        assert_eq!(count(&db, "assets"), 1);
        assert_eq!(store_file_count(app_data.path()), 1);
    }

    #[test]
    fn accept_refuses_a_manifest_turned_blocking_since_analysis() {
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = clean_folder(&tmp);
        let mut db = fresh_db();

        let before = analyze_structured_folder(&folder).expect("analyze");
        assert_eq!(before.analysis.quality, RecognitionQuality::Clean);

        write_manifest(&folder, "{ broken json");

        let err = accept_structured_creation(&mut db, app_data.path(), &folder)
            .expect_err("a manifest turned blocking must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["cause"], "revalidation");
        assert_eq!(count(&db, "stories"), 0);
        assert_eq!(store_file_count(app_data.path()), 0);
    }

    #[test]
    fn accept_rolls_back_and_compensates_when_the_provenance_insert_fails() {
        // Fault injection 1/2 (AC4 atomicity): sabotage the SECOND insert.
        // The whole transaction rolls back AND the promoted files are
        // compensated — no row, no orphan in the store.
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = clean_folder(&tmp);
        let mut db = fresh_db();
        db.conn()
            .execute_batch(
                "CREATE TRIGGER sabotage_provenance BEFORE INSERT ON story_local_imports \
                 BEGIN SELECT RAISE(ABORT, 'sabotage'); END;",
            )
            .expect("install sabotage trigger");

        let err = accept_structured_creation(&mut db, app_data.path(), &folder)
            .expect_err("the provenance insert must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "db_commit");
        assert_eq!(v["details"]["stage"], "insert_provenance");

        assert_eq!(count(&db, "stories"), 0, "atomic rollback");
        assert_eq!(count(&db, "assets"), 0);
        assert_eq!(
            store_file_count(app_data.path()),
            0,
            "promoted files must be compensated after the rollback"
        );
    }

    #[test]
    fn accept_rolls_back_and_compensates_when_the_assets_insert_fails() {
        // Fault injection 2/2: sabotage the THIRD insert stage.
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = clean_folder(&tmp);
        let mut db = fresh_db();
        db.conn()
            .execute_batch(
                "CREATE TRIGGER sabotage_assets BEFORE INSERT ON assets \
                 BEGIN SELECT RAISE(ABORT, 'sabotage'); END;",
            )
            .expect("install sabotage trigger");

        let err = accept_structured_creation(&mut db, app_data.path(), &folder)
            .expect_err("the assets insert must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "db_commit");
        assert_eq!(v["details"]["stage"], "insert_assets");

        assert_eq!(count(&db, "stories"), 0);
        assert_eq!(count(&db, "story_local_imports"), 0);
        assert_eq!(count(&db, "assets"), 0);
        assert_eq!(store_file_count(app_data.path()), 0);
    }

    #[test]
    fn a_double_accept_creates_two_distinct_stories() {
        // Idempotence is NOT required: accepting twice is two creations
        // (two fresh UUIDv7 stories); the media files are shared by
        // content-addressing underneath.
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = clean_folder(&tmp);
        let mut db = fresh_db();

        let first = accept_structured_creation(&mut db, app_data.path(), &folder).expect("first");
        let second = accept_structured_creation(&mut db, app_data.path(), &folder).expect("second");
        assert_ne!(first.id, second.id);
        assert_eq!(count(&db, "stories"), 2);
        assert_eq!(count(&db, "story_local_imports"), 2);
        assert_eq!(count(&db, "assets"), 4, "one asset row per slot");
        assert_eq!(
            store_file_count(app_data.path()),
            2,
            "identical bytes are content-addressed once"
        );
    }

    #[test]
    fn accept_does_not_touch_existing_stories_fr18() {
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let folder = clean_folder(&tmp);
        let mut db = fresh_db();
        let native = crate::application::story::create_story(
            &mut db,
            crate::application::story::CreateStoryInput {
                title: "Native intacte".into(),
            },
        )
        .expect("create native");

        accept_structured_creation(&mut db, app_data.path(), &folder).expect("accept");

        let (title, updated): (String, String) = db
            .conn()
            .query_row(
                "SELECT title, updated_at FROM stories WHERE id = ?1",
                rusqlite::params![native.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("native row");
        assert_eq!(title, "Native intacte");
        assert!(!updated.is_empty());
    }

    /// Every creation-refusal constructor must be ACTIONABLE (cause + next
    /// gesture) with a closed `details.source` — calque of the `.rustory`
    /// constructor test.
    #[test]
    fn every_structured_creation_refusal_constructor_is_actionable() {
        let sqlite_err = rusqlite::Error::QueryReturnedNoRows;
        let refusals = [
            revalidation_error(),
            invalid_provenance_error("source_name"),
            invalid_folder_path_error(),
            media_read_error("read"),
            media_promotion_error(),
            app_data_unavailable_error(),
            dialog_failed_error(),
            non_filesystem_path_error(),
            spawn_blocking_join_error(),
            clock_unavailable_error(),
            db_commit_error(&sqlite_err, "insert_story"),
        ];
        for err in &refusals {
            assert_eq!(err.code, AppErrorCode::ImportFailed, "{err:?}");
            assert!(!err.message.is_empty(), "refusal needs a cause: {err:?}");
            let action = err.user_action.as_deref().unwrap_or("");
            assert!(!action.is_empty(), "refusal needs a next gesture: {err:?}");
            let v = serde_json::to_value(err).expect("ser");
            assert!(v["details"]["source"].is_string());
        }
    }

    // ---- Paths, symlinks, promotion compensation ---------------------------

    #[cfg(unix)]
    #[test]
    fn analyze_blocks_a_symlinked_manifest() {
        // `histoire.json` must be a REGULAR file OF the folder — a symlink
        // could reach outside it (the no-follow open refuses it unread).
        let tmp = TempDir::new().expect("tmp");
        let folder = tmp.path().join("lien-manifest");
        std::fs::create_dir(&folder).expect("mkdir");
        let real = tmp.path().join("vrai-manifeste.json");
        std::fs::write(
            &real,
            r#"{ "formatVersion": 1, "title": "Hors dossier", "nodes": [ { "id": "n1" } ] }"#,
        )
        .expect("real manifest");
        std::os::unix::fs::symlink(&real, folder.join(STRUCTURED_FOLDER_MANIFEST_NAME))
            .expect("symlink");

        let outcome = analyze_structured_folder(&folder).expect("analyze");
        assert_eq!(outcome.analysis.quality, RecognitionQuality::Unusable);
        assert_eq!(outcome.analysis.state, ImportState::Blocked);
        assert!(outcome.manifest_checksum.is_none(), "never read");
    }

    #[test]
    fn analyze_refuses_a_path_without_a_real_basename_as_transport() {
        // A filesystem root has no basename: Rustory cannot carry a
        // provenance name for it — an HONEST transport refusal naming the
        // folder name, never the "manifest unreadable" verdict copy.
        let err = analyze_structured_folder(Path::new("/"))
            .expect_err("a folder without a real basename must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "IMPORT_FAILED");
        assert_eq!(v["details"]["source"], "file_read");
        assert_eq!(v["details"]["stage"], "folder_name");
    }

    #[test]
    fn analyze_refuses_a_non_sober_folder_name_as_transport() {
        // A backslash is LEGAL in a Linux directory name but cannot be
        // carried as a sober provenance source name: honest refusal, the
        // manifest inside may be perfectly readable.
        let tmp = TempDir::new().expect("tmp");
        let folder = tmp.path().join("mes\\histoires");
        std::fs::create_dir(&folder).expect("mkdir");
        write_manifest(
            &folder,
            r#"{ "formatVersion": 1, "title": "Lisible", "nodes": [ { "id": "n1" } ] }"#,
        );
        let err =
            analyze_structured_folder(&folder).expect_err("a non-sober folder name must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["stage"], "folder_name");
    }

    #[test]
    fn accept_refuses_a_relative_or_empty_folder_path_before_any_io() {
        // A forged pointer the system dialog could never produce is refused
        // up-front — nothing is read, nothing is created.
        let app_data = TempDir::new().expect("app data");
        let mut db = fresh_db();
        for forged in ["", "relatif/dossier"] {
            let err = accept_structured_creation(&mut db, app_data.path(), Path::new(forged))
                .expect_err("a non-absolute pointer must be refused");
            let v = serde_json::to_value(&err).expect("ser");
            assert_eq!(v["details"]["source"], "file_read");
            assert_eq!(v["details"]["stage"], "invalid_path");
        }
        assert_eq!(count(&db, "stories"), 0);
        assert_eq!(store_file_count(app_data.path()), 0);
    }

    #[test]
    fn a_kind_mismatch_at_promotion_never_promotes_the_file() {
        // The slot kind is validated on the full bytes BEFORE the store
        // promotion: a mismatch refuses with NOTHING new to compensate.
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let png = tmp.path().join("image.png");
        std::fs::write(&png, PNG).expect("png");

        let err = promote_media(app_data.path(), &png, FolderMediaKind::Audio)
            .expect_err("a PNG promoted as audio must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["cause"], "media_promotion");
        assert_eq!(
            store_file_count(app_data.path()),
            0,
            "the mismatch must not leave a promoted file behind"
        );
    }

    #[test]
    fn promotion_reapplies_the_total_bound_on_the_bytes_actually_read() {
        // The probe sizes may be stale (files can grow between the
        // re-analysis and the promotion): the promotion loop re-sums the
        // bytes ACTUALLY read and refuses beyond the total bound — with
        // every promoted file (current one included) handed to the
        // compensation, which reclaims them all.
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let db = fresh_db();
        std::fs::write(tmp.path().join("a.png"), PNG).expect("a");
        std::fs::write(tmp.path().join("b.png"), PNG).expect("b");
        let retained = vec![
            RetainedMediaRef {
                node_id: "n1".into(),
                kind: FolderMediaKind::Image,
                basename: "a.png".into(),
            },
            RetainedMediaRef {
                node_id: "n2".into(),
                kind: FolderMediaKind::Image,
                basename: "b.png".into(),
            },
        ];

        // A total bound the FIRST file already exhausts: the second
        // promotion trips the re-summed total.
        let failure =
            promote_retained_media(app_data.path(), tmp.path(), &retained, PNG.len() as u64)
                .expect_err("the re-summed total must refuse");
        let v = serde_json::to_value(&failure.error).expect("ser");
        assert_eq!(v["details"]["source"], "file_read");
        assert_eq!(v["details"]["stage"], "oversize_total");
        assert!(
            !failure.promoted.is_empty(),
            "the promoted files (current one included) are handed to the compensation"
        );

        compensate_structured_creation(&db, app_data.path(), &failure.promoted);
        assert_eq!(
            store_file_count(app_data.path()),
            0,
            "every promoted file is reclaimed after the refusal"
        );
    }

    #[test]
    fn the_no_follow_open_names_oversize_distinctly() {
        // The diagnostic taxonomy must point at the RIGHT cause: a plain
        // size overflow is `Oversize`, never folded into `Unusable`
        // (`not_regular_file` at the promotion path).
        let tmp = TempDir::new().expect("tmp");
        let small_bound = 4u64;
        let over = tmp.path().join("gros.bin");
        std::fs::write(&over, b"12345678").expect("seed");
        assert!(matches!(
            open_bounded_regular(&over, small_bound),
            RegularOpen::Oversize
        ));
        let absent = tmp.path().join("absent.bin");
        assert!(matches!(
            open_bounded_regular(&absent, small_bound),
            RegularOpen::Absent
        ));
        assert!(matches!(
            open_bounded_regular(tmp.path(), small_bound),
            RegularOpen::Unusable
        ));
        let fine = tmp.path().join("ok.bin");
        std::fs::write(&fine, b"123").expect("seed");
        assert!(matches!(
            open_bounded_regular(&fine, small_bound),
            RegularOpen::Open(_, 3)
        ));
    }

    #[test]
    fn promote_maps_an_oversize_file_to_the_oversize_stage() {
        // A sparse file over the per-file ceiling (instant, no real I/O):
        // the promotion refusal must carry stage `oversize`.
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let path = tmp.path().join("enorme.png");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(MAX_MEDIA_BYTES as u64 + 1).expect("set_len");
        drop(file);

        let err = promote_media(app_data.path(), &path, FolderMediaKind::Image)
            .expect_err("an oversize media must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["stage"], "oversize");
    }

    #[test]
    fn compensation_reclaims_an_unreferenced_file_and_spares_a_referenced_one() {
        // The refcounted GC shared by every post-promotion failure path:
        // a file no assets row references is removed; a content-shared file
        // still referenced by ANOTHER story survives.
        let tmp = TempDir::new().expect("tmp");
        let app_data = TempDir::new().expect("app data");
        let mut db = fresh_db();
        let folder = clean_folder(&tmp);
        let card = accept_structured_creation(&mut db, app_data.path(), &folder).expect("accept");
        assert_eq!(store_file_count(app_data.path()), 2);

        // Collect the two promoted (hash, name) pairs from the assets rows.
        let mut stmt = db
            .conn()
            .prepare("SELECT content_hash, file_name FROM assets WHERE story_id = ?1")
            .expect("prepare");
        let promoted: Vec<(String, String)> = stmt
            .query_map(rusqlite::params![card.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");
        drop(stmt);
        assert_eq!(promoted.len(), 2);

        // Still referenced: the compensation must spare BOTH files.
        compensate_structured_creation(&db, app_data.path(), &promoted);
        assert_eq!(
            store_file_count(app_data.path()),
            2,
            "referenced files survive"
        );

        // Drop the rows (cascade via the story), then compensate again: the
        // now-unreferenced files are reclaimed.
        db.conn()
            .execute(
                "DELETE FROM stories WHERE id = ?1",
                rusqlite::params![card.id],
            )
            .expect("delete story");
        compensate_structured_creation(&db, app_data.path(), &promoted);
        assert_eq!(
            store_file_count(app_data.path()),
            0,
            "unreferenced files are reclaimed"
        );
    }
}
