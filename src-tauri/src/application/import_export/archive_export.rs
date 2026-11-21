//! Export a LIBRARY story as a shareable story archive — the structured
//! archive (`story.json` + `assets/` + `thumbnail.png`, zipped) another
//! Rustory library imports as is and other pack tools read.
//!
//! The archive carries the pack the single "send to the device" gesture
//! writes, from the same source:
//!
//! - a story that RETAINED its source archive is exported from that archive
//!   (its own descriptor, its own assets);
//! - any other story (created from a web page, an RSS feed, a folder or the
//!   editor) has its pack SYNTHESIZED from its structure and its node media
//!   — the same plan, the same refusals (an episode without audio, a
//!   device-copied pack), worded for the export.
//!
//! A story WITH CHOICES is laid out by its own graph — every node a stage,
//! the options of a node the wheel that follows it — exactly as the send
//! lays it out. A story created from a pack whose archive was not retained
//! is such a story: its stages, its media and its navigation go back out
//! as the pack they came from. A node without sound is a silent stage, for
//! the archive as for the device.
//!
//! Every asset goes through the send engine's own DEVICE NORMALIZATION
//! ([`to_device_image`] / [`to_device_audio`]): what lands in the archive is
//! the format a device plays, so the archive is ready to use wherever it is
//! opened — no converter needed on the other side. No device is involved:
//! the pack is never ciphered, and nothing is read from or written to one.
//!
//! No media larger than the creation flow admits
//! ([`MAX_ARCHIVE_MEDIA_BYTES`]) is ever written: transcoding to the fixed
//! device bitrate can make a long, lightly encoded sound LARGER than its
//! source, so the bound is checked on the converted bytes and the export
//! refuses rather than write an archive another Rustory would import with
//! that sound left out.
//!
//! Two phases, like every flow that touches both the library and the disk:
//!
//! 1. [`plan_story_archive_export`] — UNDER the SQLite lock, no other I/O:
//!    the title, the cover, the pack source, and every story-level refusal,
//!    decided before a destination is even asked for.
//! 2. [`export_story_archive`] — WITHOUT the lock (a long transcode never
//!    freezes the library): reads and converts the assets one at a time
//!    (never the whole pack in memory), streams them into a temp file
//!    co-located with the destination, then promotes it with an atomic
//!    rename. Any failure drops the temp file: the destination is either
//!    the complete archive or untouched.
//!
//! The library is strictly read: no row, no stored media is ever modified.

use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::application::device::send::{
    ArchiveSource, AssetUnavailable, MediaStoreSource, PackAssetSource,
};
use crate::application::device::story_pack::{plan_named_story_pack, StoryPackRefusal};
use crate::domain::device::pack_transcode::device_asset_basename;
use crate::domain::device::{format_device_bytes, StoryPackBlocker};
use crate::domain::export::{
    prepare_story_archive_descriptor, studio_pack_descriptor, StoryArchiveAsset,
    StoryArchiveAssetKind, StoryArchiveDescriptorError, STORY_ARCHIVE_ASSETS_PREFIX,
    STORY_ARCHIVE_DESCRIPTOR_NAME, STORY_ARCHIVE_THUMBNAIL_NAME,
};
use crate::domain::import::MAX_ARCHIVE_MEDIA_BYTES;
use crate::domain::shared::AppError;
use crate::domain::story::CanonicalStructure;
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::device::audio_transcode::DEVICE_MP3_KBPS;
use crate::infrastructure::device::{
    audio_is_device_ready, to_device_audio, to_device_image, AssetConvertError,
};
use crate::infrastructure::filesystem::read_media;

use super::export::{export_write_error, promote_export_file, stage_shareable_export_file};

/// The export reports progress over one work scale: every asset costs what
/// reading, converting and storing it actually takes, per byte of source.
/// An asset that is already device-ready is cheap ([`READ_COST`]); an audio
/// that must be transcoded is not ([`TRANSCODE_COST`] — decode, resample and
/// encode dominate everything else); an image gets a fixed decode/quantize
/// allowance ([`IMAGE_COST_BYTES`]). 100 % is reserved for the settled
/// outcome, never the in-flight bar.
const READ_COST: f64 = 0.2;
const TRANSCODE_COST: f64 = 1.75;
const IMAGE_COST_BYTES: f64 = 256.0 * 1024.0;
const PROGRESS_END: u8 = 99;

/// Bytes peeked from an audio asset to decide whether it needs transcoding
/// (enough for an ID3v2 tag with cover art plus the first frame header).
const AUDIO_PEEK_BYTES: usize = 4 * 1024 * 1024;

/// Write buffer in front of the staged archive file.
const ARCHIVE_WRITE_BUFFER_BYTES: usize = 1024 * 1024;

/// What the export needs from the library — read under the SQLite lock by
/// [`plan_story_archive_export`], consumed without it by
/// [`export_story_archive`].
#[derive(Debug, Clone)]
pub struct StoryArchiveExportPlan {
    title: String,
    source: PackSource,
    cover_file_name: Option<String>,
}

/// Where the pack comes from — the send engine's own two sources.
#[derive(Debug, Clone)]
enum PackSource {
    /// The story's retained source archive.
    RetainedArchive(PathBuf),
    /// The descriptor of the pack synthesized from the story's structure;
    /// its asset references are node-media store file names.
    Synthesized(Value),
}

/// A settled export, echoed to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedStoryArchive {
    pub bytes_written: u64,
    pub image_count: usize,
    pub audio_count: usize,
}

/// Phase 1 — decide what `story_id` exports, or refuse with the reason.
///
/// `retained_archive` is the story's retained source archive when it has
/// one (resolved and probed by the caller BEFORE taking the lock, exactly as
/// the send does): its presence selects the source.
pub fn plan_story_archive_export(
    db: &DbHandle,
    story_id: &str,
    retained_archive: Option<PathBuf>,
) -> Result<StoryArchiveExportPlan, AppError> {
    let (title, source) = match retained_archive {
        Some(path) => (
            read_story_title(db, story_id)?,
            PackSource::RetainedArchive(path),
        ),
        None => {
            let planned = plan_named_story_pack(db, story_id).map_err(refusal_error)?;
            let descriptor = studio_pack_descriptor(&planned.pack, &planned.stage_names);
            (planned.title, PackSource::Synthesized(descriptor))
        }
    };
    Ok(StoryArchiveExportPlan {
        title,
        source,
        cover_file_name: read_cover_file_name(db, story_id),
    })
}

/// Phase 2 — write the archive of `plan` at `destination`, atomically.
///
/// `media_dir` is the node-media store (the assets of a synthesized pack,
/// and the cover of every story). `on_progress` receives the integer
/// percent (0..=99) as assets are converted and stored.
///
/// Every media written is within [`MAX_ARCHIVE_MEDIA_BYTES`], the bound the
/// creation flow reads an archive with: what this writes, Rustory imports
/// whole and can send.
pub fn export_story_archive(
    plan: &StoryArchiveExportPlan,
    media_dir: &Path,
    destination: &Path,
    on_progress: &dyn Fn(u8),
) -> Result<ExportedStoryArchive, AppError> {
    write_story_archive(
        plan,
        media_dir,
        destination,
        MAX_ARCHIVE_MEDIA_BYTES,
        on_progress,
    )
}

/// [`export_story_archive`] with the per-media bound of the WRITTEN assets
/// as an input (`max_media_bytes`, inclusive, on the converted bytes).
fn write_story_archive(
    plan: &StoryArchiveExportPlan,
    media_dir: &Path,
    destination: &Path,
    max_media_bytes: u64,
    on_progress: &dyn Fn(u8),
) -> Result<ExportedStoryArchive, AppError> {
    let parent = destination.parent().ok_or_else(|| {
        AppError::export_destination_unavailable(
            "Chemin d'export invalide: le dossier cible est introuvable.",
            "Choisis un emplacement sous un dossier existant et réessaie.",
        )
        .with_details(serde_json::json!({
            "source": "invalid_path",
            "kind": "invalid_input",
            "cause": "no_parent",
        }))
    })?;

    // The pack source and its descriptor.
    let (mut source, origin, source_descriptor): (Box<dyn PackAssetSource>, AssetOrigin, Value) =
        match &plan.source {
            PackSource::RetainedArchive(path) => {
                let mut archive = ArchiveSource::open(path)
                    .map_err(|failure| source_archive_error(failure.cause()))?;
                let bytes = archive
                    .read_descriptor()
                    .ok_or_else(|| source_archive_error("descriptor_missing"))?;
                let descriptor = serde_json::from_slice(&bytes)
                    .map_err(|_| source_archive_error("descriptor_invalid"))?;
                (Box::new(archive), AssetOrigin::SourceArchive, descriptor)
            }
            PackSource::Synthesized(descriptor) => (
                Box::new(MediaStoreSource::new(media_dir.to_path_buf())),
                AssetOrigin::MediaStore,
                descriptor.clone(),
            ),
        };
    let descriptor = prepare_story_archive_descriptor(source_descriptor, &plan.title)
        .map_err(|err| descriptor_error(origin, err))?;

    // Every asset is located (and weighed) BEFORE anything is staged: a
    // pack missing a media — or carrying one too large to be read — is
    // refused without touching the destination.
    let work = WorkPlan::new(&descriptor.assets, source.as_mut(), origin)?;
    let report = |done: f64| {
        let pct = (done / work.total * f64::from(PROGRESS_END)).round() as u8;
        on_progress(pct.min(PROGRESS_END));
    };

    // The cover is decoration: an unreadable one never fails the export.
    let thumbnail = plan
        .cover_file_name
        .as_deref()
        .and_then(|file_name| read_cover_png(media_dir, file_name));

    let temp = stage_shareable_export_file(parent)?;
    let bytes_written = {
        let mut archive = zip::ZipWriter::new(BufWriter::with_capacity(
            ARCHIVE_WRITE_BUFFER_BYTES,
            temp.as_file(),
        ));
        // The descriptor comes FIRST: a reader that streams the archive
        // meets the graph before the assets it must attach to it.
        write_entry(
            &mut archive,
            STORY_ARCHIVE_DESCRIPTOR_NAME,
            zip::CompressionMethod::Deflated,
            &descriptor.story_json,
        )?;
        if let Some(png) = &thumbnail {
            write_entry(
                &mut archive,
                STORY_ARCHIVE_THUMBNAIL_NAME,
                zip::CompressionMethod::Stored,
                png,
            )?;
        }

        // One asset at a time: read, normalize to the device format, store,
        // drop. Images compress further (a run-length bitmap); device audio
        // is already compressed and is stored as is.
        let mut done = 0.0f64;
        for asset in &descriptor.assets {
            let bytes = source
                .read_asset(&asset.source_ref)
                .map_err(|reason| unavailable_asset_error(origin, &asset.source_ref, reason))?;
            let (device_ready, method) = match asset.kind {
                StoryArchiveAssetKind::Image => (
                    to_device_image(&bytes).map_err(|err| convert_error(&asset.source_ref, err))?,
                    zip::CompressionMethod::Deflated,
                ),
                StoryArchiveAssetKind::Audio => (
                    to_device_audio(&bytes).map_err(|err| convert_error(&asset.source_ref, err))?,
                    zip::CompressionMethod::Stored,
                ),
            };
            drop(bytes);
            // The bound of the READER, applied to what is about to be
            // WRITTEN: a converted media over it would be left out of the
            // story by the very flow this archive is made for.
            let written_bytes = device_ready.len() as u64;
            if written_bytes > max_media_bytes {
                return Err(converted_oversize_error(
                    origin,
                    asset,
                    written_bytes,
                    max_media_bytes,
                ));
            }
            write_entry(
                &mut archive,
                &format!("{STORY_ARCHIVE_ASSETS_PREFIX}{}", asset.archive_name),
                method,
                &device_ready,
            )?;
            done += work.cost_of(asset);
            report(done);
        }

        let mut writer = archive.finish().map_err(|err| archive_write_error(&err))?;
        writer
            .flush()
            .map_err(|err| export_write_error(&err, Some("flush")))?;
        drop(writer);
        temp.as_file()
            .sync_all()
            .map_err(|err| export_write_error(&err, Some("sync_all")))?;
        temp.as_file()
            .metadata()
            .map_err(|err| export_write_error(&err, Some("sync_all")))?
            .len()
    };
    // A pack without a single asset still ends on the full in-flight bar.
    on_progress(PROGRESS_END);

    promote_export_file(temp, destination)?;

    let image_count = descriptor
        .assets
        .iter()
        .filter(|asset| asset.kind == StoryArchiveAssetKind::Image)
        .count();
    Ok(ExportedStoryArchive {
        bytes_written,
        image_count,
        audio_count: descriptor.assets.len() - image_count,
    })
}

/// Which store an asset is read from — it selects the copy of a refusal
/// (an archive the user imported, or the library's own media).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetOrigin {
    SourceArchive,
    MediaStore,
}

impl AssetOrigin {
    const fn tag(self) -> &'static str {
        match self {
            Self::SourceArchive => "source_archive",
            Self::MediaStore => "media_store",
        }
    }
}

/// The export's work plan: the cost of every asset, on one scale.
struct WorkPlan {
    costs: std::collections::HashMap<String, f64>,
    total: f64,
}

impl WorkPlan {
    fn new(
        assets: &[StoryArchiveAsset],
        source: &mut dyn PackAssetSource,
        origin: AssetOrigin,
    ) -> Result<Self, AppError> {
        let mut costs = std::collections::HashMap::with_capacity(assets.len());
        let mut total = 0.0f64;
        for asset in assets {
            let size = source
                .asset_size(&asset.source_ref)
                .map_err(|reason| unavailable_asset_error(origin, &asset.source_ref, reason))?
                as f64;
            let cost = match asset.kind {
                StoryArchiveAssetKind::Image => size * READ_COST + IMAGE_COST_BYTES,
                StoryArchiveAssetKind::Audio => {
                    let device_ready = source
                        .peek_asset(&asset.source_ref, AUDIO_PEEK_BYTES)
                        .is_some_and(|head| audio_is_device_ready(&head));
                    size * if device_ready {
                        READ_COST
                    } else {
                        TRANSCODE_COST
                    }
                }
            };
            costs.insert(asset.archive_name.clone(), cost);
            total += cost;
        }
        Ok(Self {
            costs,
            // A pack of a descriptor only still has a (tiny) write.
            total: total.max(1.0),
        })
    }

    fn cost_of(&self, asset: &StoryArchiveAsset) -> f64 {
        self.costs.get(&asset.archive_name).copied().unwrap_or(0.0)
    }
}

/// Store one entry in the staged archive. Timestamps are pinned and the
/// permissions fixed, so exporting the same story twice yields the same
/// bytes.
fn write_entry<W: Write + Seek>(
    archive: &mut zip::ZipWriter<W>,
    name: &str,
    method: zip::CompressionMethod,
    bytes: &[u8],
) -> Result<(), AppError> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(method)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644)
        .large_file(bytes.len() as u64 > u64::from(u32::MAX));
    archive
        .start_file(name, options)
        .map_err(|err| archive_write_error(&err))?;
    archive
        .write_all(bytes)
        .map_err(|err| export_write_error(&err, None))
}

/// A failure of the archive writer itself: an I/O error keeps its kind, any
/// other one is a plain write failure of the staged file.
fn archive_write_error(err: &zip::result::ZipError) -> AppError {
    match err {
        zip::result::ZipError::Io(io) => export_write_error(io, None),
        _ => export_write_error(&std::io::Error::other("archive"), None),
    }
}

/// The story's cover as a PNG: the START node's stored image. `None` when
/// the story has none, or when it cannot be read.
fn read_cover_png(media_dir: &Path, file_name: &str) -> Option<Vec<u8>> {
    let (bytes, mime) = read_media(media_dir, file_name).ok()?;
    if mime == "image/png" {
        return Some(bytes);
    }
    // The store keeps pictures as PNG; an older row in another format is
    // re-encoded rather than shipped under a name that lies about it.
    let decoded = image::load_from_memory(&bytes).ok()?;
    let mut png = Vec::new();
    decoded
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// The story's title — the name the archive is shared under.
fn read_story_title(db: &DbHandle, story_id: &str) -> Result<String, AppError> {
    db.conn()
        .query_row(
            "SELECT title FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |row| row.get(0),
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => story_missing_error(),
            _ => storage_error("read_story"),
        })
}

/// The stored file name of the story's cover — its START node's image —
/// when it has one. DEFENSIVE: the cover is decoration, so any miss (no
/// start node, no image, a row that is gone, a read hiccup) is `None`.
fn read_cover_file_name(db: &DbHandle, story_id: &str) -> Option<String> {
    let structure_json: String = db
        .conn()
        .query_row(
            "SELECT structure_json FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |row| row.get(0),
        )
        .ok()?;
    let structure: CanonicalStructure = serde_json::from_str(&structure_json).ok()?;
    let cover_asset_id = structure
        .nodes
        .iter()
        .find(|node| node.id == structure.start_node_id)?
        .image_asset_id
        .clone()
        .filter(|id| !id.is_empty())?;
    db.conn()
        .query_row(
            "SELECT file_name FROM assets WHERE id = ?1 AND story_id = ?2 AND media_type = 'image'",
            rusqlite::params![cover_asset_id, story_id],
            |row| row.get(0),
        )
        .ok()
}

// ===== Refusals =====

/// The plan's refusal worded for the EXPORT gesture. The reasons — and the
/// `details` tags — are the send's own; only the copy differs.
fn refusal_error(refusal: StoryPackRefusal) -> AppError {
    match refusal {
        StoryPackRefusal::NotFound => story_missing_error(),
        StoryPackRefusal::Storage(stage) => storage_error(stage),
        StoryPackRefusal::DevicePack => AppError::export_failed(
            "Export impossible: une histoire copiée depuis un appareil ne peut pas être exportée en archive.",
            "Exporte une histoire créée dans Rustory ou importée depuis une archive.",
        )
        .with_details(serde_json::json!({ "source": "story_pack", "cause": "device_pack" })),
        StoryPackRefusal::Blocked(blocker) => blocker_error(blocker),
        StoryPackRefusal::AssetMissing { node_id } => AppError::export_failed(
            "Export impossible: un média de l'histoire est introuvable dans la bibliothèque.",
            "Rouvre l'histoire pour vérifier ses épisodes (audio et image), puis réessaie l'export.",
        )
        .with_details(serde_json::json!({
            "source": "story_pack",
            "cause": "asset_missing",
            "node": node_id,
        })),
        StoryPackRefusal::AnnouncementMissing { which } => AppError::export_failed(
            "Export impossible: une annonce du menu est introuvable dans la bibliothèque.",
            "Régénère les annonces depuis l'histoire, puis réessaie l'export.",
        )
        .with_details(serde_json::json!({
            "source": "story_pack",
            "cause": "announcement_missing",
            "announcement": which,
        })),
        StoryPackRefusal::OptionWithoutDestination { node_id } => AppError::export_failed(
            "Export impossible: une option de cette histoire ne mène à aucun nœud.",
            "Rouvre l'histoire pour lier chaque option à un nœud (ou la retirer), puis réessaie l'export.",
        )
        .with_details(serde_json::json!({
            "source": "story_pack",
            "cause": "option_without_destination",
            "node": node_id,
        })),
    }
}

/// The story does not lay out as a pack — the reasons its library card
/// announces for the send, with the export's next step.
fn blocker_error(blocker: StoryPackBlocker) -> AppError {
    let (message, action) = match blocker {
        StoryPackBlocker::Empty => (
            "Export impossible: cette histoire n'a aucun épisode.",
            "Ajoute au moins un épisode avec un audio, puis réessaie l'export.",
        ),
        // A story with choices is laid out by its own graph: the export's
        // plan never returns `Branching`. Should it ever, the structure did
        // not read as the plan expected — the same next step.
        StoryPackBlocker::Malformed | StoryPackBlocker::Branching => (
            "Export impossible: la structure de cette histoire est illisible.",
            "Rouvre l'histoire pour la vérifier, puis réessaie l'export.",
        ),
        StoryPackBlocker::MissingAudio => (
            "Export impossible: un ou plusieurs épisodes de cette histoire n'ont pas d'audio.",
            "Ajoute un audio à chaque épisode, puis réessaie l'export.",
        ),
        StoryPackBlocker::MissingAnnouncements => (
            "Export impossible: les annonces du menu (question et titres des épisodes) ne sont pas encore générées.",
            "Génère les annonces depuis l'histoire, puis réessaie l'export.",
        ),
    };
    AppError::export_failed(message, action).with_details(serde_json::json!({
        "source": "story_pack",
        "cause": blocker.diagnostic_tag(),
    }))
}

/// The story's retained source pack cannot be read back (the file is gone
/// or damaged, its descriptor absent or unparsable).
fn source_archive_error(cause: &'static str) -> AppError {
    AppError::export_failed(
        "Export impossible: le pack source de cette histoire est illisible.",
        "Réimporte l'archive d'origine dans Rustory, puis réessaie l'export.",
    )
    .with_details(serde_json::json!({ "source": "source_archive", "cause": cause }))
}

/// The pack's graph cannot be described in an archive. For a retained pack
/// that is a malformed descriptor; for a synthesized one it cannot happen
/// by construction — kept fail-closed under the same copy.
fn descriptor_error(origin: AssetOrigin, err: StoryArchiveDescriptorError) -> AppError {
    let action = match origin {
        AssetOrigin::SourceArchive => {
            "Réimporte l'archive d'origine dans Rustory, puis réessaie l'export."
        }
        AssetOrigin::MediaStore => "Rouvre l'histoire pour la vérifier, puis réessaie l'export.",
    };
    AppError::export_failed(
        "Export impossible: le pack de cette histoire ne peut pas être décrit dans une archive.",
        action,
    )
    .with_details(serde_json::json!({
        "source": origin.tag(),
        "cause": err.diagnostic_tag(),
    }))
}

/// A media the pack references is not where the pack says it is.
fn missing_asset_error(origin: AssetOrigin, source_ref: &str) -> AppError {
    let (message, action) = match origin {
        AssetOrigin::SourceArchive => (
            "Export impossible: un média du pack est introuvable dans son archive source.",
            "Réimporte l'archive d'origine dans Rustory, puis réessaie l'export.",
        ),
        AssetOrigin::MediaStore => (
            "Export impossible: un média de l'histoire est introuvable dans la bibliothèque.",
            "Rouvre l'histoire pour vérifier ses épisodes (audio et image), puis réessaie l'export.",
        ),
    };
    AppError::export_failed(message, action).with_details(serde_json::json!({
        "source": origin.tag(),
        "cause": "asset_missing",
        // Only the device basename (8 characters), never a path.
        "asset": device_asset_basename(source_ref),
    }))
}

/// A media the pack references IS where the pack says, but weighs more than
/// a single media may — never loaded, and never worded as an absence.
fn oversize_asset_error(origin: AssetOrigin, source_ref: &str) -> AppError {
    let limit = format_device_bytes(MAX_ARCHIVE_MEDIA_BYTES);
    let (message, action) = match origin {
        AssetOrigin::SourceArchive => (
            format!("Export impossible: un média du pack dépasse la taille autorisée ({limit})."),
            "Dans l'archive d'origine, remplace ce média par un fichier plus léger, réimporte l'archive dans Rustory, puis réessaie l'export.",
        ),
        AssetOrigin::MediaStore => (
            format!(
                "Export impossible: un média de l'histoire dépasse la taille autorisée ({limit})."
            ),
            "Rouvre l'histoire pour remplacer ce média par un fichier plus léger, puis réessaie l'export.",
        ),
    };
    AppError::export_failed(message, action).with_details(serde_json::json!({
        "source": origin.tag(),
        "cause": "asset_oversize",
        "max_bytes": MAX_ARCHIVE_MEDIA_BYTES,
        // Only the device basename (8 characters), never a path.
        "asset": device_asset_basename(source_ref),
    }))
}

/// The refusal for a media its source cannot hand out — the two reasons
/// keep their own copy and their own `details.cause`.
fn unavailable_asset_error(
    origin: AssetOrigin,
    source_ref: &str,
    reason: AssetUnavailable,
) -> AppError {
    match reason {
        AssetUnavailable::Missing => missing_asset_error(origin, source_ref),
        AssetUnavailable::Oversize => oversize_asset_error(origin, source_ref),
    }
}

/// A media was read and converted, but its DEVICE form weighs more than an
/// archive media may. Only a sound gets there in practice — a device
/// picture is a few tens of kilobytes — and only when it had to be
/// transcoded: at the fixed device bitrate, a long sound outgrows the bound
/// whatever its source weighed. Written anyway, it would be left out of the
/// story by the library that imports the archive — refused instead.
fn converted_oversize_error(
    origin: AssetOrigin,
    asset: &StoryArchiveAsset,
    written_bytes: u64,
    max_media_bytes: u64,
) -> AppError {
    let message = match asset.kind {
        StoryArchiveAssetKind::Audio => format!(
            "Export impossible: un son de l'histoire est trop long pour une archive (plus de {} environ).",
            device_sound_duration_text(max_media_bytes)
        ),
        StoryArchiveAssetKind::Image => format!(
            "Export impossible: une image de l'histoire dépasse la taille autorisée une fois convertie ({}).",
            format_device_bytes(max_media_bytes)
        ),
    };
    let action = match (origin, asset.kind) {
        (AssetOrigin::SourceArchive, StoryArchiveAssetKind::Audio) => {
            "Dans l'archive d'origine, remplace ce son par des sons plus courts, réimporte l'archive dans Rustory, puis réessaie l'export."
        }
        (AssetOrigin::SourceArchive, StoryArchiveAssetKind::Image) => {
            "Dans l'archive d'origine, remplace cette image, réimporte l'archive dans Rustory, puis réessaie l'export."
        }
        (AssetOrigin::MediaStore, StoryArchiveAssetKind::Audio) => {
            "Remplace cet épisode par des épisodes plus courts, puis réessaie l'export."
        }
        (AssetOrigin::MediaStore, StoryArchiveAssetKind::Image) => {
            "Rouvre l'histoire pour remplacer cette image, puis réessaie l'export."
        }
    };
    AppError::export_failed(message, action).with_details(serde_json::json!({
        "source": "asset_convert",
        "cause": "oversize",
        "byte_size": written_bytes,
        "max_bytes": max_media_bytes,
        // Only the device basename (8 characters), never a path.
        "asset": device_asset_basename(&asset.source_ref),
    }))
}

/// How long a sound of `byte_size` bytes plays in the device format (the
/// bitrate every transcoded sound is written at), rounded to the minute and
/// worded for a refusal: `2 h 20`, `45 min`.
fn device_sound_duration_text(byte_size: u64) -> String {
    let bytes_per_second = u64::from(DEVICE_MP3_KBPS) * 1000 / 8;
    let minutes = (byte_size / bytes_per_second + 30) / 60;
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes:02}"),
    }
}

/// A media exists but cannot be converted to the format a device plays (an
/// undecodable image, or audio no decoder recognizes). Shipped as is it
/// would fail wherever the archive is opened — refused instead.
fn convert_error(source_ref: &str, err: AssetConvertError) -> AppError {
    AppError::export_failed(
        "Export impossible: un média n'est pas dans un format que Rustory sait convertir.",
        "Vérifie que les images sont des PNG/JPEG/BMP valides et les audios des fichiers MP3, M4A, WAV ou OGG lisibles, puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "asset_convert",
        "cause": err.diagnostic_tag(),
        // Only the device basename (8 characters), never a path.
        "asset": device_asset_basename(source_ref),
    }))
}

fn story_missing_error() -> AppError {
    AppError::library_inconsistent(
        "Export impossible: histoire introuvable.",
        "Retourne à la bibliothèque et recharge la liste.",
    )
    .with_details(serde_json::json!({ "source": "story_missing" }))
}

fn storage_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu lire ta bibliothèque locale.",
        "Relance l'application ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({ "source": "story_archive_export", "stage": stage }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::import_export::archive_creation::accept_structured_archive_creation;
    use crate::application::import_export::archive_creation::test_support::{
        write_pack_archive, DEVICE_SOUND_HEADER,
    };
    use crate::application::story::{create_story, CreateStoryInput};
    use crate::domain::device::{is_canonical_pack_uuid, transcode_pack, StudioStoryPack};
    use crate::domain::shared::AppErrorCode;
    use crate::infrastructure::db::{open_in_memory, run_migrations};
    use crate::infrastructure::device::audio_transcode::test_support::wav_sine;
    use crate::infrastructure::filesystem::{
        resolve_node_media_dir, resolve_source_archive_path, sniff_media,
    };
    use std::collections::BTreeMap;
    use std::io::Read;
    use tempfile::TempDir;

    fn fresh_db() -> DbHandle {
        let mut db = open_in_memory().expect("open");
        run_migrations(&mut db).expect("migrate");
        db
    }

    /// A library in a scratch app-data directory: the DB plus the media
    /// store files a story references.
    struct Library {
        db: DbHandle,
        app_data: TempDir,
    }

    impl Library {
        fn new() -> Self {
            let app_data = TempDir::new().expect("app data");
            std::fs::create_dir_all(resolve_node_media_dir(app_data.path())).expect("media dir");
            Self {
                db: fresh_db(),
                app_data,
            }
        }

        fn media_dir(&self) -> PathBuf {
            resolve_node_media_dir(self.app_data.path())
        }

        fn story(&mut self, title: &str) -> String {
            create_story(
                &mut self.db,
                CreateStoryInput {
                    title: title.into(),
                },
            )
            .expect("create")
            .id
        }

        /// Store `bytes` in the media store under `file_name` and declare
        /// the matching `assets` row.
        fn asset(&self, story_id: &str, id: &str, media_type: &str, file_name: &str, bytes: &[u8]) {
            std::fs::write(self.media_dir().join(file_name), bytes).expect("store media");
            let format = file_name.rsplit('.').next().expect("ext");
            self.db
                .conn()
                .execute(
                    "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '2026-01-01T00:00:00Z')",
                    rusqlite::params![
                        id,
                        story_id,
                        format!("{:0>64}", id.len()),
                        media_type,
                        format,
                        bytes.len() as i64,
                        file_name
                    ],
                )
                .expect("insert asset");
        }

        fn structure(&self, story_id: &str, nodes: &[(&str, &str, Option<&str>, Option<&str>)]) {
            let nodes: Vec<Value> = nodes
                .iter()
                .map(|(id, label, audio, image)| {
                    serde_json::json!({
                        "id": id, "text": "", "label": label,
                        "imageAssetId": image, "audioAssetId": audio, "options": [],
                    })
                })
                .collect();
            let json =
                serde_json::json!({ "schemaVersion": 3, "startNodeId": "n1", "nodes": nodes });
            self.db
                .conn()
                .execute(
                    "UPDATE stories SET structure_json = ?1 WHERE id = ?2",
                    rusqlite::params![json.to_string(), story_id],
                )
                .expect("update structure");
        }

        /// A story WITH CHOICES: each node is `(id, label, audio, image,
        /// destinations of its options)`.
        #[allow(clippy::type_complexity)]
        fn choice_structure(
            &self,
            story_id: &str,
            start: &str,
            nodes: &[(&str, &str, Option<&str>, Option<&str>, &[&str])],
        ) {
            let nodes: Vec<Value> = nodes
                .iter()
                .map(|(id, label, audio, image, destinations)| {
                    let options: Vec<Value> = destinations
                        .iter()
                        .map(|target| serde_json::json!({ "label": "suite", "target": target }))
                        .collect();
                    serde_json::json!({
                        "id": id, "text": "", "label": label,
                        "imageAssetId": image, "audioAssetId": audio, "options": options,
                    })
                })
                .collect();
            let json =
                serde_json::json!({ "schemaVersion": 3, "startNodeId": start, "nodes": nodes });
            self.db
                .conn()
                .execute(
                    "UPDATE stories SET structure_json = ?1 WHERE id = ?2",
                    rusqlite::params![json.to_string(), story_id],
                )
                .expect("update structure");
        }

        fn plan(&self, story_id: &str) -> Result<StoryArchiveExportPlan, AppError> {
            let retained = resolve_source_archive_path(self.app_data.path(), story_id);
            plan_story_archive_export(&self.db, story_id, retained.is_file().then_some(retained))
        }

        fn export(
            &self,
            story_id: &str,
            destination: &Path,
        ) -> Result<ExportedStoryArchive, AppError> {
            let plan = self.plan(story_id)?;
            export_story_archive(&plan, &self.media_dir(), destination, &|_| {})
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, 80, 255])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .expect("png");
        out.into_inner()
    }

    /// A two-episode story: episode 1 has a picture and a 48 kHz stereo WAV
    /// (it needs transcoding), episode 2 a mono 44.1 kHz WAV and no picture.
    fn two_episode_story(library: &mut Library) -> String {
        let id = library.story("Les Bestioles");
        let image_name = format!("{}.png", "b".repeat(56) + "5678ef01");
        let first = format!("{}.wav", "a".repeat(56) + "1234abcd");
        let second = format!("{}.wav", "c".repeat(56) + "9abc0123");
        library.asset(&id, "img-1", "image", &image_name, &png(64, 48));
        library.asset(
            &id,
            "aud-1",
            "audio",
            &first,
            &wav_sine(48_000, 2, 440.0, 0.5, 0.5),
        );
        library.asset(
            &id,
            "aud-2",
            "audio",
            &second,
            &wav_sine(44_100, 1, 660.0, 0.5, 0.5),
        );
        library.structure(
            &id,
            &[
                ("n1", "Le loup", Some("aud-1"), Some("img-1")),
                ("n2", "", Some("aud-2"), None),
            ],
        );
        id
    }

    fn read_archive(path: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path).expect("open zip"))
            .expect("a zip archive");
        let mut entries = BTreeMap::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("entry");
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read entry");
            entries.insert(entry.name().to_string(), bytes);
        }
        entries
    }

    fn entry_names_in_order(path: &Path) -> Vec<String> {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path).expect("open zip"))
            .expect("a zip archive");
        (0..archive.len())
            .map(|index| archive.by_index(index).expect("entry").name().to_string())
            .collect()
    }

    fn details(err: &AppError) -> Value {
        serde_json::to_value(err).expect("serialize")["details"].clone()
    }

    #[test]
    fn exports_a_library_story_as_a_ready_to_use_archive() {
        let mut library = Library::new();
        let id = two_episode_story(&mut library);
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("Les Bestioles.zip");

        let progress = std::sync::Mutex::new(Vec::<u8>::new());
        let plan = library.plan(&id).expect("plan");
        let exported = export_story_archive(&plan, &library.media_dir(), &destination, &|pct| {
            progress.lock().unwrap().push(pct)
        })
        .expect("export");

        let entries = read_archive(&destination);
        assert_eq!((exported.image_count, exported.audio_count), (1, 2));
        assert_eq!(
            exported.bytes_written,
            std::fs::metadata(&destination).expect("stat").len()
        );

        // The descriptor: the story's title, well-formed stage ids whose
        // entry is the story id, and a graph the send engine accepts.
        let root: Value = serde_json::from_slice(&entries["story.json"]).expect("descriptor");
        assert_eq!(root["title"], "Les Bestioles");
        assert_eq!(root["format"], "v1");
        let stages = root["stageNodes"].as_array().expect("stages");
        assert_eq!(stages.len(), 3, "cover + two episodes");
        assert_eq!(stages[0]["uuid"], id.as_str());
        for stage in stages {
            assert!(is_canonical_pack_uuid(stage["uuid"].as_str().unwrap()));
        }
        assert_eq!(stages[1]["name"], "Le loup");
        assert_eq!(stages[2]["name"], "Épisode 2");
        let pack: StudioStoryPack = serde_json::from_value(root.clone()).expect("typed");
        let transcoded = transcode_pack(&pack).expect("the exported graph transcodes");

        // Every referenced asset is in the archive, in the DEVICE format:
        // converting it again is a no-op.
        assert_eq!(transcoded.images.len(), 1);
        assert_eq!(transcoded.audios.len(), 2);
        for name in &transcoded.images {
            assert!(name.ends_with(".bmp"));
            let bytes = &entries[&format!("assets/{name}")];
            assert_eq!(&to_device_image(bytes).expect("device image"), bytes);
        }
        for name in &transcoded.audios {
            assert!(name.ends_with(".mp3"));
            let bytes = &entries[&format!("assets/{name}")];
            assert!(audio_is_device_ready(bytes), "{name} must be a device MP3");
            assert_eq!(&to_device_audio(bytes).expect("device audio"), bytes);
        }
        // Nothing but the descriptor, the cover and the referenced assets.
        assert_eq!(entries.len(), 2 + 3);
        // The cover is the start node's stored picture, as a PNG.
        assert_eq!(
            sniff_media(&entries["thumbnail.png"]).map(|m| m.format),
            Some("png")
        );

        // Progress is monotonic, never claims 100 %, and ends full.
        let progress = progress.lock().unwrap();
        assert!(progress.windows(2).all(|w| w[0] <= w[1]), "{progress:?}");
        assert!(progress.iter().all(|pct| *pct <= 99));
        assert_eq!(progress.last(), Some(&99));
    }

    #[test]
    fn the_descriptor_is_stored_first_then_the_cover_then_the_assets() {
        let mut library = Library::new();
        let id = two_episode_story(&mut library);
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("histoire.zip");
        library.export(&id, &destination).expect("export");

        let names = entry_names_in_order(&destination);
        assert_eq!(names[0], "story.json");
        assert_eq!(names[1], "thumbnail.png");
        assert!(names[2..].iter().all(|name| name.starts_with("assets/")));
        assert!(names[2].ends_with(".bmp"), "images before sounds");
    }

    #[test]
    fn exporting_twice_yields_identical_archives_and_leaves_the_library_untouched() {
        let mut library = Library::new();
        let id = two_episode_story(&mut library);
        let snapshot = |library: &Library| -> (String, String, String) {
            library
                .db
                .conn()
                .query_row(
                    "SELECT title, structure_json, updated_at FROM stories WHERE id = ?1",
                    rusqlite::params![&id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .expect("row")
        };
        let before = snapshot(&library);
        let stored_before: BTreeMap<String, Vec<u8>> = std::fs::read_dir(library.media_dir())
            .expect("media dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    std::fs::read(entry.path()).expect("read media"),
                )
            })
            .collect();

        let out_dir = TempDir::new().expect("out");
        let first = out_dir.path().join("a.zip");
        let second = out_dir.path().join("b.zip");
        library.export(&id, &first).expect("first");
        library.export(&id, &second).expect("second");

        assert_eq!(
            std::fs::read(&first).expect("first bytes"),
            std::fs::read(&second).expect("second bytes"),
            "an export is reproducible"
        );
        assert_eq!(snapshot(&library), before, "the story row is untouched");
        let stored_after: BTreeMap<String, Vec<u8>> = std::fs::read_dir(library.media_dir())
            .expect("media dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    std::fs::read(entry.path()).expect("read media"),
                )
            })
            .collect();
        assert_eq!(stored_after, stored_before, "the media store is untouched");
    }

    #[test]
    fn an_exported_archive_imports_cleanly_into_another_library_and_exports_back_unchanged() {
        let mut library = Library::new();
        let id = two_episode_story(&mut library);
        let out_dir = TempDir::new().expect("out");
        let shared = out_dir.path().join("Les Bestioles.zip");
        library.export(&id, &shared).expect("export");

        // Another Rustory: a fresh library imports the shared archive.
        let mut other = Library::new();
        let card =
            accept_structured_archive_creation(&mut other.db, other.app_data.path(), &shared)
                .expect("the shared archive imports");
        assert_eq!(card.title, "Les Bestioles");
        assert!(card.sendable, "the imported story can be sent to a device");
        assert!(
            card.import_report.is_none(),
            "a clean import: nothing to review"
        );
        // Nothing was discarded on the way: the imported cover node carries
        // both its picture and its sound.
        let structure_json: String = other
            .db
            .conn()
            .query_row(
                "SELECT structure_json FROM stories WHERE id = ?1",
                rusqlite::params![&card.id],
                |row| row.get(0),
            )
            .expect("structure");
        let structure: CanonicalStructure =
            serde_json::from_str(&structure_json).expect("canonical structure");
        assert_eq!(structure.nodes.len(), 3, "cover + two episodes");
        let cover = structure
            .nodes
            .iter()
            .find(|node| node.id == structure.start_node_id)
            .expect("start node");
        assert_eq!(cover.id, id, "the start node is the pack's entry stage");
        assert_eq!(cover.label, "Les Bestioles");
        assert!(cover.image_asset_id.is_some() && cover.audio_asset_id.is_some());
        assert!(structure
            .nodes
            .iter()
            .all(|node| node.audio_asset_id.is_some()));

        // Exported again from there (its retained archive is the source):
        // the device-ready assets pass verbatim, so the pack is the same.
        let back = out_dir.path().join("retour.zip");
        let exported = other.export(&card.id, &back).expect("export back");
        assert_eq!((exported.image_count, exported.audio_count), (1, 2));
        let first = read_archive(&shared);
        let second = read_archive(&back);
        let first_root: Value = serde_json::from_slice(&first["story.json"]).expect("first");
        let second_root: Value = serde_json::from_slice(&second["story.json"]).expect("second");
        assert_eq!(first_root["stageNodes"][0]["uuid"], id.as_str());
        assert_eq!(
            second_root["stageNodes"][0]["uuid"],
            id.as_str(),
            "the pack keeps its identity from library to library"
        );
        let asset_bytes =
            |entries: &BTreeMap<String, Vec<u8>>, root: &Value, stage: usize, field: &str| {
                let name = root["stageNodes"][stage][field]
                    .as_str()
                    .expect("reference");
                entries[&format!("assets/{name}")].clone()
            };
        for (stage, field) in [(0, "image"), (1, "audio"), (2, "audio")] {
            assert_eq!(
                asset_bytes(&first, &first_root, stage, field),
                asset_bytes(&second, &second_root, stage, field),
                "stage {stage} {field} is carried byte for byte"
            );
        }
    }

    // ===== Stories with choices: exported by their own graph =====

    /// A small story with choices written in the editor: a crossroads that
    /// offers two paths, each path leading to its own scene, both scenes
    /// coming back to the choice. The last scene has no label.
    fn crossroads_story(library: &mut Library) -> String {
        let id = library.story("Le Chemin");
        let cover = format!("{}.png", "b".repeat(56) + "5678ef01");
        library.asset(&id, "img-1", "image", &cover, &png(64, 48));
        for (index, tone) in [440.0, 520.0, 600.0, 680.0, 760.0].into_iter().enumerate() {
            let name = format!("{}{:08x}.wav", "a".repeat(56), 0x1234_ab00 + index);
            library.asset(
                &id,
                &format!("aud-{}", index + 1),
                "audio",
                &name,
                &wav_sine(44_100, 1, tone, 0.5, 0.5),
            );
        }
        library.choice_structure(
            &id,
            "n1",
            &[
                (
                    "n1",
                    "Le carrefour",
                    Some("aud-1"),
                    Some("img-1"),
                    &["n2", "n3"],
                ),
                ("n2", "À gauche", Some("aud-2"), None, &["n4"]),
                ("n3", "À droite", Some("aud-3"), None, &["n5"]),
                ("n4", "La forêt", Some("aud-4"), None, &["n2", "n3"]),
                ("n5", "", Some("aud-5"), None, &["n2", "n3"]),
            ],
        );
        id
    }

    /// The navigation a descriptor describes, stage by stage, every
    /// transition RESOLVED to the option list it enters — what two archives
    /// must agree on to be the same pack, whatever their action ids and
    /// their asset names.
    fn graph_of(root: &Value) -> Vec<Value> {
        let entered = |transition: &Value| -> Value {
            if transition.is_null() {
                return Value::Null;
            }
            let action = root["actionNodes"]
                .as_array()
                .expect("actions")
                .iter()
                .find(|action| action["id"] == transition["actionNode"])
                .expect("the action exists");
            serde_json::json!({
                "options": action["options"],
                "optionIndex": transition["optionIndex"],
            })
        };
        root["stageNodes"]
            .as_array()
            .expect("stages")
            .iter()
            .map(|stage| {
                serde_json::json!({
                    "uuid": stage["uuid"],
                    "name": stage["name"],
                    "squareOne": stage.get("squareOne").and_then(Value::as_bool).unwrap_or(false),
                    "controlSettings": stage["controlSettings"],
                    "ok": entered(&stage["okTransition"]),
                    "home": entered(&stage["homeTransition"]),
                    "hasImage": !stage["image"].is_null(),
                    "hasAudio": !stage["audio"].is_null(),
                })
            })
            .collect()
    }

    fn controls(wheel: bool, ok: bool, home: bool, pause: bool, autoplay: bool) -> Value {
        serde_json::json!({
            "wheel": wheel, "ok": ok, "home": home, "pause": pause, "autoplay": autoplay,
        })
    }

    /// The bytes of the asset stage `stage` references in `field`.
    fn stage_asset(
        entries: &BTreeMap<String, Vec<u8>>,
        root: &Value,
        stage: usize,
        field: &str,
    ) -> Vec<u8> {
        let name = root["stageNodes"][stage][field]
            .as_str()
            .expect("reference");
        entries[&format!("assets/{name}")].clone()
    }

    fn drop_retained_archive(library: &Library, story_id: &str) {
        std::fs::remove_file(resolve_source_archive_path(
            library.app_data.path(),
            story_id,
        ))
        .expect("drop the retained archive");
    }

    #[test]
    fn a_story_with_choices_is_exported_by_its_own_graph() {
        let mut library = Library::new();
        let id = crossroads_story(&mut library);
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("Le Chemin.zip");

        let exported = library.export(&id, &destination).expect("export");
        assert_eq!((exported.image_count, exported.audio_count), (1, 5));
        let entries = read_archive(&destination);
        let root: Value = serde_json::from_slice(&entries["story.json"]).expect("descriptor");
        assert_eq!(root["title"], "Le Chemin");

        // One stage per node, the start node first, every id a UUID.
        let stages = root["stageNodes"].as_array().expect("stages");
        assert_eq!(stages.len(), 5);
        let uuids: Vec<&str> = stages
            .iter()
            .map(|stage| stage["uuid"].as_str().expect("uuid"))
            .collect();
        assert_eq!(
            uuids[0],
            id.as_str(),
            "the pack is filed under the story id"
        );
        assert!(uuids.iter().all(|uuid| is_canonical_pack_uuid(uuid)));
        assert_eq!(
            uuids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            5
        );
        let graph = graph_of(&root);
        assert_eq!(
            graph.iter().map(|stage| &stage["name"]).collect::<Vec<_>>(),
            vec!["Le carrefour", "À gauche", "À droite", "La forêt", "Nœud 5"]
        );

        // The options of a node are the wheel that follows it.
        let wheel = serde_json::json!([uuids[1], uuids[2]]);
        assert_eq!(graph[0]["squareOne"], true);
        assert_eq!(
            graph[0]["controlSettings"],
            controls(true, true, false, false, false)
        );
        assert_eq!(
            graph[0]["ok"],
            serde_json::json!({ "options": wheel, "optionIndex": 0 })
        );
        assert_eq!(graph[0]["home"], Value::Null);
        // Its entries wait for the listener, and lead each to its scene.
        for (entry, scene) in [(1, 3), (2, 4)] {
            assert_eq!(
                graph[entry]["controlSettings"],
                controls(true, true, true, false, false)
            );
            assert_eq!(
                graph[entry]["ok"],
                serde_json::json!({ "options": [uuids[scene]], "optionIndex": 0 })
            );
            assert_eq!(graph[entry]["home"], Value::Null);
        }
        // A scene plays through, then returns to the wheel ON ITS OWN ENTRY
        // — and home takes the listener to that same spot.
        for (scene, entry_index) in [(3, 0), (4, 1)] {
            let back = serde_json::json!({ "options": wheel, "optionIndex": entry_index });
            assert_eq!(
                graph[scene]["controlSettings"],
                controls(false, false, true, true, true)
            );
            assert_eq!(graph[scene]["ok"], back);
            assert_eq!(graph[scene]["home"], back);
        }
        // Three option lists, three action nodes — the two scenes share the
        // wheel they return to.
        let actions = root["actionNodes"].as_array().expect("actions");
        assert_eq!(actions.len(), 3);
        assert!(actions
            .iter()
            .all(|action| is_canonical_pack_uuid(action["id"].as_str().expect("id"))));

        // A pack the device engine itself accepts, every media in place.
        let pack: StudioStoryPack = serde_json::from_value(root.clone()).expect("typed pack");
        let transcoded = transcode_pack(&pack).expect("a consistent graph");
        assert_eq!((transcoded.images.len(), transcoded.audios.len()), (1, 5));
        for name in transcoded.images.iter().chain(&transcoded.audios) {
            assert!(entries.contains_key(&format!("assets/{name}")), "{name}");
        }
        assert!(entries.contains_key("thumbnail.png"));
    }

    #[test]
    fn a_story_with_choices_travels_from_library_to_library_as_the_same_pack() {
        let mut library = Library::new();
        let id = crossroads_story(&mut library);
        let out_dir = TempDir::new().expect("out");
        let first_path = out_dir.path().join("premier.zip");
        library.export(&id, &first_path).expect("export");
        let first = read_archive(&first_path);
        let first_root: Value = serde_json::from_slice(&first["story.json"]).expect("first");

        // Another Rustory imports it: every stage a node, every wheel the
        // options of the node before it.
        let mut second_library = Library::new();
        let card = accept_structured_archive_creation(
            &mut second_library.db,
            second_library.app_data.path(),
            &first_path,
        )
        .expect("the shared archive imports");
        assert_eq!(card.title, "Le Chemin");
        assert!(card.import_report.is_none(), "a clean import");
        let structure_json: String = second_library
            .db
            .conn()
            .query_row(
                "SELECT structure_json FROM stories WHERE id = ?1",
                rusqlite::params![&card.id],
                |row| row.get(0),
            )
            .expect("structure");
        let structure: CanonicalStructure =
            serde_json::from_str(&structure_json).expect("canonical structure");
        let stages = first_root["stageNodes"].as_array().expect("stages");
        assert_eq!(structure.start_node_id, id);
        assert_eq!(
            structure
                .nodes
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            stages
                .iter()
                .map(|stage| stage["uuid"].as_str().expect("uuid"))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            structure
                .nodes
                .iter()
                .map(|node| node.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Le carrefour", "À gauche", "À droite", "La forêt", "Nœud 5"]
        );
        let destinations = |node: usize| -> Vec<&str> {
            structure.nodes[node]
                .options
                .iter()
                .map(|option| option.target.as_deref().expect("a linked option"))
                .collect()
        };
        let uuid = |stage: usize| stages[stage]["uuid"].as_str().expect("uuid");
        assert_eq!(destinations(0), vec![uuid(1), uuid(2)]);
        assert_eq!(destinations(1), vec![uuid(3)]);
        assert_eq!(destinations(2), vec![uuid(4)]);
        assert_eq!(destinations(3), vec![uuid(1), uuid(2)]);
        assert_eq!(destinations(4), vec![uuid(1), uuid(2)]);

        // Exported from there WITHOUT its retained archive — from the
        // structure and the media alone: the same pack comes out, sound for
        // sound.
        drop_retained_archive(&second_library, &card.id);
        let second_path = out_dir.path().join("deuxieme.zip");
        let exported = second_library
            .export(&card.id, &second_path)
            .expect("export from the structure");
        assert_eq!((exported.image_count, exported.audio_count), (1, 5));
        let second = read_archive(&second_path);
        let second_root: Value = serde_json::from_slice(&second["story.json"]).expect("second");
        assert_eq!(graph_of(&second_root), graph_of(&first_root));
        assert_eq!(
            stage_asset(&second, &second_root, 0, "image"),
            stage_asset(&first, &first_root, 0, "image")
        );
        for stage in 0..5 {
            assert_eq!(
                stage_asset(&second, &second_root, stage, "audio"),
                stage_asset(&first, &first_root, stage, "audio"),
                "stage {stage} sound is carried byte for byte"
            );
        }

        // One more library, one more hop: nothing moves anymore — the very
        // same archive, byte for byte.
        let mut third_library = Library::new();
        let third_card = accept_structured_archive_creation(
            &mut third_library.db,
            third_library.app_data.path(),
            &second_path,
        )
        .expect("the second archive imports");
        drop_retained_archive(&third_library, &third_card.id);
        let third_path = out_dir.path().join("troisieme.zip");
        third_library
            .export(&third_card.id, &third_path)
            .expect("export from the structure");
        assert_eq!(
            std::fs::read(&third_path).expect("third"),
            std::fs::read(&second_path).expect("second"),
        );
    }

    /// A menu pack as a pack tool writes one — cover, spoken question, one
    /// wheel entry per story, each story returning to the wheel — stored as a
    /// ready-made archive at `path`. Returns its descriptor.
    fn menu_pack_archive(path: &Path, pack_uuid: &str, stories: usize) -> Value {
        use crate::domain::device::{
            stage_names, synthesize_menu_pack, MenuEpisode, MenuPackAssets, StoryLayout,
        };
        let menu = MenuPackAssets {
            title_ref: Some("titre.mp3".into()),
            question_ref: "question.mp3".into(),
            episodes: (1..=stories)
                .map(|i| MenuEpisode {
                    audio_ref: format!("histoire-{i}.mp3"),
                    image_ref: None,
                    prompt_ref: format!("annonce-{i}.mp3"),
                })
                .collect(),
            auto_continue: false,
        };
        let pack = synthesize_menu_pack(pack_uuid, &menu);
        let labels: Vec<String> = (1..=stories).map(|i| format!("Histoire {i}")).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let names = stage_names(StoryLayout::Menu, pack_uuid, "Collection", &labels);
        let prepared =
            prepare_story_archive_descriptor(studio_pack_descriptor(&pack, &names), "Collection")
                .expect("a pack descriptor");
        // Every sound its own size, so every sound its own content.
        let sounds: Vec<(&str, u64)> = prepared
            .assets
            .iter()
            .enumerate()
            .map(|(index, asset)| (asset.archive_name.as_str(), 4096 + 64 * index as u64))
            .collect();
        write_pack_archive(
            path,
            Some(std::str::from_utf8(&prepared.story_json).expect("utf-8")),
            &sounds,
        );
        serde_json::from_slice(&prepared.story_json).expect("descriptor json")
    }

    #[test]
    fn a_story_created_from_a_pack_whose_archive_is_gone_is_exported_as_that_pack_again() {
        const PACK: &str = "01a06ed9-2040-77c1-9e03-b8f429f4e954";
        let out_dir = TempDir::new().expect("out");
        let source_path = out_dir.path().join("collection.zip");
        let source_root = menu_pack_archive(&source_path, PACK, 3);
        let source = read_archive(&source_path);

        let mut library = Library::new();
        let card = accept_structured_archive_creation(
            &mut library.db,
            library.app_data.path(),
            &source_path,
        )
        .expect("the pack imports");
        assert!(card.import_report.is_none(), "a clean import");
        // The archive the creation retained is lost (a library moved without
        // it, a retention that failed): what is left is the structure — its
        // stages as nodes, its wheels as options — and the media.
        drop_retained_archive(&library, &card.id);

        let destination = out_dir.path().join("retour.zip");
        let exported = library.export(&card.id, &destination).expect("export");
        assert_eq!(
            (exported.image_count, exported.audio_count),
            (0, 8),
            "the title, the question, three announcements, three stories"
        );
        let entries = read_archive(&destination);
        let root: Value = serde_json::from_slice(&entries["story.json"]).expect("descriptor");
        assert_eq!(root["title"], "Collection");
        assert_eq!(
            root["stageNodes"][0]["uuid"], PACK,
            "the pack's own identity"
        );

        // The same pack: its stages under their ids and names, its wheels,
        // where OK and home lead — and the controls of every stage but one.
        let mut expected = graph_of(&source_root);
        // The question played with every control off; read back from the
        // structure it plays through like any node (pause and home on).
        assert_eq!(expected[1]["name"], "Question");
        assert_eq!(
            expected[1]["controlSettings"],
            controls(false, false, false, false, true)
        );
        expected[1]["controlSettings"] = controls(false, false, true, true, true);
        assert_eq!(graph_of(&root), expected);
        for stage in 0..8 {
            assert_eq!(
                stage_asset(&entries, &root, stage, "audio"),
                stage_asset(&source, &source_root, stage, "audio"),
                "stage {stage} sound is carried byte for byte"
            );
        }
    }

    #[test]
    fn an_option_that_leads_nowhere_is_refused_before_any_destination() {
        let mut library = Library::new();
        let id = library.story("En cours");
        library.choice_structure(
            &id,
            "n1",
            &[
                ("n1", "Début", None, None, &["n2", "ailleurs"]),
                ("n2", "Suite", None, None, &[]),
            ],
        );
        let err = library.plan(&id).expect_err("refused");
        assert_eq!(err.code, AppErrorCode::ExportFailed);
        assert_eq!(details(&err)["source"], "story_pack");
        assert_eq!(details(&err)["cause"], "option_without_destination");
        assert_eq!(details(&err)["node"], "n1");
        assert_eq!(
            err.message,
            "Export impossible: une option de cette histoire ne mène à aucun nœud."
        );
        assert!(err
            .user_action
            .as_deref()
            .is_some_and(|action| action.contains("export")));
    }

    // ===== Per-media bound: what is exported is always imported whole =====

    /// A device-ready sound of `len` bytes: a conformant first frame header
    /// and filler. It passes the device normalization byte for byte — its
    /// SIZE is what the tests using it are about.
    fn device_ready_sound(len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        bytes[..DEVICE_SOUND_HEADER.len()].copy_from_slice(&DEVICE_SOUND_HEADER);
        bytes
    }

    /// The pack identity of the archive at `path` and the sound its stage
    /// `stage` plays — that one entry only (these archives carry sounds of
    /// tens of megabytes).
    fn stage_sound(path: &Path, stage: usize) -> (String, Vec<u8>) {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path).expect("open zip"))
            .expect("a zip archive");
        let mut descriptor = Vec::new();
        archive
            .by_name("story.json")
            .expect("descriptor")
            .read_to_end(&mut descriptor)
            .expect("read descriptor");
        let root: Value = serde_json::from_slice(&descriptor).expect("descriptor json");
        let pack = root["stageNodes"][0]["uuid"]
            .as_str()
            .expect("pack identity")
            .to_string();
        let name = root["stageNodes"][stage]["audio"]
            .as_str()
            .expect("sound reference");
        let mut sound = Vec::new();
        archive
            .by_name(&format!("assets/{name}"))
            .expect("sound entry")
            .read_to_end(&mut sound)
            .expect("read sound");
        (pack, sound)
    }

    /// Byte sizes of the sounds the story `story_id` holds in `library`.
    fn stored_sound_sizes(library: &Library, story_id: &str) -> Vec<u64> {
        let mut statement = library
            .db
            .conn()
            .prepare("SELECT byte_size FROM assets WHERE story_id = ?1 AND media_type = 'audio'")
            .expect("prepare");
        let rows = statement
            .query_map(rusqlite::params![story_id], |row| row.get(0))
            .expect("query");
        rows.collect::<Result<Vec<u64>, _>>().expect("rows")
    }

    /// A one-episode story whose only media is `sound`, stored as `ext`.
    fn one_sound_story(library: &mut Library, title: &str, ext: &str, sound: &[u8]) -> String {
        let id = library.story(title);
        let audio = format!("{}.{ext}", "a".repeat(56) + "1234abcd");
        library.asset(&id, "aud-1", "audio", &audio, sound);
        library.structure(&id, &[("n1", title, Some("aud-1"), None)]);
        id
    }

    #[test]
    fn a_sound_of_exactly_the_per_media_bound_travels_from_library_to_library() {
        // ONE bound on both sides of the round trip, and it is inclusive:
        // the largest sound a library holds is exported whole, imported
        // whole (nothing flagged, nothing left out), still sendable, and
        // exported again byte for byte.
        let mut library = Library::new();
        let id = one_sound_story(
            &mut library,
            "Longue veillée",
            "mp3",
            &device_ready_sound(MAX_ARCHIVE_MEDIA_BYTES as usize),
        );
        let out_dir = TempDir::new().expect("out");
        let shared = out_dir.path().join("veillee.zip");
        library
            .export(&id, &shared)
            .expect("a sound of exactly the bound exports");
        let (pack, exported) = stage_sound(&shared, 0);
        assert_eq!(pack, id);
        assert_eq!(exported.len() as u64, MAX_ARCHIVE_MEDIA_BYTES);

        let mut other = Library::new();
        let card =
            accept_structured_archive_creation(&mut other.db, other.app_data.path(), &shared)
                .expect("the shared archive imports");
        assert!(
            card.import_report.is_none(),
            "a clean import: no media was left out"
        );
        assert!(card.sendable, "the imported story can be sent to a device");
        let stored = stored_sound_sizes(&other, &card.id);
        assert!(!stored.is_empty());
        assert!(
            stored.iter().all(|size| *size == MAX_ARCHIVE_MEDIA_BYTES),
            "the sound is stored whole: {stored:?}"
        );

        let back = out_dir.path().join("retour.zip");
        other.export(&card.id, &back).expect("export back");
        let (pack_back, exported_back) = stage_sound(&back, 0);
        assert_eq!(pack_back, id, "the pack keeps its identity");
        assert!(
            exported_back == exported,
            "the sound is carried byte for byte"
        );
    }

    #[test]
    fn a_converted_sound_is_written_up_to_the_bound_included_and_refused_one_byte_over() {
        let mut library = Library::new();
        let id = one_sound_story(
            &mut library,
            "Seul",
            "wav",
            &wav_sine(44_100, 1, 440.0, 1.0, 0.5),
        );
        let plan = library.plan(&id).expect("plan");
        let out_dir = TempDir::new().expect("out");

        // What the sound weighs once converted to the device format.
        let reference = out_dir.path().join("reference.zip");
        export_story_archive(&plan, &library.media_dir(), &reference, &|_| {}).expect("export");
        let converted = stage_sound(&reference, 0).1.len() as u64;
        std::fs::remove_file(&reference).expect("remove the reference");

        // A bound of exactly that size admits it…
        let at_bound = out_dir.path().join("a-la-borne.zip");
        write_story_archive(&plan, &library.media_dir(), &at_bound, converted, &|_| {})
            .expect("a sound of exactly the bound is written");
        std::fs::remove_file(&at_bound).expect("remove the archive");

        // …one byte less refuses it BEFORE it is written: the reason is the
        // sound's length, and nothing is left behind.
        let destination = out_dir.path().join("refusee.zip");
        let err = write_story_archive(
            &plan,
            &library.media_dir(),
            &destination,
            converted - 1,
            &|_| {},
        )
        .expect_err("refused");
        assert_eq!(err.code, AppErrorCode::ExportFailed);
        assert_eq!(details(&err)["source"], "asset_convert");
        assert_eq!(details(&err)["cause"], "oversize");
        assert_eq!(details(&err)["byte_size"], converted);
        assert_eq!(details(&err)["max_bytes"], converted - 1);
        assert_eq!(details(&err)["asset"], "1234ABCD");
        assert!(
            err.message
                .starts_with("Export impossible: un son de l'histoire est trop long"),
            "{}",
            err.message
        );
        assert!(!destination.exists());
        assert_eq!(
            std::fs::read_dir(out_dir.path()).expect("out dir").count(),
            0,
            "the staged archive is dropped"
        );
    }

    #[test]
    fn the_too_long_refusal_names_the_listening_time_the_bound_stands_for() {
        // The figure the refusal (and the contract) quotes follows from the
        // two constants it is made of: the per-media bound and the bitrate
        // every transcoded sound is written at.
        assert_eq!(
            device_sound_duration_text(MAX_ARCHIVE_MEDIA_BYTES),
            "2 h 20"
        );
        assert_eq!(device_sound_duration_text(16_000 * 60 * 45), "45 min");
        assert_eq!(device_sound_duration_text(16_000 * 60 * 60), "1 h");

        let asset = StoryArchiveAsset {
            kind: StoryArchiveAssetKind::Audio,
            source_ref: format!("{}.wav", "a".repeat(56) + "1234abcd"),
            archive_name: format!("{}.mp3", "0".repeat(40)),
        };
        for (origin, next_step) in [
            (AssetOrigin::MediaStore, "épisodes plus courts"),
            (AssetOrigin::SourceArchive, "archive d'origine"),
        ] {
            let err = converted_oversize_error(
                origin,
                &asset,
                MAX_ARCHIVE_MEDIA_BYTES + 1,
                MAX_ARCHIVE_MEDIA_BYTES,
            );
            assert_eq!(
                err.message,
                "Export impossible: un son de l'histoire est trop long pour une archive (plus de 2 h 20 environ)."
            );
            assert!(err
                .user_action
                .as_deref()
                .is_some_and(|action| action.contains(next_step)));
        }
    }

    #[test]
    fn a_retained_pack_media_over_the_bound_is_refused_as_too_large_and_a_missing_one_as_missing() {
        let mut library = Library::new();
        let id = library.story("Importée");
        let audio = format!("{}.mp3", "d".repeat(32) + "0badf00d");
        let descriptor = format!(
            r#"{{"format":"v1","version":1,"title":"Importée","nightModeAvailable":false,"actionNodes":[],
            "stageNodes":[{{"uuid":"{id}","squareOne":true,"image":null,"audio":"{audio}",
              "okTransition":null,"homeTransition":null,
              "controlSettings":{{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}}}]}}"#
        );
        let retained = resolve_source_archive_path(library.app_data.path(), &id);
        std::fs::create_dir_all(retained.parent().expect("parent")).expect("dir");
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("histoire.zip");

        // The media IS in the retained pack, one byte over the bound: named
        // for what it is, before anything is staged.
        write_pack_archive(
            &retained,
            Some(&descriptor),
            &[(audio.as_str(), MAX_ARCHIVE_MEDIA_BYTES + 1)],
        );
        let err = library.export(&id, &destination).expect_err("refused");
        assert_eq!(err.code, AppErrorCode::ExportFailed);
        assert_eq!(details(&err)["source"], "source_archive");
        assert_eq!(details(&err)["cause"], "asset_oversize");
        assert_eq!(details(&err)["max_bytes"], MAX_ARCHIVE_MEDIA_BYTES);
        assert_eq!(details(&err)["asset"], "0BADF00D");
        assert_eq!(
            err.message,
            "Export impossible: un média du pack dépasse la taille autorisée (134 Mo)."
        );
        assert!(!destination.exists());
        assert_eq!(
            std::fs::read_dir(out_dir.path()).expect("out dir").count(),
            0,
            "nothing is staged"
        );

        // The media is NOT in the retained pack: the absence keeps its own
        // words.
        write_pack_archive(&retained, Some(&descriptor), &[]);
        let err = library.export(&id, &destination).expect_err("refused");
        assert_eq!(details(&err)["source"], "source_archive");
        assert_eq!(details(&err)["cause"], "asset_missing");
        assert_eq!(
            err.message,
            "Export impossible: un média du pack est introuvable dans son archive source."
        );
    }

    #[test]
    fn a_renamed_imported_story_is_exported_under_its_library_title() {
        let mut library = Library::new();
        let id = two_episode_story(&mut library);
        let out_dir = TempDir::new().expect("out");
        let shared = out_dir.path().join("partage.zip");
        library.export(&id, &shared).expect("export");

        let mut other = Library::new();
        let card =
            accept_structured_archive_creation(&mut other.db, other.app_data.path(), &shared)
                .expect("import");
        other
            .db
            .conn()
            .execute(
                "UPDATE stories SET title = 'Mes bestioles préférées' WHERE id = ?1",
                rusqlite::params![&card.id],
            )
            .expect("rename");
        let back = out_dir.path().join("renommee.zip");
        other.export(&card.id, &back).expect("export back");
        let root: Value =
            serde_json::from_slice(&read_archive(&back)["story.json"]).expect("descriptor");
        assert_eq!(root["title"], "Mes bestioles préférées");
    }

    #[test]
    fn a_story_without_a_cover_exports_without_a_thumbnail() {
        let mut library = Library::new();
        let id = library.story("Sans image");
        let audio = format!("{}.wav", "a".repeat(56) + "1234abcd");
        library.asset(
            &id,
            "aud-1",
            "audio",
            &audio,
            &wav_sine(44_100, 1, 440.0, 0.3, 0.5),
        );
        library.structure(&id, &[("n1", "Seul", Some("aud-1"), None)]);
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("sans-image.zip");

        let exported = library.export(&id, &destination).expect("export");
        assert_eq!((exported.image_count, exported.audio_count), (0, 1));
        let entries = read_archive(&destination);
        assert!(!entries.contains_key("thumbnail.png"));
        assert_eq!(entries.len(), 2, "the descriptor and one sound");
    }

    #[test]
    fn a_story_that_does_not_lay_out_as_a_pack_is_refused_before_any_destination() {
        let mut library = Library::new();
        let id = library.story("Vide");
        let err = library.plan(&id).expect_err("refused");
        assert_eq!(err.code, AppErrorCode::ExportFailed);
        assert_eq!(details(&err)["source"], "story_pack");
        assert_eq!(details(&err)["cause"], "missing_audio");
        assert!(err.message.starts_with("Export impossible:"));
        assert!(err
            .user_action
            .as_deref()
            .is_some_and(|a| a.contains("export")));
    }

    #[test]
    fn every_blocker_has_an_export_worded_refusal_with_a_next_step() {
        for blocker in [
            StoryPackBlocker::Empty,
            StoryPackBlocker::Malformed,
            StoryPackBlocker::Branching,
            StoryPackBlocker::MissingAudio,
            StoryPackBlocker::MissingAnnouncements,
        ] {
            let err = refusal_error(StoryPackRefusal::Blocked(blocker));
            assert_eq!(err.code, AppErrorCode::ExportFailed);
            assert!(err.message.starts_with("Export impossible:"), "{blocker:?}");
            assert!(!err.message.contains("Envoi"), "{blocker:?}");
            assert!(!err.user_action.as_deref().unwrap_or("").is_empty());
            assert_eq!(details(&err)["cause"], blocker.diagnostic_tag());
        }
        let device_pack = refusal_error(StoryPackRefusal::DevicePack);
        assert_eq!(device_pack.code, AppErrorCode::ExportFailed);
        assert_eq!(details(&device_pack)["cause"], "device_pack");
    }

    #[test]
    fn an_unknown_story_is_a_library_inconsistency_whatever_its_source() {
        let library = Library::new();
        let err = library.plan("nope").expect_err("refused");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        assert_eq!(details(&err)["source"], "story_missing");
        // With a retained archive on record, the title read finds no row.
        let err = plan_story_archive_export(&library.db, "nope", Some(PathBuf::from("/x.zip")))
            .expect_err("refused");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
    }

    #[test]
    fn a_media_gone_from_the_store_is_refused_and_nothing_is_written() {
        let mut library = Library::new();
        let id = two_episode_story(&mut library);
        let plan = library.plan(&id).expect("plan");
        // The file disappears between the plan and the export.
        std::fs::remove_file(
            library
                .media_dir()
                .join(format!("{}.wav", "c".repeat(56) + "9abc0123")),
        )
        .expect("remove");
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("histoire.zip");

        let err = export_story_archive(&plan, &library.media_dir(), &destination, &|_| {})
            .expect_err("refused");
        assert_eq!(err.code, AppErrorCode::ExportFailed);
        assert_eq!(details(&err)["source"], "media_store");
        assert_eq!(details(&err)["cause"], "asset_missing");
        assert_eq!(details(&err)["asset"], "9ABC0123");
        assert!(details(&err).get("path").is_none());
        assert!(!destination.exists());
        assert_eq!(
            std::fs::read_dir(out_dir.path()).expect("out dir").count(),
            0,
            "no temp file is left behind"
        );
    }

    #[test]
    fn a_media_that_cannot_be_converted_fails_the_export_and_leaves_no_residue() {
        let mut library = Library::new();
        let id = library.story("Abîmée");
        // A RIFF/WAVE header with no audio behind it: stored, but undecodable.
        let mut broken = b"RIFF\x24\x00\x00\x00WAVEfmt ".to_vec();
        broken.extend_from_slice(&[0u8; 24]);
        let audio = format!("{}.wav", "d".repeat(56) + "0badf00d");
        library.asset(&id, "aud-1", "audio", &audio, &broken);
        library.structure(&id, &[("n1", "Abîmé", Some("aud-1"), None)]);
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("histoire.zip");

        let err = library.export(&id, &destination).expect_err("refused");
        assert_eq!(err.code, AppErrorCode::ExportFailed);
        assert_eq!(details(&err)["source"], "asset_convert");
        assert_eq!(details(&err)["cause"], "audio_unsupported");
        assert_eq!(details(&err)["asset"], "0BADF00D");
        assert!(!destination.exists());
        assert_eq!(
            std::fs::read_dir(out_dir.path()).expect("out dir").count(),
            0,
            "the staged archive is dropped"
        );
    }

    #[test]
    fn a_retained_source_pack_that_cannot_be_read_is_refused() {
        let mut library = Library::new();
        let id = library.story("Importée");
        let retained = resolve_source_archive_path(library.app_data.path(), &id);
        std::fs::create_dir_all(retained.parent().expect("parent")).expect("dir");
        std::fs::write(&retained, b"not a zip").expect("write");
        let out_dir = TempDir::new().expect("out");

        let err = library
            .export(&id, &out_dir.path().join("x.zip"))
            .expect_err("refused");
        assert_eq!(err.code, AppErrorCode::ExportFailed);
        assert_eq!(details(&err)["source"], "source_archive");
        assert_eq!(details(&err)["cause"], "not_a_zip");
    }

    #[test]
    fn a_missing_destination_folder_is_a_destination_refusal() {
        let mut library = Library::new();
        let id = two_episode_story(&mut library);
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("absent").join("histoire.zip");

        let err = library.export(&id, &destination).expect_err("refused");
        assert_eq!(err.code, AppErrorCode::ExportDestinationUnavailable);
        assert_eq!(details(&err)["source"], "parent_missing");
    }

    #[cfg(unix)]
    #[test]
    fn the_archive_is_saved_with_the_permissions_of_any_saved_document() {
        use std::os::unix::fs::PermissionsExt;

        let mut library = Library::new();
        let id = two_episode_story(&mut library);
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("histoire.zip");
        library.export(&id, &destination).expect("export");

        // What the platform gives a plainly created file here (default mode
        // narrowed by the umask) is what the archive must carry — never the
        // owner-only mode of the temp file it was staged in.
        let reference = out_dir.path().join("reference");
        std::fs::File::create(&reference).expect("reference file");
        let mode =
            |path: &Path| std::fs::metadata(path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode(&destination), mode(&reference));
    }

    #[test]
    fn an_existing_destination_is_replaced_atomically() {
        let mut library = Library::new();
        let id = two_episode_story(&mut library);
        let out_dir = TempDir::new().expect("out");
        let destination = out_dir.path().join("histoire.zip");
        std::fs::write(&destination, b"an older file").expect("seed");

        library.export(&id, &destination).expect("export");
        assert!(read_archive(&destination).contains_key("story.json"));
        assert_eq!(
            std::fs::read_dir(out_dir.path()).expect("out dir").count(),
            1,
            "only the archive remains"
        );
    }

    /// Writes two sample archives — a sequential story and a menu story —
    /// into the directory named by `RUSTORY_TEST_EXPORT_DIR`, so the files
    /// the export really produces can be opened in another tool (another
    /// Rustory, a pack manager) or checked by an independent reader.
    /// Env-gated like the device ground-truth harnesses; skipped otherwise.
    #[test]
    #[ignore]
    fn writes_sample_archives_for_inspection_by_other_tools() {
        let Ok(out_dir) = std::env::var("RUSTORY_TEST_EXPORT_DIR") else {
            eprintln!("skipped: set RUSTORY_TEST_EXPORT_DIR to a writable directory");
            return;
        };
        let out_dir = PathBuf::from(out_dir);

        let mut library = Library::new();
        let sequential = two_episode_story(&mut library);
        library
            .export(&sequential, &out_dir.join("exemple-sequentiel.zip"))
            .expect("export the sequential sample");

        // The same two episodes behind a spoken menu: a question and one
        // spoken title per episode, like the announcements the app records
        // or synthesizes.
        let menu = library.story("Les Bestioles au choix");
        let first = format!("{}.wav", "e".repeat(56) + "1111aaaa");
        let second = format!("{}.wav", "e".repeat(56) + "2222bbbb");
        let question = format!("{}.wav", "f".repeat(56) + "0000cccc");
        let prompt_one = format!("{}.wav", "f".repeat(56) + "1111dddd");
        let prompt_two = format!("{}.wav", "f".repeat(56) + "2222eeee");
        let picture = format!("{}.png", "9".repeat(56) + "3333ffff");
        library.asset(&menu, "m-img", "image", &picture, &png(160, 120));
        for (id, file_name, hz) in [
            ("m-aud-1", &first, 330.0),
            ("m-aud-2", &second, 392.0),
            ("m-question", &question, 523.0),
            ("m-prompt-1", &prompt_one, 587.0),
            ("m-prompt-2", &prompt_two, 659.0),
        ] {
            library.asset(
                &menu,
                id,
                "audio",
                file_name,
                &wav_sine(44_100, 1, hz, 0.5, 0.5),
            );
        }
        library.structure(
            &menu,
            &[
                ("n1", "Le loup", Some("m-aud-1"), Some("m-img")),
                ("n2", "La pie", Some("m-aud-2"), None),
            ],
        );
        library
            .db
            .conn()
            .execute(
                "INSERT INTO story_layouts (story_id, layout, question_audio_asset_id, question_spoken_text, voice_id, updated_at) \
                 VALUES (?1, 'menu', 'm-question', 'Quelle histoire veux-tu écouter ?', 'v', '2026-01-01T00:00:00Z')",
                rusqlite::params![&menu],
            )
            .expect("menu layout");
        for (node_id, asset) in [("n1", "m-prompt-1"), ("n2", "m-prompt-2")] {
            library
                .db
                .conn()
                .execute(
                    "INSERT INTO story_node_prompts (story_id, node_id, audio_asset_id, spoken_text, voice_id, updated_at) \
                     VALUES (?1, ?2, ?3, 't', 'v', '2026-01-01T00:00:00Z')",
                    rusqlite::params![&menu, node_id, asset],
                )
                .expect("spoken title");
        }
        let exported = library
            .export(&menu, &out_dir.join("exemple-menu.zip"))
            .expect("export the menu sample");
        // question + two spoken titles + two episodes; one picture.
        assert_eq!((exported.image_count, exported.audio_count), (1, 5));

        // A story with choices written in the editor, laid out by its own
        // graph (a library of its own: the samples reuse asset ids).
        let mut choices_library = Library::new();
        let crossroads = crossroads_story(&mut choices_library);
        choices_library
            .export(&crossroads, &out_dir.join("exemple-choix.zip"))
            .expect("export the choices sample");

        // A pack read back from the story it became, its archive gone.
        let pack_path = out_dir.join("pack-source.zip");
        menu_pack_archive(&pack_path, "01a06ed9-2040-77c1-9e03-b8f429f4e954", 3);
        let mut reader = Library::new();
        let card =
            accept_structured_archive_creation(&mut reader.db, reader.app_data.path(), &pack_path)
                .expect("the pack imports");
        drop_retained_archive(&reader, &card.id);
        reader
            .export(&card.id, &out_dir.join("exemple-pack-relu.zip"))
            .expect("export the read-back pack sample");
        std::fs::remove_file(&pack_path).expect("drop the source pack");
    }
}
