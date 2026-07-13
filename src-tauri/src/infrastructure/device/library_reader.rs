//! Reads the installed-story inventory of a mounted supported device.
//!
//! Runs AFTER detection: given the `mount_path` of an already-classified
//! supported volume (the path the scanner discovered but the wire DTO
//! never exposes) and the FAMILY of the re-scanned profile (Rust
//! authority — never re-sniffed from the mount), enumerate the installed
//! stories the family's own way:
//!
//! - **Lunii** — packs from `.pi` (visible) and `.pi.hidden` (hidden),
//!   payload probed at `.content/<SHORT_ID>`. The historical path,
//!   extracted VERBATIM: its logic, bounds and error copy do not change
//!   by a byte (family isolation).
//! - **FLAM** — stories from the TEXT indexes `etc/library/list`
//!   (visible) and `etc/library/list.hidden` (hidden), payload probed at
//!   `str/<uuid>/` / `str.hidden/<uuid>/`. Born hardened: the index
//!   reads are NO-FOLLOW end to end through the shared bounded helper.
//!
//! Read-only and key-free: enumerating the inventory touches only the
//! index files and folder names — no media decryption, no write.
//!
//! A trait so the application layer is testable without a real mount:
//! [`MockDeviceLibraryReader`](super::mock::MockDeviceLibraryReader) lets
//! tests assemble a `DeviceLibrary` or a failure without touching disk.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::domain::device::{
    format_pack_uuid, pack_short_id, parse_flam_library_index, parse_pack_index, DeviceFamily,
    DeviceLibrary, DeviceStoryEntry, FLAM_CONFIG_DIR, FLAM_HIDDEN_LIBRARY_INDEX_REL,
    FLAM_HIDDEN_STORY_DIR, FLAM_LIBRARY_INDEX_REL, FLAM_PRIMARY_MARKER, FLAM_STORY_DIR,
    LUNII_CONTENT_DIR, LUNII_DEVICE_ID_MARKER, LUNII_HIDDEN_INDEX_MARKER, MAX_PACK_INDEX_BYTES,
};
use crate::domain::shared::AppError;
use crate::infrastructure::diagnostics::jsonl::io_kind_label;

use super::system::{is_real_directory, read_bounded_no_follow_with_max};

/// Reads the story inventory at a mount path for the given family. MUST
/// respect the `budget` wall-clock deadline so a stalled mount cannot
/// keep the `spawn_blocking` worker alive past the command budget.
/// Filesystem failures map to a recoverable
/// `AppError::device_scan_failed` — the local library stays intact, the
/// panel offers a retry.
pub trait DeviceLibraryReader: Send + Sync + 'static {
    fn read_library(
        &self,
        mount_path: &Path,
        family: DeviceFamily,
        budget: Duration,
    ) -> Result<DeviceLibrary, AppError>;
}

/// Production reader: stdlib filesystem reads at the mount path, with an
/// internal per-family dispatch behind the shared trait (the adapter
/// contract — one implementation, family passed from the profile).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDeviceLibraryReader;

impl DeviceLibraryReader for SystemDeviceLibraryReader {
    fn read_library(
        &self,
        mount_path: &Path,
        family: DeviceFamily,
        budget: Duration,
    ) -> Result<DeviceLibrary, AppError> {
        match family {
            DeviceFamily::Lunii => read_lunii_library(mount_path, budget),
            DeviceFamily::Flam => read_flam_library(mount_path, budget),
        }
    }
}

/// Historical Lunii inventory read, extracted VERBATIM from the pre-FLAM
/// implementation — zero behavior change (fixtures and error copy prove
/// it). The `.pi` index read is deliberately NOT retrofitted to the
/// no-follow helper: that parity is a separate, deferred hardening of
/// the most sensitive path of the product.
fn read_lunii_library(mount_path: &Path, budget: Duration) -> Result<DeviceLibrary, AppError> {
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
            return Err(read_timeout_error(DeviceFamily::Lunii, started.elapsed()));
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

/// Read a Lunii index file, bounded to [`MAX_PACK_INDEX_BYTES`] and
/// checked against the wall-clock deadline before and after the read.
/// Returns a recoverable `AppError` on I/O failure, deadline breach or
/// overflow.
fn read_index_bounded(
    path: &Path,
    started: Instant,
    budget: Duration,
) -> Result<Vec<u8>, AppError> {
    if started.elapsed() >= budget {
        return Err(read_timeout_error(DeviceFamily::Lunii, started.elapsed()));
    }
    let file = File::open(path)
        .map_err(|err| fs_read_error(DeviceFamily::Lunii, io_kind_label(err.kind())))?;
    let mut buf = Vec::new();
    // Read MAX + 1 so the overflow case is detectable without a separate
    // metadata round-trip that could race a file growing under us.
    file.take(MAX_PACK_INDEX_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|err| fs_read_error(DeviceFamily::Lunii, io_kind_label(err.kind())))?;
    if started.elapsed() >= budget {
        return Err(read_timeout_error(DeviceFamily::Lunii, started.elapsed()));
    }
    if buf.len() as u64 > MAX_PACK_INDEX_BYTES {
        return Err(unreadable_index_error(DeviceFamily::Lunii, "oversize"));
    }
    Ok(buf)
}

/// FLAM inventory read — born hardened (see
/// `device-support-profile.md` → "FLAM library inventory & story
/// import"). The index is TEXT (`etc/library/list`), authoritative,
/// read NO-FOLLOW under the 64 KiB inventory bound through the shared
/// hardened helper; a `list` ABSENT is a legitimately EMPTY inventory
/// (the index is not a recognition marker — the hidden index is not
/// consulted either), while an unreadable/irregular/oversize `list` is
/// a RECOVERABLE failure, never silently folded into "empty".
fn read_flam_library(mount_path: &Path, budget: Duration) -> Result<DeviceLibrary, AppError> {
    let started = Instant::now();

    let index_path = mount_path.join(FLAM_LIBRARY_INDEX_REL);
    let visible_payload = match read_flam_index_bounded(&index_path, started, budget)? {
        FlamIndexRead::Absent => {
            // `NotFound` on the index path is AMBIGUOUS: a fresh FLAM
            // that never wrote its index (legitimately empty), or a
            // mount that vanished between the authoritative re-scan and
            // this read (a mid-read unplug surfaces as not_found — the
            // Lunii path treats it as a recoverable failure). Re-probe
            // the REQUIRED profile markers no-follow to tell them apart:
            // device still present → honestly empty inventory; markers
            // gone → recoverable read failure, never a lying empty.
            return if flam_markers_still_present(mount_path) {
                Ok(DeviceLibrary::default())
            } else {
                Err(fs_read_error(DeviceFamily::Flam, "not_found"))
            };
        }
        FlamIndexRead::Payload(payload) => payload,
    };
    let visible_index = parse_flam_library_index(&visible_payload);
    let mut had_trailing_bytes = visible_index.had_trailing_bytes;

    let mut entries: Vec<DeviceStoryEntry> = Vec::new();
    // First-occurrence dedup ACROSS the two indexes too: a visible entry
    // wins over a hidden duplicate (contract born hardened — the Lunii
    // duplicate behavior is deliberately not changed).
    let mut seen: std::collections::HashSet<[u8; 16]> = std::collections::HashSet::new();
    let visible_content_root = mount_path.join(FLAM_STORY_DIR);
    for bytes in &visible_index.uuids {
        if started.elapsed() >= budget {
            return Err(read_timeout_error(DeviceFamily::Flam, started.elapsed()));
        }
        if !seen.insert(*bytes) {
            continue;
        }
        entries.push(flam_entry(bytes, false, &visible_content_root));
    }

    // The hidden index is OPTIONAL and best-effort, like `.pi.hidden`:
    // absent, irregular or unreadable → the visible inventory stands.
    let hidden_path = mount_path.join(FLAM_HIDDEN_LIBRARY_INDEX_REL);
    if let Ok(FlamIndexRead::Payload(hidden_payload)) =
        read_flam_index_bounded(&hidden_path, started, budget)
    {
        let hidden_index = parse_flam_library_index(&hidden_payload);
        had_trailing_bytes |= hidden_index.had_trailing_bytes;
        let hidden_content_root = mount_path.join(FLAM_HIDDEN_STORY_DIR);
        for bytes in &hidden_index.uuids {
            if started.elapsed() >= budget {
                return Err(read_timeout_error(DeviceFamily::Flam, started.elapsed()));
            }
            if !seen.insert(*bytes) {
                continue;
            }
            entries.push(flam_entry(bytes, true, &hidden_content_root));
        }
    }

    Ok(DeviceLibrary {
        entries,
        had_trailing_bytes,
    })
}

/// Re-probe the REQUIRED FLAM recognition markers no-follow (`.mdf` a
/// regular file, `str/` and `etc/` real directories). Distinguishes "the
/// device is still here, it just has no library index yet" from "the
/// mount vanished mid-read" when the index path reads NotFound.
fn flam_markers_still_present(mount_path: &Path) -> bool {
    let mdf_is_regular = match std::fs::symlink_metadata(mount_path.join(FLAM_PRIMARY_MARKER)) {
        Ok(meta) => !meta.file_type().is_symlink() && meta.is_file(),
        Err(_) => false,
    };
    mdf_is_regular
        && is_real_directory(&mount_path.join(FLAM_STORY_DIR))
        && is_real_directory(&mount_path.join(FLAM_CONFIG_DIR))
}

fn flam_entry(bytes: &[u8; 16], hidden: bool, content_root: &Path) -> DeviceStoryEntry {
    let uuid = format_pack_uuid(bytes);
    // The story payload is the REAL directory `str/<uuid>/` (or
    // `str.hidden/<uuid>/`), probed no-follow — a symlink does not count.
    let content_present = is_real_directory(&content_root.join(&uuid));
    DeviceStoryEntry {
        uuid,
        short_id: pack_short_id(bytes),
        hidden,
        content_present,
    }
}

/// Outcome of one FLAM index read.
enum FlamIndexRead {
    /// No entry at the index path — for `list`, a legitimately empty
    /// inventory; for `list.hidden`, simply nothing hidden.
    Absent,
    Payload(Vec<u8>),
}

/// Read one FLAM text index, NO-FOLLOW under the 64 KiB inventory bound
/// (the shared hardened helper — reused, not reinvented). ABSENT is a
/// typed non-error; any OTHER failure (irregular entry, oversize,
/// residual open/read failure) is a recoverable error — never silent.
/// The wall-clock deadline is checked before and after the bounded read
/// (the cooperative discipline of this reader).
fn read_flam_index_bounded(
    path: &Path,
    started: Instant,
    budget: Duration,
) -> Result<FlamIndexRead, AppError> {
    if started.elapsed() >= budget {
        return Err(read_timeout_error(DeviceFamily::Flam, started.elapsed()));
    }
    let pre = match std::fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(FlamIndexRead::Absent),
        Err(err) => return Err(fs_read_error(DeviceFamily::Flam, io_kind_label(err.kind()))),
        Ok(pre) => pre,
    };
    if pre.file_type().is_symlink() || !pre.is_file() {
        return Err(unreadable_index_error(
            DeviceFamily::Flam,
            "not_a_regular_file",
        ));
    }
    if pre.len() > MAX_PACK_INDEX_BYTES {
        return Err(unreadable_index_error(DeviceFamily::Flam, "oversize"));
    }
    // The shared deadline flag stays false for this short bounded read;
    // the reader's own checks above/below own the cooperative deadline.
    let no_deadline = Arc::new(AtomicBool::new(false));
    let payload = match read_bounded_no_follow_with_max(path, &no_deadline, MAX_PACK_INDEX_BYTES) {
        // The lstat above classified a regular in-bound file, so a
        // `None` here is a race (entry swapped/removed/grown) or a
        // per-volume I/O failure — recoverable, NEVER silently empty.
        Ok(None) => return Err(fs_read_error(DeviceFamily::Flam, "io_other")),
        Ok(Some(payload)) => payload,
        Err(err) => return Err(fs_read_error(DeviceFamily::Flam, io_kind_label(err.kind()))),
    };
    if started.elapsed() >= budget {
        return Err(read_timeout_error(DeviceFamily::Flam, started.elapsed()));
    }
    Ok(FlamIndexRead::Payload(payload))
}

// Closed user-facing copy — sober, no OS message, no path (PII rules).
// Family-correct: the Lunii read path keeps its historical copy
// VERBATIM; the FLAM path was born with the device-generic wording
// (product-language.md Change Control).

fn fs_read_error(family: DeviceFamily, kind: &'static str) -> AppError {
    let (message, action) = match family {
        DeviceFamily::Lunii => (
            "Lecture de la bibliothèque appareil indisponible: vérifie que la Lunii est branchée et réessaie.",
            "Vérifie la connexion de la Lunii puis réessaie la lecture de la bibliothèque.",
        ),
        DeviceFamily::Flam => (
            "Lecture de la bibliothèque appareil indisponible: vérifie que l'appareil est branché et réessaie.",
            "Vérifie la connexion de l'appareil puis réessaie la lecture de la bibliothèque.",
        ),
    };
    AppError::device_scan_failed(message, action).with_details(serde_json::json!({
        "source": "fs_read",
        "kind": kind,
    }))
}

/// The index exists but cannot be trusted (`oversize` beyond the 64 KiB
/// inventory bound, or `not_a_regular_file` — a symlink/special entry on
/// the FLAM path). Same `pack_index` source as the historical Lunii
/// oversize refusal.
fn unreadable_index_error(family: DeviceFamily, kind: &'static str) -> AppError {
    let action = match family {
        DeviceFamily::Lunii => {
            "Vérifie l'état de la Lunii ; si le problème persiste, consulte le profil de support."
        }
        DeviceFamily::Flam => {
            "Vérifie l'état de l'appareil ; si le problème persiste, consulte le profil de support."
        }
    };
    AppError::device_scan_failed(
        "Lecture de la bibliothèque appareil indisponible: l'index des histoires est illisible.",
        action,
    )
    .with_details(serde_json::json!({
        "source": "pack_index",
        "kind": kind,
    }))
}

fn read_timeout_error(family: DeviceFamily, elapsed: Duration) -> AppError {
    let action = match family {
        DeviceFamily::Lunii => "Réessaie la lecture ; si le problème persiste, rebranche la Lunii.",
        DeviceFamily::Flam => {
            "Réessaie la lecture ; si le problème persiste, rebranche l'appareil."
        }
    };
    AppError::device_scan_failed(
        "Lecture de la bibliothèque appareil indisponible: l'appareil met trop de temps à répondre.",
        action,
    )
    .with_details(serde_json::json!({
        "source": "read_timeout",
        "elapsed_ms": elapsed.as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::device::fixtures::{
        temp_flam_mount, temp_flam_mount_with_library, temp_lunii_mount_with_library,
        FlamLibraryEntry,
    };

    fn uuid(tail: [u8; 4]) -> [u8; 16] {
        let mut b = [0xAB; 16];
        b[12..16].copy_from_slice(&tail);
        b
    }

    fn budget() -> Duration {
        Duration::from_secs(5)
    }

    fn read_lunii(mount: &Path, budget: Duration) -> Result<DeviceLibrary, AppError> {
        SystemDeviceLibraryReader.read_library(mount, DeviceFamily::Lunii, budget)
    }

    fn read_flam(mount: &Path, budget: Duration) -> Result<DeviceLibrary, AppError> {
        SystemDeviceLibraryReader.read_library(mount, DeviceFamily::Flam, budget)
    }

    #[test]
    fn reads_visible_packs_in_order_with_content_present() {
        let packs = [(uuid([1, 1, 1, 1]), true), (uuid([2, 2, 2, 2]), true)];
        let (_guard, mount) = temp_lunii_mount_with_library(7, &packs, &[]);
        let lib = read_lunii(&mount, budget()).expect("read");
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
        let lib = read_lunii(&mount, budget()).expect("empty .pi must read as an empty library");
        assert!(lib.entries.is_empty());
    }

    #[test]
    fn orphan_uuid_without_content_folder_is_flagged_not_dropped() {
        let packs = [(uuid([9, 9, 9, 9]), false)]; // no .content/<short>
        let (_guard, mount) = temp_lunii_mount_with_library(7, &packs, &[]);
        let lib = read_lunii(&mount, budget()).expect("read");
        assert_eq!(lib.entries.len(), 1);
        assert!(!lib.entries[0].content_present);
    }

    #[test]
    fn hidden_packs_are_surfaced_with_hidden_flag() {
        let visible = [(uuid([1, 0, 0, 0]), true)];
        let hidden = [uuid([2, 0, 0, 0])];
        let (_guard, mount) = temp_lunii_mount_with_library(7, &visible, &hidden);
        let lib = read_lunii(&mount, budget()).expect("read");
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
        let err = read_lunii(dir.path(), budget()).expect_err("missing .pi must fail");
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
        let err = read_lunii(dir.path(), budget()).expect_err("oversize .pi must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_index");
        assert_eq!(v["details"]["kind"], "oversize");
    }

    #[test]
    fn zero_budget_aborts_before_reading() {
        let packs = [(uuid([1, 1, 1, 1]), true)];
        let (_guard, mount) = temp_lunii_mount_with_library(7, &packs, &[]);
        let err = read_lunii(&mount, Duration::ZERO).expect_err("zero budget must abort");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "read_timeout");
    }

    #[test]
    fn lunii_read_error_copy_stays_verbatim() {
        // Family isolation (AC2): the Lunii read path keeps its
        // historical copy — the FLAM extension must not reword it.
        let dir = tempfile::tempdir().expect("tempdir");
        let err = read_lunii(dir.path(), budget()).expect_err("missing .pi must fail");
        assert_eq!(
            err.message,
            "Lecture de la bibliothèque appareil indisponible: vérifie que la Lunii est branchée et réessaie."
        );
        assert_eq!(
            err.user_action.as_deref(),
            Some("Vérifie la connexion de la Lunii puis réessaie la lecture de la bibliothèque.")
        );
    }

    // ---------------- FLAM inventory (index-founded) ----------------

    const FLAM_UUID_A: &str = "12345678-9abc-def0-1122-334455667788";
    const FLAM_UUID_B: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeffff0000";

    #[test]
    fn flam_reads_visible_stories_in_index_order_with_content_present() {
        let (_guard, mount) = temp_flam_mount_with_library(&[
            FlamLibraryEntry::visible(FLAM_UUID_A),
            FlamLibraryEntry::visible(FLAM_UUID_B),
        ]);
        let lib = read_flam(&mount, budget()).expect("read");
        assert_eq!(lib.entries.len(), 2);
        assert_eq!(lib.entries[0].uuid, FLAM_UUID_A);
        assert_eq!(lib.entries[0].short_id, "55667788");
        assert!(lib.entries[0].content_present);
        assert!(!lib.entries[0].hidden);
        assert_eq!(lib.entries[1].uuid, FLAM_UUID_B);
        assert!(!lib.had_trailing_bytes);
    }

    #[test]
    fn flam_missing_list_yields_a_legitimately_empty_inventory() {
        // The index is NOT a recognition marker: a conforming FLAM whose
        // required markers are STILL PRESENT but without
        // `etc/library/list` reads as an EMPTY library, never an error
        // (a fresh device may not have written it yet).
        let (_guard, mount) = temp_flam_mount();
        let lib = read_flam(&mount, budget()).expect("absent list must read empty");
        assert!(lib.entries.is_empty());
        assert!(!lib.had_trailing_bytes);
    }

    #[test]
    fn flam_missing_list_ignores_a_populated_hidden_index() {
        // The primary index OWNS the inventory: with `list` absent (and
        // the device still present), a populated `list.hidden` is NOT
        // consulted — the inventory stays legitimately empty.
        let (_guard, mount) = temp_flam_mount();
        let hidden_index = mount.join(FLAM_HIDDEN_LIBRARY_INDEX_REL);
        std::fs::create_dir_all(hidden_index.parent().expect("parent")).expect("mkdir");
        std::fs::write(&hidden_index, format!("{FLAM_UUID_A}\n")).expect("write list.hidden");
        let lib = read_flam(&mount, budget()).expect("must read empty");
        assert!(
            lib.entries.is_empty(),
            "the hidden index must not be consulted without the primary index"
        );
    }

    #[test]
    fn flam_vanished_mount_is_a_recoverable_error_never_a_lying_empty_inventory() {
        // A mid-read unplug also reads NotFound on the index path — but
        // the required markers are gone too: surface a recoverable read
        // failure (the honest Lunii behavior), NEVER an empty inventory
        // that would render "aucune histoire lisible" and log a success.
        let dir = tempfile::tempdir().expect("tempdir");
        let vanished = dir.path().join("gone");
        // The mount path never existed / was yanked: nothing at all there.
        let err = read_flam(&vanished, budget()).expect_err("vanished mount must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "fs_read");
        assert_eq!(v["details"]["kind"], "not_found");
    }

    #[test]
    fn flam_mount_stripped_of_its_markers_mid_read_is_a_recoverable_error() {
        // Same ambiguity, device still mounted but no longer a FLAM
        // (markers removed between the re-scan and the read): recoverable,
        // never silently empty.
        let (_guard, mount) = temp_flam_mount();
        std::fs::remove_file(mount.join(FLAM_PRIMARY_MARKER)).expect("drop .mdf");
        let err = read_flam(&mount, budget()).expect_err("stripped markers must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "fs_read");
        assert_eq!(v["details"]["kind"], "not_found");
    }

    #[test]
    fn flam_hidden_stories_are_surfaced_with_hidden_flag_and_hidden_root() {
        let (_guard, mount) = temp_flam_mount_with_library(&[
            FlamLibraryEntry::visible(FLAM_UUID_A),
            FlamLibraryEntry::hidden(FLAM_UUID_B),
        ]);
        let lib = read_flam(&mount, budget()).expect("read");
        assert_eq!(lib.entries.len(), 2);
        assert!(!lib.entries[0].hidden);
        assert!(lib.entries[1].hidden);
        // The hidden story's payload lives under str.hidden/ — the
        // fixture materialized it there and the probe found it.
        assert!(lib.entries[1].content_present);
    }

    #[test]
    fn flam_index_entry_without_story_folder_flags_content_absent() {
        let (_guard, mount) =
            temp_flam_mount_with_library(&[FlamLibraryEntry::orphan(FLAM_UUID_A)]);
        let lib = read_flam(&mount, budget()).expect("read");
        assert_eq!(lib.entries.len(), 1);
        assert!(!lib.entries[0].content_present);
    }

    #[cfg(unix)]
    #[test]
    fn flam_symlinked_story_folder_does_not_count_as_content() {
        // The payload probe is no-follow: a symlink pretending to be the
        // story folder is not content.
        let (_guard, mount) =
            temp_flam_mount_with_library(&[FlamLibraryEntry::orphan(FLAM_UUID_A)]);
        let real = mount.join("elsewhere");
        std::fs::create_dir(&real).expect("mk real dir");
        std::os::unix::fs::symlink(&real, mount.join(FLAM_STORY_DIR).join(FLAM_UUID_A))
            .expect("symlink story dir");
        let lib = read_flam(&mount, budget()).expect("read");
        assert!(!lib.entries[0].content_present);
    }

    #[test]
    fn flam_story_folder_without_index_entry_stays_invisible() {
        // The index is authoritative: a folder alone never invents an
        // inventory entry (symmetric with the Lunii orphan rule).
        let (_guard, mount) =
            temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID_A)]);
        let stray = mount.join(FLAM_STORY_DIR).join(FLAM_UUID_B);
        std::fs::create_dir_all(&stray).expect("mk stray story dir");
        let lib = read_flam(&mount, budget()).expect("read");
        assert_eq!(lib.entries.len(), 1);
        assert_eq!(lib.entries[0].uuid, FLAM_UUID_A);
    }

    #[test]
    fn flam_malformed_index_line_is_skipped_and_flagged() {
        let (_guard, mount) =
            temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID_A)]);
        std::fs::write(
            mount.join(FLAM_LIBRARY_INDEX_REL),
            format!("{FLAM_UUID_A}\nnot-a-uuid\n"),
        )
        .expect("rewrite index");
        let lib = read_flam(&mount, budget()).expect("read");
        assert_eq!(lib.entries.len(), 1);
        assert!(lib.had_trailing_bytes);
    }

    #[test]
    fn flam_duplicate_across_visible_and_hidden_keeps_the_visible_entry() {
        // First-occurrence dedup across the two indexes: visible wins.
        let (_guard, mount) = temp_flam_mount_with_library(&[
            FlamLibraryEntry::visible(FLAM_UUID_A),
            FlamLibraryEntry::hidden(FLAM_UUID_B),
        ]);
        std::fs::write(
            mount.join(FLAM_HIDDEN_LIBRARY_INDEX_REL),
            format!("{FLAM_UUID_A}\n{FLAM_UUID_B}\n"),
        )
        .expect("rewrite hidden index");
        let lib = read_flam(&mount, budget()).expect("read");
        assert_eq!(lib.entries.len(), 2);
        assert_eq!(lib.entries[0].uuid, FLAM_UUID_A);
        assert!(!lib.entries[0].hidden, "the visible occurrence wins");
        assert_eq!(lib.entries[1].uuid, FLAM_UUID_B);
        assert!(lib.entries[1].hidden);
    }

    #[test]
    fn flam_oversize_list_is_a_recoverable_error_never_silently_empty() {
        let (_guard, mount) = temp_flam_mount();
        let index_path = mount.join(FLAM_LIBRARY_INDEX_REL);
        std::fs::create_dir_all(index_path.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &index_path,
            vec![b'a'; (MAX_PACK_INDEX_BYTES + 16) as usize],
        )
        .expect("write huge list");
        let err = read_flam(&mount, budget()).expect_err("oversize list must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "pack_index");
        assert_eq!(v["details"]["kind"], "oversize");
    }

    #[cfg(unix)]
    #[test]
    fn flam_symlinked_list_is_a_recoverable_error_never_followed() {
        let (_guard, mount) = temp_flam_mount();
        let target = mount.join("real_list");
        std::fs::write(&target, format!("{FLAM_UUID_A}\n")).expect("write target");
        let index_path = mount.join(FLAM_LIBRARY_INDEX_REL);
        std::fs::create_dir_all(index_path.parent().expect("parent")).expect("mkdir");
        std::os::unix::fs::symlink(&target, &index_path).expect("symlink list");
        let err = read_flam(&mount, budget()).expect_err("symlinked list must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "pack_index");
        assert_eq!(v["details"]["kind"], "not_a_regular_file");
    }

    #[test]
    fn flam_unreadable_hidden_index_is_best_effort_and_keeps_the_visible_inventory() {
        // `list.hidden` mirrors `.pi.hidden`: a broken companion index
        // never sinks the visible inventory.
        let (_guard, mount) =
            temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID_A)]);
        std::fs::create_dir(mount.join(FLAM_HIDDEN_LIBRARY_INDEX_REL))
            .expect("hidden index as a DIRECTORY");
        let lib = read_flam(&mount, budget()).expect("visible inventory must stand");
        assert_eq!(lib.entries.len(), 1);
        assert_eq!(lib.entries[0].uuid, FLAM_UUID_A);
    }

    #[test]
    fn flam_zero_budget_aborts_with_read_timeout() {
        let (_guard, mount) =
            temp_flam_mount_with_library(&[FlamLibraryEntry::visible(FLAM_UUID_A)]);
        let err = read_flam(&mount, Duration::ZERO).expect_err("zero budget must abort");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "read_timeout");
        assert_eq!(
            err.user_action.as_deref(),
            Some("Réessaie la lecture ; si le problème persiste, rebranche l'appareil.")
        );
    }

    #[test]
    fn flam_read_error_copy_is_family_correct() {
        // The FLAM path was born with the device-generic wording — it
        // never claims a Lunii is involved.
        let (_guard, mount) = temp_flam_mount();
        let index_path = mount.join(FLAM_LIBRARY_INDEX_REL);
        std::fs::create_dir_all(index_path.parent().expect("parent")).expect("mkdir");
        std::fs::create_dir(&index_path).expect("list as a DIRECTORY");
        let err = read_flam(&mount, budget()).expect_err("irregular list must fail");
        assert!(
            !err.message.contains("Lunii"),
            "FLAM copy must not name the Lunii: {}",
            err.message
        );
        assert!(
            !err.user_action.as_deref().unwrap_or("").contains("Lunii"),
            "FLAM next gesture must not name the Lunii"
        );
    }
}
