//! Manual smoke for the device-story import ("Copier dans ma
//! bibliothèque").
//!
//! Drives the REAL system scanner + library reader + pack reader against
//! a mounted Lunii (or a synthetic mount, see below), imports ONE pack
//! into a sandbox app-data directory and prints the canonical result.
//! Read-only on the device: the copy never writes to the mount.
//!
//! The household Lunii is a V3, where the import is deliberately gated
//! off (`DEVICE_UNSUPPORTED` / `capability_gate` — the honest matrix).
//! The success path is therefore smoked through a SYNTHETIC mount built
//! by this binary: pass `--fixture` and it assembles a metadata-v3 mount
//! with one plausible pack in a temp dir, then imports from it.
//!
//! Usage:
//!   # success path, no hardware needed (self-built v3 fixture):
//!   cargo run --example device_import_smoke -- --fixture
//!
//!   # against a real mount (V3 hardware prints the gate refusal):
//!   cargo run --example device_import_smoke -- <mount_path>
//!
//!   # production sysinfo enumeration + udisks2 auto-mount:
//!   cargo run --example device_import_smoke
//!
//! The sandbox app-data dir (SQLite + imports/) is printed and kept so
//! you can inspect the promoted files; delete it manually when done.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rustory_lib::application::device::import::{import_device_story, ImportDeviceStoryRequest};
use rustory_lib::application::device::library::{read_device_library, DeviceLibraryOutcome};
use rustory_lib::application::device::{read_connected_lunii_with_attempts, ConnectedLuniiOutcome};
use rustory_lib::infrastructure::db;
use rustory_lib::infrastructure::device::{
    SystemDeviceLibraryReader, SystemDevicePackReader, SystemDeviceScanner,
};

const PACK_UUID_BYTES: [u8; 16] = [
    0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xfa, 0xc5, 0x56, 0x2d,
];

fn write_fixture_mount(root: &Path) {
    std::fs::create_dir_all(root).expect("mkdir fixture root");
    // Metadata v3 ⇒ Origine V1 cohort ⇒ import allowed by the matrix.
    std::fs::write(root.join(".md"), [3u8, 0xff, 0xaa]).expect("write .md");
    std::fs::write(root.join(".pi"), PACK_UUID_BYTES).expect("write .pi");
    let short_id = rustory_lib::domain::device::pack_short_id(&PACK_UUID_BYTES);
    let pack = root.join(".content").join(short_id);
    std::fs::create_dir_all(&pack).expect("mkdir pack");
    std::fs::write(pack.join("ni"), vec![0x4E; 512]).expect("ni");
    std::fs::write(pack.join("li"), vec![0x4C; 256]).expect("li");
    std::fs::write(pack.join("ri"), vec![0x52; 128]).expect("ri");
    std::fs::write(pack.join("si"), vec![0x53; 128]).expect("si");
    std::fs::write(pack.join("nm"), vec![0x6E; 32]).expect("nm");
    let rf = pack.join("rf").join("000");
    std::fs::create_dir_all(&rf).expect("rf/000");
    std::fs::write(rf.join("AAAAAAAA"), vec![0xAA; 2048]).expect("rf asset");
    let sf = pack.join("sf").join("000");
    std::fs::create_dir_all(&sf).expect("sf/000");
    std::fs::write(sf.join("BBBBBBBB"), vec![0xBB; 4096]).expect("sf asset");
}

fn main() {
    let budget = Duration::from_secs(300);
    let detection_budget = Duration::from_millis(5000);

    let mut fixture_guard: Option<PathBuf> = None;
    let scanner = match std::env::args().nth(1).as_deref() {
        Some("--fixture") => {
            let root = std::env::temp_dir()
                .join(format!("rustory-import-smoke-mount-{}", std::process::id()));
            write_fixture_mount(&root);
            println!("scanner: SELF-BUILT v3 fixture mount at {}", root.display());
            fixture_guard = Some(root.clone());
            SystemDeviceScanner::with_explicit_mount_roots(vec![root])
        }
        Some(path) => {
            println!("scanner: explicit mount root {path}");
            SystemDeviceScanner::with_explicit_mount_roots(vec![path.into()])
        }
        None => {
            println!("scanner: sysinfo enumeration + auto-mount");
            SystemDeviceScanner::default()
        }
    };

    // 1) Detection — supported profile + opaque identifier.
    let (outcome, attempts) =
        read_connected_lunii_with_attempts(&scanner, detection_budget).expect("detection scan");
    if !attempts.is_empty() {
        println!("auto-mount attempts: {attempts:?}");
    }
    let identifier = match outcome {
        ConnectedLuniiOutcome::Supported(profile) => {
            println!(
                "DETECTED supported: cohort={:?} metadata_v={:?} import={}",
                profile.firmware_cohort,
                profile.metadata_format_version,
                profile.supported_operations.import_story,
            );
            profile.device_identifier
        }
        other => {
            println!("detection did not resolve to a supported device: {other:?}");
            return;
        }
    };

    // 2) Inventory — pick the first content-present pack.
    let reader = SystemDeviceLibraryReader;
    let pack_uuid = match read_device_library(&scanner, &reader, &identifier, detection_budget)
        .expect("library read")
    {
        DeviceLibraryOutcome::Readable { library, .. } => {
            println!("inventory: {} pack(s)", library.entries.len());
            match library.entries.iter().find(|e| e.content_present) {
                Some(entry) => {
                    println!(
                        "importing short_id={} hidden={} uuid={}",
                        entry.short_id, entry.hidden, entry.uuid
                    );
                    entry.uuid.clone()
                }
                None => {
                    println!("no content-present pack to import");
                    return;
                }
            }
        }
        other => {
            println!("library read did not resolve to readable: {other:?}");
            return;
        }
    };

    // 3) Sandbox app-data dir: fresh SQLite + imports/ store.
    let app_data_dir = std::env::temp_dir().join(format!(
        "rustory-import-smoke-appdata-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&app_data_dir).expect("mkdir app data");
    let mut handle = db::open_at(&app_data_dir.join("rustory.sqlite")).expect("open db");
    db::run_migrations(&mut handle).expect("migrate");
    let db = Mutex::new(handle);
    println!("sandbox app-data dir: {}", app_data_dir.display());

    // 4) Import — the full acquisition sequence on real I/O.
    let request = ImportDeviceStoryRequest {
        device_identifier: identifier,
        pack_uuid,
    };
    match import_device_story(
        &db,
        &scanner,
        &reader,
        &SystemDevicePackReader,
        &app_data_dir,
        &request,
        budget,
    ) {
        Ok(imported) => {
            println!(
                "IMPORTED: story_id={} title={:?} files={} bytes={} at={}",
                imported.story.id,
                imported.story.title,
                imported.pack_file_count,
                imported.pack_total_bytes,
                imported.imported_at,
            );
            println!(
                "promoted dir: {}",
                app_data_dir
                    .join("imports")
                    .join(&imported.story.id)
                    .display()
            );
            // 5) Re-import must be refused (provenance lock).
            match import_device_story(
                &db,
                &scanner,
                &reader,
                &SystemDevicePackReader,
                &app_data_dir,
                &request,
                budget,
            ) {
                Ok(_) => println!("UNEXPECTED: re-import succeeded — UNIQUE lock failed"),
                Err(err) => println!("re-import refused as expected: {err}"),
            }
        }
        Err(err) => println!("import refused/failed: {err}"),
    }

    if let Some(root) = fixture_guard {
        let _ = std::fs::remove_dir_all(root);
    }
    println!("(sandbox app-data dir kept for inspection — delete it manually)");
}
