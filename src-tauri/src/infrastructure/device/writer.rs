//! Writes a prepared pack back to a Lunii volume — the first real device write.
//!
//! Runs AFTER the authoritative re-scan + the `WriteStory` capability gate: given
//! the `mount_path` of the already-classified WRITABLE volume, the local source
//! folder of the imported pack (`{app_data_dir}/imports/<story_id>/`), the pack
//! UUID and a [`PackWritePlan`], it reproduces the opaque pack bytes under
//! `.content/<SHORT_ID>` and adds the UUID to the device's `.pi` index.
//!
//! Round-trip only, NO decryption, NO format invention: the bytes are the ones
//! the device itself produced (acquired verbatim by the import path) — the safest
//! possible write.
//!
//! Safety contract (mirrors the import promotion + the architecture's
//! filesystem-consistency rules):
//!   1. stage the bytes in a temp dir ON THE DEVICE VOLUME (same filesystem as
//!      the target, so the promotion `rename` is atomic and never crosses a
//!      boundary),
//!   2. re-read each source file (`symlink_metadata` + `fstat` identity check),
//!      stream-copy it, re-checksum against the plan (a drift is refused, never
//!      written),
//!   3. fsync the staged tree, promote atomically by `rename`, fsync the promoted
//!      tree + `.content` parent,
//!   4. ONLY THEN add the UUID to `.pi` atomically (temp + `rename`, fsync root).
//!
//! FILES FIRST, INDEX SECOND: `.pi` never references a pack whose content is not
//! safely present. The inverse — promoted content not yet referenced — is benign
//! unused content (reclaimed on the next attempt / by the sweep). Interruption
//! (deadline, device yanked) leaves the local draft untouched and the device's
//! existing content intact; there is NO partial resume (a fresh cycle re-writes
//! everything). Zero network, zero decryption.
//!
//! A trait keeps the application layer testable without a real volume:
//! [`MockDevicePackWriter`](super::mock::MockDevicePackWriter) scripts successes
//! and failures without hardware.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tempfile::Builder;

use crate::domain::device::{
    LUNII_CONTENT_DIR, LUNII_DEVICE_ID_MARKER, LUNII_PACK_UUID_BYTES, MAX_PACK_INDEX_BYTES,
};
use crate::domain::transfer::{
    append_pack_uuid, pack_uuid_bytes, PackWritePlan, TransferFailureCause,
};

/// Streaming copy buffer — matches the import copier so large packs never hold
/// more than a fixed slice in memory.
const COPY_BUF_BYTES: usize = 64 * 1024;

/// Recognizable prefix for the on-device staging temp dir / `.pi` temp file, so
/// [`sweep_device_transfer_staging`] can reclaim residues from an interrupted
/// write without ever touching the device's own content.
const DEVICE_STAGING_PREFIX: &str = ".rustory-staging-";

/// Writes a prepared pack onto a writable Lunii volume. MUST respect the
/// `budget` wall-clock deadline so a stalled mount cannot keep the
/// `spawn_blocking` worker alive past the command budget, and MUST update `.pi`
/// only AFTER the content is safely promoted.
pub trait DevicePackWriter: Send + Sync + 'static {
    /// `source_pack_dir` is the LOCAL `{app_data_dir}/imports/<story_id>/` folder
    /// (read-only source of the opaque bytes); `pack_uuid` is the canonical
    /// lowercase UUID added to `.pi`; `plan` lists the files to reproduce under
    /// `.content/<plan.short_id>`.
    fn write_pack(
        &self,
        mount_path: &Path,
        source_pack_dir: &Path,
        pack_uuid: &str,
        plan: &PackWritePlan,
        budget: Duration,
    ) -> Result<(), TransferFailureCause>;
}

/// Production writer: stdlib filesystem copies + atomic renames + fsync.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDevicePackWriter;

impl DevicePackWriter for SystemDevicePackWriter {
    fn write_pack(
        &self,
        mount_path: &Path,
        source_pack_dir: &Path,
        pack_uuid: &str,
        plan: &PackWritePlan,
        budget: Duration,
    ) -> Result<(), TransferFailureCause> {
        // The UUID must be canonical — callers pass the value the import
        // recorded (schema-canonical), so a non-canonical value is a caller
        // invariant violation, refused WITHOUT touching the device.
        let uuid_bytes = pack_uuid_bytes(pack_uuid).ok_or(TransferFailureCause::WriteRejected)?;

        // F8 — validate every plan path at the write boundary BEFORE any device
        // I/O: a `..`, absolute or empty/dot component is refused, never followed.
        for file in &plan.files {
            validate_rel_path(&file.rel_path)?;
        }

        // F3 — serialize writes to THIS mount: a single USB volume is written
        // serially, so the staging sweep and the `.pi` read-modify-write below
        // can never race a concurrent transfer (which would lose an index entry).
        let lock = mount_write_lock(mount_path);
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        let started = Instant::now();

        // Reclaim any staging residue left by a previously interrupted write on
        // THIS device before starting a fresh one (best-effort).
        let _ = sweep_device_transfer_staging(mount_path);

        let content_dir = mount_path.join(LUNII_CONTENT_DIR);
        let target = content_dir.join(&plan.short_id);
        let pi_path = mount_path.join(LUNII_DEVICE_ID_MARKER);

        // F7 — refuse a corrupt `.pi` (length not a whole number of 16-byte
        // UUIDs) BEFORE any mutation; an append would misalign the new entry.
        // (Guard lives in `read_pi`; the index is re-read fresh before the append.)
        read_pi(&pi_path)?;

        // F2 — an existing `.content/<SHORT_ID>` is NEVER deleted or overwritten.
        // It must be PROVEN to be the same healthy pack (every planned file
        // present, matching size + checksum) before we reuse it; a collision,
        // stale or incomplete folder under this SHORT_ID is refused, not clobbered.
        if target.exists() {
            if !pack_dir_matches_plan(&target, plan, started, budget)? {
                return Err(TransferFailureCause::WriteRejected);
            }
            // C3 — budget adherence "between steps": validating the existing pack
            // may have consumed the budget; do not run the (durable) index step
            // over budget.
            if started.elapsed() >= budget {
                return Err(TransferFailureCause::Interrupted);
            }
            // The healthy pack is already present. Converge the index (files-first
            // recovery: a prior write may have promoted content but not indexed
            // it) and stop — no staging, no destructive promote.
            return index_pack(mount_path, &pi_path, &uuid_bytes);
        }

        // 1. Stage the opaque bytes in a temp dir ON THE DEVICE VOLUME.
        let staging = Builder::new()
            .prefix(DEVICE_STAGING_PREFIX)
            .tempdir_in(mount_path)
            .map_err(|_| TransferFailureCause::WriteRejected)?;

        for file in &plan.files {
            if started.elapsed() >= budget {
                return Err(TransferFailureCause::Interrupted);
            }
            let src = safe_rel_join(source_pack_dir, &file.rel_path)?;
            let dst = safe_rel_join(staging.path(), &file.rel_path)?;
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|_| TransferFailureCause::WriteRejected)?;
            }
            copy_one_file(&src, &dst, &file.checksum, started, budget)?;
        }

        // C3 — budget adherence "between steps": the copy loop above may have
        // exhausted the budget; refuse BEFORE the durability + indexing phase
        // (fsync → promote → fsync → `.pi`) rather than running it over budget.
        // This bounds the gap between steps; it does NOT interrupt an
        // already-blocked syscall.
        if started.elapsed() >= budget {
            return Err(TransferFailureCause::Interrupted);
        }

        // 2. Persist the staged contents' directory entries before promotion.
        fsync_tree(staging.path()).map_err(|_| TransferFailureCause::WriteRejected)?;

        // 3. Promote atomically. FILES FIRST: `.pi` is touched only afterwards.
        fs::create_dir_all(&content_dir).map_err(|_| TransferFailureCause::WriteRejected)?;
        promote(staging.path(), &target)?;
        // The staged path no longer exists; the TempDir drop is a no-op.

        // 4. Persist the promoted tree + `.content` parent so a power loss after
        //    the `.pi` update cannot resurrect a half-written folder.
        fsync_tree(&target).map_err(|_| TransferFailureCause::WriteRejected)?;
        fsync_dir(&content_dir).map_err(|_| TransferFailureCause::WriteRejected)?;

        // 5. Add the UUID to `.pi` atomically (idempotent), re-reading the
        //    freshest index first (F3). A promoted folder left unreferenced if
        //    this step fails is benign unused content; the forbidden inverse (an
        //    index entry without content) cannot happen — this is the LAST step.
        index_pack(mount_path, &pi_path, &uuid_bytes)
    }
}

/// Best-effort sweep of on-device transfer staging residues (a temp dir or `.pi`
/// temp file left by an interrupted write). Recognized by the
/// [`DEVICE_STAGING_PREFIX`]; NEVER touches the device's own content. Returns the
/// number of entries removed (diagnostic only). Run before a write (and safe to
/// call whenever a writable device is present).
pub fn sweep_device_transfer_staging(mount_path: &Path) -> u32 {
    let mut removed = 0;
    let Ok(entries) = fs::read_dir(mount_path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(DEVICE_STAGING_PREFIX) {
            continue;
        }
        let path = entry.path();
        let ok = if path.is_dir() {
            fs::remove_dir_all(&path).is_ok()
        } else {
            fs::remove_file(&path).is_ok()
        };
        if ok {
            removed += 1;
        }
    }
    removed
}

/// Read the device `.pi` payload (empty when absent — a freshly wiped Lunii).
/// A non-regular, oversized, or fragment-trailing (corrupt) `.pi` is treated as a
/// refused write rather than silently rewritten (F7).
fn read_pi(pi_path: &Path) -> Result<Vec<u8>, TransferFailureCause> {
    let bytes = match fs::symlink_metadata(pi_path) {
        Ok(meta) if meta.is_file() => {
            if meta.len() > MAX_PACK_INDEX_BYTES {
                return Err(TransferFailureCause::WriteRejected);
            }
            fs::read(pi_path).map_err(|_| TransferFailureCause::WriteRejected)?
        }
        Ok(_) => return Err(TransferFailureCause::WriteRejected),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return Err(TransferFailureCause::WriteRejected),
    };
    // F7 — a `.pi` whose length is not a multiple of the 16-byte UUID size has a
    // trailing fragment (a corrupt index). Appending would misalign the new
    // entry, so refuse rather than mutate a malformed index.
    if bytes.len() % LUNII_PACK_UUID_BYTES != 0 {
        return Err(TransferFailureCause::WriteRejected);
    }
    Ok(bytes)
}

/// Promote the staged folder to `target` by an atomic `rename`. The caller has
/// already proven `target` does NOT exist (an existing folder under this SHORT_ID
/// is refused upstream, never clobbered — F2), so promotion must not silently
/// replace device content: refuse if the target appeared meanwhile (a race).
fn promote(staging_path: &Path, target: &Path) -> Result<(), TransferFailureCause> {
    if target.exists() {
        return Err(TransferFailureCause::WriteRejected);
    }
    fs::rename(staging_path, target).map_err(|_| TransferFailureCause::WriteRejected)
}

/// Stream-copy `src` → `dst`, verifying the copied bytes against `expected`
/// (the plan's per-file checksum). Deadline-checked between chunks; the staged
/// file is flushed + fsynced. The source is re-`lstat`'d before opening and the
/// OPEN HANDLE `fstat`'d after (TOCTOU guard, like the import copier): a file
/// swapped for a symlink or another inode is refused, never followed.
fn copy_one_file(
    src: &Path,
    dst: &Path,
    expected: &str,
    started: Instant,
    budget: Duration,
) -> Result<(), TransferFailureCause> {
    let expected_meta =
        fs::symlink_metadata(src).map_err(|_| TransferFailureCause::WriteRejected)?;
    if !expected_meta.is_file() {
        return Err(TransferFailureCause::WriteRejected);
    }
    let mut reader = File::open(src).map_err(|_| TransferFailureCause::WriteRejected)?;
    let opened = reader
        .metadata()
        .map_err(|_| TransferFailureCause::WriteRejected)?;
    if !opened.is_file() {
        return Err(TransferFailureCause::WriteRejected);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != expected_meta.dev() || opened.ino() != expected_meta.ino() {
            return Err(TransferFailureCause::WriteRejected);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = expected_meta;
    }

    let mut writer = File::create(dst).map_err(|_| TransferFailureCause::WriteRejected)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    loop {
        if started.elapsed() >= budget {
            return Err(TransferFailureCause::Interrupted);
        }
        let read = reader
            .read(&mut buf)
            .map_err(|_| TransferFailureCause::WriteRejected)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        writer
            .write_all(&buf[..read])
            .map_err(|_| TransferFailureCause::WriteRejected)?;
    }
    writer
        .flush()
        .map_err(|_| TransferFailureCause::WriteRejected)?;
    writer
        .sync_all()
        .map_err(|_| TransferFailureCause::WriteRejected)?;

    // The source is our own managed store, freshly assembled by the preparation
    // step; a checksum drift means on-disk corruption — refuse rather than write
    // corrupted bytes onto the device.
    if format!("{:x}", hasher.finalize()) != expected {
        return Err(TransferFailureCause::WriteRejected);
    }
    Ok(())
}

/// Atomically write `bytes` to `.pi`: a temp file ON THE DEVICE VOLUME, fsynced,
/// then `rename`d onto `.pi`, then the mount root fsynced so the directory entry
/// is durable.
fn write_pi_atomically(
    mount_path: &Path,
    pi_path: &Path,
    bytes: &[u8],
) -> Result<(), TransferFailureCause> {
    let mut tmp = Builder::new()
        .prefix(DEVICE_STAGING_PREFIX)
        .tempfile_in(mount_path)
        .map_err(|_| TransferFailureCause::WriteRejected)?;
    tmp.write_all(bytes)
        .map_err(|_| TransferFailureCause::WriteRejected)?;
    tmp.flush()
        .map_err(|_| TransferFailureCause::WriteRejected)?;
    tmp.as_file()
        .sync_all()
        .map_err(|_| TransferFailureCause::WriteRejected)?;
    tmp.persist(pi_path)
        .map_err(|_| TransferFailureCause::WriteRejected)?;
    fsync_dir(mount_path).map_err(|_| TransferFailureCause::WriteRejected)?;
    Ok(())
}

fn fsync_dir(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

fn fsync_tree(dir: &Path) -> std::io::Result<()> {
    fsync_dir(dir)?;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            fsync_tree(&path)?;
        }
    }
    Ok(())
}

/// Validate a pack-relative forward-slash path at the WRITE boundary (F8):
/// fail-closed on an absolute path or any `..`, `.`, empty or non-`Normal`
/// component. The plan comes from our own enumeration, but the writer is the last
/// gate before device I/O, so it never trusts a path it did not just validate.
fn validate_rel_path(rel_path: &str) -> Result<(), TransferFailureCause> {
    if rel_path.is_empty() || rel_path.starts_with('/') || rel_path.starts_with('\\') {
        return Err(TransferFailureCause::WriteRejected);
    }
    for component in rel_path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(TransferFailureCause::WriteRejected);
        }
        let mut parts = Path::new(component).components();
        match (parts.next(), parts.next()) {
            (Some(std::path::Component::Normal(_)), None) => {}
            _ => return Err(TransferFailureCause::WriteRejected),
        }
    }
    Ok(())
}

/// Join a forward-slash `rel_path` under `base`, fail-closed via
/// [`validate_rel_path`] (refuses traversal / absolute paths).
fn safe_rel_join(base: &Path, rel_path: &str) -> Result<PathBuf, TransferFailureCause> {
    validate_rel_path(rel_path)?;
    let mut out = base.to_path_buf();
    for component in rel_path.split('/') {
        out.push(component);
    }
    Ok(out)
}

/// Per-mount write serialization (F3). A single USB volume is written serially;
/// concurrent writes to the SAME mount would race the staging sweep and the `.pi`
/// read-modify-write (a lost index entry). One in-process lock per mount path
/// makes each transfer atomic with respect to other transfers on that volume.
fn mount_write_lock(mount_path: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let map = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .entry(mount_path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Add `uuid_bytes` to `.pi` atomically and idempotently, re-reading the FRESHEST
/// index immediately before the append (F3): even though the per-mount lock
/// already serializes in-process writers, the append is always computed from the
/// current bytes, never a stale snapshot. The fresh `read_pi` re-applies the F7
/// guard.
fn index_pack(
    mount_path: &Path,
    pi_path: &Path,
    uuid_bytes: &[u8; LUNII_PACK_UUID_BYTES],
) -> Result<(), TransferFailureCause> {
    let current = read_pi(pi_path)?;
    let updated = append_pack_uuid(&current, uuid_bytes);
    if updated != current {
        write_pi_atomically(mount_path, pi_path, &updated)?;
    }
    Ok(())
}

/// Whether the existing `.content/<SHORT_ID>` folder is EXACTLY the pack the plan
/// describes — every planned file present as a regular file with the matching
/// size and a re-computed checksum that matches. Used to decide a safe idempotent
/// reuse WITHOUT trusting a folder we did not just write (F2): a missing / extra /
/// drifted file means "not the same pack" → the caller refuses, never deletes.
/// Deadline-checked so a stalled mount aborts as `Interrupted`.
fn pack_dir_matches_plan(
    target: &Path,
    plan: &PackWritePlan,
    started: Instant,
    budget: Duration,
) -> Result<bool, TransferFailureCause> {
    for file in &plan.files {
        if started.elapsed() >= budget {
            return Err(TransferFailureCause::Interrupted);
        }
        let path = safe_rel_join(target, &file.rel_path)?;
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => {
                if meta.len() != file.byte_len {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
        if file_checksum(&path, started, budget)? != file.checksum {
            return Ok(false);
        }
    }
    // C4 — fail-closed on EXTRA files: a folder holding exactly the planned files
    // PLUS anything else was NOT produced by this plan, so it is not the same pack
    // and must be refused (never reused or indexed). Enumerate the target and
    // reject any entry absent from the plan.
    let planned: std::collections::HashSet<&str> =
        plan.files.iter().map(|f| f.rel_path.as_str()).collect();
    for rel in collect_pack_files(target, started, budget)? {
        if !planned.contains(rel.as_str()) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Recursively collect the forward-slash relative paths of every non-directory
/// entry under `root` (regular files, symlinks — anything that is not a dir),
/// used by [`pack_dir_matches_plan`] to detect entries absent from the plan (C4).
/// Deadline-checked so a stalled mount aborts as `Interrupted`.
fn collect_pack_files(
    root: &Path,
    started: Instant,
    budget: Duration,
) -> Result<Vec<String>, TransferFailureCause> {
    fn walk(
        root: &Path,
        dir: &Path,
        started: Instant,
        budget: Duration,
        out: &mut Vec<String>,
    ) -> Result<(), TransferFailureCause> {
        if started.elapsed() >= budget {
            return Err(TransferFailureCause::Interrupted);
        }
        let entries = fs::read_dir(dir).map_err(|_| TransferFailureCause::WriteRejected)?;
        for entry in entries {
            let path = entry
                .map_err(|_| TransferFailureCause::WriteRejected)?
                .path();
            let meta =
                fs::symlink_metadata(&path).map_err(|_| TransferFailureCause::WriteRejected)?;
            if meta.is_dir() {
                walk(root, &path, started, budget, out)?;
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map_err(|_| TransferFailureCause::WriteRejected)?;
            let mut parts = Vec::new();
            for component in rel.components() {
                match component {
                    std::path::Component::Normal(c) => parts.push(c.to_string_lossy().into_owned()),
                    // Defensive: a non-normal component cannot be a planned file.
                    _ => return Err(TransferFailureCause::WriteRejected),
                }
            }
            out.push(parts.join("/"));
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, started, budget, &mut out)?;
    Ok(out)
}

/// Stream the SHA-256 of an existing file (deadline-checked), to compare against a
/// plan checksum without copying.
fn file_checksum(
    path: &Path,
    started: Instant,
    budget: Duration,
) -> Result<String, TransferFailureCause> {
    let mut reader = File::open(path).map_err(|_| TransferFailureCause::WriteRejected)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    loop {
        if started.elapsed() >= budget {
            return Err(TransferFailureCause::Interrupted);
        }
        let read = reader
            .read(&mut buf)
            .map_err(|_| TransferFailureCause::WriteRejected)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::{format_pack_uuid, pack_short_id, parse_pack_index};
    use crate::domain::transfer::PackWriteFile;
    use crate::infrastructure::device::fixtures::write_plausible_pack;

    /// Test helper: is `uuid_bytes` present as a clean 16-byte chunk in `.pi`?
    fn index_contains(pi_bytes: &[u8], uuid_bytes: &[u8; 16]) -> bool {
        parse_pack_index(pi_bytes)
            .uuids
            .iter()
            .any(|existing| existing == uuid_bytes)
    }

    fn uuid_bytes(tail: [u8; 4]) -> [u8; 16] {
        let mut b = [0xAB; 16];
        b[12..16].copy_from_slice(&tail);
        b
    }

    fn budget() -> Duration {
        Duration::from_secs(30)
    }

    /// Build a write plan for the plausible pack at `source`, computing each
    /// file's SHA-256 exactly as the preparation assembler records it.
    fn plan_for(source: &Path, short_id: &str) -> PackWritePlan {
        let mut files = Vec::new();
        for rel in [
            "li",
            "ni",
            "ri",
            "si",
            "nm",
            "bt",
            "rf/000/AAAAAAAA",
            "sf/000/BBBBBBBB",
        ] {
            let path = safe_rel_join(source, rel).expect("trusted test rel");
            let bytes = std::fs::read(&path).expect("read source file");
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            files.push(PackWriteFile {
                rel_path: rel.into(),
                byte_len: bytes.len() as u64,
                checksum: format!("{:x}", hasher.finalize()),
            });
        }
        PackWritePlan {
            short_id: short_id.into(),
            files,
        }
    }

    /// A bare mount with no library yet (no `.pi`, no `.content`).
    fn empty_mount() -> tempfile::TempDir {
        tempfile::tempdir().expect("mount tempdir")
    }

    fn source_pack() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("source tempdir");
        write_plausible_pack(dir.path());
        dir
    }

    #[test]
    fn writes_a_pack_into_content_and_adds_the_uuid_to_the_index() {
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0xFA, 0xC5, 0x56, 0x2D]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect("write must succeed");

        // Content reproduced byte-for-byte under `.content/<SHORT_ID>`.
        let pack_dir = mount.path().join(".content").join(&short_id);
        assert!(pack_dir.join("ni").is_file());
        assert!(pack_dir.join("rf").join("000").join("AAAAAAAA").is_file());
        assert_eq!(
            std::fs::read(source.path().join("ni")).unwrap(),
            std::fs::read(pack_dir.join("ni")).unwrap(),
            "ni must be copied verbatim"
        );

        // The UUID is now in `.pi`, exactly once, and no staging residue remains.
        let pi = std::fs::read(mount.path().join(".pi")).expect("read .pi");
        assert!(index_contains(&pi, &uuid));
        assert_eq!(pi.len(), 16, "exactly one pack indexed");
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn re_writing_the_same_pack_is_idempotent() {
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([1, 2, 3, 4]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect("first write");
        SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect("second write is a no-op");

        let pi = std::fs::read(mount.path().join(".pi")).expect("read .pi");
        assert_eq!(pi.len(), 16, "the UUID must not be duplicated in the index");
        assert!(mount
            .path()
            .join(".content")
            .join(&short_id)
            .join("ni")
            .is_file());
    }

    #[test]
    fn mid_write_failure_cleans_staging_and_leaves_index_and_existing_content_intact() {
        let mount = empty_mount();
        // Pre-seed an EXISTING different pack: its content folder + a `.pi` entry.
        let other = uuid_bytes([0x0A, 0x0B, 0x0C, 0x0D]);
        let other_short = pack_short_id(&other);
        let other_dir = mount.path().join(".content").join(&other_short);
        std::fs::create_dir_all(&other_dir).expect("mk other content");
        std::fs::write(other_dir.join("ni"), b"OTHER").expect("seed other ni");
        std::fs::write(mount.path().join(".pi"), other).expect("seed .pi");

        let source = source_pack();
        let uuid = uuid_bytes([0xDE, 0xAD, 0xBE, 0xEF]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);
        // Remove a source file the plan still lists → the copy fails mid-write.
        std::fs::remove_file(source.path().join("ni")).expect("drop source ni");

        let err = SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect_err("a missing source file must fail the write");
        assert_eq!(err, TransferFailureCause::WriteRejected);

        // No staging residue, the new pack never promoted, `.pi` unchanged, and
        // the pre-existing pack untouched.
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
        assert!(!mount.path().join(".content").join(&short_id).exists());
        let pi = std::fs::read(mount.path().join(".pi")).expect("read .pi");
        assert_eq!(pi, other.to_vec(), ".pi must be byte-identical");
        assert_eq!(
            std::fs::read(other_dir.join("ni")).unwrap(),
            b"OTHER",
            "the existing pack must stay intact"
        );
    }

    #[test]
    fn an_exhausted_budget_aborts_with_interrupted_and_writes_nothing() {
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([4, 3, 2, 1]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                Duration::ZERO,
            )
            .expect_err("zero budget must abort");
        assert_eq!(err, TransferFailureCause::Interrupted);
        assert!(!mount.path().join(".content").join(&short_id).exists());
        assert!(!mount.path().join(".pi").exists());
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn sweep_removes_orphan_staging_but_keeps_real_content() {
        let mount = empty_mount();
        let orphan = mount.path().join(format!("{DEVICE_STAGING_PREFIX}abc123"));
        std::fs::create_dir_all(&orphan).expect("mk orphan staging");
        std::fs::write(orphan.join("ni"), b"PART").expect("seed partial");
        let content = mount.path().join(".content");
        std::fs::create_dir_all(&content).expect("mk content");

        assert_eq!(sweep_device_transfer_staging(mount.path()), 1);
        assert!(!orphan.exists(), "orphan staging must be removed");
        assert!(content.is_dir(), "real content must survive the sweep");
    }

    #[test]
    fn an_existing_mismatched_pack_under_the_same_short_id_is_refused_not_clobbered() {
        // F2 — a different / stale / incomplete folder already sits at this
        // SHORT_ID (a collision). It must be refused, NEVER deleted or
        // overwritten, and the index left intact.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x11, 0x22, 0x33, 0x44]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // Pre-seed a DIFFERENT pack folder under the SAME SHORT_ID.
        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed collision dir");
        std::fs::write(target.join("ni"), b"STALE-AND-DIFFERENT").expect("seed stale ni");
        // A `.pi` that does NOT yet reference our UUID.
        let other = uuid_bytes([9, 9, 9, 9]);
        std::fs::write(mount.path().join(".pi"), other).expect("seed .pi");

        let err = SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect_err("a mismatched existing pack must be refused");
        assert_eq!(err, TransferFailureCause::WriteRejected);
        // The existing folder is intact (not clobbered) and `.pi` is unchanged.
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            b"STALE-AND-DIFFERENT",
            "the colliding folder must be left intact"
        );
        assert_eq!(
            std::fs::read(mount.path().join(".pi")).unwrap(),
            other.to_vec()
        );
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn an_existing_healthy_pack_not_yet_indexed_is_indexed_without_re_clobbering() {
        // F2 recovery — content promoted by a prior interrupted write (folder
        // healthy + matches the plan) but missing from `.pi`: the re-run indexes
        // it WITHOUT re-staging or altering the existing content.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x55, 0x66, 0x77, 0x88]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect("initial write");
        // Simulate "content present, index lost".
        std::fs::write(mount.path().join(".pi"), Vec::<u8>::new()).expect("wipe index");
        let content_before =
            std::fs::read(mount.path().join(".content").join(&short_id).join("ni")).unwrap();

        SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect("recovery indexes the existing healthy pack");

        let pi = std::fs::read(mount.path().join(".pi")).expect("read .pi");
        assert!(index_contains(&pi, &uuid), "the UUID must be (re)indexed");
        assert_eq!(pi.len(), 16);
        assert_eq!(
            std::fs::read(mount.path().join(".content").join(&short_id).join("ni")).unwrap(),
            content_before,
            "existing content must not be re-written/altered"
        );
    }

    #[test]
    fn a_pi_with_a_trailing_fragment_is_refused_before_any_mutation() {
        // F7 — a `.pi` whose length is not a multiple of 16 is corrupt; appending
        // would misalign the new entry. Refuse before any write.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0xAA, 0xBB, 0xCC, 0xDD]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // One clean 16-byte entry followed by a 5-byte fragment.
        let mut pi = uuid_bytes([1, 1, 1, 1]).to_vec();
        pi.extend_from_slice(&[0xFF; 5]);
        std::fs::write(mount.path().join(".pi"), &pi).expect("seed fragmented .pi");

        let err = SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect_err("a fragmented .pi must be refused");
        assert_eq!(err, TransferFailureCause::WriteRejected);
        // Nothing written: no content folder, `.pi` byte-identical, no residue.
        assert!(!mount.path().join(".content").join(&short_id).exists());
        assert_eq!(std::fs::read(mount.path().join(".pi")).unwrap(), pi);
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn a_plan_with_an_unsafe_rel_path_is_refused_before_any_io() {
        // F8 — the writer is the last gate before device I/O: any `..`, absolute or
        // empty/dot component is refused before reading or creating a single file.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x13, 0x37, 0x13, 0x37]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);

        for bad in ["../escape", "/abs/path", "a//b", "ok/../bad", ".", ".."] {
            let plan = PackWritePlan {
                short_id: short_id.clone(),
                files: vec![PackWriteFile {
                    rel_path: bad.into(),
                    byte_len: 4,
                    checksum: "a".repeat(64),
                }],
            };
            let err = SystemDevicePackWriter
                .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
                .expect_err(bad);
            assert_eq!(err, TransferFailureCause::WriteRejected, "{bad}");
        }
        // No content, no `.pi`, no staging residue, and nothing escaped the mount.
        assert!(!mount.path().join(".content").join(&short_id).exists());
        assert!(!mount.path().join(".pi").exists());
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn concurrent_writes_to_the_same_mount_keep_both_index_entries() {
        // F3 — two transfers to the SAME volume must not lose a `.pi` entry. The
        // per-mount lock + fresh re-read serialize the read-modify-write so both
        // UUIDs survive.
        let mount = empty_mount();
        let mount_path = mount.path().to_path_buf();
        let source = source_pack();
        let src_path = source.path().to_path_buf();

        let uuid_a = uuid_bytes([0xA1, 0xA2, 0xA3, 0xA4]);
        let uuid_b = uuid_bytes([0xB1, 0xB2, 0xB3, 0xB4]);
        let ca = format_pack_uuid(&uuid_a);
        let cb = format_pack_uuid(&uuid_b);
        let plan_a = plan_for(source.path(), &pack_short_id(&uuid_a));
        let plan_b = plan_for(source.path(), &pack_short_id(&uuid_b));

        let (mp1, sp1) = (mount_path.clone(), src_path.clone());
        let h1 = std::thread::spawn(move || {
            SystemDevicePackWriter.write_pack(&mp1, &sp1, &ca, &plan_a, Duration::from_secs(30))
        });
        let (mp2, sp2) = (mount_path.clone(), src_path.clone());
        let h2 = std::thread::spawn(move || {
            SystemDevicePackWriter.write_pack(&mp2, &sp2, &cb, &plan_b, Duration::from_secs(30))
        });
        h1.join().unwrap().expect("write A");
        h2.join().unwrap().expect("write B");

        let pi = std::fs::read(mount_path.join(".pi")).expect("read .pi");
        assert!(index_contains(&pi, &uuid_a), "A must be present");
        assert!(index_contains(&pi, &uuid_b), "B must be present");
        assert_eq!(pi.len(), 32, "both entries kept, none lost");
    }

    #[test]
    fn a_budget_exhausted_before_the_durability_phase_aborts_without_mutation() {
        // C3 — the copy loop guards per file, but the durability + indexing phase
        // (fsync → promote → fsync → `.pi`) must also respect the budget. An empty
        // plan reaches that phase WITHOUT the copy loop firing, so a zero budget is
        // caught at the phase-entry guard — the final steps never run over budget
        // and the device is left untouched.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0xC3, 0xC3, 0xC3, 0xC3]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = PackWritePlan {
            short_id: short_id.clone(),
            files: vec![],
        };

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                Duration::ZERO,
            )
            .expect_err("zero budget at the durability phase must abort");
        assert_eq!(err, TransferFailureCause::Interrupted);
        assert!(!mount.path().join(".content").join(&short_id).exists());
        assert!(!mount.path().join(".pi").exists());
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn an_existing_pack_with_an_extra_file_is_refused_not_reused() {
        // C4 — a folder holding exactly the planned files PLUS an extra file was
        // not produced by this plan, so it is not the same pack: refuse, never
        // reuse/index, and leave the folder + `.pi` intact.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0xE4, 0xE4, 0xE4, 0xE4]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // Write the healthy pack, then drop an EXTRA file the plan never describes.
        SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect("initial write");
        let target = mount.path().join(".content").join(&short_id);
        std::fs::write(target.join("EXTRA"), b"unexpected").expect("seed extra file");
        let pi_before = std::fs::read(mount.path().join(".pi")).expect("read .pi");

        let err = SystemDevicePackWriter
            .write_pack(mount.path(), source.path(), &canonical, &plan, budget())
            .expect_err("an extra file makes the existing pack untrusted");
        assert_eq!(err, TransferFailureCause::WriteRejected);
        // The extra file + planned content are left intact (never clobbered) and
        // `.pi` is unchanged.
        assert_eq!(
            std::fs::read(target.join("EXTRA")).unwrap(),
            b"unexpected",
            "the extra file must be left intact"
        );
        assert!(target.join("ni").is_file(), "planned content stays intact");
        assert_eq!(
            std::fs::read(mount.path().join(".pi")).unwrap(),
            pi_before,
            ".pi must be unchanged"
        );
    }
}
