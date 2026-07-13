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
    pack_short_id, FLAM_CONFIG_DIR, FLAM_HIDDEN_LIBRARY_INDEX_REL, FLAM_HIDDEN_STORY_DIR,
    FLAM_LIBRARY_INDEX_REL, FLAM_PRIMARY_MARKER, FLAM_STORY_DIR, LUNII_BINARY_TOKEN_MARKER,
    LUNII_CONTENT_DIR, LUNII_DEVICE_ID_MARKER, LUNII_HIDDEN_INDEX_MARKER, LUNII_PRIMARY_MARKER,
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

/// Build a TempDir whose root carries the marker set of a conforming
/// FLAM: a non-empty `.mdf` + the REAL directories `str/` and `etc/`.
/// Returns `(TempDir guard, mount path)`.
pub fn temp_flam_mount() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let root = dir.path().to_path_buf();
    fs::write(root.join(FLAM_PRIMARY_MARKER), b"FIXTURE_MDF_PAYLOAD").expect("write .mdf");
    fs::create_dir(root.join(FLAM_STORY_DIR)).expect("mkdir str");
    fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("mkdir etc");
    (dir, root)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlamCorruptKind {
    /// `.mdf` present but EMPTY — a VISIBLE candidate that classifies
    /// `metadataCorrupt`.
    EmptyMdf,
    /// `.mdf` beyond the 4 KiB bound — not a plausible FLAM, the volume
    /// is ignored by the probe.
    OversizeMdf,
    /// `str/` missing — classifies `metadataUnsupported`.
    MissingStrDir,
    /// `etc/` missing — classifies `metadataUnsupported`.
    MissingEtcDir,
}

/// Same as [`temp_flam_mount`] but injects a controlled corruption.
pub fn temp_flam_mount_corrupt(kind: FlamCorruptKind) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let root = dir.path().to_path_buf();
    let mdf_payload: Vec<u8> = match kind {
        FlamCorruptKind::EmptyMdf => Vec::new(),
        FlamCorruptKind::OversizeMdf => vec![0x4D; 8 * 1024],
        _ => b"FIXTURE_MDF".to_vec(),
    };
    fs::write(root.join(FLAM_PRIMARY_MARKER), &mdf_payload).expect("write .mdf");
    if kind != FlamCorruptKind::MissingStrDir {
        fs::create_dir(root.join(FLAM_STORY_DIR)).expect("mkdir str");
    }
    if kind != FlamCorruptKind::MissingEtcDir {
        fs::create_dir(root.join(FLAM_CONFIG_DIR)).expect("mkdir etc");
    }
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

/// One FLAM library entry for [`temp_flam_mount_with_library`]: the
/// canonical lowercase UUID line, whether the index lists it hidden, and
/// whether its story folder (`str/<uuid>/` or `str.hidden/<uuid>/`) is
/// materialized with a small opaque payload.
#[derive(Debug, Clone)]
pub struct FlamLibraryEntry {
    pub uuid: String,
    pub hidden: bool,
    pub content_present: bool,
}

impl FlamLibraryEntry {
    pub fn visible(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            hidden: false,
            content_present: true,
        }
    }

    pub fn hidden(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            hidden: true,
            content_present: true,
        }
    }

    pub fn orphan(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
            hidden: false,
            content_present: false,
        }
    }
}

/// Fill a FLAM story directory with a small OPAQUE payload (the format
/// is publicly unknown — the fixture writes arbitrary regular files, one
/// of them inside a subdirectory, exactly what the structural-only
/// validation accepts).
pub fn write_opaque_flam_story(story_dir: &std::path::Path) {
    fs::create_dir_all(story_dir).expect("mkdir flam story dir");
    fs::write(story_dir.join("00000001"), vec![0x11; 256]).expect("write flam payload 1");
    fs::write(story_dir.join("index.bin"), vec![0x22; 64]).expect("write flam payload 2");
    let nested = story_dir.join("data");
    fs::create_dir_all(&nested).expect("mkdir flam nested dir");
    fs::write(nested.join("chunk"), vec![0x33; 128]).expect("write flam payload 3");
}

/// Build a conforming FLAM mount whose library inventory is fully
/// described: `etc/library/list` lists the non-hidden entries (in
/// order), `etc/library/list.hidden` the hidden ones (file omitted when
/// none), and each `content_present` entry gets its real story folder
/// under `str/` (visible) or `str.hidden/` (hidden) with an opaque
/// payload. Returns `(TempDir guard, mount path)`.
pub fn temp_flam_mount_with_library(entries: &[FlamLibraryEntry]) -> (tempfile::TempDir, PathBuf) {
    let (dir, root) = temp_flam_mount();

    let index_path = root.join(FLAM_LIBRARY_INDEX_REL);
    fs::create_dir_all(index_path.parent().expect("index parent")).expect("mkdir etc/library");
    let mut visible_lines = String::new();
    let mut hidden_lines = String::new();
    for entry in entries {
        let target = if entry.hidden {
            hidden_lines.push_str(&entry.uuid);
            hidden_lines.push('\n');
            root.join(FLAM_HIDDEN_STORY_DIR).join(&entry.uuid)
        } else {
            visible_lines.push_str(&entry.uuid);
            visible_lines.push('\n');
            root.join(FLAM_STORY_DIR).join(&entry.uuid)
        };
        if entry.content_present {
            write_opaque_flam_story(&target);
        }
    }
    fs::write(&index_path, visible_lines).expect("write etc/library/list");
    if !hidden_lines.is_empty() {
        fs::write(root.join(FLAM_HIDDEN_LIBRARY_INDEX_REL), hidden_lines)
            .expect("write etc/library/list.hidden");
    }

    (dir, root)
}

/// Fill `pack_dir` with a complete, plausible pack honoring the declared
/// supported subset: non-empty required files (`ni/li/ri/si`), optionals
/// (`nm`, `bt`) and one asset per tree (`rf/000/…`, `sf/000/…`).
pub fn write_plausible_pack(pack_dir: &std::path::Path) {
    fs::create_dir_all(pack_dir).expect("mkdir pack dir");
    fs::write(pack_dir.join("ni"), vec![0x4E; 512]).expect("write ni");
    fs::write(pack_dir.join("li"), vec![0x4C; 256]).expect("write li");
    fs::write(pack_dir.join("ri"), vec![0x52; 128]).expect("write ri");
    fs::write(pack_dir.join("si"), vec![0x53; 128]).expect("write si");
    fs::write(pack_dir.join("nm"), vec![0x6E; 32]).expect("write nm");
    fs::write(pack_dir.join("bt"), vec![0x62; 64]).expect("write bt");
    let rf = pack_dir.join("rf").join("000");
    fs::create_dir_all(&rf).expect("mkdir rf/000");
    fs::write(rf.join("AAAAAAAA"), vec![0xAA; 2048]).expect("write rf asset");
    let sf = pack_dir.join("sf").join("000");
    fs::create_dir_all(&sf).expect("mkdir sf/000");
    fs::write(sf.join("BBBBBBBB"), vec![0xBB; 4096]).expect("write sf asset");
}

/// Build a Lunii mount whose `.pi` lists exactly `pack_uuid` and whose
/// `.content/<SHORT_ID>` carries a complete plausible pack (see
/// [`write_plausible_pack`]). Returns `(TempDir guard, mount path)`.
pub fn temp_lunii_mount_with_pack_content(
    metadata_version: u8,
    pack_uuid: [u8; 16],
) -> (tempfile::TempDir, PathBuf) {
    let (dir, root) = temp_lunii_mount_with_library(metadata_version, &[(pack_uuid, true)], &[]);
    let pack_dir = root.join(LUNII_CONTENT_DIR).join(pack_short_id(&pack_uuid));
    write_plausible_pack(&pack_dir);
    (dir, root)
}
