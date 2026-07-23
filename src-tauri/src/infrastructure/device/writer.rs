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
//! An already-present `.content/<SHORT_ID>` resolves to one of THREE PROVEN
//! outcomes (FR23, see `docs/architecture/device-support-profile.md`): identical
//! → idempotent re-index ([`WriteOutcome::ReusedIdentical`]); divergent, made
//! exclusively of regular entries PROVEN readable (read in full) AND
//! ATTRIBUTABLE to the target UUID (indexed, with no other indexed UUID sharing
//! the SHORT_ID) → atomic replacement ([`WriteOutcome::ReplacedDivergent`], old
//! content set aside by `rename` only AFTER the full replacement is staged +
//! fsynced AND the state RE-PROVEN adjacent to that mutation — a residual
//! window of the order of the `rename` remains, not eliminable in stdlib);
//! unprovable (non-directory / symlinked target root, symlink / empty
//! directory / special file inside, unreadable entry or I/O, or a divergent
//! folder that cannot be attributed to the target UUID) → refusal with the
//! dedicated [`TransferFailureCause::DevicePackUnprovable`], zero device byte
//! modified.
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
    pack_short_id, parse_pack_index, LUNII_CONTENT_DIR, LUNII_DEVICE_ID_MARKER,
    LUNII_PACK_UUID_BYTES, MAX_PACK_INDEX_BYTES,
};
use crate::domain::transfer::{
    append_pack_uuid, pack_uuid_bytes, PackWritePlan, TransferFailureCause, WriteOutcome,
};

/// Streaming copy buffer — matches the import copier so large packs never hold
/// more than a fixed slice in memory.
const COPY_BUF_BYTES: usize = 64 * 1024;

/// Recognizable prefix for the on-device staging temp dir / `.pi` temp file, so
/// [`sweep_device_transfer_staging`] can reclaim residues from an interrupted
/// write without ever touching the device's own content.
pub(super) const DEVICE_STAGING_PREFIX: &str = ".rustory-staging-";

/// Recognizable prefix for the set-aside folder holding the OLD pack during an
/// atomic replacement (FR23). An orphan under this prefix is the residue of a
/// write interrupted mid-swap — the job already ended `transfert incomplet`, the
/// old content is superseded by construction (the full replacement was staged +
/// fsynced BEFORE the set-aside), so the sweep reclaims it exactly like a
/// staging residue.
pub(super) const DEVICE_REPLACED_PREFIX: &str = ".rustory-replaced-";

/// Progress of the content-copy step, reported by [`DevicePackWriter::write_pack`]
/// so the application can surface an HONEST fraction. Emitted ONLY during the
/// measurable content copy (never for preflight / durability / index), monotone,
/// never exceeding the total. The application turns it into a `job:progress`
/// fraction and clamps it below 100 % (reserved for the completed terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteProgress {
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// A device-write failure plus whether the DEVICE was already mutated when it
/// happened — the input the domain
/// [`classify`](crate::domain::transfer::classify) turns into `Failed` vs
/// `Incomplete`. `reached_device_mutation` is `false` until the content promotion
/// succeeds, then `true` for the durability + index steps (and the reuse-path
/// index update), so a post-promotion I/O failure is honestly reported as a
/// possible partial copy rather than a clean failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteFailure {
    pub cause: TransferFailureCause,
    pub reached_device_mutation: bool,
}

/// A failure BEFORE the device was mutated (existing content intact → `Failed`).
fn clean(cause: TransferFailureCause) -> WriteFailure {
    WriteFailure {
        cause,
        reached_device_mutation: false,
    }
}

/// A failure AFTER the device mutation began (content promoted, possibly not yet
/// indexed → `Incomplete`): a possible partial copy a fresh relaunch converges.
fn mutated(cause: TransferFailureCause) -> WriteFailure {
    WriteFailure {
        cause,
        reached_device_mutation: true,
    }
}

/// Writes a prepared pack onto a writable Lunii volume. MUST respect the
/// `budget` wall-clock deadline so a stalled mount cannot keep the
/// `spawn_blocking` worker alive past the command budget, and MUST update `.pi`
/// only AFTER the content is safely promoted.
pub trait DevicePackWriter: Send + Sync + 'static {
    /// `source_pack_dir` is the LOCAL `{app_data_dir}/imports/<story_id>/` folder
    /// (read-only source of the opaque bytes); `pack_uuid` is the canonical
    /// lowercase UUID added to `.pi`; `plan` lists the files to reproduce under
    /// `.content/<plan.short_id>`. `progress` is called during the content copy
    /// with a monotone [`WriteProgress`]. On success the returned
    /// [`WriteOutcome`] is the outcome the writer CONSTATED (created / reused
    /// identical / replaced divergent — FR23); on failure the [`WriteFailure`]
    /// reports whether the device was already mutated (for the
    /// `Failed`/`Incomplete` distinction).
    fn write_pack(
        &self,
        mount_path: &Path,
        source_pack_dir: &Path,
        pack_uuid: &str,
        plan: &PackWritePlan,
        budget: Duration,
        progress: &dyn Fn(WriteProgress),
    ) -> Result<WriteOutcome, WriteFailure>;
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
        progress: &dyn Fn(WriteProgress),
    ) -> Result<WriteOutcome, WriteFailure> {
        // The UUID must be canonical — callers pass the value the import
        // recorded (schema-canonical), so a non-canonical value is a caller
        // invariant violation, refused WITHOUT touching the device.
        let uuid_bytes =
            pack_uuid_bytes(pack_uuid).ok_or_else(|| clean(TransferFailureCause::WriteRejected))?;

        // F8 — validate every plan path at the write boundary BEFORE any device
        // I/O: a `..`, absolute or empty/dot component is refused, never followed.
        for file in &plan.files {
            validate_rel_path(&file.rel_path).map_err(clean)?;
        }
        // Defense in depth, same F8 spirit: the plan's `short_id` must be the
        // one THIS UUID derives — a drifted caller invariant would aim every
        // proof and mutation at another story's folder. Refused before any I/O.
        if pack_short_id(&uuid_bytes) != plan.short_id {
            return Err(clean(TransferFailureCause::WriteRejected));
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
        // The bytes also feed the attribution guard below (can the divergent
        // folder be attributed to the target UUID?); the index is re-read fresh
        // before the append.
        let pi_bytes = read_pi(&pi_path).map_err(clean)?;
        let attributable = divergent_folder_attributable(&pi_bytes, &uuid_bytes, &plan.short_id);

        // F2 (FR23 evolution) — an existing `.content/<SHORT_ID>` is never lost
        // before its replacement is complete, and never touched when its state
        // cannot be PROVEN. The state proof classifies the target ROOT (no-follow
        // — a symlinked / non-directory root is unprovable) then its contents:
        //   - identical (every planned file present with exact size + checksum,
        //     not a single extra entry) → idempotent re-index only;
        //   - divergent-but-sound (any drift, but EVERY entry is a readable
        //     regular file — readability PROVEN by reading them) → atomic
        //     replacement further below, and ONLY when the folder is
        //     ATTRIBUTABLE to the target UUID: the index must reference it and
        //     no OTHER indexed UUID may share the target SHORT_ID — an
        //     unindexed divergent folder is ambiguous (an unknown residue, a
        //     collision with an unindexed UUID), and a bi-indexed SHORT_ID
        //     collision means the folder may hold the OTHER story's only
        //     content. Both are refused, never replaced;
        //   - unprovable (symlink / unplanned empty directory / special file /
        //     unreadable I/O) → refused with the dedicated cause, not clobbered.
        let replacing = match prove_target_state(&target, plan, started, budget).map_err(clean)? {
            Some(ExistingPackState::Identical) => {
                // C3 — budget adherence "between steps": proving the existing
                // pack may have consumed the budget; do not run the (durable)
                // index step over budget. Pre-mutation for THIS run → `Failed`.
                if started.elapsed() >= budget {
                    return Err(clean(TransferFailureCause::Interrupted));
                }
                // The pack is already the plan's bytes (a prior write promoted
                // it). Converging the index now touches the device, so an index
                // failure here leaves content-present-not-indexed → `Incomplete`.
                index_pack(mount_path, &pi_path, &uuid_bytes).map_err(mutated)?;
                return Ok(WriteOutcome::ReusedIdentical);
            }
            Some(ExistingPackState::DivergentSound) => {
                if !attributable {
                    // Attribution guard: nothing proves this divergent folder
                    // IS "the story already present" — fail-closed, zero byte
                    // touched (see `divergent_folder_attributable`).
                    return Err(clean(TransferFailureCause::DevicePackUnprovable));
                }
                true
            }
            Some(ExistingPackState::Unprovable) => {
                // Rustory never deletes what it cannot understand: refuse with
                // the HONEST dedicated cause (it is Rustory protecting the
                // present content, not the device refusing), zero byte touched.
                return Err(clean(TransferFailureCause::DevicePackUnprovable));
            }
            None => false,
        };

        // 1. Stage the opaque bytes in a temp dir ON THE DEVICE VOLUME.
        let staging = Builder::new()
            .prefix(DEVICE_STAGING_PREFIX)
            .tempdir_in(mount_path)
            .map_err(|_| clean(TransferFailureCause::WriteRejected))?;

        let bytes_total: u64 = plan.files.iter().map(|f| f.byte_len).sum();
        let mut bytes_done: u64 = 0;
        for file in &plan.files {
            if started.elapsed() >= budget {
                return Err(clean(TransferFailureCause::Interrupted));
            }
            let src = safe_rel_join(source_pack_dir, &file.rel_path).map_err(clean)?;
            let dst = safe_rel_join(staging.path(), &file.rel_path).map_err(clean)?;
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)
                    .map_err(|_| clean(TransferFailureCause::WriteRejected))?;
            }
            copy_one_file(&src, &dst, &file.checksum, started, budget).map_err(clean)?;
            // Honest progress: report ONLY the measurable content copy, monotone
            // and bounded by the total. All of this is pre-promotion (staging) →
            // any failure above is `Failed`.
            bytes_done = bytes_done.saturating_add(file.byte_len);
            progress(WriteProgress {
                bytes_done,
                bytes_total,
            });
        }

        // C3 — budget adherence "between steps": the copy loop above may have
        // exhausted the budget; refuse BEFORE the durability + indexing phase
        // (fsync → promote → fsync → `.pi`) rather than running it over budget.
        // Still pre-promotion → `Failed`. This bounds the gap between steps; it
        // does NOT interrupt an already-blocked syscall.
        if started.elapsed() >= budget {
            return Err(clean(TransferFailureCause::Interrupted));
        }

        // 2. Persist the staged contents' directory entries before promotion.
        //    For a replacement this completes the F2 promise: the old content is
        //    only set aside AFTER the full replacement is staged AND durable.
        fsync_tree(staging.path()).map_err(|_| clean(TransferFailureCause::WriteRejected))?;

        // Budget adherence after the durability sync too: the staging fsync may
        // have consumed the remaining budget — a mutation NEVER starts over
        // budget. Still pre-mutation → `Failed`.
        if started.elapsed() >= budget {
            return Err(clean(TransferFailureCause::Interrupted));
        }

        // 3. Promote atomically. FILES FIRST: `.pi` is touched only afterwards.
        fs::create_dir_all(&content_dir).map_err(|_| clean(TransferFailureCause::WriteRejected))?;
        let set_aside = if replacing {
            // RE-PROVE immediately before the set-aside: the initial proof aged
            // across the whole staging phase (copy + fsync can be long), and the
            // state that gets set aside MUST be the state that was proven. The
            // fresh classification decides:
            //   - still divergent-but-sound AND still attributable → replace;
            //   - became identical (something wrote exactly the plan meanwhile)
            //     → drop the staging, idempotent re-index, `ReusedIdentical`;
            //   - became unprovable / no longer attributable → refuse untouched;
            //   - vanished → nothing to set aside, plain creation below.
            match prove_target_state(&target, plan, started, budget).map_err(clean)? {
                Some(ExistingPackState::DivergentSound) => {
                    let pi_now = read_pi(&pi_path).map_err(clean)?;
                    if !divergent_folder_attributable(&pi_now, &uuid_bytes, &plan.short_id) {
                        return Err(clean(TransferFailureCause::DevicePackUnprovable));
                    }
                    // The re-proof re-read the whole old pack — its internal
                    // deadline checks run per entry, so the last read may end
                    // past the budget: re-check before the FIRST mutating act
                    // (still pre-mutation → `Failed`, old pack intact). The
                    // residual window between this check and the `rename` is
                    // not deterministically triggerable without an fs hook.
                    if started.elapsed() >= budget {
                        return Err(clean(TransferFailureCause::Interrupted));
                    }
                    // Set the old pack aside by a same-volume `rename` to a
                    // sweepable name. THE DEVICE MUTATION STARTS HERE: a
                    // successful set-aside that is not followed by a completed
                    // promotion is an honest `transfert incomplet` (a relaunch
                    // converges — the swept residue and the fresh cycle re-create
                    // the pack). A FAILED rename moved nothing → still `Failed`.
                    // No budget check between set-aside and promotion: the swap
                    // is deliberately not interrupted at its most fragile point.
                    let set_aside =
                        mount_path.join(format!("{DEVICE_REPLACED_PREFIX}{}", plan.short_id));
                    fs::rename(&target, &set_aside)
                        .map_err(|_| clean(TransferFailureCause::WriteRejected))?;
                    Some(set_aside)
                }
                Some(ExistingPackState::Identical) => {
                    if started.elapsed() >= budget {
                        return Err(clean(TransferFailureCause::Interrupted));
                    }
                    index_pack(mount_path, &pi_path, &uuid_bytes).map_err(mutated)?;
                    return Ok(WriteOutcome::ReusedIdentical);
                }
                Some(ExistingPackState::Unprovable) => {
                    return Err(clean(TransferFailureCause::DevicePackUnprovable));
                }
                None => None,
            }
        } else {
            None
        };
        // Failures from here on are post-mutation for a replacement (the old pack
        // left its canonical folder), pre-mutation otherwise.
        let promoted = |cause: TransferFailureCause| {
            if set_aside.is_some() {
                mutated(cause)
            } else {
                clean(cause)
            }
        };
        promote(staging.path(), &target).map_err(promoted)?;
        // The staged path no longer exists; the TempDir drop is a no-op.

        // FROM HERE the device IS mutated (content promoted): a durability or
        // index I/O failure is `Incomplete` (a partial copy may remain until the
        // next relaunch converges it via the three-outcome reuse path), never
        // `Failed`.
        // 4. Persist the promoted tree + `.content` parent so a power loss after
        //    the `.pi` update cannot resurrect a half-written folder.
        fsync_tree(&target).map_err(|_| mutated(TransferFailureCause::WriteRejected))?;
        fsync_dir(&content_dir).map_err(|_| mutated(TransferFailureCause::WriteRejected))?;

        // 5. Add the UUID to `.pi` atomically (idempotent), re-reading the
        //    freshest index first (F3). A promoted folder left unreferenced if
        //    this step fails is benign unused content; the forbidden inverse (an
        //    index entry without content) cannot happen FROM THIS STEP — the
        //    transient set-aside window of a replacement (between set-aside and
        //    promotion) is the assumed, honestly-classified exception (see the
        //    module doc). For a replacement the UUID is normally already
        //    indexed — the append is a no-op.
        index_pack(mount_path, &pi_path, &uuid_bytes).map_err(mutated)?;

        // 6. Best-effort cleanup of the set-aside old pack, AFTER the fsyncs and
        //    the index convergence: a failure leaves a sweepable residue, never a
        //    failed transfer.
        if let Some(old_pack) = set_aside {
            let _ = fs::remove_dir_all(&old_pack);
            return Ok(WriteOutcome::ReplacedDivergent);
        }
        Ok(WriteOutcome::CreatedNew)
    }
}

/// Best-effort sweep of on-device transfer residues: staging temp dirs / `.pi`
/// temp files ([`DEVICE_STAGING_PREFIX`]) left by an interrupted write, and
/// set-aside old packs ([`DEVICE_REPLACED_PREFIX`]) orphaned by a write
/// interrupted mid-replacement (their job already ended `transfert incomplet`;
/// the old content was superseded by a fully staged replacement before being
/// set aside). NEVER touches the device's own content. Returns the number of
/// entries removed (diagnostic only). Run before a write (and safe to call
/// whenever a writable device is present).
pub fn sweep_device_transfer_staging(mount_path: &Path) -> u32 {
    let mut removed = 0;
    let Ok(entries) = fs::read_dir(mount_path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(DEVICE_STAGING_PREFIX) && !name.starts_with(DEVICE_REPLACED_PREFIX) {
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
pub(super) fn read_pi(pi_path: &Path) -> Result<Vec<u8>, TransferFailureCause> {
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
/// already proven `target` does NOT exist — either nothing was there, or the
/// proven-divergent old pack was just set aside by `rename` (FR23); an
/// unprovable folder was refused upstream, never clobbered — so promotion must
/// not silently replace device content: refuse if ANY entry appeared meanwhile
/// (a race). The probe is no-follow (`symlink_metadata`): `exists()` would
/// report a dangling symlink as absent and the `rename` would clobber it.
pub(super) fn promote(staging_path: &Path, target: &Path) -> Result<(), TransferFailureCause> {
    match fs::symlink_metadata(target) {
        Ok(_) => return Err(TransferFailureCause::WriteRejected),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(TransferFailureCause::WriteRejected),
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
pub(super) fn write_pi_atomically(
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

pub(super) fn fsync_dir(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

pub(super) fn fsync_tree(dir: &Path) -> std::io::Result<()> {
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
pub(super) fn safe_rel_join(base: &Path, rel_path: &str) -> Result<PathBuf, TransferFailureCause> {
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
pub(super) fn mount_write_lock(mount_path: &Path) -> Arc<Mutex<()>> {
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

/// The PROVEN classification of an existing `.content/<SHORT_ID>` folder against
/// the plan (FR23). The proof is the write-job comparison of record — fresh, run
/// immediately before any mutation, never a cache or the read-only preview.
enum ExistingPackState {
    /// Every planned file present as a regular file with the exact size and a
    /// matching re-computed checksum, and NOT A SINGLE extra entry: the folder
    /// IS the plan's bytes.
    Identical,
    /// Any drift (missing / differing / extra files) where EVERY entry
    /// encountered is a readable regular file (directories only as containers of
    /// such files): safe to replace atomically.
    DivergentSound,
    /// An entry the proof cannot vouch for: a symlink, a special file, an empty
    /// directory (nothing a files-only pack can explain), or an unreadable I/O
    /// during the proof. Never reused, never replaced, never deleted.
    Unprovable,
}

/// Whether a divergent folder at the target SHORT_ID can be ATTRIBUTED to the
/// target UUID — the guard that authorizes a replacement. Two conditions, both
/// required:
///   - the index references the target UUID (an unindexed divergent folder is
///     an unknown residue or a collision with an unindexed UUID — nothing says
///     it is "the story already present");
///   - no OTHER indexed UUID shares the target SHORT_ID (the folder name is
///     only the last 8 hex — a bi-indexed collision means the folder may hold
///     the OTHER story's only content, and replacing it would strand that
///     story as index-without-content, the forbidden inverse, definitively).
///
/// Consulted at the initial proof AND at the re-proof (fresh `.pi` each time).
fn divergent_folder_attributable(
    pi_bytes: &[u8],
    uuid_bytes: &[u8; LUNII_PACK_UUID_BYTES],
    short_id: &str,
) -> bool {
    let index = parse_pack_index(pi_bytes);
    let ours_indexed = index.uuids.iter().any(|existing| existing == uuid_bytes);
    let colliding_other = index
        .uuids
        .iter()
        .any(|existing| existing != uuid_bytes && pack_short_id(existing) == short_id);
    ours_indexed && !colliding_other
}

/// Classify what sits at the target path, STARTING with the root itself,
/// no-follow: `Ok(None)` when nothing exists there; `Some(Unprovable)` when the
/// root entry is not a real directory (a symlink — dangling or not —, a regular
/// file, a special file: `exists()` would follow or hide those) or is unreadable
/// during the proof; otherwise the content classification of
/// [`prove_existing_pack_state`]. The write path calls this for the initial
/// decision AND re-calls it immediately before the set-aside mutation (the
/// proof must be adjacent to what it authorizes — a proof aged across the
/// staging phase proves nothing).
fn prove_target_state(
    target: &Path,
    plan: &PackWritePlan,
    started: Instant,
    budget: Duration,
) -> Result<Option<ExistingPackState>, TransferFailureCause> {
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Ok(Some(ExistingPackState::Unprovable)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Ok(Some(ExistingPackState::Unprovable)),
    }
    prove_existing_pack_state(target, plan, started, budget).map(Some)
}

/// Classify the existing target folder against the plan (see
/// [`ExistingPackState`]). EVERY existing entry is opened and read in full
/// (no-follow) — readability is part of the proof, for the divergent case too:
/// a folder is only "divergent-but-sound" when each of its entries could
/// actually be read, since the replacement verdict leads to deleting it.
/// Deadline-checked so a stalled mount aborts as `Interrupted`; every other
/// proof-time I/O failure classifies as [`ExistingPackState::Unprovable`]
/// rather than erroring (fail-closed refusal, not a transport failure).
fn prove_existing_pack_state(
    target: &Path,
    plan: &PackWritePlan,
    started: Instant,
    budget: Duration,
) -> Result<ExistingPackState, TransferFailureCause> {
    let Some(existing) = collect_regular_pack_files(target, started, budget)? else {
        return Ok(ExistingPackState::Unprovable);
    };

    let planned: std::collections::HashMap<&str, &crate::domain::transfer::PackWriteFile> = plan
        .files
        .iter()
        .map(|f| (f.rel_path.as_str(), f))
        .collect();

    // A missing or extra file is a divergence — recorded WITHOUT returning yet:
    // every existing entry below must still prove its readability first.
    let mut divergent = existing.len() != planned.len()
        || !existing
            .iter()
            .all(|(rel, _)| planned.contains_key(rel.as_str()));

    // Read EVERY existing entry in full (readability proof), comparing against
    // the plan when the path matches (size + checksum decide the divergence).
    for (rel, byte_len) in &existing {
        if started.elapsed() >= budget {
            return Err(TransferFailureCause::Interrupted);
        }
        let path = safe_rel_join(target, rel)?;
        match checksum_regular_no_follow(&path, started, budget) {
            Ok(Some(sum)) => match planned.get(rel.as_str()) {
                Some(file) if *byte_len == file.byte_len && sum == file.checksum => {}
                _ => divergent = true,
            },
            // The entry stopped being a provable regular file mid-proof, or its
            // bytes could not be read: unprovable, refuse — an unreadable entry
            // must never be classified "sound" and deleted.
            Ok(None) => return Ok(ExistingPackState::Unprovable),
            Err(cause) => return Err(cause),
        }
    }
    if divergent {
        Ok(ExistingPackState::DivergentSound)
    } else {
        Ok(ExistingPackState::Identical)
    }
}

/// Recursively collect `(forward-slash rel path, byte length)` for every entry
/// under `root`, PROVING regularity along the way: returns `Ok(None)` as soon as
/// an entry is not a readable regular file or a non-empty directory of such
/// files (symlink, special file, empty directory, unreadable I/O — the
/// [`ExistingPackState::Unprovable`] triggers). Deadline-checked so a stalled
/// mount aborts as `Interrupted`.
#[allow(clippy::type_complexity)]
fn collect_regular_pack_files(
    root: &Path,
    started: Instant,
    budget: Duration,
) -> Result<Option<Vec<(String, u64)>>, TransferFailureCause> {
    fn walk(
        root: &Path,
        dir: &Path,
        started: Instant,
        budget: Duration,
        out: &mut Vec<(String, u64)>,
    ) -> Result<bool, TransferFailureCause> {
        if started.elapsed() >= budget {
            return Err(TransferFailureCause::Interrupted);
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return Ok(false);
        };
        let mut seen_any = false;
        for entry in entries {
            seen_any = true;
            let Ok(entry) = entry else {
                return Ok(false);
            };
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                return Ok(false);
            };
            if meta.is_dir() {
                if !walk(root, &path, started, budget, out)? {
                    return Ok(false);
                }
                continue;
            }
            if !meta.is_file() {
                // Symlink / FIFO / socket / device: not provable.
                return Ok(false);
            }
            let Ok(rel) = path.strip_prefix(root) else {
                return Ok(false);
            };
            let mut parts = Vec::new();
            for component in rel.components() {
                match component {
                    std::path::Component::Normal(c) => parts.push(c.to_string_lossy().into_owned()),
                    // Defensive: a non-normal component cannot be a pack file.
                    _ => return Ok(false),
                }
            }
            out.push((parts.join("/"), meta.len()));
        }
        // An empty directory is nothing a files-only pack can explain — the
        // "unplanned directory" trigger of the unprovable refusal. The pack ROOT
        // itself is exempt: an empty target folder simply diverges (every
        // planned file missing) and is replaced.
        if !seen_any && dir != root {
            return Ok(false);
        }
        Ok(true)
    }
    let mut out = Vec::new();
    if walk(root, root, started, budget, &mut out)? {
        Ok(Some(out))
    } else {
        Ok(None)
    }
}

/// Stream the SHA-256 of an existing REGULAR file without ever following a
/// symlink: `lstat` first, open, then re-check the OPEN HANDLE's identity
/// (`fstat`, `(dev, ino)` on Unix) — the TOCTOU guard the copy path already
/// applies, required here because the proof's verdict can lead to deleting the
/// old pack. `Ok(None)` when the entry is not (or stopped being) a provable
/// regular file; deadline-checked (`Interrupted`).
fn checksum_regular_no_follow(
    path: &Path,
    started: Instant,
    budget: Duration,
) -> Result<Option<String>, TransferFailureCause> {
    let Ok(expected_meta) = fs::symlink_metadata(path) else {
        return Ok(None);
    };
    if !expected_meta.is_file() {
        return Ok(None);
    }
    let Ok(mut reader) = File::open(path) else {
        return Ok(None);
    };
    let Ok(opened) = reader.metadata() else {
        return Ok(None);
    };
    if !opened.is_file() {
        return Ok(None);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != expected_meta.dev() || opened.ino() != expected_meta.ino() {
            return Ok(None);
        }
    }
    #[cfg(not(unix))]
    {
        // Windows: the device is a FAT volume with no symlink support and
        // the lstat above already refused reparse points; the open handle is
        // verified to be a regular file, which is the available guarantee —
        // the same assumed limit as the shared copier pattern. This matters
        // here because the proof's verdict can lead to deleting the old pack.
        let _ = expected_meta;
    }

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    loop {
        if started.elapsed() >= budget {
            return Err(TransferFailureCause::Interrupted);
        }
        let Ok(read) = reader.read(&mut buf) else {
            return Ok(None);
        };
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(Some(format!("{:x}", hasher.finalize())))
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

    /// A no-op progress sink for the writes whose progress is not under test.
    fn noop_progress(_: WriteProgress) {}

    #[test]
    fn progress_is_monotone_and_bounded_during_a_successful_write() {
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x9A, 0x9B, 0x9C, 0x9D]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);
        let total: u64 = plan.files.iter().map(|f| f.byte_len).sum();

        let seen: Mutex<Vec<WriteProgress>> = Mutex::new(Vec::new());
        SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &|p| seen.lock().unwrap().push(p),
            )
            .expect("write must succeed");

        let seen = seen.into_inner().unwrap();
        assert!(
            !seen.is_empty(),
            "progress must be reported during the copy"
        );
        // Every report carries the same total, a monotone non-decreasing
        // bytes_done bounded by the total, ending exactly at the total (the
        // application clamps the fraction below 100 %, reserved for `completed`).
        assert!(seen.iter().all(|p| p.bytes_total == total));
        assert!(seen.windows(2).all(|w| w[1].bytes_done >= w[0].bytes_done));
        assert!(seen.iter().all(|p| p.bytes_done <= p.bytes_total));
        assert_eq!(seen.last().unwrap().bytes_done, total);
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

        let outcome = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("write must succeed");
        assert_eq!(
            outcome,
            WriteOutcome::CreatedNew,
            "nothing pre-existed — a first send"
        );

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
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("first write");
        let second = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("second write is a no-op");
        assert_eq!(
            second,
            WriteOutcome::ReusedIdentical,
            "an identical pack is constated as already up to date"
        );

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
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("a missing source file must fail the write");
        assert_eq!(err.cause, TransferFailureCause::WriteRejected);
        assert!(
            !err.reached_device_mutation,
            "a failure before promotion leaves the device unmutated"
        );

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
                &noop_progress,
            )
            .expect_err("zero budget must abort");
        assert_eq!(err.cause, TransferFailureCause::Interrupted);
        assert!(
            !err.reached_device_mutation,
            "an interruption is always before promotion → not mutated"
        );
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
    fn sweep_removes_orphan_set_aside_packs_too() {
        // FR23 — a `.rustory-replaced-*` residue (a swap interrupted between
        // set-aside and promotion) is reclaimed exactly like a staging residue,
        // and the device's own content survives.
        let mount = empty_mount();
        let orphan = mount
            .path()
            .join(format!("{DEVICE_REPLACED_PREFIX}FAC5562D"));
        std::fs::create_dir_all(&orphan).expect("mk orphan set-aside");
        std::fs::write(orphan.join("ni"), b"OLD").expect("seed old content");
        let content = mount.path().join(".content");
        std::fs::create_dir_all(&content).expect("mk content");

        assert_eq!(sweep_device_transfer_staging(mount.path()), 1);
        assert!(!orphan.exists(), "orphan set-aside must be removed");
        assert!(content.is_dir(), "real content must survive the sweep");
    }

    #[test]
    fn an_existing_divergent_sound_pack_under_the_same_short_id_is_replaced_atomically() {
        // FR23 — RE-SCOPED from the historical refused-not-clobbered semantics
        // by the update flow: a divergent folder made exclusively of readable
        // regular files is now REPLACED byte-faithfully by the plan's bytes.
        // The old content is gone, no residue remains, and the index converges.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x11, 0x22, 0x33, 0x44]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // Pre-seed a DIFFERENT pack folder under the SAME SHORT_ID (regular
        // files only — the divergent-but-sound case), with the target UUID
        // ALREADY indexed alongside another one: the realistic "an older
        // version of THIS story sits on the device" state (an unindexed
        // divergent folder is ambiguous and refused — its own test below).
        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed divergent dir");
        std::fs::write(target.join("ni"), b"STALE-AND-DIFFERENT").expect("seed stale ni");
        std::fs::write(target.join("old-extra"), b"OLD").expect("seed old extra");
        let other = uuid_bytes([9, 9, 9, 9]);
        let mut pi_seed = other.to_vec();
        pi_seed.extend_from_slice(&uuid);
        std::fs::write(mount.path().join(".pi"), &pi_seed).expect("seed .pi");

        let outcome = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("a divergent-but-sound indexed pack is replaced");
        assert_eq!(outcome, WriteOutcome::ReplacedDivergent);

        // Byte-faithful replacement: the planned bytes landed, the old content
        // and the old extra file are gone.
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            std::fs::read(source.path().join("ni")).unwrap(),
            "the plan's bytes must have replaced the stale ones"
        );
        assert!(
            !target.join("old-extra").exists(),
            "the old extra file must not survive the replacement"
        );
        assert!(target.join("rf").join("000").join("AAAAAAAA").is_file());
        // The index gained our UUID (and kept the other), and no set-aside /
        // staging residue remains.
        let pi = std::fs::read(mount.path().join(".pi")).expect("read .pi");
        assert!(index_contains(&pi, &uuid));
        assert!(index_contains(&pi, &other));
        assert_eq!(
            sweep_device_transfer_staging(mount.path()),
            0,
            "the set-aside old pack must be cleaned after a successful replacement"
        );
    }

    #[test]
    fn an_existing_unprovable_pack_under_the_same_short_id_is_refused_not_clobbered() {
        // FR23 — the historical refused-not-clobbered promise survives VERBATIM
        // for the UNPROVABLE case: an entry the proof cannot vouch for (here a
        // symlink) refuses with the dedicated cause, and the device tree is left
        // byte-for-byte intact.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x12, 0x23, 0x34, 0x45]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed dir");
        std::fs::write(target.join("ni"), b"WHO-KNOWS").expect("seed ni");
        #[cfg(unix)]
        std::os::unix::fs::symlink(target.join("ni"), target.join("li"))
            .expect("seed symlink entry");
        #[cfg(not(unix))]
        std::fs::create_dir_all(target.join("li")).expect("seed empty dir entry");
        let other = uuid_bytes([9, 9, 9, 9]);
        std::fs::write(mount.path().join(".pi"), other).expect("seed .pi");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("an unprovable existing pack must be refused");
        assert_eq!(err.cause, TransferFailureCause::DevicePackUnprovable);
        assert!(
            !err.reached_device_mutation,
            "a protective refusal leaves the device unmutated"
        );
        // The existing folder is intact (not clobbered) and `.pi` is unchanged.
        assert_eq!(std::fs::read(target.join("ni")).unwrap(), b"WHO-KNOWS");
        #[cfg(unix)]
        assert!(
            target.join("li").is_symlink(),
            "the unexplained entry must survive untouched"
        );
        assert_eq!(
            std::fs::read(mount.path().join(".pi")).unwrap(),
            other.to_vec()
        );
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn an_empty_directory_inside_the_existing_pack_refuses_unprovable() {
        // An empty directory is nothing a files-only pack can explain — the
        // "unplanned directory" trigger: refuse, never replace, tree intact.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x21, 0x32, 0x43, 0x54]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // Write the healthy pack first, then plant an empty directory inside.
        SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("initial write");
        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(target.join("mystery")).expect("seed empty dir");
        let pi_before = std::fs::read(mount.path().join(".pi")).expect("read .pi");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("an empty directory makes the pack unprovable");
        assert_eq!(err.cause, TransferFailureCause::DevicePackUnprovable);
        assert!(!err.reached_device_mutation);
        assert!(target.join("mystery").is_dir(), "the entry must survive");
        assert!(target.join("ni").is_file(), "planned content stays intact");
        assert_eq!(std::fs::read(mount.path().join(".pi")).unwrap(), pi_before);
    }

    #[test]
    fn a_staging_failure_on_a_divergent_pack_leaves_the_old_content_intact() {
        // FR23 / F2 spirit — the old content is only lost AFTER the full
        // replacement is staged: a copy failure during staging (missing source
        // file) must leave the divergent old pack byte-for-byte in place.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x31, 0x42, 0x53, 0x64]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed divergent dir");
        std::fs::write(target.join("ni"), b"OLD-DIVERGENT").expect("seed old ni");
        // The target UUID is indexed (the replacement would be authorized) —
        // the staging failure alone must stop the write.
        let other = uuid_bytes([9, 9, 9, 9]);
        let mut pi_seed = other.to_vec();
        pi_seed.extend_from_slice(&uuid);
        std::fs::write(mount.path().join(".pi"), &pi_seed).expect("seed .pi");
        // Break the SOURCE so the staging copy fails after the proof.
        std::fs::remove_file(source.path().join("ni")).expect("drop source ni");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("a staging failure must fail the write");
        assert_eq!(err.cause, TransferFailureCause::WriteRejected);
        assert!(
            !err.reached_device_mutation,
            "the old pack never left its folder — the device is unmutated"
        );
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            b"OLD-DIVERGENT",
            "the old content must be intact when the staging fails"
        );
        assert_eq!(std::fs::read(mount.path().join(".pi")).unwrap(), pi_seed);
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn a_relaunch_converges_after_an_interrupted_swap() {
        // Reproduce the exact mid-swap state (set-aside done, promotion not):
        // `.content/<SHORT_ID>` absent, the old pack under the set-aside name,
        // the UUID still indexed. A fresh cycle sweeps the residue and re-creates
        // the pack byte-faithfully — the honest convergence FR23 promises.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x41, 0x52, 0x63, 0x74]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let set_aside = mount
            .path()
            .join(format!("{DEVICE_REPLACED_PREFIX}{short_id}"));
        std::fs::create_dir_all(&set_aside).expect("seed set-aside residue");
        std::fs::write(set_aside.join("ni"), b"OLD").expect("seed old ni");
        std::fs::create_dir_all(mount.path().join(".content")).expect("mk content");
        std::fs::write(mount.path().join(".pi"), uuid).expect("seed .pi with uuid");

        let outcome = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("the relaunch must converge");
        assert_eq!(
            outcome,
            WriteOutcome::CreatedNew,
            "the canonical folder was absent — the relaunch re-creates it"
        );
        assert!(!set_aside.exists(), "the residue must have been swept");
        let target = mount.path().join(".content").join(&short_id);
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            std::fs::read(source.path().join("ni")).unwrap()
        );
        let pi = std::fs::read(mount.path().join(".pi")).expect("read .pi");
        assert_eq!(pi.len(), 16, "the UUID stays indexed exactly once");
        assert!(index_contains(&pi, &uuid));
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
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("initial write");
        // Simulate "content present, index lost".
        std::fs::write(mount.path().join(".pi"), Vec::<u8>::new()).expect("wipe index");
        let content_before =
            std::fs::read(mount.path().join(".content").join(&short_id).join("ni")).unwrap();

        let outcome = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("recovery indexes the existing healthy pack");
        assert_eq!(
            outcome,
            WriteOutcome::ReusedIdentical,
            "the healthy pack is reused, not re-written"
        );

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
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("a fragmented .pi must be refused");
        assert_eq!(err.cause, TransferFailureCause::WriteRejected);
        assert!(
            !err.reached_device_mutation,
            "a failure before promotion leaves the device unmutated"
        );
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
                .write_pack(
                    mount.path(),
                    source.path(),
                    &canonical,
                    &plan,
                    budget(),
                    &noop_progress,
                )
                .expect_err(bad);
            assert_eq!(err.cause, TransferFailureCause::WriteRejected, "{bad}");
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
            SystemDevicePackWriter.write_pack(
                &mp1,
                &sp1,
                &ca,
                &plan_a,
                Duration::from_secs(30),
                &noop_progress,
            )
        });
        let (mp2, sp2) = (mount_path.clone(), src_path.clone());
        let h2 = std::thread::spawn(move || {
            SystemDevicePackWriter.write_pack(
                &mp2,
                &sp2,
                &cb,
                &plan_b,
                Duration::from_secs(30),
                &noop_progress,
            )
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
                &noop_progress,
            )
            .expect_err("zero budget at the durability phase must abort");
        assert_eq!(err.cause, TransferFailureCause::Interrupted);
        assert!(
            !err.reached_device_mutation,
            "an interruption is always before promotion → not mutated"
        );
        assert!(!mount.path().join(".content").join(&short_id).exists());
        assert!(!mount.path().join(".pi").exists());
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn an_existing_pack_with_an_extra_regular_file_is_replaced_not_refused() {
        // C4 RE-SCOPED by the FR23 update flow: a folder holding the planned
        // files PLUS an extra REGULAR file was not produced by this plan — it
        // is a sound divergence now resolved by the atomic replacement (the
        // extra file does not survive), no longer a refusal.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0xE4, 0xE4, 0xE4, 0xE4]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // Write the healthy pack, then drop an EXTRA file the plan never describes.
        SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("initial write");
        let target = mount.path().join(".content").join(&short_id);
        std::fs::write(target.join("EXTRA"), b"unexpected").expect("seed extra file");
        let pi_before = std::fs::read(mount.path().join(".pi")).expect("read .pi");

        let outcome = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("an extra regular file is a sound divergence — replaced");
        assert_eq!(outcome, WriteOutcome::ReplacedDivergent);
        assert!(
            !target.join("EXTRA").exists(),
            "the extra file must not survive the replacement"
        );
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            std::fs::read(source.path().join("ni")).unwrap(),
            "planned content is the plan's bytes"
        );
        assert_eq!(
            std::fs::read(mount.path().join(".pi")).unwrap(),
            pi_before,
            ".pi already referenced the UUID — the append is a no-op"
        );
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_target_root_refuses_unprovable() {
        // The target ROOT itself is proven no-follow: a `.content/<SHORT_ID>`
        // that is a symlink to a real directory elsewhere must refuse — never
        // be traversed, classified, reused or replaced.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x91, 0x91, 0x91, 0x91]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // A REAL directory elsewhere on the volume, symlinked at the target.
        let elsewhere = mount.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("mk elsewhere");
        std::fs::write(elsewhere.join("ni"), b"ELSEWHERE").expect("seed elsewhere ni");
        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(mount.path().join(".content")).expect("mk content");
        std::os::unix::fs::symlink(&elsewhere, &target).expect("symlink root");
        // Indexed or not makes no difference: the root is unprovable first.
        std::fs::write(mount.path().join(".pi"), uuid).expect("seed .pi");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("a symlinked target root must be refused");
        assert_eq!(err.cause, TransferFailureCause::DevicePackUnprovable);
        assert!(!err.reached_device_mutation);
        // The symlink AND its destination are byte-for-byte intact.
        assert!(target.is_symlink(), "the symlink itself must survive");
        assert_eq!(
            std::fs::read(elsewhere.join("ni")).unwrap(),
            b"ELSEWHERE",
            "the symlink destination must never be touched"
        );
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_at_the_target_root_refuses_unprovable() {
        // `exists()` reports a dangling symlink as ABSENT — the old probe would
        // have taken the creation path and the promotion `rename` would have
        // clobbered the link. The no-follow root proof refuses instead.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x92, 0x92, 0x92, 0x92]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(mount.path().join(".content")).expect("mk content");
        std::os::unix::fs::symlink(mount.path().join("nowhere"), &target)
            .expect("dangling symlink");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("a dangling symlink at the target root must be refused");
        assert_eq!(err.cause, TransferFailureCause::DevicePackUnprovable);
        assert!(!err.reached_device_mutation);
        assert!(
            target.is_symlink(),
            "the dangling link must survive untouched"
        );
        assert!(!mount.path().join(".pi").exists(), "no index mutation");
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn a_target_mutated_during_staging_is_re_proven_and_refused() {
        // The initial proof ages across the staging phase: the state is
        // RE-PROVEN immediately before the set-aside. Here the progress
        // callback (which fires during the staging copy) swaps a file of the
        // proven-divergent pack for a symlink — the re-proof must classify the
        // NEW state unprovable and refuse with zero mutation.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x93, 0x93, 0x93, 0x93]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed divergent dir");
        std::fs::write(target.join("ni"), b"DIVERGENT-AT-PROOF-TIME").expect("seed ni");
        let mut pi_seed = Vec::new();
        pi_seed.extend_from_slice(&uuid);
        std::fs::write(mount.path().join(".pi"), &pi_seed).expect("seed .pi");

        // Mutate the target DURING the staging copy (after the initial proof).
        let mutate_once = {
            let target = target.clone();
            let done = Mutex::new(false);
            move |_p: WriteProgress| {
                let mut done = done.lock().unwrap();
                if !*done {
                    *done = true;
                    #[cfg(unix)]
                    {
                        std::fs::remove_file(target.join("ni")).expect("drop ni");
                        std::os::unix::fs::symlink(target.join("nowhere"), target.join("ni"))
                            .expect("swap for symlink");
                    }
                    #[cfg(not(unix))]
                    {
                        std::fs::remove_file(target.join("ni")).expect("drop ni");
                        std::fs::create_dir_all(target.join("ni")).expect("swap for empty dir");
                    }
                }
            }
        };

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &mutate_once,
            )
            .expect_err("a target mutated during staging must be re-proven and refused");
        assert_eq!(err.cause, TransferFailureCause::DevicePackUnprovable);
        assert!(
            !err.reached_device_mutation,
            "the refusal happens BEFORE the set-aside — zero device mutation"
        );
        // The mutated state survives untouched: no set-aside, no staging residue.
        #[cfg(unix)]
        assert!(target.join("ni").is_symlink(), "the swapped entry survives");
        assert_eq!(std::fs::read(mount.path().join(".pi")).unwrap(), pi_seed);
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    /// Make `path` unreadable (mode 000). Returns `false` when the running
    /// process can still open it (root — permissions are inoperative), in
    /// which case the caller skips: the unreadable path is untestable.
    #[cfg(unix)]
    fn make_unreadable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        File::open(path).is_err()
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_extra_file_refuses_unprovable() {
        // Readability is part of the proof for EVERY existing entry, extra
        // files included: an extra regular file that cannot be read must
        // refuse (it would otherwise be deleted by the replacement without
        // ever having been provable).
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x94, 0x94, 0x94, 0x94]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // A healthy indexed pack + an EXTRA unreadable file.
        SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect("initial write");
        let target = mount.path().join(".content").join(&short_id);
        let extra = target.join("EXTRA");
        std::fs::write(&extra, b"cannot-read-me").expect("seed extra");
        if !make_unreadable(&extra) {
            return; // root: permissions inoperative, path untestable here.
        }
        let pi_before = std::fs::read(mount.path().join(".pi")).expect("read .pi");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("an unreadable extra file must refuse, not be replaced away");
        assert_eq!(err.cause, TransferFailureCause::DevicePackUnprovable);
        assert!(!err.reached_device_mutation);
        assert!(extra.exists(), "the unreadable entry must survive");
        assert!(target.join("ni").is_file(), "planned content stays intact");
        assert_eq!(std::fs::read(mount.path().join(".pi")).unwrap(), pi_before);
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_planned_file_with_size_drift_refuses_unprovable() {
        // A planned file whose size drifts AND which cannot be read: the size
        // drift alone must not shortcut to "divergent-but-sound" — the entry
        // never proved its readability, so the pack is unprovable.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x95, 0x95, 0x95, 0x95]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed dir");
        // Only `ni`, with a size that differs from the plan, unreadable.
        let ni = target.join("ni");
        std::fs::write(&ni, b"SHORT").expect("seed drifted ni");
        if !make_unreadable(&ni) {
            return; // root: permissions inoperative, path untestable here.
        }
        let mut pi_seed = Vec::new();
        pi_seed.extend_from_slice(&uuid);
        std::fs::write(mount.path().join(".pi"), &pi_seed).expect("seed .pi");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("an unreadable planned file must refuse despite the size drift");
        assert_eq!(err.cause, TransferFailureCause::DevicePackUnprovable);
        assert!(!err.reached_device_mutation);
        assert!(ni.exists(), "the unreadable entry must survive");
        assert_eq!(std::fs::read(mount.path().join(".pi")).unwrap(), pi_seed);
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn a_divergent_pack_not_referenced_by_the_index_is_refused_as_ambiguous() {
        // Ambiguity guard: a divergent folder whose UUID is NOT in `.pi` is not
        // provably "the story already present" — it can be a SHORT_ID collision
        // with ANOTHER UUID (the last 8 hex are not unique) or an unknown
        // residue. Fail-closed: refuse, never replace, never index.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x96, 0x96, 0x96, 0x96]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // A divergent folder at OUR SHORT_ID, `.pi` referencing only ANOTHER
        // UUID (the collision scenario).
        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed collision dir");
        std::fs::write(target.join("ni"), b"SOMEONE-ELSES-PACK").expect("seed ni");
        let other = uuid_bytes([9, 9, 9, 9]);
        std::fs::write(mount.path().join(".pi"), other).expect("seed .pi");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("an unindexed divergent folder is ambiguous — refused");
        assert_eq!(err.cause, TransferFailureCause::DevicePackUnprovable);
        assert!(!err.reached_device_mutation);
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            b"SOMEONE-ELSES-PACK",
            "the ambiguous folder must be left intact"
        );
        assert_eq!(
            std::fs::read(mount.path().join(".pi")).unwrap(),
            other.to_vec(),
            "the index must not gain our UUID"
        );
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn a_budget_exhausted_during_the_staging_copy_never_starts_the_mutation() {
        // Budget adherence around the mutation: exhaust the budget from inside
        // the staging copy (the progress callback sleeps past it) — the write
        // must end `Interrupted` PRE-mutation: the proven-divergent old pack is
        // never set aside, nothing is promoted, no residue remains. (The
        // narrower fsync-only window is guarded by the same post-durability
        // check but cannot be triggered deterministically without an fs hook.)
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x97, 0x97, 0x97, 0x97]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed divergent dir");
        std::fs::write(target.join("ni"), b"OLD-DIVERGENT").expect("seed ni");
        let mut pi_seed = Vec::new();
        pi_seed.extend_from_slice(&uuid);
        std::fs::write(mount.path().join(".pi"), &pi_seed).expect("seed .pi");

        let tight_budget = Duration::from_millis(300);
        let sleep_past_budget = |_p: WriteProgress| std::thread::sleep(Duration::from_millis(400));

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                tight_budget,
                &sleep_past_budget,
            )
            .expect_err("an exhausted budget must interrupt before any mutation");
        assert_eq!(err.cause, TransferFailureCause::Interrupted);
        assert!(
            !err.reached_device_mutation,
            "the old pack was never set aside — pre-mutation"
        );
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            b"OLD-DIVERGENT",
            "the proven-divergent pack must be intact"
        );
        assert_eq!(std::fs::read(mount.path().join(".pi")).unwrap(), pi_seed);
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn a_bi_indexed_short_id_collision_is_refused_not_replaced() {
        // Attribution guard, second direction: TWO indexed UUIDs share the
        // target SHORT_ID (the folder name is only the last 8 hex). The
        // divergent folder may hold the OTHER story's only content — replacing
        // it would strand that story as index-without-content. Refuse, arbre
        // and index intact.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x98, 0x98, 0x98, 0x98]);
        // A DIFFERENT UUID with the SAME last-8-hex SHORT_ID.
        let mut colliding = uuid;
        colliding[0] = 0xCD;
        assert_ne!(uuid, colliding);
        assert_eq!(pack_short_id(&uuid), pack_short_id(&colliding));
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        // The divergent folder (plausibly the OTHER story's pack), with BOTH
        // UUIDs indexed.
        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed dir");
        std::fs::write(target.join("ni"), b"THE-OTHER-STORYS-PACK").expect("seed ni");
        let mut pi_seed = uuid.to_vec();
        pi_seed.extend_from_slice(&colliding);
        std::fs::write(mount.path().join(".pi"), &pi_seed).expect("seed .pi");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("a bi-indexed SHORT_ID collision must be refused");
        assert_eq!(err.cause, TransferFailureCause::DevicePackUnprovable);
        assert!(!err.reached_device_mutation);
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            b"THE-OTHER-STORYS-PACK",
            "the possibly-foreign folder must be left intact"
        );
        assert_eq!(
            std::fs::read(mount.path().join(".pi")).unwrap(),
            pi_seed,
            "the index must be untouched"
        );
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    /// Copy the plausible pack's files from `source` to `target` with the
    /// exact plan path-set — used by the re-prove tests to make the target
    /// IDENTICAL to the plan mid-staging.
    fn copy_plan_files(source: &Path, target: &Path) {
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
            let src = safe_rel_join(source, rel).expect("trusted test rel");
            let dst = safe_rel_join(target, rel).expect("trusted test rel");
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).expect("mk parent");
            }
            std::fs::copy(&src, &dst).expect("copy plan file");
        }
    }

    #[test]
    fn a_target_become_identical_during_staging_is_reused_not_replaced() {
        // Re-prove outcome "became identical": something wrote exactly the
        // plan's bytes during the staging phase — the fresh proof classifies
        // Identical, the staging is dropped, the index converges idempotently
        // and the outcome is HONESTLY ReusedIdentical (nothing was replaced).
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x99, 0x99, 0x99, 0x99]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed divergent dir");
        std::fs::write(target.join("ni"), b"DIVERGENT-AT-PROOF-TIME").expect("seed ni");
        let mut pi_seed = Vec::new();
        pi_seed.extend_from_slice(&uuid);
        std::fs::write(mount.path().join(".pi"), &pi_seed).expect("seed .pi");

        // During the staging copy, make the target EXACTLY the plan's bytes.
        let make_identical_once = {
            let source = source.path().to_path_buf();
            let target = target.clone();
            let done = Mutex::new(false);
            move |_p: WriteProgress| {
                let mut done = done.lock().unwrap();
                if !*done {
                    *done = true;
                    std::fs::remove_file(target.join("ni")).expect("drop divergent ni");
                    copy_plan_files(&source, &target);
                }
            }
        };

        let outcome = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &make_identical_once,
            )
            .expect("a target become identical is reused");
        assert_eq!(outcome, WriteOutcome::ReusedIdentical);
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            std::fs::read(source.path().join("ni")).unwrap(),
            "the target holds the plan's bytes"
        );
        let pi = std::fs::read(mount.path().join(".pi")).expect("read .pi");
        assert_eq!(pi.len(), 16, "the UUID stays indexed exactly once");
        assert!(index_contains(&pi, &uuid));
        assert_eq!(
            sweep_device_transfer_staging(mount.path()),
            0,
            "the dropped staging leaves no residue"
        );
    }

    #[test]
    fn a_target_vanished_during_staging_falls_back_to_plain_creation() {
        // Re-prove outcome "vanished": the divergent folder disappeared during
        // the staging phase — nothing to set aside, the write falls back to a
        // plain creation and reports CreatedNew honestly.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x9A, 0x9A, 0x9A, 0x9A]);
        let canonical = format_pack_uuid(&uuid);
        let short_id = pack_short_id(&uuid);
        let plan = plan_for(source.path(), &short_id);

        let target = mount.path().join(".content").join(&short_id);
        std::fs::create_dir_all(&target).expect("seed divergent dir");
        std::fs::write(target.join("ni"), b"ABOUT-TO-VANISH").expect("seed ni");
        let mut pi_seed = Vec::new();
        pi_seed.extend_from_slice(&uuid);
        std::fs::write(mount.path().join(".pi"), &pi_seed).expect("seed .pi");

        // During the staging copy, remove the target entirely.
        let vanish_once = {
            let target = target.clone();
            let done = Mutex::new(false);
            move |_p: WriteProgress| {
                let mut done = done.lock().unwrap();
                if !*done {
                    *done = true;
                    std::fs::remove_dir_all(&target).expect("vanish target");
                }
            }
        };

        let outcome = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &vanish_once,
            )
            .expect("a vanished target falls back to plain creation");
        assert_eq!(outcome, WriteOutcome::CreatedNew);
        assert_eq!(
            std::fs::read(target.join("ni")).unwrap(),
            std::fs::read(source.path().join("ni")).unwrap(),
            "the created pack holds the plan's bytes"
        );
        assert!(target.join("rf").join("000").join("AAAAAAAA").is_file());
        let pi = std::fs::read(mount.path().join(".pi")).expect("read .pi");
        assert_eq!(pi.len(), 16);
        assert!(index_contains(&pi, &uuid));
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }

    #[test]
    fn a_plan_short_id_incoherent_with_the_uuid_is_refused_before_any_io() {
        // Defense in depth at the write boundary: a plan whose `short_id` is
        // not the one the UUID derives would aim every proof and mutation at
        // ANOTHER story's folder — refused before a single device I/O.
        let mount = empty_mount();
        let source = source_pack();
        let uuid = uuid_bytes([0x9B, 0x9B, 0x9B, 0x9B]);
        let canonical = format_pack_uuid(&uuid);
        // A plan built for a DIFFERENT short id.
        let plan = plan_for(source.path(), "DEADBEEF");

        let err = SystemDevicePackWriter
            .write_pack(
                mount.path(),
                source.path(),
                &canonical,
                &plan,
                budget(),
                &noop_progress,
            )
            .expect_err("an incoherent plan short_id must be refused");
        assert_eq!(err.cause, TransferFailureCause::WriteRejected);
        assert!(!err.reached_device_mutation);
        assert!(!mount.path().join(".content").exists());
        assert!(!mount.path().join(".pi").exists());
        assert_eq!(sweep_device_transfer_staging(mount.path()), 0);
    }
}
