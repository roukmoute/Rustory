//! End-to-end integration of the device-story import: REAL system
//! scanner + REAL library reader + REAL pack reader exercised against a
//! temp mount and an on-disk SQLite database. Proves the whole seam —
//! re-scan → identity → gate → index re-read → bounded copy → atomic
//! promotion → canonical commit — on actual file I/O.
//!
//! The `#[cfg(test)]` fixtures module is not visible to this separate
//! test crate, so the mount is built inline (same pattern as
//! `device_library.rs`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rustory_lib::application::device::import::{
    import_device_story, sweep_import_artifacts, ImportDeviceStoryRequest,
};
use rustory_lib::application::story::get_story_detail;
use rustory_lib::domain::device::{format_pack_uuid, pack_short_id};
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::device::{
    compute_device_identifier, SystemDeviceLibraryReader, SystemDevicePackReader,
    SystemDeviceScanner,
};
use tempfile::TempDir;

fn uuid(tail: [u8; 4]) -> [u8; 16] {
    let mut b = [0xAB; 16];
    b[12..16].copy_from_slice(&tail);
    b
}

/// Write a complete plausible pack (declared subset) into `pack_dir`.
fn write_pack(pack_dir: &Path) {
    std::fs::create_dir_all(pack_dir).expect("mkdir pack");
    std::fs::write(pack_dir.join("ni"), vec![0x4E; 512]).expect("ni");
    std::fs::write(pack_dir.join("li"), vec![0x4C; 256]).expect("li");
    std::fs::write(pack_dir.join("ri"), vec![0x52; 128]).expect("ri");
    std::fs::write(pack_dir.join("si"), vec![0x53; 128]).expect("si");
    std::fs::write(pack_dir.join("nm"), vec![0x6E; 32]).expect("nm");
    let rf = pack_dir.join("rf").join("000");
    std::fs::create_dir_all(&rf).expect("rf/000");
    std::fs::write(rf.join("AAAAAAAA"), vec![0xAA; 2048]).expect("rf asset");
    let sf = pack_dir.join("sf").join("000");
    std::fs::create_dir_all(&sf).expect("sf/000");
    std::fs::write(sf.join("BBBBBBBB"), vec![0xBB; 4096]).expect("sf asset");
}

/// Build a temp Lunii mount with one pack: markers + `.pi` listing the
/// uuid + a full `.content/<SHORT_ID>` payload. Returns
/// `(guard, mount, device_identifier, pack_uuid_string, short_id)`.
fn build_mount_with_pack(
    metadata_version: u8,
    pack_uuid: [u8; 16],
    hidden: bool,
) -> (TempDir, PathBuf, String, String, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    std::fs::write(root.join(".md"), [metadata_version, 0xff, 0xaa]).expect(".md");
    let (visible_pi, hidden_pi): (Vec<u8>, Vec<u8>) = if hidden {
        // `.pi` must stay non-empty for detection; list a placeholder
        // visible pack and put the target in `.pi.hidden`.
        let placeholder = uuid([0xEE, 0xEE, 0xEE, 0xEE]);
        (placeholder.to_vec(), pack_uuid.to_vec())
    } else {
        (pack_uuid.to_vec(), Vec::new())
    };
    std::fs::write(root.join(".pi"), &visible_pi).expect(".pi");
    if !hidden_pi.is_empty() {
        std::fs::write(root.join(".pi.hidden"), &hidden_pi).expect(".pi.hidden");
    }
    let short_id = pack_short_id(&pack_uuid);
    write_pack(&root.join(".content").join(&short_id));
    let identifier = compute_device_identifier(&visible_pi, None);
    (
        dir,
        root,
        identifier,
        format_pack_uuid(&pack_uuid),
        short_id,
    )
}

fn fresh_disk_db(tmp: &TempDir) -> Mutex<DbHandle> {
    let path = tmp.path().join("rustory.sqlite");
    let mut handle = db::open_at(&path).expect("open");
    db::run_migrations(&mut handle).expect("migrate");
    Mutex::new(handle)
}

fn budget() -> Duration {
    Duration::from_secs(30)
}

struct RealHarness {
    db: Mutex<DbHandle>,
    app_data: TempDir,
    db_dir: TempDir,
}

impl RealHarness {
    fn new() -> Self {
        let db_dir = TempDir::new().expect("db dir");
        Self {
            db: fresh_disk_db(&db_dir),
            app_data: TempDir::new().expect("app data"),
            db_dir,
        }
    }

    fn run(
        &self,
        mount_root: PathBuf,
        identifier: &str,
        pack_uuid: &str,
    ) -> Result<
        rustory_lib::application::device::import::ImportedDeviceStory,
        rustory_lib::domain::shared::AppError,
    > {
        let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![mount_root]);
        import_device_story(
            &self.db,
            &scanner,
            &SystemDeviceLibraryReader,
            &SystemDevicePackReader,
            self.app_data.path(),
            &ImportDeviceStoryRequest {
                device_identifier: identifier.into(),
                pack_uuid: pack_uuid.into(),
            },
            budget(),
        )
    }

    fn imports_dir(&self) -> PathBuf {
        self.app_data.path().join("imports")
    }

    fn story_rows(&self) -> u32 {
        let db = self.db.lock().expect("lock");
        db.conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count")
    }

    fn import_rows(&self) -> u32 {
        let db = self.db.lock().expect("lock");
        db.conn()
            .query_row("SELECT COUNT(*) FROM story_imports", [], |row| row.get(0))
            .expect("count")
    }

    fn non_staging_import_dirs(&self) -> Vec<String> {
        if !self.imports_dir().is_dir() {
            return Vec::new();
        }
        std::fs::read_dir(self.imports_dir())
            .expect("read imports")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != ".staging")
            .collect()
    }
}

/// Recursive `(rel_path, bytes)` snapshot for read-only proofs.
fn snapshot_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .expect("under base")
                    .to_string_lossy()
                    .into_owned();
                out.push((rel, std::fs::read(&path).expect("bytes")));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn imports_end_to_end_and_the_story_survives_without_the_device() {
    let pack = uuid([0xFA, 0xC5, 0x56, 0x2D]);
    let (guard, mount, identifier, pack_uuid, short_id) = build_mount_with_pack(3, pack, false);
    let h = RealHarness::new();

    let outcome = h
        .run(mount.clone(), &identifier, &pack_uuid)
        .expect("import");

    // Canonical stories row, strictly conforming to the create_story model.
    {
        let db = h.db.lock().expect("lock");
        let detail = get_story_detail(&db, &std::env::temp_dir(), &outcome.story.id, None)
            .expect("detail read")
            .expect("row present");
        assert_eq!(detail.title, format!("Histoire de ma Lunii ({short_id})"));
        assert_eq!(detail.schema_version, 3);
        assert_eq!(detail.structure_json, "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}");
        assert_eq!(detail.content_checksum.len(), 64);
        assert_eq!(detail.created_at, detail.updated_at);

        // Provenance row: the exact link 2.6 Phase D will consume.
        let (db_pack_uuid, file_count, total_bytes, checksum): (String, u32, u64, String) = db
            .conn()
            .query_row(
                "SELECT pack_uuid, pack_file_count, pack_total_bytes, pack_checksum \
                 FROM story_imports WHERE story_id = ?1",
                rusqlite::params![&outcome.story.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("provenance row");
        assert_eq!(db_pack_uuid, pack_uuid);
        assert_eq!(file_count, 7); // ni/li/ri/si/nm + rf + sf assets (no bt in this fixture)
        assert_eq!(total_bytes, 512 + 256 + 128 + 128 + 32 + 2048 + 4096);
        assert_eq!(checksum.len(), 64);
    }

    // Promoted files are byte-identical to the source pack.
    let promoted = h.imports_dir().join(&outcome.story.id);
    let source_tree = snapshot_tree(&mount.join(".content").join(&short_id));
    let promoted_tree = snapshot_tree(&promoted);
    assert_eq!(
        source_tree, promoted_tree,
        "copied bytes must match the device pack"
    );

    // AC1: drop the device entirely — the story re-opens without it.
    drop(guard);
    assert!(!mount.exists(), "fixture mount must be gone");
    {
        let db = h.db.lock().expect("lock");
        let detail = get_story_detail(&db, &std::env::temp_dir(), &outcome.story.id, None)
            .expect("re-read without device")
            .expect("still present");
        assert_eq!(detail.title, outcome.story.title);
    }
    assert!(
        promoted.join("ni").is_file(),
        "imported payload must survive the unplug"
    );
}

#[test]
fn the_source_mount_is_strictly_read_only_across_an_import() {
    let pack = uuid([0x11, 0x22, 0x33, 0x44]);
    let (_guard, mount, identifier, pack_uuid, _short_id) = build_mount_with_pack(6, pack, false);
    let before = snapshot_tree(&mount);

    let h = RealHarness::new();
    h.run(mount.clone(), &identifier, &pack_uuid)
        .expect("import");

    let after = snapshot_tree(&mount);
    assert_eq!(
        before, after,
        "the device volume must be byte-identical after an import (read-only end to end)"
    );
}

#[test]
fn a_hidden_pack_is_importable() {
    let pack = uuid([0x99, 0x88, 0x77, 0x66]);
    let (_guard, mount, identifier, pack_uuid, short_id) = build_mount_with_pack(3, pack, true);
    let h = RealHarness::new();
    let outcome = h
        .run(mount, &identifier, &pack_uuid)
        .expect("hidden pack import");
    assert_eq!(outcome.pack_short_id, short_id);
}

#[test]
fn missing_content_folder_refuses_with_pack_missing_and_creates_nothing() {
    let pack = uuid([0x01, 0x02, 0x03, 0x04]);
    let (_guard, mount, identifier, pack_uuid, short_id) = build_mount_with_pack(3, pack, false);
    std::fs::remove_dir_all(mount.join(".content").join(&short_id)).expect("drop content");

    let h = RealHarness::new();
    let err = h
        .run(mount, &identifier, &pack_uuid)
        .expect_err("absent content must fail");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "IMPORT_FAILED");
    assert_eq!(v["details"]["source"], "pack_missing");
    assert_eq!(h.story_rows(), 0);
    assert_eq!(h.import_rows(), 0);
    assert!(h.non_staging_import_dirs().is_empty());
}

#[test]
fn unknown_pack_uuid_refuses_with_pack_missing() {
    let pack = uuid([0x0F, 0x0E, 0x0D, 0x0C]);
    let (_guard, mount, identifier, _pack_uuid, _short_id) = build_mount_with_pack(3, pack, false);
    let other_uuid = format_pack_uuid(&uuid([0xDE, 0xAD, 0xBE, 0xEF]));

    let h = RealHarness::new();
    let err = h
        .run(mount, &identifier, &other_uuid)
        .expect_err("unlisted pack must fail");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["source"], "pack_missing");
}

#[test]
fn identifier_mismatch_refuses_with_device_changed() {
    let pack = uuid([0x21, 0x22, 0x23, 0x24]);
    let (_guard, mount, _identifier, pack_uuid, _short_id) = build_mount_with_pack(3, pack, false);

    let h = RealHarness::new();
    let err = h
        .run(mount, "00000000000000000000000000000000", &pack_uuid)
        .expect_err("identity mismatch must fail");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "IMPORT_FAILED");
    assert_eq!(v["details"]["source"], "device_changed");
}

#[test]
fn v3_profile_is_gated_with_device_unsupported_capability_gate() {
    let pack = uuid([0x31, 0x32, 0x33, 0x34]);
    // metadata v7 ⇒ V3 cohort ⇒ import_story = false.
    let (_guard, mount, identifier, pack_uuid, _short_id) = build_mount_with_pack(7, pack, false);

    let h = RealHarness::new();
    let err = h
        .run(mount, &identifier, &pack_uuid)
        .expect_err("V3 must be gated");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["code"], "DEVICE_UNSUPPORTED");
    assert_eq!(v["details"]["source"], "capability_gate");
    assert_eq!(v["details"]["operation"], "import_story");
    assert_eq!(h.story_rows(), 0);
}

#[test]
fn re_import_is_blocked_and_first_artifacts_stay_intact() {
    let pack = uuid([0x41, 0x42, 0x43, 0x44]);
    let (_guard, mount, identifier, pack_uuid, _short_id) = build_mount_with_pack(3, pack, false);

    let h = RealHarness::new();
    let first = h
        .run(mount.clone(), &identifier, &pack_uuid)
        .expect("first import");
    let err = h
        .run(mount, &identifier, &pack_uuid)
        .expect_err("second import must be blocked");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["source"], "already_imported");

    assert_eq!(h.story_rows(), 1, "no second stories row");
    assert_eq!(h.import_rows(), 1, "no second provenance row");
    assert!(
        h.imports_dir().join(&first.story.id).join("ni").is_file(),
        "first import's files stay intact"
    );
    assert_eq!(h.non_staging_import_dirs().len(), 1);
}

#[test]
fn invalid_pack_content_refuses_all_or_nothing_with_no_residue() {
    let pack = uuid([0x51, 0x52, 0x53, 0x54]);
    let (_guard, mount, identifier, pack_uuid, short_id) = build_mount_with_pack(3, pack, false);
    // An unknown root entry violates the declared subset.
    std::fs::write(
        mount.join(".content").join(&short_id).join("payload.zip"),
        b"zip",
    )
    .expect("seed unknown entry");

    let h = RealHarness::new();
    let err = h
        .run(mount, &identifier, &pack_uuid)
        .expect_err("invalid pack must fail");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["source"], "pack_invalid");

    assert_eq!(h.story_rows(), 0, "no DB row may exist (AC3)");
    assert_eq!(h.import_rows(), 0);
    assert!(h.non_staging_import_dirs().is_empty(), "no orphan folder");
    // The staging area is empty (TempDir dropped on the error path).
    let staging = h.imports_dir().join(".staging");
    if staging.is_dir() {
        assert!(
            std::fs::read_dir(&staging)
                .expect("read staging")
                .next()
                .is_none(),
            "staging must be clean after a refusal"
        );
    }
}

// ---------------- FLAM import through the shared bridge ----------------

mod flam_mount {
    //! Thin per-file alias over the SHARED harness fixture
    //! (`crate::flam_support`) — one FLAM mount construction for the
    //! whole integration crate.
    use std::path::PathBuf;
    use tempfile::TempDir;

    pub const FLAM_UUID: &str = "12345678-9abc-def0-1122-334455667788";
    pub const FLAM_SHORT_ID: &str = "55667788";

    /// Conforming FLAM mount holding one indexed story. Returns
    /// `(guard, mount, device_identifier)`.
    pub fn build(hidden: bool, present: bool) -> (TempDir, PathBuf, String) {
        crate::flam_support::temp_flam_mount_with_entries(&[(FLAM_UUID, hidden, present)])
    }
}

#[test]
fn imports_a_flam_story_end_to_end_with_family_correct_title_and_inherited_provenance() {
    // Signature path: the SHARED bridge imports a FLAM story — local
    // canonical story titled `Histoire de mon FLAM (…)`, `story_imports`
    // provenance carrying the FLAM story UUID verbatim,
    // promoted artifacts byte-identical to the opaque source.
    let (guard, mount, identifier) = flam_mount::build(false, true);
    let h = RealHarness::new();

    let outcome = h
        .run(mount.clone(), &identifier, flam_mount::FLAM_UUID)
        .expect("FLAM import");
    assert_eq!(
        outcome.story.title,
        format!("Histoire de mon FLAM ({})", flam_mount::FLAM_SHORT_ID)
    );
    assert_eq!(outcome.pack_short_id, flam_mount::FLAM_SHORT_ID);

    {
        let db = h.db.lock().expect("lock");
        let detail = get_story_detail(&db, &std::env::temp_dir(), &outcome.story.id, None)
            .expect("detail read")
            .expect("row present");
        assert_eq!(detail.schema_version, 3);
        assert_eq!(detail.created_at, detail.updated_at);

        let (db_pack_uuid, source_id, file_count, total_bytes, checksum): (
            String,
            String,
            u32,
            u64,
            String,
        ) = db
            .conn()
            .query_row(
                "SELECT pack_uuid, source_device_identifier, pack_file_count, pack_total_bytes, \
                 pack_checksum FROM story_imports WHERE story_id = ?1",
                rusqlite::params![&outcome.story.id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("provenance row");
        assert_eq!(db_pack_uuid, flam_mount::FLAM_UUID);
        assert_eq!(source_id, identifier);
        assert_eq!(file_count, 3);
        assert_eq!(total_bytes, 256 + 64 + 128);
        assert_eq!(checksum.len(), 64);
    }

    // Promoted files are byte-identical to the opaque source story.
    let promoted = h.imports_dir().join(&outcome.story.id);
    let source_tree = snapshot_tree(&mount.join("str").join(flam_mount::FLAM_UUID));
    let promoted_tree = snapshot_tree(&promoted);
    assert_eq!(source_tree, promoted_tree, "opaque bytes must match");

    // The story survives without the device (same AC1 proof as Lunii).
    drop(guard);
    {
        let db = h.db.lock().expect("lock");
        let detail = get_story_detail(&db, &std::env::temp_dir(), &outcome.story.id, None)
            .expect("re-read without device")
            .expect("still present");
        assert_eq!(detail.title, outcome.story.title);
    }
}

#[test]
fn re_importing_the_same_flam_story_refuses_with_the_inherited_already_imported() {
    let (_guard, mount, identifier) = flam_mount::build(false, true);
    let h = RealHarness::new();
    h.run(mount.clone(), &identifier, flam_mount::FLAM_UUID)
        .expect("first FLAM import");
    let err = h
        .run(mount, &identifier, flam_mount::FLAM_UUID)
        .expect_err("second import must be blocked");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["source"], "already_imported");
    assert_eq!(h.story_rows(), 1);
    assert_eq!(h.import_rows(), 1);
}

#[test]
fn a_hidden_flam_story_is_importable_from_the_hidden_root() {
    let (_guard, mount, identifier) = flam_mount::build(true, true);
    let h = RealHarness::new();
    let outcome = h
        .run(mount, &identifier, flam_mount::FLAM_UUID)
        .expect("hidden FLAM import");
    assert_eq!(
        outcome.story.title,
        format!("Histoire de mon FLAM ({})", flam_mount::FLAM_SHORT_ID)
    );
}

#[test]
fn a_hidden_flam_entry_imports_the_hidden_folder_never_a_visible_homonym() {
    // `list.hidden` references a UUID whose payload lives under
    // `str.hidden/<uuid>`, while a DIFFERENT orphan folder with the same
    // UUID sits under `str/<uuid>`. The import must take the hidden
    // folder — the selected index is authoritative, the other root is
    // never consulted.
    let (_guard, mount, identifier) = flam_mount::build(true, true);
    let decoy = mount.join("str").join(flam_mount::FLAM_UUID);
    std::fs::create_dir_all(&decoy).expect("mk decoy");
    std::fs::write(decoy.join("decoy.bin"), b"DECOY").expect("seed decoy");

    let h = RealHarness::new();
    let outcome = h
        .run(mount.clone(), &identifier, flam_mount::FLAM_UUID)
        .expect("hidden FLAM import");

    let promoted = h.imports_dir().join(&outcome.story.id);
    let hidden_tree = snapshot_tree(&mount.join("str.hidden").join(flam_mount::FLAM_UUID));
    let promoted_tree = snapshot_tree(&promoted);
    assert_eq!(
        hidden_tree, promoted_tree,
        "the promoted bytes must be the HIDDEN folder's, never the visible decoy's"
    );
    assert!(
        !promoted.join("decoy.bin").exists(),
        "the visible decoy must never be imported"
    );
}

#[test]
fn flam_index_entry_without_story_folder_refuses_with_pack_missing() {
    // The UI already refuses `contentPresent:false`; the forced import
    // hits the live re-check and refuses with `pack_missing`.
    let (_guard, mount, identifier) = flam_mount::build(false, false);
    let h = RealHarness::new();
    let err = h
        .run(mount, &identifier, flam_mount::FLAM_UUID)
        .expect_err("absent story folder must fail");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["source"], "pack_missing");
    assert_eq!(h.story_rows(), 0);
}

#[test]
fn an_empty_flam_story_folder_refuses_with_pack_invalid_and_no_residue() {
    let (_guard, mount, identifier) = flam_mount::build(false, false);
    std::fs::create_dir_all(mount.join("str").join(flam_mount::FLAM_UUID))
        .expect("mk empty story dir");
    let h = RealHarness::new();
    let err = h
        .run(mount, &identifier, flam_mount::FLAM_UUID)
        .expect_err("empty story must fail");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["source"], "pack_invalid");
    assert_eq!(v["details"]["cause"], "empty_pack");
    assert_eq!(h.story_rows(), 0);
    assert!(h.non_staging_import_dirs().is_empty());
}

#[cfg(unix)]
#[test]
fn a_symlink_inside_a_flam_story_refuses_all_or_nothing_with_no_residue() {
    let (_guard, mount, identifier) = flam_mount::build(false, true);
    let story_dir = mount.join("str").join(flam_mount::FLAM_UUID);
    std::os::unix::fs::symlink(story_dir.join("00000001"), story_dir.join("link")).expect("mklink");
    let h = RealHarness::new();
    let err = h
        .run(mount, &identifier, flam_mount::FLAM_UUID)
        .expect_err("symlink must refuse the whole pack");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["source"], "pack_invalid");
    assert_eq!(v["details"]["cause"], "not_a_regular_file");
    assert_eq!(h.story_rows(), 0, "no DB row may exist");
    assert!(h.non_staging_import_dirs().is_empty(), "no orphan folder");
}

#[test]
fn an_oversize_flam_story_refuses_with_pack_oversize() {
    // A sparse file beyond the pack byte bound: refused by the pure
    // inventory validation BEFORE any copy (fast, no 2 GiB read).
    let (_guard, mount, identifier) = flam_mount::build(false, false);
    let story_dir = mount.join("str").join(flam_mount::FLAM_UUID);
    std::fs::create_dir_all(&story_dir).expect("mk story dir");
    let big = std::fs::File::create(story_dir.join("huge")).expect("create sparse");
    big.set_len(rustory_lib::domain::device::MAX_IMPORT_PACK_BYTES + 1)
        .expect("set_len");

    let h = RealHarness::new();
    let err = h
        .run(mount, &identifier, flam_mount::FLAM_UUID)
        .expect_err("oversize story must fail");
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["source"], "pack_oversize");
    assert_eq!(h.story_rows(), 0);
}

#[test]
fn the_flam_mount_is_strictly_read_only_across_an_import() {
    let (_guard, mount, identifier) = flam_mount::build(false, true);
    let before = snapshot_tree(&mount);
    let h = RealHarness::new();
    h.run(mount.clone(), &identifier, flam_mount::FLAM_UUID)
        .expect("import");
    let after = snapshot_tree(&mount);
    assert_eq!(
        before, after,
        "the FLAM volume must be byte-identical after an import (read-only end to end)"
    );
}

#[test]
fn boot_sweep_clears_residues_left_by_a_simulated_crash() {
    let h = RealHarness::new();
    // Simulate a crash mid-acquisition + a crash between promotion and
    // commit: a stale staging dir and a promoted dir without a DB row.
    let staging = h.imports_dir().join(".staging").join("crashed-acquisition");
    std::fs::create_dir_all(&staging).expect("mk staging residue");
    std::fs::write(staging.join("ni"), b"PART").expect("partial file");
    let orphan = h.imports_dir().join("0197-orphan");
    std::fs::create_dir_all(&orphan).expect("mk orphan");
    std::fs::write(orphan.join("ni"), b"ORPHAN").expect("orphan file");

    let outcome = {
        let db = h.db.lock().expect("lock");
        sweep_import_artifacts(&db, h.app_data.path()).expect("sweep")
    };
    assert_eq!(outcome.staging_entries_removed, 1);
    assert_eq!(outcome.orphan_dirs_removed, 1);
    assert!(!staging.exists());
    assert!(!orphan.exists());
    // Keep the db guard alive until here so the TempDirs drop cleanly.
    let _ = &h.db_dir;
}
