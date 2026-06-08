//! Reads the installed-pack inventory of a mounted Lunii.
//!
//! Runs AFTER detection: given the `mount_path` of an already-classified
//! supported volume (the path the scanner discovered but the wire DTO
//! never exposes), enumerate the packs from `.pi` (visible) and
//! `.pi.hidden` (hidden), and probe `.content/<SHORT_ID>` to confirm each
//! pack payload is actually present.
//!
//! Read-only and key-free: enumerating the inventory touches only the
//! index files and folder names — no media decryption, no write. This is
//! why `read_library` is authorized for every supported cohort (V1/V2/V3)
//! even though import stays gated.
//!
//! A trait so the application layer is testable without a real mount:
//! [`MockDeviceLibraryReader`](super::mock::MockDeviceLibraryReader) lets
//! tests assemble a `DeviceLibrary` or a failure without touching disk.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::domain::device::{
    format_pack_uuid, pack_short_id, parse_pack_index, DeviceLibrary, DeviceStoryEntry,
    LUNII_CONTENT_DIR, LUNII_DEVICE_ID_MARKER, LUNII_HIDDEN_INDEX_MARKER, MAX_PACK_INDEX_BYTES,
};
use crate::domain::shared::AppError;
use crate::infrastructure::diagnostics::jsonl::io_kind_label;

/// Reads the pack inventory at a mount path. MUST respect the `budget`
/// wall-clock deadline so a stalled mount cannot keep the
/// `spawn_blocking` worker alive past the command budget. Filesystem
/// failures map to a recoverable `AppError::device_scan_failed` — the
/// local library stays intact, the panel offers a retry.
pub trait DeviceLibraryReader: Send + Sync + 'static {
    fn read_library(&self, mount_path: &Path, budget: Duration) -> Result<DeviceLibrary, AppError>;
}

/// Production reader: stdlib filesystem reads at the mount path.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDeviceLibraryReader;

impl DeviceLibraryReader for SystemDeviceLibraryReader {
    fn read_library(&self, mount_path: &Path, budget: Duration) -> Result<DeviceLibrary, AppError> {
        let started = Instant::now();
        let content_dir = mount_path.join(LUNII_CONTENT_DIR);
        let mut entries: Vec<DeviceStoryEntry> = Vec::new();
        let mut had_trailing_bytes = false;

        // `.pi` is REQUIRED. Its absence or illegibility at read time is
        // exactly the AC #3 "device disappeared mid-read" branch: surface
        // a recoverable read failure rather than an empty inventory.
        let pi_path = mount_path.join(LUNII_DEVICE_ID_MARKER);
        let pi_payload = read_index_bounded(&pi_path, started, budget)?;
        let pi_index = parse_pack_index(&pi_payload);
        had_trailing_bytes |= pi_index.had_trailing_bytes;
        append_entries(
            &mut entries,
            &pi_index.uuids,
            false,
            &content_dir,
            started,
            budget,
        )?;

        // `.pi.hidden` is OPTIONAL and best-effort: a device with no
        // hidden pack ships no file, and a transient failure reading this
        // companion index must not sink the whole inventory. The required
        // `.pi` already proved the mount is readable.
        let hidden_path = mount_path.join(LUNII_HIDDEN_INDEX_MARKER);
        if hidden_path.is_file() {
            if let Ok(hidden_payload) = read_index_bounded(&hidden_path, started, budget) {
                let hidden_index = parse_pack_index(&hidden_payload);
                had_trailing_bytes |= hidden_index.had_trailing_bytes;
                append_entries(
                    &mut entries,
                    &hidden_index.uuids,
                    true,
                    &content_dir,
                    started,
                    budget,
                )?;
            }
        }

        Ok(DeviceLibrary {
            entries,
            had_trailing_bytes,
        })
    }
}

fn append_entries(
    out: &mut Vec<DeviceStoryEntry>,
    uuids: &[[u8; 16]],
    hidden: bool,
    content_dir: &Path,
    started: Instant,
    budget: Duration,
) -> Result<(), AppError> {
    for bytes in uuids {
        if started.elapsed() >= budget {
            return Err(read_timeout_error(started.elapsed()));
        }
        let short_id = pack_short_id(bytes);
        let content_present = content_dir.join(&short_id).is_dir();
        out.push(DeviceStoryEntry {
            uuid: format_pack_uuid(bytes),
            short_id,
            hidden,
            content_present,
        });
    }
    Ok(())
}

/// Read an index file, bounded to [`MAX_PACK_INDEX_BYTES`] and checked
/// against the wall-clock deadline before and after the read. Returns a
/// recoverable `AppError` on I/O failure, deadline breach or overflow.
fn read_index_bounded(
    path: &Path,
    started: Instant,
    budget: Duration,
) -> Result<Vec<u8>, AppError> {
    if started.elapsed() >= budget {
        return Err(read_timeout_error(started.elapsed()));
    }
    let file = File::open(path).map_err(|err| fs_read_error(io_kind_label(err.kind())))?;
    let mut buf = Vec::new();
    // Read MAX + 1 so the overflow case is detectable without a separate
    // metadata round-trip that could race a file growing under us.
    file.take(MAX_PACK_INDEX_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|err| fs_read_error(io_kind_label(err.kind())))?;
    if started.elapsed() >= budget {
        return Err(read_timeout_error(started.elapsed()));
    }
    if buf.len() as u64 > MAX_PACK_INDEX_BYTES {
        return Err(oversize_index_error());
    }
    Ok(buf)
}

fn fs_read_error(kind: &'static str) -> AppError {
    AppError::device_scan_failed(
        "Lecture de la bibliothèque appareil indisponible: vérifie que la Lunii est branchée et réessaie.",
        "Vérifie la connexion de la Lunii puis réessaie la lecture de la bibliothèque.",
    )
    .with_details(serde_json::json!({
        "source": "fs_read",
        "kind": kind,
    }))
}

fn oversize_index_error() -> AppError {
    AppError::device_scan_failed(
        "Lecture de la bibliothèque appareil indisponible: l'index des histoires est illisible.",
        "Vérifie l'état de la Lunii ; si le problème persiste, consulte le profil de support.",
    )
    .with_details(serde_json::json!({
        "source": "pack_index",
        "kind": "oversize",
    }))
}

fn read_timeout_error(elapsed: Duration) -> AppError {
    AppError::device_scan_failed(
        "Lecture de la bibliothèque appareil indisponible: l'appareil met trop de temps à répondre.",
        "Réessaie la lecture ; si le problème persiste, rebranche la Lunii.",
    )
    .with_details(serde_json::json!({
        "source": "read_timeout",
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::device::fixtures::temp_lunii_mount_with_library;

    fn uuid(tail: [u8; 4]) -> [u8; 16] {
        let mut b = [0xAB; 16];
        b[12..16].copy_from_slice(&tail);
        b
    }

    fn budget() -> Duration {
        Duration::from_secs(5)
    }

    #[test]
    fn reads_visible_packs_in_order_with_content_present() {
        let packs = [(uuid([1, 1, 1, 1]), true), (uuid([2, 2, 2, 2]), true)];
        let (_guard, mount) = temp_lunii_mount_with_library(7, &packs, &[]);
        let lib = SystemDeviceLibraryReader
            .read_library(&mount, budget())
            .expect("read");
        assert_eq!(lib.entries.len(), 2);
        assert_eq!(lib.entries[0].short_id, "01010101");
        assert!(lib.entries[0].content_present);
        assert!(!lib.entries[0].hidden);
        assert_eq!(lib.entries[1].short_id, "02020202");
        assert!(!lib.had_trailing_bytes);
    }

    #[test]
    fn empty_pi_yields_readable_empty_library_not_an_error() {
        let (_guard, mount) = temp_lunii_mount_with_library(7, &[], &[]);
        let lib = SystemDeviceLibraryReader
            .read_library(&mount, budget())
            .expect("empty .pi must read as an empty library");
        assert!(lib.entries.is_empty());
    }

    #[test]
    fn orphan_uuid_without_content_folder_is_flagged_not_dropped() {
        let packs = [(uuid([9, 9, 9, 9]), false)]; // no .content/<short>
        let (_guard, mount) = temp_lunii_mount_with_library(7, &packs, &[]);
        let lib = SystemDeviceLibraryReader
            .read_library(&mount, budget())
            .expect("read");
        assert_eq!(lib.entries.len(), 1);
        assert!(!lib.entries[0].content_present);
    }

    #[test]
    fn hidden_packs_are_surfaced_with_hidden_flag() {
        let visible = [(uuid([1, 0, 0, 0]), true)];
        let hidden = [uuid([2, 0, 0, 0])];
        let (_guard, mount) = temp_lunii_mount_with_library(7, &visible, &hidden);
        let lib = SystemDeviceLibraryReader
            .read_library(&mount, budget())
            .expect("read");
        assert_eq!(lib.entries.len(), 2);
        assert!(!lib.entries[0].hidden);
        assert!(lib.entries[1].hidden);
        assert_eq!(lib.entries[1].short_id, "02000000");
    }

    #[test]
    fn missing_pi_mid_read_maps_to_recoverable_device_scan_failed() {
        // No `.pi` at all (e.g. mount vanished): a recoverable read
        // failure, never an empty inventory.
        let dir = tempfile::tempdir().expect("tempdir");
        let err = SystemDeviceLibraryReader
            .read_library(dir.path(), budget())
            .expect_err("missing .pi must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "fs_read");
        assert_eq!(v["details"]["kind"], "not_found");
    }

    #[test]
    fn oversize_pi_is_rejected_rather_than_truncated() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(LUNII_DEVICE_ID_MARKER),
            vec![0u8; (MAX_PACK_INDEX_BYTES + 16) as usize],
        )
        .expect("write huge .pi");
        let err = SystemDeviceLibraryReader
            .read_library(dir.path(), budget())
            .expect_err("oversize .pi must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_index");
        assert_eq!(v["details"]["kind"], "oversize");
    }

    #[test]
    fn zero_budget_aborts_before_reading() {
        let packs = [(uuid([1, 1, 1, 1]), true)];
        let (_guard, mount) = temp_lunii_mount_with_library(7, &packs, &[]);
        let err = SystemDeviceLibraryReader
            .read_library(&mount, Duration::ZERO)
            .expect_err("zero budget must abort");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "read_timeout");
    }
}
