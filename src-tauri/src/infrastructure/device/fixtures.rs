//! Test-only fixtures that build a TempDir whose root carries the marker
//! files of a Lunii. The PathBuf returned is the mount point a system
//! scanner would see if the user plugged in such a device.
//!
//! These helpers are exposed under `#[cfg(test)]` only to keep the
//! production binary lean; integration tests in `src-tauri/tests/`
//! reach them via the `infrastructure::device::fixtures` path.

#![cfg(test)]

use std::fs;
use std::path::PathBuf;

use crate::domain::device::{
    LUNII_BINARY_TOKEN_MARKER, LUNII_DEVICE_ID_MARKER, LUNII_PRIMARY_MARKER,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorruptKind {
    MissingPi,
    MissingBt,
    EmptyMd,
    OversizeMd,
}

/// Build a TempDir whose root carries the marker files of a Lunii with
/// the requested metadata version. Returns `(TempDir guard, mount path)`.
pub fn temp_lunii_mount(metadata_version: u8) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let root = dir.path().to_path_buf();
    fs::write(
        root.join(LUNII_PRIMARY_MARKER),
        [metadata_version, 0xff, 0xaa],
    )
    .expect("write .md");
    fs::write(root.join(LUNII_DEVICE_ID_MARKER), b"FIXTURE_PI_PAYLOAD").expect("write .pi");
    fs::write(root.join(LUNII_BINARY_TOKEN_MARKER), b"FIXTURE_BT").expect("write .bt");
    (dir, root)
}

/// Same as [`temp_lunii_mount`] but injects a controlled corruption.
pub fn temp_lunii_mount_corrupt(kind: CorruptKind) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let root = dir.path().to_path_buf();
    let md_payload: Vec<u8> = match kind {
        CorruptKind::EmptyMd => Vec::new(),
        CorruptKind::OversizeMd => vec![3u8; 8 * 1024],
        _ => vec![3, 0xff],
    };
    fs::write(root.join(LUNII_PRIMARY_MARKER), &md_payload).expect("write .md");
    if kind != CorruptKind::MissingPi {
        fs::write(root.join(LUNII_DEVICE_ID_MARKER), b"FIXTURE_PI").expect("write .pi");
    }
    if kind != CorruptKind::MissingBt {
        fs::write(root.join(LUNII_BINARY_TOKEN_MARKER), b"FIXTURE_BT").expect("write .bt");
    }
    (dir, root)
}
