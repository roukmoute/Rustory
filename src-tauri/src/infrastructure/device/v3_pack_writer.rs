//! Writes an assembled V3 pack (from [`assemble_v3_pack`]) onto a device
//! volume — the LAST step of a V3 send.
//!
//! Same atomic discipline as the round-trip [`writer`](super::writer), and the
//! same shared per-mount lock (a send and a delete never race the `.pi`):
//!   1. stage every file under a temp dir ON the volume (so the promote
//!      `rename` is same-filesystem/atomic), creating `rf/000` / `sf/000`;
//!   2. fsync the staged tree;
//!   3. if `.content/<SHORTID>` already exists, `rename` it aside (FR23-style),
//!      then promote the staging by `rename`, fsync `.content`, remove the old;
//!   4. **FILES FIRST, INDEX SECOND** — only then append the pack UUID to `.pi`.
//!
//! An interruption leaves a swept-later staging/replaced residue and the `.pi`
//! unchanged (the pack simply isn't listed) — never a `.pi` entry pointing at
//! absent content.
//!
//! [`assemble_v3_pack`]: super::pack_assembly::assemble_v3_pack

use std::fs;
use std::path::Path;

use tempfile::Builder;

use crate::domain::device::{LUNII_CONTENT_DIR, LUNII_DEVICE_ID_MARKER};
use crate::domain::transfer::{
    append_pack_uuid, pack_uuid_bytes, short_id_from_pack_uuid, TransferFailureCause,
};

use super::pack_assembly::AssembledFile;
use super::writer::{
    fsync_dir, fsync_tree, mount_write_lock, promote, read_pi, safe_rel_join, write_pi_atomically,
    DEVICE_REPLACED_PREFIX, DEVICE_STAGING_PREFIX,
};

/// Writes an assembled pack to a device. A trait keeps the application layer
/// testable without a real volume.
pub trait DeviceV3PackWriter: Send + Sync + 'static {
    fn write_pack(
        &self,
        mount_path: &Path,
        pack_uuid: &str,
        files: &[AssembledFile],
    ) -> Result<(), TransferFailureCause>;
}

/// Production writer: stage on the volume + atomic promotion + fsync + `.pi`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDeviceV3PackWriter;

impl DeviceV3PackWriter for SystemDeviceV3PackWriter {
    fn write_pack(
        &self,
        mount_path: &Path,
        pack_uuid: &str,
        files: &[AssembledFile],
    ) -> Result<(), TransferFailureCause> {
        let uuid_bytes = pack_uuid_bytes(pack_uuid).ok_or(TransferFailureCause::WriteRejected)?;
        let short_id =
            short_id_from_pack_uuid(pack_uuid).ok_or(TransferFailureCause::WriteRejected)?;

        // Serialize against other writes/deletes on this mount (shared lock).
        let lock = mount_write_lock(mount_path);
        let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());

        // 1. Stage every file under a temp dir on the volume.
        let staging = Builder::new()
            .prefix(DEVICE_STAGING_PREFIX)
            .tempdir_in(mount_path)
            .map_err(|_| TransferFailureCause::WriteRejected)?;
        for file in files {
            let path = safe_rel_join(staging.path(), &file.rel_path)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|_| TransferFailureCause::WriteRejected)?;
            }
            fs::write(&path, &file.bytes).map_err(|_| TransferFailureCause::WriteRejected)?;
        }
        // 2. Durability of the staged tree before any promotion.
        fsync_tree(staging.path()).map_err(|_| TransferFailureCause::WriteRejected)?;

        let content_parent = mount_path.join(LUNII_CONTENT_DIR);
        fs::create_dir_all(&content_parent).map_err(|_| TransferFailureCause::WriteRejected)?;
        let target = content_parent.join(&short_id);

        // 3. If a pack already occupies the SHORT_ID, set it aside so the
        //    promote `rename` lands on a free name; remove it only AFTER the new
        //    content is promoted and durable.
        let set_aside = match fs::symlink_metadata(&target) {
            Ok(_) => {
                let aside = content_parent.join(format!("{DEVICE_REPLACED_PREFIX}{short_id}"));
                let _ = fs::remove_dir_all(&aside); // clear a prior residue
                fs::rename(&target, &aside).map_err(|_| TransferFailureCause::WriteRejected)?;
                Some(aside)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(TransferFailureCause::WriteRejected),
        };

        // Persist the staging dir (cancel TempDir's drop-cleanup) so the rename
        // owns it; a failure past here leaves a swept-later residue.
        let staging_path = staging.keep();
        if let Err(err) = promote(&staging_path, &target) {
            let _ = fs::remove_dir_all(&staging_path);
            return Err(err);
        }
        let _ = fsync_dir(&content_parent);
        if let Some(aside) = set_aside {
            let _ = fs::remove_dir_all(&aside);
        }

        // 4. FILES FIRST, INDEX SECOND: append the UUID to `.pi` (idempotent).
        let pi_path = mount_path.join(LUNII_DEVICE_ID_MARKER);
        let current = read_pi(&pi_path)?;
        let updated = append_pack_uuid(&current, &uuid_bytes);
        if updated != current {
            write_pi_atomically(mount_path, &pi_path, &updated)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::parse_pack_index;
    use tempfile::tempdir;

    const UUID: &str = "93ff05bf-2c00-4bca-bef0-2b779838693b";
    const SHORT: &str = "9838693B";

    fn files() -> Vec<AssembledFile> {
        vec![
            AssembledFile {
                rel_path: "ni".into(),
                bytes: vec![1, 2, 3],
            },
            AssembledFile {
                rel_path: "rf/000/AAAA1234".into(),
                bytes: vec![9; 10],
            },
        ]
    }

    #[test]
    fn writes_the_files_and_indexes_the_uuid() {
        let dir = tempdir().unwrap();
        // A pre-existing (empty) .pi so read_pi succeeds.
        fs::write(dir.path().join(LUNII_DEVICE_ID_MARKER), []).unwrap();

        SystemDeviceV3PackWriter
            .write_pack(dir.path(), UUID, &files())
            .expect("write");

        let content = dir.path().join(LUNII_CONTENT_DIR).join(SHORT);
        assert_eq!(fs::read(content.join("ni")).unwrap(), vec![1, 2, 3]);
        assert_eq!(
            fs::read(content.join("rf/000/AAAA1234")).unwrap(),
            vec![9; 10]
        );
        // The UUID is now listed in `.pi`.
        let pi = fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap();
        let listed = parse_pack_index(&pi)
            .uuids
            .iter()
            .any(|u| u == &pack_uuid_bytes(UUID).unwrap());
        assert!(listed, "the written pack is indexed in .pi");
    }

    /// Full pipeline ground truth (write to a SCRATCH copy, never the real
    /// device): transcode + assemble a real pack, WRITE it to the scratch mount,
    /// then assert every file under the reference device pack now exists on the
    /// scratch mount byte-for-byte, and the `.pi` lists the UUID. Env:
    /// RUSTORY_TEST_STORYJSON, RUSTORY_TEST_ASSETS, RUSTORY_TEST_MD,
    /// RUSTORY_TEST_WRITE_MOUNT (scratch), RUSTORY_TEST_CONTENT_REF (real
    /// device `.content/<SHORTID>`), RUSTORY_TEST_UUID.
    #[test]
    #[ignore = "manual: writes to a scratch device copy; needs the env set"]
    fn writes_a_real_pack_to_a_scratch_device_matching_the_reference() {
        use crate::domain::device::{transcode_pack, StudioStoryPack};
        use crate::infrastructure::device::assemble_v3_pack;
        use std::path::PathBuf;

        let json =
            std::fs::read_to_string(std::env::var("RUSTORY_TEST_STORYJSON").unwrap()).unwrap();
        let pack: StudioStoryPack = serde_json::from_str(&json).unwrap();
        let transcoded = transcode_pack(&pack).unwrap();
        let md = std::fs::read(std::env::var("RUSTORY_TEST_MD").unwrap()).unwrap();
        let assets = PathBuf::from(std::env::var("RUSTORY_TEST_ASSETS").unwrap());
        let files = assemble_v3_pack(&transcoded, &md, &|f| std::fs::read(assets.join(f)).ok())
            .expect("assemble");

        let mount = PathBuf::from(std::env::var("RUSTORY_TEST_WRITE_MOUNT").unwrap());
        let uuid = std::env::var("RUSTORY_TEST_UUID").unwrap();
        SystemDeviceV3PackWriter
            .write_pack(&mount, &uuid, &files)
            .expect("write to scratch");

        let short = short_id_from_pack_uuid(&uuid).unwrap();
        let written = mount.join(LUNII_CONTENT_DIR).join(&short);
        let reference = PathBuf::from(std::env::var("RUSTORY_TEST_CONTENT_REF").unwrap());
        let mut checked = 0usize;
        fn walk(dir: &Path, base: &Path, written: &Path, checked: &mut usize) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, base, written, checked);
                } else {
                    let rel = p.strip_prefix(base).unwrap();
                    let mine = std::fs::read(written.join(rel))
                        .unwrap_or_else(|_| panic!("scratch missing {rel:?}"));
                    let refb = std::fs::read(&p).unwrap();
                    assert_eq!(mine, refb, "mismatch on {rel:?}");
                    *checked += 1;
                }
            }
        }
        walk(&reference, &reference, &written, &mut checked);
        // The written pack is indexed.
        let pi = std::fs::read(mount.join(LUNII_DEVICE_ID_MARKER)).unwrap();
        assert!(parse_pack_index(&pi)
            .uuids
            .iter()
            .any(|u| u == &pack_uuid_bytes(&uuid).unwrap()));
        eprintln!("[v3write-smoke] {checked} files written to scratch match the device ✓");
    }

    #[test]
    fn replacing_an_existing_pack_swaps_the_content_and_keeps_one_index_entry() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(LUNII_DEVICE_ID_MARKER), []).unwrap();
        // First write.
        SystemDeviceV3PackWriter
            .write_pack(dir.path(), UUID, &files())
            .expect("first write");
        // Second write with DIFFERENT content replaces it.
        let new_files = vec![AssembledFile {
            rel_path: "ni".into(),
            bytes: vec![7, 7, 7, 7],
        }];
        SystemDeviceV3PackWriter
            .write_pack(dir.path(), UUID, &new_files)
            .expect("replace");

        let content = dir.path().join(LUNII_CONTENT_DIR).join(SHORT);
        assert_eq!(fs::read(content.join("ni")).unwrap(), vec![7, 7, 7, 7]);
        // Old-only file is gone (full replacement, not a merge).
        assert!(!content.join("rf/000/AAAA1234").exists());
        // Still exactly one .pi entry (append is idempotent).
        let pi = fs::read(dir.path().join(LUNII_DEVICE_ID_MARKER)).unwrap();
        assert_eq!(pi.len(), 16, "one 16-byte uuid, not duplicated");
        // No set-aside residue left behind.
        let residue: Vec<_> = fs::read_dir(dir.path().join(LUNII_CONTENT_DIR))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(DEVICE_REPLACED_PREFIX)
            })
            .collect();
        assert!(residue.is_empty(), "the set-aside old pack was removed");
    }
}
