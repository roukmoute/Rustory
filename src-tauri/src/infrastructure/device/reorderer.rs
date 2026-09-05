//! Reorders the stories of a Lunii volume — the device plays its packs in
//! `.pi` order, so "move a story on the wheel" is a rewrite of that index
//! in the requested order. Index ONLY: no content is touched. Same
//! disciplines as the deleter: one shared write lock per mount, a strict
//! read-modify-write (the requested order must match EXACTLY the packs the
//! device lists — a stale list is refused, never guessed around), and an
//! atomic rewrite (temp + rename) so an interruption leaves the original
//! index intact.

use std::path::Path;

use crate::domain::device::LUNII_DEVICE_ID_MARKER;
use crate::domain::transfer::{
    pack_uuid_bytes, reorder_pack_index, ReorderIndexError, TransferFailureCause,
};

use super::writer::{mount_write_lock, read_pi, write_pi_atomically};

/// What a reorder did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderOutcome {
    /// The index now lists the packs in the requested order.
    Reordered,
    /// The requested order was already the device's — nothing written.
    Unchanged,
}

/// Why a reorder was refused. Nothing was written in either case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderFailure {
    /// The requested order does not match the packs the device lists (a
    /// pack added or removed since the list was read): re-read and retry.
    Diverged,
    /// A malformed uuid, a corrupt index, or the write itself failed.
    Rejected(TransferFailureCause),
}

/// Rewrites the visible index of a writable Lunii volume in a new order.
pub trait DevicePackReorderer: Send + Sync + 'static {
    fn reorder_packs(
        &self,
        mount_path: &Path,
        ordered_pack_uuids: &[String],
    ) -> Result<ReorderOutcome, ReorderFailure>;
}

/// Production reorderer: lock → read `.pi` → strict permutation → atomic rewrite.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDevicePackReorderer;

impl DevicePackReorderer for SystemDevicePackReorderer {
    fn reorder_packs(
        &self,
        mount_path: &Path,
        ordered_pack_uuids: &[String],
    ) -> Result<ReorderOutcome, ReorderFailure> {
        let mut ordered = Vec::with_capacity(ordered_pack_uuids.len());
        for uuid in ordered_pack_uuids {
            ordered.push(pack_uuid_bytes(uuid).ok_or(ReorderFailure::Rejected(
                TransferFailureCause::WriteRejected,
            ))?);
        }

        let lock = mount_write_lock(mount_path);
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        let pi_path = mount_path.join(LUNII_DEVICE_ID_MARKER);
        let current = read_pi(&pi_path).map_err(ReorderFailure::Rejected)?;
        let updated = reorder_pack_index(&current, &ordered).map_err(|err| match err {
            ReorderIndexError::Diverged => ReorderFailure::Diverged,
            ReorderIndexError::Corrupt => {
                ReorderFailure::Rejected(TransferFailureCause::WriteRejected)
            }
        })?;
        if updated == current {
            return Ok(ReorderOutcome::Unchanged);
        }
        write_pi_atomically(mount_path, &pi_path, &updated).map_err(ReorderFailure::Rejected)?;
        Ok(ReorderOutcome::Reordered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    const A: &str = "11111111-1111-1111-1111-1111aaaaaaaa";
    const B: &str = "22222222-2222-2222-2222-2222bbbbbbbb";
    const C: &str = "33333333-3333-3333-3333-3333cccccccc";

    fn setup_device(uuids: &[&str]) -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        let mut pi = Vec::new();
        for u in uuids {
            pi.extend_from_slice(&pack_uuid_bytes(u).unwrap());
        }
        fs::write(dir.path().join(LUNII_DEVICE_ID_MARKER), &pi).unwrap();
        dir
    }

    fn pi_of(dir: &tempfile::TempDir) -> Vec<u8> {
        fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap()
    }

    #[test]
    fn rewrites_the_index_in_the_requested_order_and_nothing_else() {
        let dir = setup_device(&[A, B, C]);
        let out = SystemDevicePackReorderer
            .reorder_packs(dir.path(), &[C.into(), A.into(), B.into()])
            .expect("reorder");
        assert_eq!(out, ReorderOutcome::Reordered);
        let expected: Vec<u8> = [C, A, B]
            .iter()
            .flat_map(|u| pack_uuid_bytes(u).unwrap())
            .collect();
        assert_eq!(pi_of(&dir), expected);
        // The same order again writes nothing.
        let out = SystemDevicePackReorderer
            .reorder_packs(dir.path(), &[C.into(), A.into(), B.into()])
            .expect("no-op");
        assert_eq!(out, ReorderOutcome::Unchanged);
        assert_eq!(pi_of(&dir), expected);
    }

    #[test]
    fn a_stale_order_is_refused_and_the_index_untouched() {
        let dir = setup_device(&[A, B, C]);
        let before = pi_of(&dir);
        // A pack the device no longer lists / a pack missing from the order.
        assert_eq!(
            SystemDevicePackReorderer.reorder_packs(dir.path(), &[A.into(), B.into()]),
            Err(ReorderFailure::Diverged)
        );
        assert_eq!(
            SystemDevicePackReorderer
                .reorder_packs(dir.path(), &[A.into(), B.into(), C.into(), A.into()]),
            Err(ReorderFailure::Diverged)
        );
        assert_eq!(pi_of(&dir), before);
    }

    #[test]
    fn a_malformed_uuid_or_a_corrupt_index_is_rejected_before_any_write() {
        let dir = setup_device(&[A, B]);
        let before = pi_of(&dir);
        assert_eq!(
            SystemDevicePackReorderer.reorder_packs(dir.path(), &["not-a-uuid".into(), B.into()]),
            Err(ReorderFailure::Rejected(
                TransferFailureCause::WriteRejected
            ))
        );
        assert_eq!(pi_of(&dir), before);
        // A trailing fragment makes the index corrupt: refused, never rewritten.
        let mut corrupt = before.clone();
        corrupt.extend_from_slice(&[0xAB]);
        fs::write(dir.path().join(LUNII_DEVICE_ID_MARKER), &corrupt).unwrap();
        assert_eq!(
            SystemDevicePackReorderer.reorder_packs(dir.path(), &[B.into(), A.into()]),
            Err(ReorderFailure::Rejected(
                TransferFailureCause::WriteRejected
            ))
        );
        assert_eq!(pi_of(&dir), corrupt);
    }
}
