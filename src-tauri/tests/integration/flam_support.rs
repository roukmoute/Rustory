//! Shared FLAM mount fixture for the integration crate.
//!
//! The `#[cfg(test)]` fixtures module of the lib crate is not visible
//! here (the same structural reason `device_scan.rs` mirrors the Lunii
//! marker writes), so the FLAM mount construction lives ONCE in this
//! harness module and the per-flow test files reuse it.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// Write a small OPAQUE story payload (arbitrary regular files, one
/// nested) — exactly what the structural-only validation accepts.
/// 3 files, 256 + 64 + 128 = 448 bytes (the counts the import tests
/// assert against the recorded provenance).
pub fn write_story_payload(dir: &Path) {
    std::fs::create_dir_all(dir).expect("mk story dir");
    std::fs::write(dir.join("00000001"), vec![0x11; 256]).expect("payload 1");
    std::fs::write(dir.join("index.bin"), vec![0x22; 64]).expect("payload 2");
    let nested = dir.join("data");
    std::fs::create_dir_all(&nested).expect("nested dir");
    std::fs::write(nested.join("chunk"), vec![0x33; 128]).expect("payload 3");
}

/// Conforming FLAM mount: `.mdf` + real `str/` + `etc/`, plus the text
/// indexes/folders described per entry `(uuid, hidden, present)` —
/// hidden entries list in `etc/library/list.hidden` and materialize
/// under `str.hidden/`, visible ones in `etc/library/list` under
/// `str/`. Returns `(guard, mount, device_identifier)`.
pub fn temp_flam_mount_with_entries(entries: &[(&str, bool, bool)]) -> (TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    let mdf_payload = b"FIXTURE_MDF_PAYLOAD";
    std::fs::write(root.join(".mdf"), mdf_payload).expect(".mdf");
    std::fs::create_dir(root.join("str")).expect("str");
    std::fs::create_dir(root.join("etc")).expect("etc");
    std::fs::create_dir_all(root.join("etc").join("library")).expect("etc/library");

    let mut visible = String::new();
    let mut hidden = String::new();
    for (uuid, is_hidden, present) in entries {
        let story_root = if *is_hidden {
            hidden.push_str(uuid);
            hidden.push('\n');
            root.join("str.hidden").join(uuid)
        } else {
            visible.push_str(uuid);
            visible.push('\n');
            root.join("str").join(uuid)
        };
        if *present {
            write_story_payload(&story_root);
        }
    }
    std::fs::write(root.join("etc/library/list"), visible).expect("list");
    if !hidden.is_empty() {
        std::fs::write(root.join("etc/library/list.hidden"), hidden).expect("list.hidden");
    }

    let identifier =
        rustory_lib::infrastructure::device::compute_device_identifier(mdf_payload, None);
    (dir, root, identifier)
}
