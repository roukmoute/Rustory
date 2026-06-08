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
    pack_short_id, LUNII_BINARY_TOKEN_MARKER, LUNII_CONTENT_DIR, LUNII_DEVICE_ID_MARKER,
    LUNII_HIDDEN_INDEX_MARKER, LUNII_PRIMARY_MARKER,
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

/// Build a Lunii mount whose installed-pack inventory is fully described:
/// `.pi` lists `visible` UUIDs (in order); `.pi.hidden` lists `hidden`
/// UUIDs (omitted entirely when empty); a `.content/<SHORT_ID>` directory
/// is created for each `visible` pack flagged `true` and for every
/// `hidden` pack. A `visible` pack flagged `false` is an orphan
/// (referenced in `.pi`, no payload folder).
///
/// Empty `visible` + empty `hidden` produces a valid empty library (an
/// empty `.pi`, no `.content`).
pub fn temp_lunii_mount_with_library(
    metadata_version: u8,
    visible: &[([u8; 16], bool)],
    hidden: &[[u8; 16]],
) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let root = dir.path().to_path_buf();

    fs::write(
        root.join(LUNII_PRIMARY_MARKER),
        [metadata_version, 0xff, 0xaa],
    )
    .expect("write .md");

    let mut pi_payload = Vec::with_capacity(visible.len() * 16);
    for (uuid, _present) in visible {
        pi_payload.extend_from_slice(uuid);
    }
    fs::write(root.join(LUNII_DEVICE_ID_MARKER), &pi_payload).expect("write .pi");

    if !hidden.is_empty() {
        let mut hidden_payload = Vec::with_capacity(hidden.len() * 16);
        for uuid in hidden {
            hidden_payload.extend_from_slice(uuid);
        }
        fs::write(root.join(LUNII_HIDDEN_INDEX_MARKER), &hidden_payload).expect("write .pi.hidden");
    }

    let content = root.join(LUNII_CONTENT_DIR);
    for (uuid, present) in visible {
        if *present {
            fs::create_dir_all(content.join(pack_short_id(uuid))).expect("mkdir content folder");
        }
    }
    for uuid in hidden {
        fs::create_dir_all(content.join(pack_short_id(uuid))).expect("mkdir hidden content folder");
    }

    (dir, root)
}
