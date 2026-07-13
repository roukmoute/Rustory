//! Acquires a device pack into a local staging directory.
//!
//! Runs AFTER the authoritative re-scan: given the `mount_path` of the
//! already-classified supported volume, the FAMILY of the re-scanned
//! profile (Rust authority — never re-sniffed from the mount) and the
//! family's pack reference (`pack_ref` — the `.content` SHORT_ID for a
//! Lunii, the canonical story-folder UUID for a FLAM), the reader
//! enumerates the pack (never following symlinks), validates the
//! inventory against the family's PURE rules (`domain::device::pack`:
//! the declared Lunii subset, or the STRUCTURAL-only opaque rules for a
//! FLAM whose internal format is publicly unknown), then copies the
//! retained files into the caller-provided staging directory with
//! per-file `flush` + `sync_all` and a wall-clock deadline checked
//! between files. The aggregate SHA-256 checksum is computed over the
//! bytes actually staged, in the manifest's deterministic order — the
//! SAME copy/checksum mechanics for both families (shared helpers).
//! Two DELIBERATE shared hardenings apply to both families: staging
//! writes are EXCLUSIVE (`create_new` — a path collision refuses
//! instead of truncating) and the opened source handle must keep the
//! lstat'ed `(dev, ino)` identity (a swap refuses instead of being
//! followed). Everything else on the Lunii path is the historical
//! behavior.
//!
//! Strictly READ-ONLY on the mount: the source tree is opened for
//! reading only — no write, no rename, no temp file lands on the device.
//!
//! The deadline is enforced COOPERATIVELY (checked between entries,
//! files and copy chunks). A single `read`/`write` syscall blocked by a
//! stalled mount cannot be interrupted mid-call — an accepted MVP
//! residual documented in `device-support-profile.md#Story Import
//! Contract`: the worker runs off the async runtime so the UI stays
//! responsive, and a yanked device surfaces as a kernel I/O error. Hard
//! per-syscall bounding belongs to the post-MVP transfer job contract.
//!
//! A trait so the application layer is testable without a real mount:
//! [`MockDevicePackReader`](super::mock::MockDevicePackReader) scripts
//! successes, mid-copy interruptions and invalid packs without hardware.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::domain::device::pack::{
    validate_opaque_pack_inventory, validate_pack_inventory, PackEntry, PackEntryKind,
    PackManifest, PackValidationIssue, MAX_IMPORT_PACK_BYTES, MAX_IMPORT_PACK_FILES,
    MAX_PACK_ASSET_DEPTH,
};
use crate::domain::device::{
    DeviceFamily, FLAM_HIDDEN_STORY_DIR, FLAM_STORY_DIR, LUNII_CONTENT_DIR,
};
use crate::domain::shared::AppError;
use crate::infrastructure::filesystem::io_error_kind_tag;

/// Result of a successful acquisition: the deterministic manifest of the
/// staged files (actual copied sizes) plus the aggregate SHA-256 hex
/// checksum over `rel_path \0 content` in manifest order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquiredPack {
    pub manifest: PackManifest,
    pub checksum: String,
}

/// Copies a validated pack from the mount into `staging_dir`. `pack_ref`
/// is the family's own pack reference: the uppercase `.content`
/// SHORT_ID for a Lunii, the canonical lowercase story-folder UUID for a
/// FLAM. `hidden` is the SELECTED index entry's visibility fact (the
/// index is authoritative): a FLAM acquisition reads `str.hidden/<uuid>/`
/// for a hidden entry and `str/<uuid>/` for a visible one — NEVER the
/// other root, so a same-UUID folder on the wrong root can never be
/// acquired in its place. A Lunii pack lives at `.content/<SHORT_ID>`
/// regardless of visibility, so the Lunii path ignores the flag. MUST
/// respect the `budget` wall-clock deadline so a stalled mount cannot
/// keep the `spawn_blocking` worker alive past the command budget.
pub trait DevicePackReader: Send + Sync + 'static {
    fn acquire_pack(
        &self,
        mount_path: &Path,
        family: DeviceFamily,
        pack_ref: &str,
        hidden: bool,
        staging_dir: &Path,
        budget: Duration,
    ) -> Result<AcquiredPack, AppError>;
}

/// Production reader: stdlib filesystem walks + copies, with an internal
/// per-family dispatch behind the shared trait (the adapter contract —
/// one implementation, family passed from the profile).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDevicePackReader;

/// Streaming copy buffer. 64 KB amortizes syscalls on packs that weigh
/// hundreds of MB without holding more than a fixed slice in memory.
const COPY_BUF_BYTES: usize = 64 * 1024;

impl DevicePackReader for SystemDevicePackReader {
    fn acquire_pack(
        &self,
        mount_path: &Path,
        family: DeviceFamily,
        pack_ref: &str,
        hidden: bool,
        staging_dir: &Path,
        budget: Duration,
    ) -> Result<AcquiredPack, AppError> {
        match family {
            // A Lunii pack lives at `.content/<SHORT_ID>` regardless of
            // its `.pi` vs `.pi.hidden` listing — the flag is a FLAM fact.
            DeviceFamily::Lunii => acquire_lunii_pack(mount_path, pack_ref, staging_dir, budget),
            DeviceFamily::Flam => {
                acquire_flam_pack(mount_path, pack_ref, hidden, staging_dir, budget)
            }
        }
    }
}

/// Historical Lunii acquisition, extracted from the pre-FLAM
/// implementation. Its structure, bounds and error copy are unchanged
/// (fixtures prove it); the only behavior deltas are the two DELIBERATE
/// shared hardenings (exclusive staging creation + source identity
/// re-check), which refuse explicitly where the historical code could
/// silently truncate or follow a swapped entry.
fn acquire_lunii_pack(
    mount_path: &Path,
    short_id: &str,
    staging_dir: &Path,
    budget: Duration,
) -> Result<AcquiredPack, AppError> {
    let started = Instant::now();
    let pack_dir = mount_path.join(LUNII_CONTENT_DIR).join(short_id);

    // The pack folder itself must be a real directory (not a symlink
    // pretending to be one). Absence is the recoverable "pack left
    // the device between index read and acquisition" branch.
    match fs::symlink_metadata(&pack_dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(pack_invalid_error("pack_root_not_a_directory")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(pack_missing_error());
        }
        Err(err) => return Err(fs_read_error(DeviceFamily::Lunii, io_error_kind_tag(&err))),
    }

    // 1. Bounded enumeration (never follows symlinks, never recurses
    //    past the depth the domain would refuse anyway).
    let mut entries: Vec<PackEntry> = Vec::new();
    walk_pack_dir(
        DeviceFamily::Lunii,
        &pack_dir,
        &mut Vec::new(),
        &mut entries,
        started,
        budget,
    )?;

    // 2. Pure structural validation → deterministic manifest.
    let manifest = validate_pack_inventory(&entries).map_err(|issue| validation_error(&issue))?;

    // 3. Copy file-by-file in manifest order, hashing the staged bytes.
    let (staged_files, checksum) = stage_manifest_files(
        DeviceFamily::Lunii,
        &pack_dir,
        &manifest,
        staging_dir,
        started,
        budget,
    )?;

    // 4. Re-validate the ACTUAL staged shape (a required file may have
    //    been truncated to empty between enumeration and copy).
    let staged_entries: Vec<PackEntry> = staged_files
        .iter()
        .map(|f| PackEntry {
            rel_path: f.rel_path.clone(),
            kind: PackEntryKind::File,
            size: f.size,
        })
        .collect();
    let manifest =
        validate_pack_inventory(&staged_entries).map_err(|issue| validation_error(&issue))?;

    Ok(AcquiredPack { manifest, checksum })
}

/// FLAM acquisition — a raw, OPAQUE, all-or-nothing copy (see
/// `device-support-profile.md#FLAM library inventory & story import`).
/// The story folder is located under the ONE root the selected index
/// entry owns — `str/<uuid>/` for a visible entry, `str.hidden/<uuid>/`
/// for a hidden one; the index is authoritative, the other root is
/// NEVER consulted (a same-UUID folder there cannot be acquired in its
/// place). The walk is no-follow end to end and the validation is
/// STRUCTURAL only
/// (no entry-name whitelist — the internal format is publicly unknown),
/// with the Lunii bounds reused. Copy/checksum mechanics are the SHARED
/// helpers — byte fidelity and determinism are the same contract as the
/// Lunii import.
fn acquire_flam_pack(
    mount_path: &Path,
    uuid: &str,
    hidden: bool,
    staging_dir: &Path,
    budget: Duration,
) -> Result<AcquiredPack, AppError> {
    let started = Instant::now();

    let pack_dir = match locate_flam_story_dir(mount_path, uuid, hidden) {
        Ok(Some(dir)) => dir,
        Ok(None) => return Err(pack_missing_error()),
        Err(err) => return Err(fs_read_error(DeviceFamily::Flam, io_error_kind_tag(&err))),
    };

    // 1. Bounded no-follow enumeration (shared walker — lstat per entry).
    let mut entries: Vec<PackEntry> = Vec::new();
    walk_pack_dir(
        DeviceFamily::Flam,
        &pack_dir,
        &mut Vec::new(),
        &mut entries,
        started,
        budget,
    )?;

    // 2. Pure STRUCTURAL validation → deterministic manifest (opaque
    //    rules: regular entries only, reused bounds, empty pack refused).
    let manifest =
        validate_opaque_pack_inventory(&entries).map_err(|issue| validation_error(&issue))?;

    // 3. Copy file-by-file in manifest order, hashing the staged bytes
    //    (shared mechanics — per-file fsync, TOCTOU re-check, deadline).
    let (staged_files, checksum) = stage_manifest_files(
        DeviceFamily::Flam,
        &pack_dir,
        &manifest,
        staging_dir,
        started,
        budget,
    )?;

    // 4. Re-validate the ACTUAL staged shape (all-or-nothing on what was
    //    really staged, not on a stale enumeration).
    let staged_entries: Vec<PackEntry> = staged_files
        .iter()
        .map(|f| PackEntry {
            rel_path: f.rel_path.clone(),
            kind: PackEntryKind::File,
            size: f.size,
        })
        .collect();
    let manifest = validate_opaque_pack_inventory(&staged_entries)
        .map_err(|issue| validation_error(&issue))?;

    Ok(AcquiredPack { manifest, checksum })
}

/// Locate a FLAM story directory by its canonical UUID under the ONE
/// root its index entry owns (`hidden` ⇒ `str.hidden/`, else `str/` —
/// the index is authoritative, the other root is never probed). The
/// entry must be a REAL directory (no-follow — a symlink does not count
/// and does not locate). `Ok(None)` means "not on the device" (the
/// recoverable `pack_missing` branch); an lstat failure other than
/// NotFound propagates as an I/O error.
fn locate_flam_story_dir(
    mount_path: &Path,
    uuid: &str,
    hidden: bool,
) -> Result<Option<PathBuf>, std::io::Error> {
    let root = if hidden {
        FLAM_HIDDEN_STORY_DIR
    } else {
        FLAM_STORY_DIR
    };
    let candidate = mount_path.join(root).join(uuid);
    match fs::symlink_metadata(&candidate) {
        Ok(meta) if meta.is_dir() => Ok(Some(candidate)),
        // A symlink/irregular entry at the story path does not count as
        // content (no-follow).
        Ok(_) => Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Recursive bounded walk. `rel_components` tracks the path below the
/// pack root; recursion stops at the depth the domain refuses, so a
/// hostile deep tree can neither exhaust the stack nor hide content
/// (the too-deep DIRECTORY entry itself is recorded and refused).
/// Family-shared: the per-entry classification is identical for both
/// families (lstat, no-follow); only the downstream validation differs.
fn walk_pack_dir(
    family: DeviceFamily,
    dir: &Path,
    rel_components: &mut Vec<String>,
    out: &mut Vec<PackEntry>,
    started: Instant,
    budget: Duration,
) -> Result<(), AppError> {
    let read_dir =
        fs::read_dir(dir).map_err(|err| fs_read_error(family, io_error_kind_tag(&err)))?;
    for dir_entry in read_dir {
        if started.elapsed() >= budget {
            return Err(read_timeout_error(family, started.elapsed()));
        }
        // Defense in depth: stop enumerating long before an adversarial
        // file count could exhaust memory — the domain bound would refuse
        // the pack anyway.
        if out.len() > MAX_IMPORT_PACK_FILES.saturating_mul(2) {
            return Err(pack_oversize_error());
        }

        let dir_entry = dir_entry.map_err(|err| fs_read_error(family, io_error_kind_tag(&err)))?;
        let name = match dir_entry.file_name().into_string() {
            Ok(name) => name,
            // A non-UTF-8 name cannot be carried by a manifest rel_path.
            Err(_) => return Err(pack_invalid_error("non_utf8_entry_name")),
        };

        // Probe with `symlink_metadata` (lstat): `DirEntry::metadata`
        // follows symlinks on some platforms, which would hide a link as
        // its target. lstat keeps a symlink classified as a symlink so
        // the validator can refuse it.
        let meta = fs::symlink_metadata(dir_entry.path())
            .map_err(|err| fs_read_error(family, io_error_kind_tag(&err)))?;
        let kind = if meta.file_type().is_symlink() {
            PackEntryKind::Symlink
        } else if meta.is_dir() {
            PackEntryKind::Dir
        } else if meta.is_file() {
            PackEntryKind::File
        } else {
            PackEntryKind::Other
        };

        rel_components.push(name);
        let rel_path = rel_components.join("/");
        let depth = rel_components.len();
        out.push(PackEntry {
            rel_path,
            kind,
            size: if kind == PackEntryKind::File {
                meta.len()
            } else {
                0
            },
        });

        // Recurse only into directories the domain accepts (≤ the asset
        // depth); a deeper dir is already recorded and will be refused.
        if kind == PackEntryKind::Dir && depth <= MAX_PACK_ASSET_DEPTH {
            // Re-probe IMMEDIATELY before recursing (the same
            // "re-probe right before opening" discipline as the file
            // copy): a directory swapped for a symlink between the
            // lstat above and the recursive `read_dir` would otherwise
            // be FOLLOWED, enumerating bytes from OUTSIDE the pack that
            // the staging would then re-traverse and copy as pack
            // content. A changed entry refuses the whole pack.
            let re_probed = fs::symlink_metadata(dir_entry.path())
                .map_err(|err| fs_read_error(family, io_error_kind_tag(&err)))?;
            if re_probed.file_type().is_symlink() || !re_probed.is_dir() {
                return Err(pack_invalid_error("dir_swapped_between_stat_and_recursion"));
            }
            walk_pack_dir(
                family,
                &dir_entry.path(),
                rel_components,
                out,
                started,
                budget,
            )?;
        }
        rel_components.pop();
    }
    Ok(())
}

/// Copy every manifest file from `pack_dir` into `staging_dir` in
/// manifest order, hashing `rel_path \0 content` into the aggregate
/// checksum, re-checking the deadline between files and re-probing each
/// source with `symlink_metadata` right before opening it (closes the
/// enumerate→copy TOCTOU window). Returns the ACTUALLY staged files
/// (real copied sizes) and the hex checksum. Shared by both families —
/// the family only selects the error copy; the exclusive-creation and
/// identity-re-check hardenings apply to both.
fn stage_manifest_files(
    family: DeviceFamily,
    pack_dir: &Path,
    manifest: &PackManifest,
    staging_dir: &Path,
    started: Instant,
    budget: Duration,
) -> Result<(Vec<crate::domain::device::pack::PackFile>, String), AppError> {
    let mut hasher = Sha256::new();
    let mut staged_files = Vec::with_capacity(manifest.files.len());
    let mut staged_total: u64 = 0;
    // Directories THIS staging created: a directory that already exists
    // without being ours is a path collision (case-insensitive /
    // normalizing local filesystem merging two distinct source dirs) —
    // refused like the file collisions, never a silently merged tree.
    let mut created_dirs: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for file in &manifest.files {
        if started.elapsed() >= budget {
            return Err(read_timeout_error(family, started.elapsed()));
        }

        let src = join_rel_path(pack_dir, &file.rel_path);
        let meta = fs::symlink_metadata(&src)
            .map_err(|err| fs_read_error(family, io_error_kind_tag(&err)))?;
        if !meta.is_file() {
            return Err(pack_invalid_error("source_changed_to_non_regular_file"));
        }

        let dst = join_rel_path(staging_dir, &file.rel_path);
        if let Some(parent) = dst.parent() {
            ensure_staging_dirs(staging_dir, parent, &mut created_dirs)?;
        }

        hasher.update(file.rel_path.as_bytes());
        hasher.update([0u8]);

        let copied = copy_one_file(family, &src, &meta, &dst, &mut hasher, started, budget)?;
        staged_total = staged_total.saturating_add(copied);
        // The source may have grown since enumeration; the byte bound
        // holds on what we actually stage, not on a stale stat.
        if staged_total > MAX_IMPORT_PACK_BYTES {
            return Err(pack_oversize_error());
        }
        staged_files.push(crate::domain::device::pack::PackFile {
            rel_path: file.rel_path.clone(),
            size: copied,
        });
    }

    Ok((staged_files, format!("{:x}", hasher.finalize())))
}

/// Create every ancestor of `target` STRICTLY below `staging_root`,
/// EXCLUSIVELY (`create_dir`, never `create_dir_all`): a directory that
/// already exists without having been created by THIS staging is a path
/// collision (two distinct source directories merging on a
/// case-insensitive / Unicode-normalizing local filesystem) and refuses
/// the pack — the staged tree must be the source tree or nothing.
fn ensure_staging_dirs(
    staging_root: &Path,
    target: &Path,
    created: &mut std::collections::HashSet<PathBuf>,
) -> Result<(), AppError> {
    let mut pending: Vec<&Path> = Vec::new();
    let mut cursor = target;
    while cursor != staging_root {
        pending.push(cursor);
        match cursor.parent() {
            Some(parent) => cursor = parent,
            None => break,
        }
    }
    for dir in pending.into_iter().rev() {
        if created.contains(dir) {
            continue;
        }
        match fs::create_dir(dir) {
            Ok(()) => {
                created.insert(dir.to_path_buf());
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(pack_invalid_error("staging_path_collision"));
            }
            Err(err) => return Err(staging_write_error(io_error_kind_tag(&err))),
        }
    }
    Ok(())
}

/// Join a validated forward-slash rel_path under `base` component by
/// component. The components come from our own enumeration (never `..`,
/// never absolute), so this is a plain controlled join.
fn join_rel_path(base: &Path, rel_path: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for component in rel_path.split('/') {
        out.push(component);
    }
    out
}

/// Copy `src` → `dst` streaming through `hasher`, with deadline checks
/// between chunks. Returns the byte count actually staged. The staged
/// file is flushed and fsynced so a post-promotion crash cannot leave
/// silently empty payloads behind a committed DB row.
///
/// `expected` is the `symlink_metadata` (lstat) taken by the caller just
/// before this call. After `File::open`, the OPEN HANDLE is fstat'ed and
/// must still be a regular file with the same `(dev, ino)` identity —
/// closing the lstat→open TOCTOU window where the path could be swapped
/// for a symlink (the open would silently follow it).
fn copy_one_file(
    family: DeviceFamily,
    src: &Path,
    expected: &fs::Metadata,
    dst: &Path,
    hasher: &mut Sha256,
    started: Instant,
    budget: Duration,
) -> Result<u64, AppError> {
    let mut reader =
        File::open(src).map_err(|err| fs_read_error(family, io_error_kind_tag(&err)))?;
    let opened = reader
        .metadata()
        .map_err(|err| fs_read_error(family, io_error_kind_tag(&err)))?;
    if !opened.is_file() {
        return Err(pack_invalid_error("source_changed_to_non_regular_file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != expected.dev() || opened.ino() != expected.ino() {
            return Err(pack_invalid_error("source_swapped_between_stat_and_open"));
        }
    }
    #[cfg(not(unix))]
    {
        // Windows: the source is a FAT volume with no symlink support and
        // the lstat-equivalent above already refused reparse points; the
        // handle is verified to be a regular file, which is the available
        // guarantee.
        let _ = expected;
    }
    // EXCLUSIVE creation: on a case-insensitive / Unicode-normalizing
    // local filesystem, two DISTINCT source rel_paths can collide onto
    // the same staging path — a plain `File::create` would silently
    // truncate the first file while the manifest/checksum still claim
    // both were preserved. `create_new` turns the collision into an
    // explicit `pack_invalid` refusal (all-or-nothing, never an altered
    // pack).
    let mut writer = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                pack_invalid_error("staging_path_collision")
            } else {
                staging_write_error(io_error_kind_tag(&err))
            }
        })?;

    let mut buf = vec![0u8; COPY_BUF_BYTES];
    let mut copied: u64 = 0;
    loop {
        if started.elapsed() >= budget {
            return Err(read_timeout_error(family, started.elapsed()));
        }
        let read = reader
            .read(&mut buf)
            .map_err(|err| fs_read_error(family, io_error_kind_tag(&err)))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        writer
            .write_all(&buf[..read])
            .map_err(|err| staging_write_error(io_error_kind_tag(&err)))?;
        copied += read as u64;
        // A source growing past the pack bound mid-copy must not fill the
        // local disk — stop at the bound, the caller refuses the pack.
        if copied > MAX_IMPORT_PACK_BYTES {
            return Err(pack_oversize_error());
        }
    }
    writer
        .flush()
        .map_err(|err| staging_write_error(io_error_kind_tag(&err)))?;
    writer
        .sync_all()
        .map_err(|err| staging_write_error(io_error_kind_tag(&err)))?;
    Ok(copied)
}

fn validation_error(issue: &PackValidationIssue) -> AppError {
    match issue.source_tag() {
        "pack_oversize" => pack_oversize_error(),
        _ => pack_invalid_error(validation_cause(issue)),
    }
}

/// Closed-set `details.cause` tokens for `pack_invalid` so support can
/// triage WHICH structural rule refused the pack without the rel_path
/// (device file names stay out of the wire payload).
fn validation_cause(issue: &PackValidationIssue) -> &'static str {
    match issue {
        PackValidationIssue::MissingRequired { .. } => "missing_required",
        PackValidationIssue::EmptyRequired { .. } => "empty_required",
        PackValidationIssue::UnknownEntry { .. } => "unknown_entry",
        PackValidationIssue::NotARegularFile { .. } => "not_a_regular_file",
        PackValidationIssue::TooDeep { .. } => "too_deep",
        PackValidationIssue::EmptyPack => "empty_pack",
        PackValidationIssue::EmptyDirectory { .. } => "empty_directory",
        PackValidationIssue::TooManyFiles { .. } | PackValidationIssue::TooLarge { .. } => {
            "oversize"
        }
    }
}

// Closed user-facing copy — sober, no OS message, no path (PII rules).
// Family-correct where the copy named the Lunii: the Lunii path keeps
// its historical wording VERBATIM; the FLAM path was born with the
// device-generic wording (product-language.md Change Control). The
// family-neutral refusals (pack_missing / pack_invalid / pack_oversize /
// staging_write) are shared verbatim.

fn pack_missing_error() -> AppError {
    AppError::import_failed(
        "Copie impossible: l'histoire est introuvable sur l'appareil.",
        "Vérifie l'appareil puis relance la lecture de sa bibliothèque.",
    )
    .with_details(serde_json::json!({
        "source": "pack_missing",
    }))
}

fn pack_invalid_error(cause: &'static str) -> AppError {
    AppError::import_failed(
        "Copie impossible: le contenu de l'histoire n'est pas dans un format supporté.",
        "Consulte le profil de support pour les contenus pris en charge.",
    )
    .with_details(serde_json::json!({
        "source": "pack_invalid",
        "cause": cause,
    }))
}

fn pack_oversize_error() -> AppError {
    AppError::import_failed(
        "Copie impossible: l'histoire dépasse la taille maximale supportée.",
        "Consulte le profil de support pour les limites de copie.",
    )
    .with_details(serde_json::json!({
        "source": "pack_oversize",
    }))
}

fn fs_read_error(family: DeviceFamily, kind: &'static str) -> AppError {
    let action = match family {
        DeviceFamily::Lunii => "Vérifie la connexion de la Lunii puis réessaie la copie.",
        DeviceFamily::Flam => "Vérifie la connexion de l'appareil puis réessaie la copie.",
    };
    AppError::import_failed(
        "Copie impossible: lecture de l'appareil interrompue.",
        action,
    )
    .with_details(serde_json::json!({
        "source": "fs_read",
        "kind": kind,
    }))
}

fn staging_write_error(kind: &'static str) -> AppError {
    AppError::import_failed(
        "Copie impossible: écriture locale refusée.",
        "Vérifie l'espace disque et les permissions puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "staging_write",
        "kind": kind,
    }))
}

fn read_timeout_error(family: DeviceFamily, elapsed: Duration) -> AppError {
    let action = match family {
        DeviceFamily::Lunii => "Réessaie la copie ; si le problème persiste, rebranche la Lunii.",
        DeviceFamily::Flam => "Réessaie la copie ; si le problème persiste, rebranche l'appareil.",
    };
    AppError::import_failed(
        "Copie impossible: l'appareil met trop de temps à répondre.",
        action,
    )
    .with_details(serde_json::json!({
        "source": "read_timeout",
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::device::fixtures::{
        temp_flam_mount_with_library, temp_lunii_mount_with_pack_content, write_plausible_pack,
        FlamLibraryEntry,
    };

    fn uuid(tail: [u8; 4]) -> [u8; 16] {
        let mut b = [0xAB; 16];
        b[12..16].copy_from_slice(&tail);
        b
    }

    fn budget() -> Duration {
        Duration::from_secs(30)
    }

    fn staging() -> tempfile::TempDir {
        tempfile::tempdir().expect("staging tempdir")
    }

    fn acquire_lunii(
        mount: &Path,
        short_id: &str,
        staging_dir: &Path,
        budget: Duration,
    ) -> Result<AcquiredPack, AppError> {
        SystemDevicePackReader.acquire_pack(
            mount,
            DeviceFamily::Lunii,
            short_id,
            false,
            staging_dir,
            budget,
        )
    }

    fn acquire_flam(
        mount: &Path,
        uuid: &str,
        staging_dir: &Path,
        budget: Duration,
    ) -> Result<AcquiredPack, AppError> {
        acquire_flam_at(mount, uuid, false, staging_dir, budget)
    }

    fn acquire_flam_at(
        mount: &Path,
        uuid: &str,
        hidden: bool,
        staging_dir: &Path,
        budget: Duration,
    ) -> Result<AcquiredPack, AppError> {
        SystemDevicePackReader.acquire_pack(
            mount,
            DeviceFamily::Flam,
            uuid,
            hidden,
            staging_dir,
            budget,
        )
    }

    /// Discipline: every pack-read refusal constructor must be ACTIONABLE
    /// — a non-empty cause AND a non-empty next gesture — so the import UI
    /// never surfaces an opaque refusal (AC1). Locks the canonical fr copy
    /// without adding any new error code / `details.source`, for BOTH
    /// family variants of the family-correct constructors.
    #[test]
    fn every_pack_read_refusal_constructor_is_actionable() {
        let refusals = [
            pack_missing_error(),
            pack_invalid_error("missing_required"),
            pack_invalid_error("empty_pack"),
            pack_oversize_error(),
            fs_read_error(DeviceFamily::Lunii, "io"),
            fs_read_error(DeviceFamily::Flam, "io"),
            staging_write_error("io"),
            read_timeout_error(DeviceFamily::Lunii, Duration::from_secs(1)),
            read_timeout_error(DeviceFamily::Flam, Duration::from_secs(1)),
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
    fn family_correct_copy_lunii_stays_verbatim_and_flam_never_names_the_lunii() {
        // AC2 family isolation: the Lunii next gestures do not change by
        // a byte; the FLAM ones never claim a Lunii is involved.
        assert_eq!(
            fs_read_error(DeviceFamily::Lunii, "io")
                .user_action
                .as_deref(),
            Some("Vérifie la connexion de la Lunii puis réessaie la copie.")
        );
        assert_eq!(
            read_timeout_error(DeviceFamily::Lunii, Duration::from_secs(1))
                .user_action
                .as_deref(),
            Some("Réessaie la copie ; si le problème persiste, rebranche la Lunii.")
        );
        for err in [
            fs_read_error(DeviceFamily::Flam, "io"),
            read_timeout_error(DeviceFamily::Flam, Duration::from_secs(1)),
        ] {
            assert!(
                !err.user_action.as_deref().unwrap_or("").contains("Lunii"),
                "FLAM copy must not name the Lunii: {err:?}"
            );
        }
    }

    #[test]
    fn acquires_a_plausible_pack_into_staging_with_deterministic_checksum() {
        let pack_uuid = uuid([0xFA, 0xC5, 0x56, 0x2D]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);

        let stage_a = staging();
        let acquired_a =
            acquire_lunii(&mount, &short_id, stage_a.path(), budget()).expect("acquire");

        assert!(acquired_a.manifest.files.len() >= 4);
        assert_eq!(acquired_a.checksum.len(), 64);
        assert!(acquired_a.checksum.chars().all(|c| c.is_ascii_hexdigit()));

        // Staged bytes match the source byte-for-byte.
        for f in &acquired_a.manifest.files {
            let src = join_rel_path(&mount.join(".content").join(&short_id), &f.rel_path);
            let dst = join_rel_path(stage_a.path(), &f.rel_path);
            assert_eq!(
                std::fs::read(&src).expect("src"),
                std::fs::read(&dst).expect("dst"),
                "{} must be copied verbatim",
                f.rel_path
            );
        }

        // Same pack acquired twice → same checksum (deterministic order).
        let stage_b = staging();
        let acquired_b =
            acquire_lunii(&mount, &short_id, stage_b.path(), budget()).expect("acquire again");
        assert_eq!(acquired_a.checksum, acquired_b.checksum);
        assert_eq!(acquired_a.manifest, acquired_b.manifest);
    }

    #[test]
    fn missing_pack_dir_maps_to_pack_missing() {
        let pack_uuid = uuid([1, 2, 3, 4]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let err = acquire_lunii(&mount, "DEADBEEF", staging().path(), budget())
            .expect_err("absent pack must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "IMPORT_FAILED");
        assert_eq!(v["details"]["source"], "pack_missing");
    }

    #[test]
    fn unknown_entry_in_pack_refuses_with_pack_invalid_and_stages_nothing() {
        let pack_uuid = uuid([5, 5, 5, 5]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);
        let pack_dir = mount.join(".content").join(&short_id);
        std::fs::write(pack_dir.join("evil.bin"), b"x").expect("seed unknown entry");

        let stage = staging();
        let err = acquire_lunii(&mount, &short_id, stage.path(), budget())
            .expect_err("unknown entry must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(v["details"]["cause"], "unknown_entry");
        // All-or-nothing: validation runs BEFORE any copy.
        assert!(
            std::fs::read_dir(stage.path())
                .expect("read staging")
                .next()
                .is_none(),
            "staging must stay empty on refusal"
        );
    }

    #[test]
    fn missing_required_file_refuses_with_pack_invalid() {
        let pack_uuid = uuid([6, 6, 6, 6]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);
        std::fs::remove_file(mount.join(".content").join(&short_id).join("ni"))
            .expect("drop required file");

        let err = acquire_lunii(&mount, &short_id, staging().path(), budget())
            .expect_err("missing required must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(v["details"]["cause"], "missing_required");
    }

    #[test]
    fn os_cruft_is_skipped_not_copied_and_not_fatal() {
        let pack_uuid = uuid([7, 7, 7, 7]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);
        let pack_dir = mount.join(".content").join(&short_id);
        std::fs::write(pack_dir.join("Thumbs.db"), b"cruft").expect("seed cruft");
        std::fs::write(pack_dir.join("._resource"), b"cruft").expect("seed apple double");

        let stage = staging();
        let acquired = acquire_lunii(&mount, &short_id, stage.path(), budget())
            .expect("cruft must not refuse");
        assert!(acquired
            .manifest
            .files
            .iter()
            .all(|f| !f.rel_path.contains("Thumbs") && !f.rel_path.starts_with("._")));
        assert!(!stage.path().join("Thumbs.db").exists());
        assert!(!stage.path().join("._resource").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_inside_pack_refuses_with_pack_invalid() {
        let pack_uuid = uuid([8, 8, 8, 8]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);
        let pack_dir = mount.join(".content").join(&short_id);
        std::os::unix::fs::symlink(pack_dir.join("ni"), pack_dir.join("rf").join("link"))
            .expect("mklink");

        let err = acquire_lunii(&mount, &short_id, staging().path(), budget())
            .expect_err("symlink must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(v["details"]["cause"], "not_a_regular_file");
    }

    #[test]
    fn too_deep_directory_refuses_without_unbounded_recursion() {
        let pack_uuid = uuid([9, 9, 9, 9]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);
        let deep = mount
            .join(".content")
            .join(&short_id)
            .join("rf")
            .join("a")
            .join("b")
            .join("c");
        std::fs::create_dir_all(&deep).expect("mk deep tree");
        std::fs::write(deep.join("hidden"), b"nested").expect("seed nested file");

        let err = acquire_lunii(&mount, &short_id, staging().path(), budget())
            .expect_err("too-deep tree must refuse, never silently skip");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(v["details"]["cause"], "too_deep");
    }

    #[cfg(unix)]
    #[test]
    fn copy_refuses_a_source_swapped_between_stat_and_open() {
        // Model the lstat→open race: the caller captured the metadata of
        // the original file, then the path is swapped for ANOTHER inode
        // (the symlink-swap attack reduces to this identity change once
        // `open` has followed the link). The handle fstat must refuse.
        let dir = tempfile::tempdir().expect("tempdir");
        let original = dir.path().join("ni");
        std::fs::write(&original, b"ORIGINAL").expect("seed original");
        let expected = std::fs::symlink_metadata(&original).expect("lstat original");

        let other = dir.path().join("other");
        std::fs::write(&other, b"SWAPPED").expect("seed other");
        std::fs::rename(&other, &original).expect("swap inode under the same path");

        let stage = staging();
        let mut hasher = Sha256::new();
        let err = copy_one_file(
            DeviceFamily::Lunii,
            &original,
            &expected,
            &stage.path().join("ni"),
            &mut hasher,
            std::time::Instant::now(),
            budget(),
        )
        .expect_err("identity change must refuse the copy");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(
            v["details"]["cause"],
            "source_swapped_between_stat_and_open"
        );
    }

    #[test]
    fn a_staging_directory_collision_refuses_the_pack_instead_of_merging() {
        // Directory pendant of the file-collision refusal: a directory
        // that already exists in the staging without having been created
        // by this acquisition (the on-disk shape of two source dirs
        // merging on a case-insensitive target) refuses the pack.
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID)]);
        let stage = staging();
        // Pre-occupy the directory path the opaque fixture will need.
        std::fs::create_dir(stage.path().join("data")).expect("pre-occupy dir");
        let err = acquire_flam(&mount, FLAM_UUID, stage.path(), budget())
            .expect_err("an occupied staging directory must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(v["details"]["cause"], "staging_path_collision");
    }

    #[test]
    fn copy_refuses_an_already_existing_staging_destination_instead_of_truncating() {
        // Collision-safety: two distinct source rel_paths colliding onto
        // one staging path (case-insensitive / normalizing target FS)
        // must refuse the pack — never silently truncate the first copy
        // while the manifest claims both files were preserved. The
        // mechanism is exclusive creation: a pre-existing destination
        // refuses with `pack_invalid` / `staging_path_collision`.
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("payload");
        std::fs::write(&src, b"SOURCE").expect("seed source");
        let expected = std::fs::symlink_metadata(&src).expect("lstat");

        let stage = staging();
        let dst = stage.path().join("collide");
        std::fs::write(&dst, b"FIRST").expect("seed first occupant");

        for family in [DeviceFamily::Lunii, DeviceFamily::Flam] {
            let mut hasher = Sha256::new();
            let err = copy_one_file(
                family,
                &src,
                &expected,
                &dst,
                &mut hasher,
                std::time::Instant::now(),
                budget(),
            )
            .expect_err("an occupied destination must refuse, never truncate");
            let v = serde_json::to_value(&err).expect("ser");
            assert_eq!(v["details"]["source"], "pack_invalid", "{family:?}");
            assert_eq!(
                v["details"]["cause"], "staging_path_collision",
                "{family:?}"
            );
            // The first occupant is untouched.
            assert_eq!(std::fs::read(&dst).expect("read"), b"FIRST", "{family:?}");
        }
    }

    #[test]
    fn zero_budget_aborts_with_read_timeout_before_copying() {
        let pack_uuid = uuid([4, 3, 2, 1]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);

        let err = acquire_lunii(&mount, &short_id, staging().path(), Duration::ZERO)
            .expect_err("zero budget must abort");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "read_timeout");
    }

    #[test]
    fn source_mount_is_never_mutated_by_an_acquisition() {
        let pack_uuid = uuid([0x0A, 0x0B, 0x0C, 0x0D]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);
        let pack_dir = mount.join(".content").join(&short_id);

        let snapshot = |dir: &Path| -> Vec<(String, Vec<u8>)> {
            let mut out = Vec::new();
            let mut stack = vec![dir.to_path_buf()];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).expect("read") {
                    let e = e.expect("entry");
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        out.push((
                            p.to_string_lossy().into_owned(),
                            std::fs::read(&p).expect("bytes"),
                        ));
                    }
                }
            }
            out.sort();
            out
        };

        let before = snapshot(&pack_dir);
        acquire_lunii(&mount, &short_id, staging().path(), budget()).expect("acquire");
        let after = snapshot(&pack_dir);
        assert_eq!(
            before, after,
            "the device pack must be byte-identical after acquisition"
        );
    }

    #[test]
    fn write_plausible_pack_helper_produces_a_valid_pack() {
        // Guards the fixture itself: if the helper drifts away from the
        // declared subset, every dependent test would fail confusingly.
        let dir = tempfile::tempdir().expect("tempdir");
        write_plausible_pack(dir.path());
        let stage = staging();
        // A pack root without mount context: acquire via a synthetic mount.
        let mount = tempfile::tempdir().expect("mount");
        let content = mount.path().join(".content").join("01020304");
        std::fs::create_dir_all(content.parent().unwrap()).expect("mk .content");
        std::fs::rename(dir.path(), &content).ok();
        if !content.exists() {
            // Cross-device rename fallback: rebuild in place.
            std::fs::create_dir_all(&content).expect("mk pack dir");
            write_plausible_pack(&content);
        }
        acquire_lunii(mount.path(), "01020304", stage.path(), budget())
            .expect("the plausible pack fixture must validate");
    }

    // ---------------- FLAM acquisition (opaque, structural) ----------------

    const FLAM_UUID: &str = "12345678-9abc-def0-1122-334455667788";

    #[test]
    fn acquires_an_opaque_flam_story_with_deterministic_checksum_and_byte_fidelity() {
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID)]);
        let story_dir = mount.join(FLAM_STORY_DIR).join(FLAM_UUID);

        let stage_a = staging();
        let acquired_a =
            acquire_flam(&mount, FLAM_UUID, stage_a.path(), budget()).expect("acquire");
        // The opaque fixture stages every regular file, nested one included.
        assert_eq!(acquired_a.manifest.files.len(), 3);
        assert_eq!(acquired_a.checksum.len(), 64);
        for f in &acquired_a.manifest.files {
            let src = join_rel_path(&story_dir, &f.rel_path);
            let dst = join_rel_path(stage_a.path(), &f.rel_path);
            assert_eq!(
                std::fs::read(&src).expect("src"),
                std::fs::read(&dst).expect("dst"),
                "{} must be copied verbatim",
                f.rel_path
            );
        }

        let stage_b = staging();
        let acquired_b =
            acquire_flam(&mount, FLAM_UUID, stage_b.path(), budget()).expect("acquire again");
        assert_eq!(acquired_a.checksum, acquired_b.checksum);
        assert_eq!(acquired_a.manifest, acquired_b.manifest);
    }

    #[test]
    fn acquires_a_hidden_flam_story_from_the_hidden_root() {
        // The SELECTED index entry owns its root: hidden ⇒ str.hidden/.
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::hidden(FLAM_UUID)]);
        let acquired = acquire_flam_at(&mount, FLAM_UUID, true, staging().path(), budget())
            .expect("hidden story must acquire");
        assert_eq!(acquired.manifest.files.len(), 3);
    }

    #[test]
    fn hidden_entry_acquires_the_hidden_folder_never_a_same_uuid_visible_homonym() {
        // The index is authoritative: a hidden entry reads str.hidden/
        // ONLY — a same-UUID folder sitting on the VISIBLE root (an
        // orphan the index never selected) can never be copied in its
        // place.
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::hidden(FLAM_UUID)]);
        // Plant a DIFFERENT same-UUID folder on the visible root.
        let decoy = mount.join(FLAM_STORY_DIR).join(FLAM_UUID);
        std::fs::create_dir_all(&decoy).expect("mk decoy dir");
        std::fs::write(decoy.join("decoy.bin"), b"DECOY").expect("seed decoy");

        let stage = staging();
        let acquired = acquire_flam_at(&mount, FLAM_UUID, true, stage.path(), budget())
            .expect("hidden story must acquire");
        // The staged bytes are the HIDDEN folder's, byte for byte.
        let hidden_dir = mount.join(FLAM_HIDDEN_STORY_DIR).join(FLAM_UUID);
        for f in &acquired.manifest.files {
            assert_eq!(
                std::fs::read(join_rel_path(&hidden_dir, &f.rel_path)).expect("src"),
                std::fs::read(join_rel_path(stage.path(), &f.rel_path)).expect("dst"),
                "{} must come from the hidden root",
                f.rel_path
            );
        }
        assert!(
            !stage.path().join("decoy.bin").exists(),
            "the visible decoy must never be staged"
        );
    }

    #[test]
    fn visible_entry_never_falls_back_to_a_hidden_folder() {
        // Symmetric: a VISIBLE entry whose folder is absent from str/
        // refuses pack_missing even when str.hidden/ holds the UUID —
        // no cross-root fallback exists.
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::hidden(FLAM_UUID)]);
        let err = acquire_flam_at(&mount, FLAM_UUID, false, staging().path(), budget())
            .expect_err("a visible entry must not read the hidden root");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_missing");
    }

    #[test]
    fn missing_flam_story_dir_maps_to_pack_missing() {
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::orphan(FLAM_UUID)]);
        let err = acquire_flam(&mount, FLAM_UUID, staging().path(), budget())
            .expect_err("absent story dir must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "IMPORT_FAILED");
        assert_eq!(v["details"]["source"], "pack_missing");
    }

    #[test]
    fn empty_flam_story_dir_refuses_with_pack_invalid_empty_pack() {
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::orphan(FLAM_UUID)]);
        std::fs::create_dir_all(mount.join(FLAM_STORY_DIR).join(FLAM_UUID))
            .expect("mk empty story dir");
        let stage = staging();
        let err = acquire_flam(&mount, FLAM_UUID, stage.path(), budget())
            .expect_err("empty story dir must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(v["details"]["cause"], "empty_pack");
        assert!(
            std::fs::read_dir(stage.path())
                .expect("read staging")
                .next()
                .is_none(),
            "staging must stay empty on refusal"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_inside_flam_story_refuses_with_pack_invalid_and_stages_nothing() {
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID)]);
        let story_dir = mount.join(FLAM_STORY_DIR).join(FLAM_UUID);
        std::os::unix::fs::symlink(story_dir.join("00000001"), story_dir.join("link"))
            .expect("mklink");

        let stage = staging();
        let err = acquire_flam(&mount, FLAM_UUID, stage.path(), budget())
            .expect_err("symlink must refuse the whole pack");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(v["details"]["cause"], "not_a_regular_file");
        assert!(
            std::fs::read_dir(stage.path())
                .expect("read staging")
                .next()
                .is_none(),
            "staging must stay empty on refusal (all-or-nothing)"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_flam_story_dir_does_not_locate_and_maps_to_pack_missing() {
        // The story-dir probe is no-follow: a symlink at `str/<uuid>` is
        // not content — and the hidden root does not hold it either.
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::orphan(FLAM_UUID)]);
        let real = mount.join("elsewhere");
        std::fs::create_dir(&real).expect("mk real dir");
        std::fs::write(real.join("payload"), b"x").expect("seed payload");
        std::os::unix::fs::symlink(&real, mount.join(FLAM_STORY_DIR).join(FLAM_UUID))
            .expect("symlink story dir");
        let err = acquire_flam(&mount, FLAM_UUID, staging().path(), budget())
            .expect_err("symlinked story dir must not locate");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_missing");
    }

    #[test]
    fn empty_directory_inside_a_flam_story_refuses_with_pack_invalid() {
        // A FLAM story holding at least one file AND an empty directory
        // refuses: the files-only manifest cannot round-trip the empty
        // directory, and the contract is all-or-nothing.
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID)]);
        std::fs::create_dir(mount.join(FLAM_STORY_DIR).join(FLAM_UUID).join("empty"))
            .expect("mk empty dir");
        let stage = staging();
        let err = acquire_flam(&mount, FLAM_UUID, stage.path(), budget())
            .expect_err("an empty directory must refuse the pack");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(v["details"]["cause"], "empty_directory");
        assert!(
            std::fs::read_dir(stage.path())
                .expect("read staging")
                .next()
                .is_none(),
            "staging must stay empty on refusal (all-or-nothing)"
        );
    }

    #[test]
    fn too_deep_flam_tree_refuses_with_pack_invalid() {
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID)]);
        let deep = mount
            .join(FLAM_STORY_DIR)
            .join(FLAM_UUID)
            .join("a")
            .join("b")
            .join("c");
        std::fs::create_dir_all(&deep).expect("mk deep tree");
        std::fs::write(deep.join("hidden"), b"nested").expect("seed nested file");

        let err = acquire_flam(&mount, FLAM_UUID, staging().path(), budget())
            .expect_err("too-deep tree must refuse, never silently skip");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_invalid");
        assert_eq!(v["details"]["cause"], "too_deep");
    }

    #[test]
    fn oversize_flam_story_refuses_with_pack_oversize() {
        // A sparse file larger than the pack byte bound: refused at the
        // copy bound without filling the local disk.
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::orphan(FLAM_UUID)]);
        let story_dir = mount.join(FLAM_STORY_DIR).join(FLAM_UUID);
        std::fs::create_dir_all(&story_dir).expect("mk story dir");
        let big = File::create(story_dir.join("huge")).expect("create sparse");
        big.set_len(MAX_IMPORT_PACK_BYTES + 1).expect("set_len");

        let err = acquire_flam(&mount, FLAM_UUID, staging().path(), budget())
            .expect_err("oversize story must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_oversize");
    }

    #[test]
    fn flam_zero_budget_aborts_with_read_timeout() {
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID)]);
        let err = acquire_flam(&mount, FLAM_UUID, staging().path(), Duration::ZERO)
            .expect_err("zero budget must abort");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "read_timeout");
    }

    #[test]
    fn flam_source_mount_is_never_mutated_by_an_acquisition() {
        let (_guard, mount) = temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID)]);
        let story_dir = mount.join(FLAM_STORY_DIR).join(FLAM_UUID);

        let snapshot = |dir: &Path| -> Vec<(String, Vec<u8>)> {
            let mut out = Vec::new();
            let mut stack = vec![dir.to_path_buf()];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).expect("read") {
                    let e = e.expect("entry");
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        out.push((
                            p.to_string_lossy().into_owned(),
                            std::fs::read(&p).expect("bytes"),
                        ));
                    }
                }
            }
            out.sort();
            out
        };

        let before = snapshot(&story_dir);
        acquire_flam(&mount, FLAM_UUID, staging().path(), budget()).expect("acquire");
        let after = snapshot(&story_dir);
        assert_eq!(
            before, after,
            "the device story must be byte-identical after acquisition"
        );
    }
}
