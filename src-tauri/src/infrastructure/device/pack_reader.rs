//! Acquires a device pack into a local staging directory.
//!
//! Runs AFTER the authoritative re-scan: given the `mount_path` of the
//! already-classified supported volume and the pack's `SHORT_ID`, the
//! reader enumerates `.content/<SHORT_ID>` (never following symlinks),
//! validates the inventory against the PURE declared-subset rules
//! (`domain::device::pack`), then copies the retained files into the
//! caller-provided staging directory with per-file `flush` + `sync_all`
//! and a wall-clock deadline checked between files. The aggregate
//! SHA-256 checksum is computed over the bytes actually staged, in the
//! manifest's deterministic order.
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
    validate_pack_inventory, PackEntry, PackEntryKind, PackManifest, PackValidationIssue,
    MAX_IMPORT_PACK_BYTES, MAX_IMPORT_PACK_FILES, MAX_PACK_ASSET_DEPTH,
};
use crate::domain::device::LUNII_CONTENT_DIR;
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

/// Copies a validated pack from the mount into `staging_dir`. MUST
/// respect the `budget` wall-clock deadline so a stalled mount cannot
/// keep the `spawn_blocking` worker alive past the command budget.
pub trait DevicePackReader: Send + Sync + 'static {
    fn acquire_pack(
        &self,
        mount_path: &Path,
        short_id: &str,
        staging_dir: &Path,
        budget: Duration,
    ) -> Result<AcquiredPack, AppError>;
}

/// Production reader: stdlib filesystem walks + copies.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDevicePackReader;

/// Streaming copy buffer. 64 KB amortizes syscalls on packs that weigh
/// hundreds of MB without holding more than a fixed slice in memory.
const COPY_BUF_BYTES: usize = 64 * 1024;

impl DevicePackReader for SystemDevicePackReader {
    fn acquire_pack(
        &self,
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
            Err(err) => return Err(fs_read_error(io_error_kind_tag(&err))),
        }

        // 1. Bounded enumeration (never follows symlinks, never recurses
        //    past the depth the domain would refuse anyway).
        let mut entries: Vec<PackEntry> = Vec::new();
        walk_pack_dir(&pack_dir, &mut Vec::new(), &mut entries, started, budget)?;

        // 2. Pure structural validation → deterministic manifest.
        let manifest =
            validate_pack_inventory(&entries).map_err(|issue| validation_error(&issue))?;

        // 3. Copy file-by-file in manifest order, hashing the staged
        //    bytes, re-checking the deadline between files and re-probing
        //    each source with `symlink_metadata` right before opening it
        //    (closes the enumerate→copy TOCTOU window).
        let mut hasher = Sha256::new();
        let mut staged_files = Vec::with_capacity(manifest.files.len());
        let mut staged_total: u64 = 0;

        for file in &manifest.files {
            if started.elapsed() >= budget {
                return Err(read_timeout_error(started.elapsed()));
            }

            let src = join_rel_path(&pack_dir, &file.rel_path);
            let meta =
                fs::symlink_metadata(&src).map_err(|err| fs_read_error(io_error_kind_tag(&err)))?;
            if !meta.is_file() {
                return Err(pack_invalid_error("source_changed_to_non_regular_file"));
            }

            let dst = join_rel_path(staging_dir, &file.rel_path);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| staging_write_error(io_error_kind_tag(&err)))?;
            }

            hasher.update(file.rel_path.as_bytes());
            hasher.update([0u8]);

            let copied = copy_one_file(&src, &meta, &dst, &mut hasher, started, budget)?;
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

        let checksum = format!("{:x}", hasher.finalize());
        Ok(AcquiredPack { manifest, checksum })
    }
}

/// Recursive bounded walk. `rel_components` tracks the path below the
/// pack root; recursion stops at the depth the domain refuses, so a
/// hostile deep tree can neither exhaust the stack nor hide content
/// (the too-deep DIRECTORY entry itself is recorded and refused).
fn walk_pack_dir(
    dir: &Path,
    rel_components: &mut Vec<String>,
    out: &mut Vec<PackEntry>,
    started: Instant,
    budget: Duration,
) -> Result<(), AppError> {
    let read_dir = fs::read_dir(dir).map_err(|err| fs_read_error(io_error_kind_tag(&err)))?;
    for dir_entry in read_dir {
        if started.elapsed() >= budget {
            return Err(read_timeout_error(started.elapsed()));
        }
        // Defense in depth: stop enumerating long before an adversarial
        // file count could exhaust memory — the domain bound would refuse
        // the pack anyway.
        if out.len() > MAX_IMPORT_PACK_FILES.saturating_mul(2) {
            return Err(pack_oversize_error());
        }

        let dir_entry = dir_entry.map_err(|err| fs_read_error(io_error_kind_tag(&err)))?;
        let name = match dir_entry.file_name().into_string() {
            Ok(name) => name,
            // A non-UTF-8 name cannot be part of the declared subset.
            Err(_) => return Err(pack_invalid_error("non_utf8_entry_name")),
        };

        // Probe with `symlink_metadata` (lstat): `DirEntry::metadata`
        // follows symlinks on some platforms, which would hide a link as
        // its target. lstat keeps a symlink classified as a symlink so
        // the validator can refuse it.
        let meta = fs::symlink_metadata(dir_entry.path())
            .map_err(|err| fs_read_error(io_error_kind_tag(&err)))?;
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
            walk_pack_dir(&dir_entry.path(), rel_components, out, started, budget)?;
        }
        rel_components.pop();
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
    src: &Path,
    expected: &fs::Metadata,
    dst: &Path,
    hasher: &mut Sha256,
    started: Instant,
    budget: Duration,
) -> Result<u64, AppError> {
    let mut reader = File::open(src).map_err(|err| fs_read_error(io_error_kind_tag(&err)))?;
    let opened = reader
        .metadata()
        .map_err(|err| fs_read_error(io_error_kind_tag(&err)))?;
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
    let mut writer =
        File::create(dst).map_err(|err| staging_write_error(io_error_kind_tag(&err)))?;

    let mut buf = vec![0u8; COPY_BUF_BYTES];
    let mut copied: u64 = 0;
    loop {
        if started.elapsed() >= budget {
            return Err(read_timeout_error(started.elapsed()));
        }
        let read = reader
            .read(&mut buf)
            .map_err(|err| fs_read_error(io_error_kind_tag(&err)))?;
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
        PackValidationIssue::TooManyFiles { .. } | PackValidationIssue::TooLarge { .. } => {
            "oversize"
        }
    }
}

// Closed user-facing copy — sober, no OS message, no path (PII rules).

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

fn fs_read_error(kind: &'static str) -> AppError {
    AppError::import_failed(
        "Copie impossible: lecture de l'appareil interrompue.",
        "Vérifie la connexion de la Lunii puis réessaie la copie.",
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

fn read_timeout_error(elapsed: Duration) -> AppError {
    AppError::import_failed(
        "Copie impossible: l'appareil met trop de temps à répondre.",
        "Réessaie la copie ; si le problème persiste, rebranche la Lunii.",
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
        temp_lunii_mount_with_pack_content, write_plausible_pack,
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

    #[test]
    fn acquires_a_plausible_pack_into_staging_with_deterministic_checksum() {
        let pack_uuid = uuid([0xFA, 0xC5, 0x56, 0x2D]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);

        let stage_a = staging();
        let acquired_a = SystemDevicePackReader
            .acquire_pack(&mount, &short_id, stage_a.path(), budget())
            .expect("acquire");

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
        let acquired_b = SystemDevicePackReader
            .acquire_pack(&mount, &short_id, stage_b.path(), budget())
            .expect("acquire again");
        assert_eq!(acquired_a.checksum, acquired_b.checksum);
        assert_eq!(acquired_a.manifest, acquired_b.manifest);
    }

    #[test]
    fn missing_pack_dir_maps_to_pack_missing() {
        let pack_uuid = uuid([1, 2, 3, 4]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let err = SystemDevicePackReader
            .acquire_pack(&mount, "DEADBEEF", staging().path(), budget())
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
        let err = SystemDevicePackReader
            .acquire_pack(&mount, &short_id, stage.path(), budget())
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

        let err = SystemDevicePackReader
            .acquire_pack(&mount, &short_id, staging().path(), budget())
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
        let acquired = SystemDevicePackReader
            .acquire_pack(&mount, &short_id, stage.path(), budget())
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

        let err = SystemDevicePackReader
            .acquire_pack(&mount, &short_id, staging().path(), budget())
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

        let err = SystemDevicePackReader
            .acquire_pack(&mount, &short_id, staging().path(), budget())
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
    fn zero_budget_aborts_with_read_timeout_before_copying() {
        let pack_uuid = uuid([4, 3, 2, 1]);
        let (_guard, mount) = temp_lunii_mount_with_pack_content(7, pack_uuid);
        let short_id = crate::domain::device::pack_short_id(&pack_uuid);

        let err = SystemDevicePackReader
            .acquire_pack(&mount, &short_id, staging().path(), Duration::ZERO)
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
        SystemDevicePackReader
            .acquire_pack(&mount, &short_id, staging().path(), budget())
            .expect("acquire");
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
        SystemDevicePackReader
            .acquire_pack(mount.path(), "01020304", stage.path(), budget())
            .expect("the plausible pack fixture must validate");
    }
}
