//! Deletes a story already present on a Lunii volume — the inverse of the
//! [`writer`](super::writer). No decryption, no format knowledge: deleting only
//! removes opaque bytes the device already held, so it is cohort-agnostic (it
//! works on V3 exactly like V1/V2).
//!
//! Safety contract — the MIRROR of the writer's "files first, index second":
//!   1. acquire the per-mount write lock (shared with the writer, so a delete
//!      and a transfer on the same volume never race the `.pi` read-modify-write);
//!   2. read the FRESHEST `.pi` (F7 corruption guard); if the UUID is not listed,
//!      the delete is a no-op ([`DeleteOutcome::NotPresent`], idempotent);
//!   3. **INDEX FIRST** — rewrite `.pi` WITHOUT the UUID, atomically (temp +
//!      `rename` + fsync). The story disappears from the library the instant this
//!      lands;
//!   4. **CONTENT SECOND** — remove `.content/<SHORT_ID>` (no-follow: a symlinked
//!      or non-directory target is left untouched, never followed), then fsync the
//!      `.content` parent.
//!
//! An interruption between steps 3 and 4 can only ORPHAN a content folder
//! (harmless, reclaimable) — it can never leave a `.pi` entry pointing at deleted
//! content. A content-removal I/O error after a successful delist is therefore
//! non-fatal: the story is already gone from the library.
//!
//! A trait keeps the application layer testable without a real volume
//! ([`MockDevicePackDeleter`](super::mock::MockDevicePackDeleter)).

use std::fs;
use std::path::Path;

use crate::domain::device::{parse_pack_index, LUNII_CONTENT_DIR, LUNII_DEVICE_ID_MARKER};
use crate::domain::transfer::{
    pack_uuid_bytes, remove_pack_uuid, short_id_from_pack_uuid, TransferFailureCause,
};

use super::writer::{fsync_dir, mount_write_lock, read_pi, write_pi_atomically};

/// What a delete CONSTATED. Both variants are successes: the story is not on the
/// device's list afterward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// The UUID was listed and has been delisted; its content folder was removed
    /// (or was already absent / left untouched because unprovable).
    Deleted,
    /// The UUID was not in the index — nothing to delete (a re-issued delete, or
    /// a stale selection after a concurrent re-read).
    NotPresent,
}

/// A delete failure plus whether the DEVICE was already mutated (`.pi`
/// rewritten) when it happened — the honest input for a `Failed` vs `Incomplete`
/// distinction, mirroring [`WriteFailure`](super::writer::WriteFailure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteFailure {
    pub cause: TransferFailureCause,
    pub reached_device_mutation: bool,
}

fn clean(cause: TransferFailureCause) -> DeleteFailure {
    DeleteFailure {
        cause,
        reached_device_mutation: false,
    }
}

/// Removes a story from a writable Lunii volume: delist its `.pi` entry, then
/// remove its content folder. MUST update `.pi` BEFORE touching the content so an
/// interruption can only orphan content, never dangle the index.
pub trait DevicePackDeleter: Send + Sync + 'static {
    fn delete_pack(
        &self,
        mount_path: &Path,
        pack_uuid: &str,
    ) -> Result<DeleteOutcome, DeleteFailure>;
}

/// Production deleter: atomic `.pi` rewrite + no-follow content removal + fsync.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDevicePackDeleter;

impl DevicePackDeleter for SystemDevicePackDeleter {
    fn delete_pack(
        &self,
        mount_path: &Path,
        pack_uuid: &str,
    ) -> Result<DeleteOutcome, DeleteFailure> {
        // Canonicalize the identifiers up-front — a non-canonical UUID is a
        // programming error (the caller passes the value the read surfaced),
        // refused before any device touch.
        let uuid_bytes =
            pack_uuid_bytes(pack_uuid).ok_or_else(|| clean(TransferFailureCause::WriteRejected))?;
        let short_id = short_id_from_pack_uuid(pack_uuid)
            .ok_or_else(|| clean(TransferFailureCause::WriteRejected))?;

        // Serialize against writes AND other deletes on the same volume (shared
        // lock map): the `.pi` read-modify-write must be atomic per mount.
        let lock = mount_write_lock(mount_path);
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        let pi_path = mount_path.join(LUNII_DEVICE_ID_MARKER);
        let current = read_pi(&pi_path).map_err(clean)?;

        // Idempotent: an unlisted UUID is nothing to delete (never mutate on a
        // no-op — a stale/duplicate delete stays harmless).
        let listed = parse_pack_index(&current)
            .uuids
            .iter()
            .any(|existing| existing == &uuid_bytes);
        if !listed {
            return Ok(DeleteOutcome::NotPresent);
        }

        // INDEX FIRST: delist atomically. A failure here leaves the ORIGINAL
        // `.pi` intact (temp + persist is atomic), so the device is not mutated.
        let updated = remove_pack_uuid(&current, &uuid_bytes);
        write_pi_atomically(mount_path, &pi_path, &updated).map_err(clean)?;

        // CONTENT SECOND (best-effort, non-fatal): the story is already gone from
        // the library. Remove the payload folder no-follow; a symlinked or
        // non-directory target is left untouched (never followed/clobbered), and
        // an absent one is already done. A removal I/O error only orphans content.
        let content_parent = mount_path.join(LUNII_CONTENT_DIR);
        let content_dir = content_parent.join(&short_id);
        if let Ok(meta) = fs::symlink_metadata(&content_dir) {
            if meta.is_dir() {
                let _ = fs::remove_dir_all(&content_dir);
                let _ = fsync_dir(&content_parent);
            }
        }

        Ok(DeleteOutcome::Deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::transfer::pack_uuid_bytes;
    use std::fs;
    use tempfile::tempdir;

    const A: &str = "11111111-1111-1111-1111-1111aaaaaaaa";
    const TARGET: &str = "22222222-2222-2222-2222-2222bbbbbbbb";
    const C: &str = "33333333-3333-3333-3333-3333cccccccc";

    fn short(uuid: &str) -> String {
        uuid[uuid.len() - 8..].to_ascii_uppercase()
    }

    /// Build a fake device volume: a `.pi` listing `uuids` back-to-back, and a
    /// `.content/<SHORT_ID>/ni` payload for each — the shape the deleter mutates.
    fn setup_device(uuids: &[&str]) -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        let mut pi = Vec::new();
        for u in uuids {
            pi.extend_from_slice(&pack_uuid_bytes(u).unwrap());
        }
        fs::write(dir.path().join(LUNII_DEVICE_ID_MARKER), &pi).unwrap();
        for u in uuids {
            let cdir = dir.path().join(LUNII_CONTENT_DIR).join(short(u));
            fs::create_dir_all(&cdir).unwrap();
            fs::write(cdir.join("ni"), b"opaque-bytes").unwrap();
        }
        dir
    }

    #[test]
    fn delists_the_target_and_removes_its_content_keeping_the_others() {
        let dir = setup_device(&[A, TARGET, C]);
        let out = SystemDevicePackDeleter
            .delete_pack(dir.path(), TARGET)
            .expect("delete ok");
        assert_eq!(out, DeleteOutcome::Deleted);

        // `.pi` no longer lists TARGET but keeps A then C, in reading order.
        let pi = fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap();
        let mut expected = pack_uuid_bytes(A).unwrap().to_vec();
        expected.extend_from_slice(&pack_uuid_bytes(C).unwrap());
        assert_eq!(pi, expected);

        // TARGET's content folder is gone; the siblings remain untouched.
        let content = dir.path().join(LUNII_CONTENT_DIR);
        assert!(!content.join(short(TARGET)).exists());
        assert!(content.join(short(A)).join("ni").exists());
        assert!(content.join(short(C)).join("ni").exists());
    }

    #[test]
    fn an_unlisted_pack_is_a_no_op_leaving_the_index_and_content_intact() {
        let dir = setup_device(&[A, C]);
        let before = fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap();
        let out = SystemDevicePackDeleter
            .delete_pack(dir.path(), TARGET)
            .expect("no-op ok");
        assert_eq!(out, DeleteOutcome::NotPresent);
        let after = fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap();
        assert_eq!(before, after, ".pi is untouched for an unlisted pack");
        assert!(dir.path().join(LUNII_CONTENT_DIR).join(short(A)).exists());
    }

    #[test]
    fn a_corrupt_pi_with_a_trailing_fragment_is_refused_never_rewritten() {
        let dir = tempdir().expect("tempdir");
        let mut pi = pack_uuid_bytes(TARGET).unwrap().to_vec();
        pi.extend_from_slice(&[0xAB, 0xCD]); // a trailing fragment → corrupt index
        fs::write(dir.path().join(LUNII_DEVICE_ID_MARKER), &pi).unwrap();

        let err = SystemDevicePackDeleter
            .delete_pack(dir.path(), TARGET)
            .expect_err("a corrupt index is refused, never mutated");
        assert_eq!(err.cause, TransferFailureCause::WriteRejected);
        assert!(!err.reached_device_mutation);
        // The corrupt bytes are left exactly as they were.
        let after = fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap();
        assert_eq!(after, pi);
    }

    #[test]
    fn an_absent_content_folder_still_delists_the_pi() {
        // The UUID is listed but its payload folder is already gone: the delist
        // still succeeds (INDEX FIRST), nothing to remove.
        let dir = tempdir().expect("tempdir");
        let mut pi = pack_uuid_bytes(A).unwrap().to_vec();
        pi.extend_from_slice(&pack_uuid_bytes(TARGET).unwrap());
        fs::write(dir.path().join(LUNII_DEVICE_ID_MARKER), &pi).unwrap();
        fs::create_dir_all(dir.path().join(LUNII_CONTENT_DIR).join(short(A))).unwrap();

        let out = SystemDevicePackDeleter
            .delete_pack(dir.path(), TARGET)
            .expect("delist ok");
        assert_eq!(out, DeleteOutcome::Deleted);
        let pi = fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap();
        assert_eq!(pi, pack_uuid_bytes(A).unwrap().to_vec());
    }

    #[test]
    fn a_non_canonical_uuid_is_refused_before_any_device_touch() {
        let dir = setup_device(&[A]);
        let before = fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap();
        let err = SystemDevicePackDeleter
            .delete_pack(dir.path(), "not-a-uuid")
            .expect_err("refused");
        assert!(!err.reached_device_mutation);
        let after = fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap();
        assert_eq!(before, after);
    }

    /// Manual smoke against a SCRATCH COPY of a real device (never the device
    /// itself). Point `RUSTORY_TEST_DELETE_MOUNT` at a copy holding the real
    /// `.pi`, optionally `RUSTORY_TEST_DELETE_INDEX` (default 0) to pick which
    /// listed pack to delete. Proves the production deleter, run on the actual
    /// on-device `.pi` bytes, removes exactly that pack and preserves every
    /// other one. Ignored by default (needs the env + a real copy).
    #[test]
    #[ignore = "manual: set RUSTORY_TEST_DELETE_MOUNT to a scratch device copy"]
    fn real_device_copy_delete_smoke() {
        use crate::domain::device::format_pack_uuid;

        let mount = std::path::PathBuf::from(
            std::env::var("RUSTORY_TEST_DELETE_MOUNT")
                .expect("set RUSTORY_TEST_DELETE_MOUNT to a scratch device copy"),
        );
        let index: usize = std::env::var("RUSTORY_TEST_DELETE_INDEX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let pi_before = fs::read(mount.join(LUNII_DEVICE_ID_MARKER)).expect("read .pi");
        let before = parse_pack_index(&pi_before);
        let total = before.uuids.len();
        assert!(index < total, "index {index} out of range ({total} packs)");
        let target = format_pack_uuid(&before.uuids[index]);
        let target_short = short_id_from_pack_uuid(&target).unwrap();
        eprintln!("[smoke] {total} packs; deleting #{index} = {target} ({target_short})");

        let out = SystemDevicePackDeleter
            .delete_pack(&mount, &target)
            .expect("delete ok");
        assert_eq!(out, DeleteOutcome::Deleted);

        // Exactly one pack removed; every OTHER uuid survives, in order.
        let pi_after = fs::read(mount.join(LUNII_DEVICE_ID_MARKER)).expect("read .pi after");
        let after = parse_pack_index(&pi_after);
        assert_eq!(after.uuids.len(), total - 1, "exactly one pack delisted");
        let expected: Vec<_> = before
            .uuids
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, u)| *u)
            .collect();
        assert_eq!(
            after.uuids, expected,
            "the other packs are untouched, in order"
        );
        // The target's content folder is gone; the siblings remain.
        assert!(!mount.join(LUNII_CONTENT_DIR).join(&target_short).exists());
        for (i, u) in before.uuids.iter().enumerate() {
            if i == index {
                continue;
            }
            let sid = short_id_from_pack_uuid(&format_pack_uuid(u)).unwrap();
            assert!(
                mount.join(LUNII_CONTENT_DIR).join(&sid).exists(),
                "sibling {sid} content preserved"
            );
        }
        eprintln!(
            "[smoke] OK — {} packs remain, target content removed",
            after.uuids.len()
        );
    }
}
