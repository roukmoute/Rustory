//! Manual hardware smoke for the device-library read.
//!
//! Drives the REAL system scanner + filesystem reader against a mounted
//! Lunii and prints the enumerated inventory. Read-only: it opens
//! `.md` / `.pi` / `.pi.hidden` and lists `.content/` — it never writes
//! to the device.
//!
//! Usage:
//!   cargo run --example device_library_smoke -- <mount_path>
//!   # or, to use production sysinfo enumeration + udisks2 auto-mount:
//!   cargo run --example device_library_smoke
//!
//! Set `RUSTORY_DEVICE_AUTOMOUNT=0` to skip the auto-mount step when the
//! volume is already mounted.

use std::time::Duration;

use rustory_lib::application::device::library::{read_device_library, DeviceLibraryOutcome};
use rustory_lib::application::device::{read_connected_lunii_with_attempts, ConnectedLuniiOutcome};
use rustory_lib::infrastructure::device::{SystemDeviceLibraryReader, SystemDeviceScanner};

fn main() {
    let budget = Duration::from_millis(5000);
    let scanner = match std::env::args().nth(1) {
        Some(path) => {
            println!("scanner: explicit mount root {path}");
            SystemDeviceScanner::with_explicit_mount_roots(vec![path.into()])
        }
        None => {
            println!("scanner: sysinfo enumeration + auto-mount");
            SystemDeviceScanner::default()
        }
    };
    let reader = SystemDeviceLibraryReader;

    // 1) Detection — obtain the supported profile + opaque identifier.
    let (outcome, attempts) =
        read_connected_lunii_with_attempts(&scanner, budget).expect("detection scan");
    if !attempts.is_empty() {
        println!("auto-mount attempts: {attempts:?}");
    }
    let identifier = match outcome {
        ConnectedLuniiOutcome::Supported(profile) => {
            println!(
                "DETECTED supported: cohort={:?} metadata_v={:?} read_library={} import={} write={}",
                profile.firmware_cohort,
                profile.metadata_format_version,
                profile.supported_operations.read_library,
                profile.supported_operations.import_story,
                profile.supported_operations.write_story,
            );
            println!("device_identifier = {}", profile.device_identifier);
            profile.device_identifier
        }
        other => {
            println!("detection did not resolve to a supported device: {other:?}");
            return;
        }
    };

    // 2) Inventory read.
    match read_device_library(&scanner, &reader, &identifier, budget).expect("library read") {
        DeviceLibraryOutcome::Readable {
            device_identifier,
            library,
            ..
        } => {
            println!(
                "READABLE: device={device_identifier} stories={} trailing_bytes={}",
                library.entries.len(),
                library.had_trailing_bytes,
            );
            for (index, entry) in library.entries.iter().enumerate() {
                println!(
                    "  [{index}] short_id={} hidden={} content_present={} uuid={}",
                    entry.short_id, entry.hidden, entry.content_present, entry.uuid,
                );
            }
        }
        other => println!("library read did not resolve to readable: {other:?}"),
    }
}
