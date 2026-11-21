//! The story archive export, end to end through the public services: a
//! story built with the editor's own writes (create, add an episode, label,
//! attach real media) is exported as an archive, read back as the zip it
//! is, and imported by a second, empty library — the "share it with another
//! Rustory" path, with no device anywhere.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use rustory_lib::application::import_export::{
    accept_structured_archive_creation, export_story_archive, plan_story_archive_export,
};

use rustory_lib::application::story::node::{
    attach_node_media, save_node_content, AttachNodeMediaInput, SaveNodeContentInput,
};
use rustory_lib::application::story::structure::add_node;
use rustory_lib::application::story::{create_story, CreateStoryInput};
use rustory_lib::domain::shared::AppErrorCode;
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::device::{audio_is_device_ready, to_device_image};
use rustory_lib::infrastructure::filesystem::{
    resolve_db_path, resolve_node_media_dir, resolve_source_archive_path, MediaKind,
};
use tempfile::TempDir;

/// A library on disk: its app-data directory and its migrated database.
struct Library {
    app_data: TempDir,
    db: DbHandle,
}

fn library() -> Library {
    let app_data = TempDir::new().expect("app data");
    let mut db = db::open_at(&resolve_db_path(app_data.path())).expect("open db");
    db::run_migrations(&mut db).expect("migrate");
    Library { app_data, db }
}

/// A 16-bit PCM WAV of a sine tone — real, decodable audio.
fn wav(rate: u32, channels: u16, seconds: f64) -> Vec<u8> {
    let frames = (f64::from(rate) * seconds) as usize;
    let data_len = frames * usize::from(channels) * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        let phase = 2.0 * std::f64::consts::PI * 440.0 * i as f64 / f64::from(rate);
        let sample = (0.4 * phase.sin() * f64::from(i16::MAX)) as i16;
        for _ in 0..channels {
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
    out
}

/// A real PNG picture.
fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(96, 64, |x, y| {
        image::Rgba([(x * 2) as u8, (y * 3) as u8, 120, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode png");
    out.into_inner()
}

/// Build "Les Bestioles": two labeled episodes with their sounds, the first
/// one with a picture — written the way the editor writes them.
fn bestioles(library: &mut Library) -> String {
    let id = create_story(
        &mut library.db,
        CreateStoryInput {
            title: "Les Bestioles".into(),
        },
    )
    .expect("create story")
    .id;
    let added = add_node(&mut library.db, &id, None).expect("add an episode");
    let second = added.structure.nodes[1].id.clone();
    for (node_id, label, audio) in [
        ("n1", "Le loup", wav(48_000, 2, 0.4)),
        (second.as_str(), "La pie", wav(44_100, 1, 0.4)),
    ] {
        save_node_content(
            &mut library.db,
            library.app_data.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: node_id.into(),
                text: String::new(),
                label: label.into(),
            },
        )
        .expect("label the episode");
        attach_node_media(
            &mut library.db,
            library.app_data.path(),
            AttachNodeMediaInput {
                story_id: id.clone(),
                node_id: node_id.into(),
                kind: MediaKind::Audio,
                bytes: audio,
            },
        )
        .expect("attach the sound");
    }
    attach_node_media(
        &mut library.db,
        library.app_data.path(),
        AttachNodeMediaInput {
            story_id: id.clone(),
            node_id: "n1".into(),
            kind: MediaKind::Image,
            bytes: png(),
        },
    )
    .expect("attach the picture");
    id
}

fn export(library: &Library, story_id: &str, destination: &Path) {
    let retained = resolve_source_archive_path(library.app_data.path(), story_id);
    let plan = plan_story_archive_export(
        &library.db,
        story_id,
        retained.is_file().then_some(retained),
    )
    .expect("plan the export");
    export_story_archive(
        &plan,
        &resolve_node_media_dir(library.app_data.path()),
        destination,
        &|_| {},
    )
    .expect("export the archive");
}

fn read_zip(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(path).expect("open")).expect("a zip archive");
    (0..archive.len())
        .map(|index| {
            let mut entry = archive.by_index(index).expect("entry");
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read");
            (entry.name().to_string(), bytes)
        })
        .collect()
}

#[test]
fn a_story_written_in_the_editor_is_exported_as_a_device_ready_archive() {
    let mut source = library();
    let id = bestioles(&mut source);
    let out = TempDir::new().expect("out");
    let archive = out.path().join("Les Bestioles.zip");

    export(&source, &id, &archive);

    let entries = read_zip(&archive);
    let root: serde_json::Value =
        serde_json::from_slice(&entries["story.json"]).expect("the descriptor is JSON");
    assert_eq!(root["title"], "Les Bestioles");
    let stages = root["stageNodes"].as_array().expect("stage nodes");
    let names: Vec<&str> = stages
        .iter()
        .map(|stage| stage["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, vec!["Les Bestioles", "Le loup", "La pie"]);
    assert_eq!(stages[0]["uuid"], id.as_str(), "the pack is the story");

    // Every asset the descriptor names is in the archive, in the format a
    // device plays: a 320×240 16-color bitmap, a bare mono 44.1 kHz MP3.
    let mut images = 0;
    let mut sounds = 0;
    for stage in stages {
        if let Some(name) = stage["image"].as_str() {
            let bytes = &entries[&format!("assets/{name}")];
            assert_eq!(&to_device_image(bytes).expect("a device image"), bytes);
            images += 1;
        }
        if let Some(name) = stage["audio"].as_str() {
            let bytes = &entries[&format!("assets/{name}")];
            assert!(audio_is_device_ready(bytes), "{name} is a device MP3");
            sounds += 1;
        }
    }
    assert_eq!((images, sounds), (2, 3), "the cover replays episode 1");
    assert!(entries.contains_key("thumbnail.png"), "the cover picture");
}

#[test]
fn another_library_imports_the_shared_archive_as_a_sendable_story() {
    let mut source = library();
    let id = bestioles(&mut source);
    let out = TempDir::new().expect("out");
    let archive = out.path().join("Les Bestioles.zip");
    export(&source, &id, &archive);

    let mut other = library();
    let card = accept_structured_archive_creation(&mut other.db, other.app_data.path(), &archive)
        .expect("the shared archive imports");
    assert_eq!(card.title, "Les Bestioles");
    assert!(card.import_report.is_none(), "nothing to review");

    // The second library holds it like any imported pack: sendable to a
    // device from its retained archive, with every media kept.
    assert!(card.sendable);
    assert!(resolve_source_archive_path(other.app_data.path(), &card.id).is_file());
    let (images, sounds): (i64, i64) = other
        .db
        .conn()
        .query_row(
            "SELECT SUM(media_type = 'image'), SUM(media_type = 'audio') \
             FROM assets WHERE story_id = ?1",
            rusqlite::params![&card.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("count the imported media");
    assert_eq!((images, sounds), (2, 3), "cover + two episodes");

    // …and can share it onward: exported again, the pack keeps its identity.
    let onward = out.path().join("plus-loin.zip");
    export(&other, &card.id, &onward);
    let root: serde_json::Value =
        serde_json::from_slice(&read_zip(&onward)["story.json"]).expect("descriptor");
    assert_eq!(root["stageNodes"][0]["uuid"], id.as_str());
}

#[test]
fn a_freshly_created_story_is_refused_with_the_reason_and_writes_nothing() {
    let mut source = library();
    let id = create_story(
        &mut source.db,
        CreateStoryInput {
            title: "Vide".into(),
        },
    )
    .expect("create")
    .id;

    let err = plan_story_archive_export(&source.db, &id, None).expect_err("refused");
    assert_eq!(err.code, AppErrorCode::ExportFailed);
    let wire = serde_json::to_value(&err).expect("serialize");
    assert_eq!(wire["code"], "EXPORT_FAILED");
    assert_eq!(wire["details"]["source"], "story_pack");
    assert_eq!(wire["details"]["cause"], "missing_audio");
}
