//! Send a pack TO a connected V3 device — the write flow behind the single
//! "Envoyer vers la Lunii" gesture, fed by one of TWO sources:
//!
//! - [`send_archive_to_device`] — a STUdio-format pack archive (`.zip`, the
//!   story's retained source): `story.json` + assets read from the archive;
//! - [`send_story_pack_to_device`] — a LIBRARY story with no archive (created
//!   from a web page, an RSS feed, a folder or the editor): the pack is
//!   SYNTHESIZED from the story's structure (`domain::device::story_pack`)
//!   and its assets read from the node-media store.
//!
//! Both compose the proven V3 engine: authoritative re-scan + `send_archive`
//! gate → [`transcode_pack`] → per-asset DEVICE NORMALIZATION
//! ([`to_device_image`] / [`to_device_audio`]: verbatim when already
//! device-ready, converted/transcoded otherwise, refused when unprovable) →
//! [`assemble_v3_pack`] (with the device `.md`) → [`DeviceV3PackWriter`]. The
//! flow re-keys the ciphering for the TARGET device (its own `.md` content
//! key), so a pack made for one device plays on another. Synchronous by
//! design (the command hands it to `spawn_blocking`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::domain::device::{
    check_device_space, format_device_bytes, transcode_pack, DeviceFamily, DeviceProfile,
    DeviceSpaceVerdict, FirmwareCohort, StudioStoryPack, SupportedOperation, LUNII_PRIMARY_MARKER,
};
use crate::domain::import::MAX_ARCHIVE_MEDIA_BYTES;
use crate::domain::shared::AppError;
use crate::domain::transfer::short_id_from_pack_uuid;
use crate::infrastructure::device::{
    assemble_v3_pack, audio_is_device_ready, to_device_audio, to_device_image, AssembleError,
    AssetConvertError, DeviceScanner, DeviceV3PackWriter, WriteProgress,
};
use crate::infrastructure::filesystem::{read_media, NodeMediaError, WEB_MAX_MEDIA_BYTES};

use super::{check_operation_allowed, resolve_connected_lunii, ConnectedLuniiOutcome};

/// Entry name of the pack descriptor inside a structured archive.
const STORY_JSON_NAME: &str = "story.json";
/// Assets live under this prefix (bare basename is a hand-made-zip fallback).
const ASSETS_PREFIX: &str = "assets/";
/// Byte bound on the descriptor (defensive, generous). A single ASSET is
/// bounded by [`MAX_ARCHIVE_MEDIA_BYTES`] — the very bound the creation flow
/// applied when it admitted the archive, so every media it retained can be
/// read back here.
const MAX_STORY_JSON_BYTES: u64 = 64 * 1024 * 1024;
/// Bound on the archive's entry count.
const MAX_ARCHIVE_ENTRIES: usize = 200_000;
/// Bytes peeked from an audio asset to decide whether it needs transcoding
/// (enough for an ID3v2 tag with cover art plus the first frame header).
const AUDIO_PEEK_BYTES: usize = 4 * 1024 * 1024;

/// The send reports progress over ONE work scale shared by its two measurable
/// phases, weighted by what each actually costs so a big pack never looks
/// frozen and the bar tracks the felt time: reading + normalizing every
/// asset first, then the device write. Per byte of source asset, relative to
/// writing one byte to the device (1.0): reading and normalizing an
/// already-device-ready asset is cheap ([`READ_COST`], the small head the
/// hardware-tuned split used to give assets), TRANSCODING an audio is not
/// ([`TRANSCODE_COST`], measured: a 24 MB AAC episode takes ~7 s to decode,
/// resample and encode, against ~3 s to write its 16 MB result). Images get a
/// fixed decode/quantize allowance ([`IMAGE_COST_BYTES`]). 100 % is reserved
/// for the settled terminal, never the in-flight bar.
const READ_COST: f64 = 0.15;
const TRANSCODE_COST: f64 = 1.75;
const IMAGE_COST_BYTES: f64 = 256.0 * 1024.0;
const PROGRESS_END: u8 = 99;

/// Input of [`send_archive_to_device`]. `device_identifier` is validated at the
/// IPC boundary; `archive_path` is the story's retained source `.zip`.
#[derive(Debug, Clone)]
pub struct SendArchiveRequest {
    pub device_identifier: String,
    pub archive_path: PathBuf,
}

/// Input of [`send_story_pack_to_device`]: the pack SYNTHESIZED from a library
/// story (its asset references are node-media store file names) and the
/// store directory to read them from.
#[derive(Debug, Clone)]
pub struct SendStoryPackRequest {
    pub device_identifier: String,
    pub pack: StudioStoryPack,
    pub media_dir: PathBuf,
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
    on_progress: &dyn Fn(u8),
) -> Result<SentToDevice, AppError> {
    let started = Instant::now();
    let (profile, mount_path) =
        resolve_send_target(scanner, &request.device_identifier, budget, started)?;

    // Read + parse the archive descriptor.
    let mut source = ArchiveSource::open(&request.archive_path)
        .map_err(|failure| archive_error(failure.cause()))?;
    let story_json = source
        .read_descriptor()
        .ok_or_else(|| archive_error("descriptor_missing"))?;
    let pack: StudioStoryPack =
        serde_json::from_slice(&story_json).map_err(|_| archive_error("descriptor_invalid"))?;

    send_pack(
        writer,
        &profile,
        &mount_path,
        &pack,
        &mut source,
        on_progress,
    )
}

pub fn send_story_pack_to_device(
    scanner: &dyn DeviceScanner,
    writer: &dyn DeviceV3PackWriter,
    request: &SendStoryPackRequest,
    budget: Duration,
    on_progress: &dyn Fn(u8),
) -> Result<SentToDevice, AppError> {
    let started = Instant::now();
    let (profile, mount_path) =
        resolve_send_target(scanner, &request.device_identifier, budget, started)?;
    let mut source = MediaStoreSource::new(request.media_dir.clone());
    send_pack(
        writer,
        &profile,
        &mount_path,
        &request.pack,
        &mut source,
        on_progress,
    )
}

/// Steps shared by both sends BEFORE any pack is read: the authoritative
/// re-scan (identity + capability re-proven live) and the fail-closed gate.
fn resolve_send_target(
    scanner: &dyn DeviceScanner,
    device_identifier: &str,
    budget: Duration,
    started: Instant,
) -> Result<(DeviceProfile, PathBuf), AppError> {
    let remaining = budget.saturating_sub(started.elapsed());
    let resolved = resolve_connected_lunii(scanner, remaining)?;
    let (profile, mount_path) = match resolved.outcome {
        ConnectedLuniiOutcome::Supported(profile) => {
            if profile.device_identifier != device_identifier {
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

    // Fail-closed gate BEFORE any device mutation. The DEDICATED archive-send
    // capability (the V3 pack engine) — never `write_story` (the round-trip
    // of an imported pack), so opening one can never open the other.
    check_operation_allowed(&profile, SupportedOperation::SendArchive)?;
    Ok((profile, mount_path))
}

/// Why a source cannot hand out one of the pack's assets. Closed set — the
/// two reasons are never folded together: a media that is THERE but too
/// large is not "missing", and the user's next gesture differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetUnavailable {
    /// Not there, or there but unreadable.
    Missing,
    /// There, but over the byte bound of a single media — never loaded.
    Oversize,
}

/// Where a pack's assets come from. Each source owns the copy of its
/// "unavailable asset" SEND refusals (an archive is a file the user holds, a
/// media store is the library's own). Shared with the story archive export,
/// which reads the very same assets and words its own refusals.
pub(crate) trait PackAssetSource {
    /// Byte size of the asset, without reading it. An asset over the
    /// source's per-media bound is refused HERE already — before any work —
    /// exactly as its read would be.
    fn asset_size(&mut self, name: &str) -> Result<u64, AssetUnavailable>;
    /// Up to `max_bytes` leading bytes of the asset. `None` = absent.
    fn peek_asset(&mut self, name: &str, max_bytes: usize) -> Option<Vec<u8>>;
    /// The whole asset, bounded on the bytes actually read.
    fn read_asset(&mut self, name: &str) -> Result<Vec<u8>, AssetUnavailable>;
    /// The SEND refusal for an asset this source cannot provide.
    fn unavailable_asset_error(&self, name: &str, reason: AssetUnavailable) -> AppError;
}

/// The engine proper, from a parsed pack + its asset source to the device.
fn send_pack(
    writer: &dyn DeviceV3PackWriter,
    profile: &DeviceProfile,
    mount_path: &Path,
    pack: &StudioStoryPack,
    source: &mut dyn PackAssetSource,
    on_progress: &dyn Fn(u8),
) -> Result<SentToDevice, AppError> {
    let pack_uuid = pack_entry_uuid(pack).ok_or_else(|| pack_error("no_entry_node"))?;
    let short_id = short_id_from_pack_uuid(&pack_uuid).ok_or_else(|| pack_error("bad_uuid"))?;

    // Transcode the graph → binary index files + ordered asset lists.
    let transcoded = transcode_pack(pack).map_err(|_| pack_error("transcode"))?;

    // Plan the progress: the cost of every asset (a transcode is what it
    // is BEFORE the work starts — a cheap header peek decides), then the
    // write, on one shared scale.
    let plan = ProgressPlan::new(&transcoded, source)?;

    // The device's free bytes, read ONCE before any work: every room check
    // below compares against this single snapshot. `None` = the platform
    // cannot answer, and the checks stand down rather than refuse a send
    // they cannot justify (the write itself stays the last word).
    let free_space = writer.free_space(mount_path);
    // Running total of the DEVICE-READY asset bytes normalized so far. The
    // assembled pack carries every one of them plus its index files, so this
    // is a genuine LOWER BOUND of what the write will need: the moment it
    // alone exceeds the free space, the send is certainly too big and stops
    // there, instead of transcoding to the end for nothing.
    let mut device_ready_bytes: u64 = 0;

    let mut done_cost = 0.0f64;
    let report = |done: f64| {
        let pct = (done / plan.total * f64::from(PROGRESS_END)).round() as u8;
        on_progress(pct.min(PROGRESS_END));
    };

    // Read every referenced asset and NORMALIZE it to the device format:
    // images → BMP 320×240 4-bit RLE4 (PNG/JPEG sources are converted;
    // already-device-ready BMPs pass verbatim), audio → bare mono 44100 Hz
    // MP3 (ID3 stripped verbatim; m4a / stereo / 48 kHz TRANSCODED;
    // undecodable audio refused BEFORE any device byte). A raw export would
    // otherwise reach the device as files it cannot decode — a blank menu
    // image, then "Error SD Card".
    let mut assets = std::collections::HashMap::new();
    for filename in &transcoded.images {
        if !assets.contains_key(filename) {
            let bytes = source
                .read_asset(filename)
                .map_err(|reason| source.unavailable_asset_error(filename, reason))?;
            let device_ready =
                to_device_image(&bytes).map_err(|e| asset_convert_error(filename, e))?;
            device_ready_bytes += device_ready.len() as u64;
            ensure_device_room(free_space, device_ready_bytes, SpaceProof::AtLeast)?;
            assets.insert(filename.clone(), device_ready);
        }
        done_cost += plan.cost_of(filename);
        report(done_cost);
    }
    for filename in &transcoded.audios {
        if !assets.contains_key(filename) {
            let bytes = source
                .read_asset(filename)
                .map_err(|reason| source.unavailable_asset_error(filename, reason))?;
            let device_ready =
                to_device_audio(&bytes).map_err(|e| asset_convert_error(filename, e))?;
            device_ready_bytes += device_ready.len() as u64;
            ensure_device_room(free_space, device_ready_bytes, SpaceProof::AtLeast)?;
            assets.insert(filename.clone(), device_ready);
        }
        done_cost += plan.cost_of(filename);
        report(done_cost);
    }
    let assets_done = plan.asset_cost;

    // The TARGET device's `.md` (content key + IV + SNU) — re-keys the pack
    // for THIS device.
    let md = std::fs::read(mount_path.join(LUNII_PRIMARY_MARKER))
        .map_err(|_| device_write_error("md_unreadable"))?;

    // Assemble every `.content/<SHORTID>/` file (cleartext + ciphered).
    let files =
        assemble_v3_pack(&transcoded, &md, &|f| assets.get(f).cloned()).map_err(|e| match e {
            AssembleError::UnreadableDeviceMetadata => device_write_error("md_unreadable"),
            AssembleError::MissingAsset(f) => {
                source.unavailable_asset_error(&f, AssetUnavailable::Missing)
            }
        })?;

    // The EXACT room check, on the bytes actually about to be written and
    // BEFORE the first of them. A repeated asset lands on the SAME relative
    // path, so it costs its bytes once. The writer stages the whole pack on
    // the volume before promoting it and only then drops the pack it
    // replaces, so a replacement discounts nothing. This models no
    // filesystem slack (cluster rounding, directory entries), so a pack that
    // fits by a hair can still be refused by the device itself — with its
    // own reason, after this check passed.
    let required_bytes: u64 = {
        let mut counted = std::collections::HashSet::new();
        files
            .iter()
            .filter(|file| counted.insert(file.rel_path.as_str()))
            .map(|file| file.bytes.len() as u64)
            .sum()
    };
    ensure_device_room(free_space, required_bytes, SpaceProof::Exact)?;

    // Write to the device (atomic staging + promotion + `.pi`). The writer's
    // per-file byte progress fills the write share of the scale so the bar
    // keeps moving through the (I/O-bound) device write.
    let write_report = |p: WriteProgress| {
        if p.bytes_total == 0 {
            return;
        }
        let frac = (p.bytes_done as f64 / p.bytes_total as f64).min(1.0);
        report(assets_done + frac * plan.write_cost);
    };
    writer
        .write_pack(mount_path, &pack_uuid, &files, &write_report)
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

/// The send's work plan (see the cost constants): per-asset costs summed
/// into the asset share, plus the write share, on one scale.
struct ProgressPlan {
    costs: std::collections::HashMap<String, f64>,
    asset_cost: f64,
    write_cost: f64,
    total: f64,
}

impl ProgressPlan {
    fn new(
        transcoded: &crate::domain::device::TranscodedPack,
        source: &mut dyn PackAssetSource,
    ) -> Result<Self, AppError> {
        let mut costs = std::collections::HashMap::new();
        let mut asset_cost = 0.0f64;
        let mut write_cost = 0.0f64;
        for filename in &transcoded.images {
            if costs.contains_key(filename) {
                continue;
            }
            let size = source
                .asset_size(filename)
                .map_err(|reason| source.unavailable_asset_error(filename, reason))?
                as f64;
            let cost = size * READ_COST + IMAGE_COST_BYTES;
            costs.insert(filename.clone(), cost);
            asset_cost += cost;
            write_cost += size;
        }
        for filename in &transcoded.audios {
            if costs.contains_key(filename) {
                continue;
            }
            let size = source
                .asset_size(filename)
                .map_err(|reason| source.unavailable_asset_error(filename, reason))?
                as f64;
            let device_ready = source
                .peek_asset(filename, AUDIO_PEEK_BYTES)
                .is_some_and(|head| audio_is_device_ready(&head));
            let cost = size
                * if device_ready {
                    READ_COST
                } else {
                    TRANSCODE_COST
                };
            costs.insert(filename.clone(), cost);
            asset_cost += cost;
            write_cost += size;
        }
        // A pack of index files only still has a (tiny) write.
        let total = (asset_cost + write_cost).max(1.0);
        Ok(Self {
            costs,
            asset_cost,
            write_cost,
            total,
        })
    }

    fn cost_of(&self, filename: &str) -> f64 {
        self.costs.get(filename).copied().unwrap_or(0.0)
    }
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

// ===== Archive source =====

/// The assets (and the descriptor) of a STUdio-format pack archive.
pub(crate) struct ArchiveSource {
    archive: zip::ZipArchive<std::fs::File>,
}

/// Why a pack archive could not be opened. Closed set — the caller words
/// the refusal for its own gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveOpenFailure {
    /// Not a regular readable file.
    Open,
    /// Not a zip archive.
    NotAZip,
    /// More entries than a pack may carry.
    TooManyEntries,
}

impl ArchiveOpenFailure {
    /// Stable diagnostic tag. Closed set.
    pub(crate) const fn cause(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::NotAZip => "not_a_zip",
            Self::TooManyEntries => "too_many_entries",
        }
    }
}

impl ArchiveSource {
    /// Open the pack archive at `path` (a regular file, a zip, within the
    /// entry-count bound).
    pub(crate) fn open(path: &Path) -> Result<Self, ArchiveOpenFailure> {
        let meta = std::fs::symlink_metadata(path).map_err(|_| ArchiveOpenFailure::Open)?;
        if !meta.is_file() {
            return Err(ArchiveOpenFailure::Open);
        }
        let file = std::fs::File::open(path).map_err(|_| ArchiveOpenFailure::Open)?;
        let archive = zip::ZipArchive::new(file).map_err(|_| ArchiveOpenFailure::NotAZip)?;
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            return Err(ArchiveOpenFailure::TooManyEntries);
        }
        Ok(Self { archive })
    }

    /// The pack descriptor (`story.json`), bounded. `None` = absent or
    /// oversize.
    pub(crate) fn read_descriptor(&mut self) -> Option<Vec<u8>> {
        read_entry(&mut self.archive, STORY_JSON_NAME, MAX_STORY_JSON_BYTES).ok()
    }
}

impl PackAssetSource for ArchiveSource {
    fn asset_size(&mut self, name: &str) -> Result<u64, AssetUnavailable> {
        for candidate in entry_candidates(name) {
            if let Ok(entry) = self.archive.by_name(&candidate) {
                if !entry.is_file() {
                    return Err(AssetUnavailable::Missing);
                }
                let size = entry.size();
                return if size > MAX_ARCHIVE_MEDIA_BYTES {
                    Err(AssetUnavailable::Oversize)
                } else {
                    Ok(size)
                };
            }
        }
        Err(AssetUnavailable::Missing)
    }

    fn peek_asset(&mut self, name: &str, max_bytes: usize) -> Option<Vec<u8>> {
        for candidate in entry_candidates(name) {
            if let Ok(mut entry) = self.archive.by_name(&candidate) {
                if !entry.is_file() {
                    return None;
                }
                let mut buf = Vec::new();
                entry
                    .by_ref()
                    .take(max_bytes as u64)
                    .read_to_end(&mut buf)
                    .ok()?;
                return Some(buf);
            }
        }
        None
    }

    fn read_asset(&mut self, name: &str) -> Result<Vec<u8>, AssetUnavailable> {
        read_entry(&mut self.archive, name, MAX_ARCHIVE_MEDIA_BYTES)
    }

    fn unavailable_asset_error(&self, name: &str, reason: AssetUnavailable) -> AppError {
        match reason {
            AssetUnavailable::Missing => archive_asset_error(name),
            AssetUnavailable::Oversize => archive_asset_oversize_error(name),
        }
    }
}

/// `story.json` lives at the root; assets under `assets/`. The plain name is
/// tried first (the descriptor), then the prefixed form.
fn entry_candidates(name: &str) -> [String; 2] {
    [name.to_string(), format!("{ASSETS_PREFIX}{name}")]
}

/// Read one entry (bare `<name>` first, then `assets/<name>`), bounded by
/// `max_bytes` (inclusive). The DECLARED size refuses an honest oversize
/// entry before a single byte of it is inflated; the bound is then
/// re-applied to the bytes actually read, so a lying header gains nothing.
fn read_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, AssetUnavailable> {
    for candidate in entry_candidates(name) {
        let Ok(mut entry) = archive.by_name(&candidate) else {
            continue;
        };
        if !entry.is_file() {
            return Err(AssetUnavailable::Missing);
        }
        if entry.size() > max_bytes {
            return Err(AssetUnavailable::Oversize);
        }
        let mut buf = Vec::new();
        entry
            .by_ref()
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut buf)
            .map_err(|_| AssetUnavailable::Missing)?;
        if buf.len() as u64 > max_bytes {
            return Err(AssetUnavailable::Oversize);
        }
        return Ok(buf);
    }
    Err(AssetUnavailable::Missing)
}

// ===== Media-store source =====

/// Assets of a synthesized story pack: the node-media store's `<hash>.<ext>`
/// files, read through the store's own guarded reader (bare-name check,
/// re-sniff) — a crafted reference can never escape the store directory.
pub(crate) struct MediaStoreSource {
    media_dir: PathBuf,
}

impl MediaStoreSource {
    /// The assets of the node-media store at `media_dir`.
    pub(crate) fn new(media_dir: PathBuf) -> Self {
        Self { media_dir }
    }
}

impl PackAssetSource for MediaStoreSource {
    fn asset_size(&mut self, name: &str) -> Result<u64, AssetUnavailable> {
        if !is_bare_file_name(name) {
            return Err(AssetUnavailable::Missing);
        }
        let size = std::fs::metadata(self.media_dir.join(name))
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .ok_or(AssetUnavailable::Missing)?;
        // The store's own read-back ceiling: nothing it admitted is larger,
        // so only a file altered behind its back lands here.
        if size > WEB_MAX_MEDIA_BYTES as u64 {
            return Err(AssetUnavailable::Oversize);
        }
        Ok(size)
    }

    fn peek_asset(&mut self, name: &str, max_bytes: usize) -> Option<Vec<u8>> {
        if !is_bare_file_name(name) {
            return None;
        }
        let file = std::fs::File::open(self.media_dir.join(name)).ok()?;
        let mut buf = Vec::new();
        file.take(max_bytes as u64).read_to_end(&mut buf).ok()?;
        Some(buf)
    }

    fn read_asset(&mut self, name: &str) -> Result<Vec<u8>, AssetUnavailable> {
        match read_media(&self.media_dir, name) {
            Ok((bytes, _)) => Ok(bytes),
            Err(NodeMediaError::Oversize) => Err(AssetUnavailable::Oversize),
            Err(_) => Err(AssetUnavailable::Missing),
        }
    }

    fn unavailable_asset_error(&self, name: &str, reason: AssetUnavailable) -> AppError {
        match reason {
            AssetUnavailable::Missing => media_asset_error(name),
            AssetUnavailable::Oversize => media_asset_oversize_error(name),
        }
    }
}

/// A bare file name: no path separators, no parent reference.
fn is_bare_file_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")
}

// ===== Errors =====

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

/// The pack's graph cannot become a device pack (no entry node, a dangling
/// reference, a non-canonical uuid). For an archive that is a malformed
/// descriptor; for a synthesized story pack it cannot happen by
/// construction — kept fail-closed under the archive copy.
fn pack_error(cause: &'static str) -> AppError {
    archive_error(cause)
}

fn archive_asset_error(filename: &str) -> AppError {
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

/// A media of the pack IS in the archive but weighs more than a single
/// media may ([`MAX_ARCHIVE_MEDIA_BYTES`]) — never loaded, and never worded
/// as an absence: the archive is intact, one of its files is too large.
fn archive_asset_oversize_error(filename: &str) -> AppError {
    AppError::device_write_failed(
        format!(
            "Envoi impossible: un média du pack dépasse la taille autorisée ({}).",
            format_device_bytes(MAX_ARCHIVE_MEDIA_BYTES)
        ),
        "Dans l'archive d'origine, remplace ce média par un fichier plus léger, réimporte l'archive dans Rustory, puis réessaie l'envoi.",
    )
    .with_details(serde_json::json!({
        "source": "archive",
        "cause": "asset_oversize",
        "max_bytes": MAX_ARCHIVE_MEDIA_BYTES,
        // Only the device basename (8 hex), never a path.
        "asset": crate::domain::device::pack_transcode::device_asset_basename(filename),
    }))
}

/// A media the story's structure references is not in the library's store
/// (removed, or the store is damaged) — the send never reaches the device.
fn media_asset_error(filename: &str) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: un média de l'histoire est introuvable dans la bibliothèque.",
        "Rouvre l'histoire pour vérifier ses épisodes (audio et image), puis réessaie l'envoi.",
    )
    .with_details(serde_json::json!({
        "source": "media_store",
        "cause": "asset_missing",
        // Only the device basename (8 hex), never a path.
        "asset": crate::domain::device::pack_transcode::device_asset_basename(filename),
    }))
}

/// A media of the library's store weighs more than the store itself admits
/// (a file altered behind its back) — never loaded, and named for what it
/// is rather than as a missing media.
fn media_asset_oversize_error(filename: &str) -> AppError {
    AppError::device_write_failed(
        format!(
            "Envoi impossible: un média de l'histoire dépasse la taille autorisée ({}).",
            format_device_bytes(WEB_MAX_MEDIA_BYTES as u64)
        ),
        "Rouvre l'histoire pour remplacer ce média par un fichier plus léger, puis réessaie l'envoi.",
    )
    .with_details(serde_json::json!({
        "source": "media_store",
        "cause": "asset_oversize",
        "max_bytes": WEB_MAX_MEDIA_BYTES as u64,
        // Only the device basename (8 hex), never a path.
        "asset": crate::domain::device::pack_transcode::device_asset_basename(filename),
    }))
}

/// A media exists but cannot be made device-playable (an undecodable image,
/// or audio no decoder recognizes). Refused BEFORE any device byte: sent
/// as-is it would fail ON the device as an opaque "Error SD Card".
fn asset_convert_error(filename: &str, err: AssetConvertError) -> AppError {
    AppError::device_write_failed(
        "Envoi impossible: un média n'est pas dans un format lisible par l'appareil.",
        "Vérifie que les images sont des PNG/JPEG/BMP valides et les audios des fichiers MP3, M4A, WAV ou OGG lisibles, puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "asset_convert",
        "cause": err.diagnostic_tag(),
        // Only the device basename (8 hex), never a path.
        "asset": crate::domain::device::pack_transcode::device_asset_basename(filename),
    }))
}

/// How well the caller knows the byte need it is checking: the running
/// lower bound during asset normalization, or the assembled pack's exact
/// size. Only the wording differs — both refuse before any device byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpaceProof {
    AtLeast,
    Exact,
}

/// Refuse the send when the device cannot hold `required` bytes. An unknown
/// free space (`None`) never refuses.
fn ensure_device_room(
    free_space: Option<u64>,
    required: u64,
    proof: SpaceProof,
) -> Result<(), AppError> {
    let Some(available) = free_space else {
        return Ok(());
    };
    match check_device_space(required, available) {
        DeviceSpaceVerdict::Fits => Ok(()),
        DeviceSpaceVerdict::Short { missing } => {
            Err(not_enough_space_error(required, available, missing, proof))
        }
    }
}

/// The device has less free space than the pack needs — refused with the
/// missing amount named, BEFORE any device byte. `details` carries the raw
/// figures for support; the message carries the one the user acts on.
fn not_enough_space_error(
    required: u64,
    available: u64,
    missing: u64,
    proof: SpaceProof,
) -> AppError {
    let missing_text = format_device_bytes(missing);
    let message = match proof {
        SpaceProof::Exact => {
            format!("Envoi impossible: il manque {missing_text} d'espace libre sur l'appareil.")
        }
        SpaceProof::AtLeast => {
            format!(
                "Envoi impossible: il manque au moins {missing_text} d'espace libre sur l'appareil."
            )
        }
    };
    AppError::device_write_failed(
        message,
        "Supprime une histoire de l'appareil puis relance l'envoi.",
    )
    .with_details(serde_json::json!({
        "source": "device_space",
        "cause": "not_enough_space",
        "exact": proof == SpaceProof::Exact,
        "required_bytes": required,
        "available_bytes": available,
        "missing_bytes": missing,
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
    /// asked to write, the deduplicated byte total it would land on the
    /// volume, and answers a programmable free space (`None` by default —
    /// unknown, so the room checks stand down).
    #[derive(Default)]
    struct RecordingWriter {
        calls: std::sync::Mutex<Vec<(String, usize)>>,
        written_bytes: std::sync::Mutex<u64>,
        free_space: Option<u64>,
    }
    impl RecordingWriter {
        fn with_free_space(bytes: u64) -> Self {
            Self {
                free_space: Some(bytes),
                ..Self::default()
            }
        }
    }
    impl DeviceV3PackWriter for RecordingWriter {
        fn free_space(&self, _mount: &Path) -> Option<u64> {
            self.free_space
        }

        fn write_pack(
            &self,
            _mount: &Path,
            pack_uuid: &str,
            files: &[crate::infrastructure::device::AssembledFile],
            progress: &dyn Fn(WriteProgress),
        ) -> Result<(), crate::domain::transfer::TransferFailureCause> {
            self.calls
                .lock()
                .unwrap()
                .push((pack_uuid.to_string(), files.len()));
            // What the volume actually gains: a repeated asset lands on the
            // same relative path, so it counts once.
            let mut counted = std::collections::HashSet::new();
            *self.written_bytes.lock().unwrap() = files
                .iter()
                .filter(|f| counted.insert(f.rel_path.clone()))
                .map(|f| f.bytes.len() as u64)
                .sum();
            // Report a complete write, like the real writer's last tick.
            let total: u64 = files.iter().map(|f| f.bytes.len() as u64).sum();
            progress(WriteProgress {
                bytes_done: total,
                bytes_total: total,
            });
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
        let err =
            send_archive_to_device(&scanner, &writer, &req, Duration::from_millis(200), &|_| {})
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
        let err =
            send_archive_to_device(&scanner, &writer, &req, Duration::from_millis(200), &|_| {})
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
        let err =
            send_archive_to_device(&scanner, &writer, &req, Duration::from_millis(200), &|_| {})
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
            &|_| {},
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

    /// Ground-truth harness of the WHOLE REWORKED path (single entry point):
    /// import a `.zip` via `accept_structured_archive_creation` (which RETAINS
    /// the source archive), then RESOLVE that retained archive by story id (as
    /// the `send_pack_to_device` command does) and send it to the real device.
    /// Proves retention → resolution → transcode → cipher → write end-to-end.
    /// Same env gating, plus the import happens into a fresh temp DB +
    /// app_data_dir (no picker, no library dependency).
    ///
    /// - `RUSTORY_DEVICE_MOUNT_ROOTS` — points the scanner at the real mount;
    /// - `RUSTORY_TEST_SEND_MOUNT` — the mount dir (byte-compare target);
    /// - `RUSTORY_TEST_SEND_ZIP` — the source pack archive to import + send;
    /// - `RUSTORY_TEST_SEND_UUID` — the expected pack uuid;
    /// - `RUSTORY_TEST_SEND_CONTENT_REF` — the device-truth `.content/<SHORTID>`.
    #[test]
    #[ignore]
    fn imports_with_retention_then_sends_the_retained_archive_matching_device_truth() {
        use crate::application::import_export::archive_creation::accept_structured_archive_creation;
        use crate::infrastructure::db::{open_in_memory, run_migrations};
        use crate::infrastructure::device::{SystemDeviceScanner, SystemDeviceV3PackWriter};
        use crate::infrastructure::filesystem::resolve_source_archive_path;

        let mount = PathBuf::from(env_or_skip("RUSTORY_TEST_SEND_MOUNT"));
        let zip = PathBuf::from(env_or_skip("RUSTORY_TEST_SEND_ZIP"));
        let expected_uuid = env_or_skip("RUSTORY_TEST_SEND_UUID");
        let content_ref = PathBuf::from(env_or_skip("RUSTORY_TEST_SEND_CONTENT_REF"));
        assert!(
            std::env::var(crate::infrastructure::device::EXTRA_MOUNT_ROOTS_ENV).is_ok(),
            "point RUSTORY_DEVICE_MOUNT_ROOTS at the mount"
        );

        // 1. Import the archive → the story is committed AND its source `.zip`
        //    is retained (the rework). A fresh temp app_data_dir + DB.
        let app_data = tempfile::tempdir().expect("app_data");
        let mut db = open_in_memory().expect("open db");
        run_migrations(&mut db).expect("migrate");
        let card = accept_structured_archive_creation(&mut db, app_data.path(), &zip)
            .expect("import the archive");
        assert!(card.sendable, "an imported archive must be V3-sendable");

        // 2. Resolve the retained archive by story id — EXACTLY what the
        //    `send_pack_to_device` command does (no path from the UI).
        let retained = resolve_source_archive_path(app_data.path(), &card.id);
        assert!(retained.is_file(), "the source archive must be retained");

        // 3. Send the RETAINED archive to the real device via the whole service.
        let scanner = SystemDeviceScanner::default();
        let profile = match resolve_connected_lunii(&scanner, Duration::from_secs(10))
            .expect("scan")
            .outcome
        {
            ConnectedLuniiOutcome::Supported(p) => p,
            other => panic!("expected a supported V3, got {other:?}"),
        };
        let out = send_archive_to_device(
            &scanner,
            &SystemDeviceV3PackWriter,
            &SendArchiveRequest {
                device_identifier: profile.device_identifier,
                archive_path: retained,
            },
            Duration::from_secs(300),
            &|_| {},
        )
        .expect("send the retained archive");
        assert_eq!(out.pack_uuid, expected_uuid);

        // 4. Byte-compare the written pack against the device-truth capture.
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

        // 5. The pack is listed exactly once in `.pi` (repairs the orphan).
        let pi = std::fs::read(mount.join(".pi")).expect("read .pi");
        let uuid_bytes =
            crate::domain::transfer::pack_uuid_bytes(&out.pack_uuid).expect("uuid bytes");
        let listed = pi
            .chunks_exact(16)
            .filter(|c| *c == uuid_bytes.as_slice())
            .count();
        assert_eq!(listed, 1, "the pack must be listed exactly once in .pi");
    }

    // ===== Synthesized story pack (no archive) =====

    /// A scratch V3 mount: a 256-byte v7 `.md` (version byte 7, a
    /// deterministic key/IV/SNU region) + `.pi` + `.bt`, and a mock scanner
    /// report pointing at it — enough for the whole engine (gate → assemble
    /// with the mount's `.md` → writer) without hardware.
    fn scratch_v3_mount() -> (tempfile::TempDir, MockDeviceScanner, String) {
        let dir = tempfile::tempdir().expect("mount dir");
        let (scanner, identifier) = scratch_v3_mount_at(dir.path());
        (dir, scanner, identifier)
    }

    /// Seed (or re-seed the markers of) a V3 scratch mount at `root` and a
    /// mock scanner report pointing at it. The mock identity is derived from
    /// the report's `.pi` bytes + serial, like the production scanner's.
    fn scratch_v3_mount_at(root: &Path) -> (MockDeviceScanner, String) {
        use crate::domain::device::{LUNII_BINARY_TOKEN_MARKER, LUNII_DEVICE_ID_MARKER};
        use crate::infrastructure::device::{CandidateFacts, DeviceCandidate, DeviceScanReport};

        let mut md = vec![0u8; 256];
        md[0] = V3_METADATA_VERSION;
        for (i, b) in md.iter_mut().enumerate().skip(0x1A).take(14) {
            *b = b"0123456789abcd"[i - 0x1A];
        }
        for (i, b) in md.iter_mut().enumerate().skip(0x40).take(32) {
            *b = i as u8;
        }
        std::fs::write(root.join(LUNII_PRIMARY_MARKER), &md).expect(".md");
        // An EMPTY `.pi` on first seeding (a whole number of 16-byte entries —
        // the real writer refuses a fragment); the scanner report carries the
        // identity bytes. A re-seed keeps the `.pi` the writer appended to.
        if !root.join(LUNII_DEVICE_ID_MARKER).exists() {
            std::fs::write(root.join(LUNII_DEVICE_ID_MARKER), b"").expect(".pi");
        }
        std::fs::write(root.join(LUNII_BINARY_TOKEN_MARKER), b"bt").expect(".bt");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue(Ok(DeviceScanReport {
            candidates: vec![DeviceCandidate {
                mount_path: root.to_path_buf(),
                volume_serial: Some("MOCK_SERIAL".into()),
                facts: CandidateFacts::Lunii {
                    metadata_payload: md,
                    pi_payload: b"MOCK_PI".to_vec(),
                    has_bt: true,
                },
            }],
            elapsed: Duration::from_millis(2),
            truncated_due_to_timeout: false,
        }));
        (scanner, mock_identifier())
    }

    /// A media store with one 48 kHz stereo WAV (needs transcoding) and one
    /// PNG, named the way the store names them (`<hash>.<ext>`).
    fn scratch_media_store() -> (tempfile::TempDir, String, String) {
        use crate::infrastructure::device::audio_transcode::test_support::wav_sine;
        let dir = tempfile::tempdir().expect("media dir");
        let wav = wav_sine(48_000, 2, 440.0, 1.0, 0.5);
        let audio_name = format!("{}.wav", "a".repeat(56) + "1234abcd");
        std::fs::write(dir.path().join(&audio_name), wav).expect("wav");
        let img = image::RgbaImage::from_fn(64, 48, |x, _| image::Rgba([x as u8 * 4, 0, 0, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("png");
        let image_name = format!("{}.png", "b".repeat(56) + "5678ef01");
        std::fs::write(dir.path().join(&image_name), png.into_inner()).expect("png");
        (dir, audio_name, image_name)
    }

    const STORY_ID: &str = "01a06ed9-2040-77c1-9e03-b8f429f4e954";

    #[test]
    fn sends_a_synthesized_story_pack_transcoding_its_audio_and_reporting_progress() {
        use crate::domain::device::{synthesize_sequential_pack, EpisodeAssets};

        let (_mount, scanner, device_identifier) = scratch_v3_mount();
        let (media, audio_name, image_name) = scratch_media_store();
        let pack = synthesize_sequential_pack(
            STORY_ID,
            &[
                EpisodeAssets {
                    audio_ref: audio_name.clone(),
                    image_ref: Some(image_name.clone()),
                },
                EpisodeAssets {
                    audio_ref: audio_name.clone(),
                    image_ref: None,
                },
            ],
        );
        let writer = RecordingWriter::default();
        let progress = std::sync::Mutex::new(Vec::<u8>::new());
        let out = send_story_pack_to_device(
            &scanner,
            &writer,
            &SendStoryPackRequest {
                device_identifier,
                pack,
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_millis(500),
            &|pct| progress.lock().unwrap().push(pct),
        )
        .expect("send the story pack");

        // The pack is the story: its uuid, short id, deduplicated assets.
        assert_eq!(out.pack_uuid, STORY_ID);
        assert_eq!(out.short_id, "29F4E954");
        assert_eq!((out.image_count, out.audio_count), (1, 1));
        // One write of the full file set: ni + bt + li/ri/si + 1 image + 1 audio.
        let calls = writer.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], (STORY_ID.to_string(), 7));
        // Progress is monotonic, never claims 100 %, and reaches the write.
        let progress = progress.lock().unwrap();
        assert!(!progress.is_empty());
        assert!(progress.windows(2).all(|w| w[0] <= w[1]), "{progress:?}");
        assert!(progress.iter().all(|p| *p <= 99));
        assert_eq!(
            *progress.last().unwrap(),
            99,
            "the recorded write completes"
        );
    }

    /// The two-episode synthesized pack the room-check tests send.
    fn scratch_story_pack(
        audio_name: &str,
        image_name: &str,
    ) -> crate::domain::device::StudioStoryPack {
        use crate::domain::device::{synthesize_sequential_pack, EpisodeAssets};
        synthesize_sequential_pack(
            STORY_ID,
            &[
                EpisodeAssets {
                    audio_ref: audio_name.to_string(),
                    image_ref: Some(image_name.to_string()),
                },
                EpisodeAssets {
                    audio_ref: audio_name.to_string(),
                    image_ref: None,
                },
            ],
        )
    }

    #[test]
    fn a_device_one_byte_short_refuses_the_send_before_any_device_byte() {
        let (media, audio_name, image_name) = scratch_media_store();

        // A first send with an UNKNOWN free space: no room check runs (the
        // send succeeds exactly as before), and the writer tells us what the
        // assembled pack weighs on the volume. Each send gets its own mock
        // scan (the scanner answers one re-scan per send).
        let (_mount, scanner, device_identifier) = scratch_v3_mount();
        let writer = RecordingWriter::default();
        send_story_pack_to_device(
            &scanner,
            &writer,
            &SendStoryPackRequest {
                device_identifier,
                pack: scratch_story_pack(&audio_name, &image_name),
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect("an unknown free space never refuses");
        let needed = *writer.written_bytes.lock().unwrap();
        assert!(needed > 0, "the pack weighs something");

        // The SAME send onto a device one byte short: refused, with the exact
        // figures, and not a single byte written.
        let (_mount, scanner, device_identifier) = scratch_v3_mount();
        let writer = RecordingWriter::with_free_space(needed - 1);
        let err = send_story_pack_to_device(
            &scanner,
            &writer,
            &SendStoryPackRequest {
                device_identifier,
                pack: scratch_story_pack(&audio_name, &image_name),
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect_err("a device without room refuses");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_WRITE_FAILED");
        assert_eq!(
            v["message"],
            "Envoi impossible: il manque moins de 1 Mo d'espace libre sur l'appareil."
        );
        assert_eq!(
            v["userAction"],
            "Supprime une histoire de l'appareil puis relance l'envoi."
        );
        assert_eq!(v["details"]["source"], "device_space");
        assert_eq!(v["details"]["cause"], "not_enough_space");
        assert_eq!(v["details"]["exact"], true);
        assert_eq!(v["details"]["required_bytes"], needed);
        assert_eq!(v["details"]["available_bytes"], needed - 1);
        assert_eq!(v["details"]["missing_bytes"], 1);
        assert!(
            writer.calls.lock().unwrap().is_empty(),
            "zero device write on a refusal"
        );
    }

    #[test]
    fn a_device_far_too_small_stops_during_the_asset_work_instead_of_transcoding_to_the_end() {
        let (_mount, scanner, device_identifier) = scratch_v3_mount();
        let (media, audio_name, image_name) = scratch_media_store();

        // 32 free bytes: the FIRST normalized asset alone already exceeds
        // them. The lower-bound check fires inside the asset loop, so the
        // remaining assets are never read nor transcoded, and the pack is
        // never assembled — `exact: false` is that proof.
        let writer = RecordingWriter::with_free_space(32);
        let err = send_story_pack_to_device(
            &scanner,
            &writer,
            &SendStoryPackRequest {
                device_identifier,
                pack: scratch_story_pack(&audio_name, &image_name),
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect_err("a device far too small refuses");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_WRITE_FAILED");
        assert_eq!(v["details"]["source"], "device_space");
        assert_eq!(v["details"]["exact"], false);
        assert_eq!(v["details"]["available_bytes"], 32);
        let message = v["message"].as_str().expect("message");
        assert!(
            message.starts_with("Envoi impossible: il manque au moins "),
            "the early refusal never overstates what it knows: {message}"
        );
        assert!(
            writer.calls.lock().unwrap().is_empty(),
            "zero device write on a refusal"
        );
    }

    #[test]
    fn a_device_with_room_to_spare_sends_normally() {
        let (_mount, scanner, device_identifier) = scratch_v3_mount();
        let (media, audio_name, image_name) = scratch_media_store();
        let writer = RecordingWriter::with_free_space(8_000_000_000);
        send_story_pack_to_device(
            &scanner,
            &writer,
            &SendStoryPackRequest {
                device_identifier,
                pack: scratch_story_pack(&audio_name, &image_name),
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect("a device with room sends");
        assert_eq!(writer.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn writes_a_synthesized_story_pack_onto_a_scratch_mount_with_the_real_writer() {
        use crate::domain::device::{synthesize_sequential_pack, EpisodeAssets};
        use crate::infrastructure::device::SystemDeviceV3PackWriter;

        let (mount, scanner, device_identifier) = scratch_v3_mount();
        let (media, audio_name, image_name) = scratch_media_store();
        let pack = synthesize_sequential_pack(
            STORY_ID,
            &[EpisodeAssets {
                audio_ref: audio_name.clone(),
                image_ref: Some(image_name),
            }],
        );
        let out = send_story_pack_to_device(
            &scanner,
            &SystemDeviceV3PackWriter,
            &SendStoryPackRequest {
                device_identifier,
                pack,
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect("send with the real writer");

        // The device layout: `.content/<SHORTID>/` with the index files, the
        // forged `bt`, and the assets under their 8-hex basenames.
        let content = mount.path().join(".content").join(&out.short_id);
        for rel in [
            "ni",
            "li",
            "ri",
            "si",
            "bt",
            "rf/000/5678EF01",
            "sf/000/1234ABCD",
        ] {
            assert!(content.join(rel).is_file(), "missing {rel}");
        }
        // The written audio is the transcoded device MP3 (1 s at 128 kbps ≈
        // 16 KB), far past the ciphered first 512 bytes.
        let audio = std::fs::read(content.join("sf/000/1234ABCD")).expect("audio");
        assert!(audio.len() > 12_000, "audio len {}", audio.len());
        // The pack is indexed exactly once in `.pi`.
        let pi = std::fs::read(mount.path().join(".pi")).expect(".pi");
        let uuid_bytes = crate::domain::transfer::pack_uuid_bytes(STORY_ID).expect("uuid");
        assert_eq!(pi.chunks_exact(16).filter(|c| *c == uuid_bytes).count(), 1);

        // Re-sending the same story REPLACES its pack (same uuid): still one
        // `.pi` entry, and the previous content (its image) is gone.
        let (scanner2, device_identifier2) = scratch_v3_mount_at(mount.path());
        send_story_pack_to_device(
            &scanner2,
            &SystemDeviceV3PackWriter,
            &SendStoryPackRequest {
                device_identifier: device_identifier2,
                pack: synthesize_sequential_pack(
                    STORY_ID,
                    &[EpisodeAssets {
                        audio_ref: audio_name,
                        image_ref: None,
                    }],
                ),
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect("re-send");
        let pi = std::fs::read(mount.path().join(".pi")).expect(".pi");
        assert_eq!(pi.chunks_exact(16).filter(|c| *c == uuid_bytes).count(), 1);
        assert!(
            !content.join("rf/000/5678EF01").exists(),
            "the replaced pack's image is gone"
        );
        assert!(content.join("sf/000/1234ABCD").is_file());
    }

    #[test]
    fn a_story_pack_whose_media_is_gone_is_refused_before_the_writer_is_touched() {
        use crate::domain::device::{synthesize_sequential_pack, EpisodeAssets};

        let (_mount, scanner, device_identifier) = scratch_v3_mount();
        let media = tempfile::tempdir().expect("empty store");
        let pack = synthesize_sequential_pack(
            STORY_ID,
            &[EpisodeAssets {
                audio_ref: format!("{}.mp3", "c".repeat(64)),
                image_ref: None,
            }],
        );
        let writer = RecordingWriter::default();
        let err = send_story_pack_to_device(
            &scanner,
            &writer,
            &SendStoryPackRequest {
                device_identifier,
                pack,
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect_err("missing media refuses");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], "DEVICE_WRITE_FAILED");
        assert_eq!(v["details"]["source"], "media_store");
        assert_eq!(v["details"]["cause"], "asset_missing");
        assert_eq!(v["details"]["asset"], "CCCCCCCC", "device basename only");
        assert!(
            writer.calls.lock().unwrap().is_empty(),
            "no write attempted"
        );
    }

    #[test]
    fn a_story_pack_asset_reference_can_never_escape_the_media_store() {
        use crate::domain::device::{synthesize_sequential_pack, EpisodeAssets};

        let (_mount, scanner, device_identifier) = scratch_v3_mount();
        let media = tempfile::tempdir().expect("store");
        // A file OUTSIDE the store that a crafted reference would point at.
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("secret.mp3"), b"x").expect("outside file");
        let escaped = format!(
            "../{}/secret.mp3",
            outside.path().file_name().unwrap().to_string_lossy()
        );
        let pack = synthesize_sequential_pack(
            STORY_ID,
            &[EpisodeAssets {
                audio_ref: escaped,
                image_ref: None,
            }],
        );
        let writer = RecordingWriter::default();
        let err = send_story_pack_to_device(
            &scanner,
            &writer,
            &SendStoryPackRequest {
                device_identifier,
                pack,
                media_dir: media.path().join("node-media"),
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect_err("escaping reference refuses");
        assert_eq!(
            serde_json::to_value(&err).unwrap()["details"]["cause"],
            "asset_missing"
        );
        assert!(writer.calls.lock().unwrap().is_empty());
    }

    // ===== Per-media bound: too large is never "missing" =====

    /// A one-stage pack descriptor whose only stage plays `audio`.
    fn one_sound_descriptor(audio: &str) -> String {
        format!(
            r#"{{"format":"v1","version":1,"nightModeAvailable":false,"actionNodes":[],
            "stageNodes":[{{"uuid":"{STORY_ID}","squareOne":true,"image":null,"audio":"{audio}",
              "okTransition":null,"homeTransition":null,
              "controlSettings":{{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}}}]}}"#
        )
    }

    #[test]
    fn an_archive_media_is_read_up_to_the_bound_included_and_refused_one_byte_over() {
        use crate::application::import_export::archive_creation::test_support::write_pack_archive;

        let dir = tempfile::tempdir().expect("dir");
        let zip = dir.path().join("pack.zip");
        write_pack_archive(
            &zip,
            None,
            &[
                ("limite.mp3", MAX_ARCHIVE_MEDIA_BYTES),
                ("trop.mp3", MAX_ARCHIVE_MEDIA_BYTES + 1),
            ],
        );
        let mut source = ArchiveSource::open(&zip).expect("open the pack");

        // Exactly the bound: sized, then read whole.
        assert_eq!(source.asset_size("limite.mp3"), Ok(MAX_ARCHIVE_MEDIA_BYTES));
        let read = source.read_asset("limite.mp3").expect("read at the bound");
        assert_eq!(read.len() as u64, MAX_ARCHIVE_MEDIA_BYTES);
        drop(read);

        // One byte more: THERE, but too large — known when the asset is
        // sized (before any work), and again when it is read.
        assert_eq!(
            source.asset_size("trop.mp3"),
            Err(AssetUnavailable::Oversize)
        );
        assert_eq!(
            source.read_asset("trop.mp3"),
            Err(AssetUnavailable::Oversize)
        );

        // Not in the archive at all: missing — a different reason.
        assert_eq!(
            source.asset_size("absent.mp3"),
            Err(AssetUnavailable::Missing)
        );
        assert_eq!(
            source.read_asset("absent.mp3"),
            Err(AssetUnavailable::Missing)
        );
    }

    #[test]
    fn a_pack_archive_sound_of_exactly_the_bound_is_sent_whole() {
        use crate::application::import_export::archive_creation::test_support::write_pack_archive;

        // The largest sound the creation admits goes through the WHOLE
        // engine: sized, read, normalized (already a device sound — carried
        // as is), assembled and handed to the writer.
        let audio = format!("{}.mp3", "d".repeat(32) + "0badf00d");
        let dir = tempfile::tempdir().expect("dir");
        let zip = dir.path().join("long-son.zip");
        write_pack_archive(
            &zip,
            Some(&one_sound_descriptor(&audio)),
            &[(audio.as_str(), MAX_ARCHIVE_MEDIA_BYTES)],
        );
        let (_mount, scanner, device_identifier) = scratch_v3_mount();
        let writer = RecordingWriter::default();
        let sent = send_archive_to_device(
            &scanner,
            &writer,
            &SendArchiveRequest {
                device_identifier,
                archive_path: zip,
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect("a sound of exactly the bound is sent");
        assert_eq!(sent.pack_uuid, STORY_ID);
        assert_eq!((sent.image_count, sent.audio_count), (0, 1));
        assert_eq!(writer.calls.lock().unwrap().len(), 1, "one device write");
        assert!(
            *writer.written_bytes.lock().unwrap() >= MAX_ARCHIVE_MEDIA_BYTES,
            "the whole sound reaches the device"
        );
    }

    #[test]
    fn a_pack_archive_media_over_the_bound_is_refused_as_too_large_and_a_missing_one_as_missing() {
        use crate::application::import_export::archive_creation::test_support::write_pack_archive;

        let audio = format!("{}.mp3", "d".repeat(32) + "0badf00d");
        let dir = tempfile::tempdir().expect("dir");
        let send = |zip: PathBuf| {
            let (_mount, scanner, device_identifier) = scratch_v3_mount();
            let writer = RecordingWriter::default();
            let err = send_archive_to_device(
                &scanner,
                &writer,
                &SendArchiveRequest {
                    device_identifier,
                    archive_path: zip,
                },
                Duration::from_millis(500),
                &|_| {},
            )
            .expect_err("the pack must be refused");
            assert!(
                writer.calls.lock().unwrap().is_empty(),
                "zero device write on a refusal"
            );
            serde_json::to_value(&err).expect("ser")
        };

        // The media is in the archive, one byte over the bound.
        let oversize = dir.path().join("trop-gros.zip");
        write_pack_archive(
            &oversize,
            Some(&one_sound_descriptor(&audio)),
            &[(audio.as_str(), MAX_ARCHIVE_MEDIA_BYTES + 1)],
        );
        let v = send(oversize);
        assert_eq!(v["code"], "DEVICE_WRITE_FAILED");
        assert_eq!(v["details"]["source"], "archive");
        assert_eq!(v["details"]["cause"], "asset_oversize");
        assert_eq!(v["details"]["max_bytes"], MAX_ARCHIVE_MEDIA_BYTES);
        assert_eq!(v["details"]["asset"], "0BADF00D", "device basename only");
        assert_eq!(
            v["message"],
            "Envoi impossible: un média du pack dépasse la taille autorisée (134 Mo)."
        );
        assert!(v["userAction"]
            .as_str()
            .is_some_and(|action| action.contains("plus léger")));

        // The media is not in the archive: the absence keeps its own words.
        let missing = dir.path().join("incomplet.zip");
        write_pack_archive(&missing, Some(&one_sound_descriptor(&audio)), &[]);
        let v = send(missing);
        assert_eq!(v["details"]["source"], "archive");
        assert_eq!(v["details"]["cause"], "asset_missing");
        assert_eq!(
            v["message"],
            "Envoi impossible: un média du pack est introuvable dans l'archive."
        );
    }

    #[test]
    fn a_store_media_over_the_store_ceiling_is_refused_as_too_large_before_any_read() {
        use crate::domain::device::{synthesize_sequential_pack, EpisodeAssets};

        // Nothing the store admitted is larger than its ceiling: only a file
        // altered behind its back gets there. A sparse file (instant) that
        // carries a sound signature, one byte over.
        let media = tempfile::tempdir().expect("store");
        let name = format!("{}.mp3", "e".repeat(56) + "0badcafe");
        let path = media.path().join(&name);
        std::fs::write(&path, [0xFF, 0xFB, 0x90, 0xC0]).expect("sound header");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open")
            .set_len(WEB_MAX_MEDIA_BYTES as u64 + 1)
            .expect("set_len");

        let mut source = MediaStoreSource::new(media.path().to_path_buf());
        assert_eq!(source.asset_size(&name), Err(AssetUnavailable::Oversize));
        assert_eq!(source.read_asset(&name), Err(AssetUnavailable::Oversize));

        let (_mount, scanner, device_identifier) = scratch_v3_mount();
        let writer = RecordingWriter::default();
        let err = send_story_pack_to_device(
            &scanner,
            &writer,
            &SendStoryPackRequest {
                device_identifier,
                pack: synthesize_sequential_pack(
                    STORY_ID,
                    &[EpisodeAssets {
                        audio_ref: name,
                        image_ref: None,
                    }],
                ),
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_millis(500),
            &|_| {},
        )
        .expect_err("an oversize store media refuses");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_WRITE_FAILED");
        assert_eq!(v["details"]["source"], "media_store");
        assert_eq!(v["details"]["cause"], "asset_oversize");
        assert_eq!(v["details"]["asset"], "0BADCAFE");
        assert_eq!(
            v["message"],
            "Envoi impossible: un média de l'histoire dépasse la taille autorisée (134 Mo)."
        );
        assert!(
            writer.calls.lock().unwrap().is_empty(),
            "no write attempted"
        );
    }

    /// Timing harness of the WHOLE story-pack path on a REAL episode file,
    /// without hardware: a scratch V3 mount + the production writer, a story
    /// of `RUSTORY_TEST_STORY_EPISODES` (default 3) episodes all backed by
    /// the audio at `RUSTORY_TEST_STORY_AUDIO` (an m4a / mp3 / wav / ogg as
    /// the media store holds it). Prints the elapsed time per phase — the
    /// figure behind the progress-plan cost constants. Env-gated, ignored.
    #[test]
    #[ignore]
    fn sends_a_real_episode_story_pack_to_a_scratch_mount_and_reports_timing() {
        use crate::domain::device::{synthesize_sequential_pack, EpisodeAssets};
        use crate::infrastructure::device::SystemDeviceV3PackWriter;

        let audio = PathBuf::from(env_or_skip("RUSTORY_TEST_STORY_AUDIO"));
        let episodes: usize = std::env::var("RUSTORY_TEST_STORY_EPISODES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        let ext = audio.extension().and_then(|e| e.to_str()).unwrap_or("mp3");
        let (mount, scanner, device_identifier) = scratch_v3_mount();
        let media = tempfile::tempdir().expect("media dir");
        let name = format!("{}.{ext}", "d".repeat(56) + "0badf00d");
        std::fs::copy(&audio, media.path().join(&name)).expect("copy the episode");
        let pack = synthesize_sequential_pack(
            STORY_ID,
            &vec![
                EpisodeAssets {
                    audio_ref: name,
                    image_ref: None,
                };
                episodes
            ],
        );
        let started = Instant::now();
        let ticks = std::sync::Mutex::new(Vec::<(u8, Duration)>::new());
        let out = send_story_pack_to_device(
            &scanner,
            &SystemDeviceV3PackWriter,
            &SendStoryPackRequest {
                device_identifier,
                pack,
                media_dir: media.path().to_path_buf(),
            },
            Duration::from_secs(30),
            &|pct| ticks.lock().unwrap().push((pct, started.elapsed())),
        )
        .expect("send the real episode story pack");
        let elapsed = started.elapsed();
        let written = mount.path().join(".content").join(&out.short_id);
        let audio_out = std::fs::read(written.join("sf/000/0BADF00D")).expect("audio");
        let ticks = ticks.lock().unwrap();
        eprintln!(
            "story pack: {episodes} episode(s) of {} bytes → device audio {} bytes (deduplicated), total {:?}",
            std::fs::metadata(&audio).unwrap().len(),
            audio_out.len(),
            elapsed
        );
        for (pct, at) in ticks.iter() {
            eprintln!("  {pct:>3} % at {at:?}");
        }
        assert_eq!(out.audio_count, 1, "the same file dedups to one asset");
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
