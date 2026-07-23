//! Send a STUdio-format pack (`.zip`) TO a connected V3 device — the write
//! flow ("Envoyer un pack vers l'appareil").
//!
//! Composes the proven V3 engine: authoritative re-scan + `send_archive` gate →
//! read the archive (`story.json` + assets) → [`transcode_pack`] →
//! [`assemble_v3_pack`] (with the device `.md`) → [`DeviceV3PackWriter`]. The
//! source archive's assets are written VERBATIM (community packs already carry
//! device-format BMP/MP3); this flow re-keys the ciphering for the TARGET
//! device (its own `.md` content key), so a pack made for one device plays on
//! another. Synchronous by design (the command hands it to `spawn_blocking`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::domain::device::{
    transcode_pack, DeviceFamily, FirmwareCohort, StudioStoryPack, SupportedOperation,
    LUNII_PRIMARY_MARKER,
};
use crate::domain::shared::AppError;
use crate::domain::transfer::short_id_from_pack_uuid;
use crate::infrastructure::device::{
    assemble_v3_pack, AssembleError, DeviceScanner, DeviceV3PackWriter,
};

use super::{check_operation_allowed, resolve_connected_lunii, ConnectedLuniiOutcome};

/// Entry name of the pack descriptor inside a structured archive.
const STORY_JSON_NAME: &str = "story.json";
/// Assets live under this prefix (bare basename is a hand-made-zip fallback).
const ASSETS_PREFIX: &str = "assets/";
/// Byte bound on the descriptor and on a single asset (defensive, generous).
const MAX_STORY_JSON_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;
/// Bound on the archive's entry count.
const MAX_ARCHIVE_ENTRIES: usize = 200_000;

/// Input of [`send_archive_to_device`]. `device_identifier` is validated at the
/// IPC boundary; `archive_path` is the user-picked `.zip`.
#[derive(Debug, Clone)]
pub struct SendArchiveRequest {
    pub device_identifier: String,
    pub archive_path: PathBuf,
}

/// Result of a settled send, echoed to the UI. Family/cohort feed the
/// diagnostic event only (never the wire — family-neutral outcome).
#[derive(Debug, Clone)]
pub struct SentToDevice {
    pub pack_uuid: String,
    pub short_id: String,
    pub image_count: usize,
    pub audio_count: usize,
    pub family: DeviceFamily,
    pub firmware_cohort: FirmwareCohort,
}

pub fn send_archive_to_device(
    scanner: &dyn DeviceScanner,
    writer: &dyn DeviceV3PackWriter,
    request: &SendArchiveRequest,
    budget: Duration,
) -> Result<SentToDevice, AppError> {
    let started = Instant::now();
    let remaining = |started: Instant| budget.saturating_sub(started.elapsed());

    // 1. Authoritative re-scan: identity + capability re-proven live.
    let resolved = resolve_connected_lunii(scanner, remaining(started))?;
    let (profile, mount_path) = match resolved.outcome {
        ConnectedLuniiOutcome::Supported(profile) => {
            if profile.device_identifier != request.device_identifier {
                return Err(device_changed_error("identifier_mismatch"));
            }
            let mount = resolved
                .supported_mount_path
                .ok_or_else(|| device_changed_error("mount_unavailable"))?;
            (profile, mount)
        }
        ConnectedLuniiOutcome::None => return Err(device_changed_error("device_absent")),
        ConnectedLuniiOutcome::Unsupported { .. } => {
            return Err(device_changed_error("device_unsupported"))
        }
        ConnectedLuniiOutcome::Ambiguous { .. } => {
            return Err(device_changed_error("multiple_candidates"))
        }
    };

    // 2. Fail-closed gate BEFORE any device mutation. The DEDICATED
    //    archive-send capability — never `write_story` (the round-trip of an
    //    imported pack), so opening one can never open the other.
    check_operation_allowed(&profile, SupportedOperation::SendArchive)?;

    // 3. Read + parse the archive descriptor.
    let mut archive = open_archive(&request.archive_path)?;
    let story_json = read_entry(&mut archive, STORY_JSON_NAME, MAX_STORY_JSON_BYTES)
        .ok_or_else(|| archive_error("descriptor_missing"))?;
    let pack: StudioStoryPack =
        serde_json::from_slice(&story_json).map_err(|_| archive_error("descriptor_invalid"))?;
    let pack_uuid = pack_entry_uuid(&pack).ok_or_else(|| archive_error("no_entry_node"))?;
    let short_id = short_id_from_pack_uuid(&pack_uuid).ok_or_else(|| archive_error("bad_uuid"))?;

    // 4. Transcode the graph → binary index files + ordered asset lists.
    let transcoded = transcode_pack(&pack).map_err(|_| archive_error("transcode"))?;

    // 5. Read every referenced asset from the archive into memory.
    let mut assets = std::collections::HashMap::new();
    for filename in transcoded.images.iter().chain(transcoded.audios.iter()) {
        if assets.contains_key(filename) {
            continue;
        }
        let bytes = read_entry(&mut archive, filename, MAX_ASSET_BYTES)
            .ok_or_else(|| asset_error(filename))?;
        assets.insert(filename.clone(), bytes);
    }

    // 6. The TARGET device's `.md` (content key + IV + SNU) — re-keys the pack
    //    for THIS device.
    let md = std::fs::read(mount_path.join(LUNII_PRIMARY_MARKER))
        .map_err(|_| device_write_error("md_unreadable"))?;

    // 7. Assemble every `.content/<SHORTID>/` file (cleartext + ciphered).
    let files =
        assemble_v3_pack(&transcoded, &md, &|f| assets.get(f).cloned()).map_err(|e| match e {
            AssembleError::UnreadableDeviceMetadata => device_write_error("md_unreadable"),
            AssembleError::MissingAsset(f) => asset_error(&f),
        })?;

    // 8. Write to the device (atomic staging + promotion + `.pi`).
    writer
        .write_pack(&mount_path, &pack_uuid, &files)
        .map_err(|_| device_write_error("write_rejected"))?;

    Ok(SentToDevice {
        pack_uuid,
        short_id,
        image_count: transcoded.images.len(),
        audio_count: transcoded.audios.len(),
        family: profile.family,
        firmware_cohort: profile.firmware_cohort,
    })
}

/// The pack UUID = the entry ("squareOne") stage node's uuid, falling back to
/// the first stage node. `None` for an empty pack. Lowercased: some community
/// archives carry uppercase hex, but every downstream consumer (`short_id`,
/// `.pi` bytes, the wire) requires the canonical lowercase form.
fn pack_entry_uuid(pack: &StudioStoryPack) -> Option<String> {
    pack.stage_nodes
        .iter()
        .find(|n| n.square_one)
        .or_else(|| pack.stage_nodes.first())
        .map(|n| n.uuid.to_ascii_lowercase())
}

fn open_archive(path: &Path) -> Result<zip::ZipArchive<std::fs::File>, AppError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| archive_error("open"))?;
    if !meta.is_file() {
        return Err(archive_error("open"));
    }
    let file = std::fs::File::open(path).map_err(|_| archive_error("open"))?;
    let archive = zip::ZipArchive::new(file).map_err(|_| archive_error("not_a_zip"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(archive_error("too_many_entries"));
    }
    Ok(archive)
}

/// Read one entry (`assets/<name>` first, bare `<name>` fallback), bounded by
/// `max_bytes` on the bytes actually read. `None` = absent / oversize.
fn read_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    max_bytes: u64,
) -> Option<Vec<u8>> {
    let prefixed = format!("{ASSETS_PREFIX}{name}");
    // `story.json` lives at the root; assets under `assets/`. Try the plain
    // name first for the descriptor, then the prefixed form.
    let candidates = [name.to_string(), prefixed];
    for candidate in candidates {
        if archive.by_name(&candidate).is_ok() {
            let mut entry = archive.by_name(&candidate).ok()?;
            if !entry.is_file() {
                return None;
            }
            let mut buf = Vec::new();
            entry
                .by_ref()
                .take(max_bytes + 1)
                .read_to_end(&mut buf)
                .ok()?;
            if buf.len() as u64 > max_bytes {
                return None;
            }
            return Some(buf);
        }
    }
    None
}

fn device_changed_error(cause: &'static str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: l'appareil connecté a changé.",
        "Rebranche l'appareil souhaité puis relance l'envoi.",
    )
    .with_details(serde_json::json!({ "source": "device_changed", "cause": cause }))
}

fn archive_error(cause: &'static str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: le pack source est illisible.",
        "Vérifie que le fichier est bien une archive de pack (.zip) valide.",
    )
    .with_details(serde_json::json!({ "source": "archive", "cause": cause }))
}

fn asset_error(filename: &str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: un média du pack est introuvable dans l'archive.",
        "Vérifie l'intégrité de l'archive de pack puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "archive",
        "cause": "asset_missing",
        // Only the device basename (8 hex), never a path.
        "asset": crate::domain::device::pack_transcode::device_asset_basename(filename),
    }))
}

fn device_write_error(cause: &'static str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: l'appareil a refusé l'écriture.",
        "Vérifie que l'appareil est bien connecté puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "device_write", "cause": cause }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::device::{compute_device_identifier, MockDeviceScanner};

    const V1_METADATA_VERSION: u8 = 3;
    const V3_METADATA_VERSION: u8 = 7;

    /// The identifier `enqueue_supported_lunii` synthesizes (`.pi` = MOCK_PI,
    /// serial = MOCK_SERIAL) — the value a matching request must carry.
    fn mock_identifier() -> String {
        compute_device_identifier(b"MOCK_PI", Some("MOCK_SERIAL"))
    }

    /// A `DeviceV3PackWriter` that records the pack UUID + file count it was
    /// asked to write.
    #[derive(Default)]
    struct RecordingWriter {
        calls: std::sync::Mutex<Vec<(String, usize)>>,
    }
    impl DeviceV3PackWriter for RecordingWriter {
        fn write_pack(
            &self,
            _mount: &Path,
            pack_uuid: &str,
            files: &[crate::infrastructure::device::AssembledFile],
        ) -> Result<(), crate::domain::transfer::TransferFailureCause> {
            self.calls
                .lock()
                .unwrap()
                .push((pack_uuid.to_string(), files.len()));
            Ok(())
        }
    }

    #[test]
    fn refuses_before_any_write_when_the_device_is_absent() {
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_no_device();
        let writer = RecordingWriter::default();
        let req = SendArchiveRequest {
            device_identifier: "0123456789abcdef0123456789abcdef".into(),
            archive_path: PathBuf::from("/nonexistent.zip"),
        };
        let err = send_archive_to_device(&scanner, &writer, &req, Duration::from_millis(200))
            .expect_err("absent device refuses");
        assert_eq!(
            serde_json::to_value(&err).unwrap()["code"],
            "DEVICE_WRITE_FAILED"
        );
        assert!(
            writer.calls.lock().unwrap().is_empty(),
            "no write attempted"
        );
    }

    #[test]
    fn refuses_a_v1_cohort_at_the_dedicated_gate_even_though_it_may_write_story() {
        // V1's matrix line opens `write_story` (round-trip) but CLOSES
        // `send_archive` (XXTEA not ported) — the refusal proves the send
        // service consults the DEDICATED capability, not `write_story`.
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(V1_METADATA_VERSION);
        let writer = RecordingWriter::default();
        let req = SendArchiveRequest {
            device_identifier: mock_identifier(),
            archive_path: PathBuf::from("/nonexistent.zip"),
        };
        let err = send_archive_to_device(&scanner, &writer, &req, Duration::from_millis(200))
            .expect_err("V1 must refuse the archive send");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "DEVICE_UNSUPPORTED");
        assert_eq!(v["details"]["source"], "capability_gate");
        assert_eq!(v["details"]["operation"], "send_archive");
        assert!(
            writer.calls.lock().unwrap().is_empty(),
            "no write attempted"
        );
    }

    #[test]
    fn passes_the_v3_gate_then_fails_honestly_on_an_unreadable_archive() {
        // V3's matrix line CLOSES `write_story` but OPENS `send_archive` —
        // the flow must get PAST the capability gate (no DEVICE_UNSUPPORTED)
        // and only then refuse the unreadable source archive.
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(V3_METADATA_VERSION);
        let writer = RecordingWriter::default();
        let req = SendArchiveRequest {
            device_identifier: mock_identifier(),
            archive_path: PathBuf::from("/nonexistent.zip"),
        };
        let err = send_archive_to_device(&scanner, &writer, &req, Duration::from_millis(200))
            .expect_err("missing archive refuses");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "DEVICE_WRITE_FAILED");
        assert_eq!(v["details"]["source"], "archive");
        assert!(
            writer.calls.lock().unwrap().is_empty(),
            "no write attempted"
        );
    }

    #[test]
    fn pack_entry_uuid_prefers_the_square_one_node() {
        let json = r#"{"version":1,"nightModeAvailable":false,"actionNodes":[],
            "stageNodes":[
              {"uuid":"aaa","squareOne":false,"controlSettings":{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}},
              {"uuid":"bbb","squareOne":true,"controlSettings":{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}
            ]}"#;
        let pack: StudioStoryPack = serde_json::from_str(json).unwrap();
        assert_eq!(pack_entry_uuid(&pack).as_deref(), Some("bbb"));
    }

    #[test]
    fn pack_entry_uuid_lowercases_an_uppercase_community_uuid() {
        // Some community archives carry uppercase hex; every downstream
        // consumer (short id, `.pi` bytes, the wire) needs canonical
        // lowercase.
        let json = r#"{"version":1,"nightModeAvailable":false,"actionNodes":[],
            "stageNodes":[
              {"uuid":"ABABABAB-ABAB-ABAB-ABAB-ABABFAC5562D","squareOne":true,"controlSettings":{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}
            ]}"#;
        let pack: StudioStoryPack = serde_json::from_str(json).unwrap();
        assert_eq!(
            pack_entry_uuid(&pack).as_deref(),
            Some("abababab-abab-abab-abab-ababfac5562d")
        );
    }

    /// Ground-truth harness of the WHOLE wired service against a scratch V3
    /// mount seeded with a real device's markers — the strongest
    /// pre-hardware validation of the app path (real scanner → gate →
    /// archive read → transcode → cipher → atomic write), byte-compared
    /// against a folder captured from the real device. Env-gated like every
    /// V3 ground-truth test:
    ///
    /// - `RUSTORY_DEVICE_MOUNT_ROOTS` — points the system scanner at the
    ///   scratch mount (set it to the mount path itself);
    /// - `RUSTORY_TEST_SEND_MOUNT` — the scratch mount dir, pre-seeded with
    ///   a REAL `.md` + `.pi` (never a live device!);
    /// - `RUSTORY_TEST_SEND_ZIP` — the source pack archive;
    /// - `RUSTORY_TEST_SEND_UUID` — the expected pack uuid;
    /// - `RUSTORY_TEST_SEND_CONTENT_REF` — the device-truth
    ///   `.content/<SHORTID>` capture to byte-compare against.
    #[test]
    #[ignore]
    fn sends_a_real_archive_to_a_scratch_v3_mount_and_matches_the_device_truth() {
        use crate::infrastructure::device::{SystemDeviceScanner, SystemDeviceV3PackWriter};

        let mount = PathBuf::from(env_or_skip("RUSTORY_TEST_SEND_MOUNT"));
        let zip = PathBuf::from(env_or_skip("RUSTORY_TEST_SEND_ZIP"));
        let expected_uuid = env_or_skip("RUSTORY_TEST_SEND_UUID");
        let content_ref = PathBuf::from(env_or_skip("RUSTORY_TEST_SEND_CONTENT_REF"));
        assert!(
            std::env::var(crate::infrastructure::device::EXTRA_MOUNT_ROOTS_ENV).is_ok(),
            "point RUSTORY_DEVICE_MOUNT_ROOTS at the scratch mount"
        );

        // 1. The REAL scanner resolves the scratch mount as a supported V3.
        let scanner = SystemDeviceScanner::default();
        let resolved =
            resolve_connected_lunii(&scanner, Duration::from_secs(10)).expect("scan the mount");
        let profile = match resolved.outcome {
            ConnectedLuniiOutcome::Supported(p) => p,
            other => panic!("expected a supported V3 scratch mount, got {other:?}"),
        };

        // 2. The WHOLE service, with the production writer.
        let out = send_archive_to_device(
            &scanner,
            &SystemDeviceV3PackWriter,
            &SendArchiveRequest {
                device_identifier: profile.device_identifier,
                archive_path: zip,
            },
            Duration::from_secs(300),
        )
        .expect("send the archive");
        assert_eq!(out.pack_uuid, expected_uuid);

        // 3. Byte-compare the written pack against the device-truth capture:
        //    same file set, identical bytes, file by file.
        let written = mount.join(".content").join(&out.short_id);
        let reference = collect_files(&content_ref);
        assert!(!reference.is_empty(), "empty reference capture");
        let produced = collect_files(&written);
        assert_eq!(
            produced.keys().collect::<Vec<_>>(),
            reference.keys().collect::<Vec<_>>(),
            "file sets differ"
        );
        for (rel, ref_bytes) in &reference {
            assert_eq!(
                produced.get(rel).expect("present"),
                ref_bytes,
                "bytes differ for {rel}"
            );
        }

        // 4. The pack is indexed exactly once (idempotent `.pi` append).
        let pi = std::fs::read(mount.join(".pi")).expect("read .pi");
        let uuid_bytes =
            crate::domain::transfer::pack_uuid_bytes(&out.pack_uuid).expect("uuid bytes");
        let listed = pi
            .chunks_exact(16)
            .filter(|c| *c == uuid_bytes.as_slice())
            .count();
        assert_eq!(listed, 1, "the pack must be listed exactly once in .pi");
    }

    /// Read one required env var of the ground-truth harness (panics with
    /// the setup hint when absent — the test only runs explicitly).
    fn env_or_skip(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("set {name} to run this ground-truth test"))
    }

    /// Recursively collect `rel-path → bytes` of every file under `root`.
    fn collect_files(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).expect("readdir").flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    let rel = p
                        .strip_prefix(root)
                        .expect("under root")
                        .to_string_lossy()
                        .into_owned();
                    out.insert(rel, std::fs::read(&p).expect("read file"));
                }
            }
        }
        let mut out = std::collections::BTreeMap::new();
        walk(root, root, &mut out);
        out
    }
}
