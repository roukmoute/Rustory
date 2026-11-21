//! A story WITH CHOICES sent to a device, end to end through the public
//! services: written with the editor's own writes (options created and
//! linked, a voice on every node) or created from a pack whose archive is
//! gone, planned, then written onto a scratch V3 volume by the real pack
//! writer — and read back the way the device reads it: the cleartext node
//! index, the deciphered option lists, pictures and sounds.

use std::path::Path;
use std::time::Duration;

use rustory_lib::application::device::send::{
    send_archive_to_device, send_story_pack_to_device, SendArchiveRequest, SendStoryPackRequest,
};
use rustory_lib::application::device::story_pack::plan_story_pack;
use rustory_lib::application::device::{read_connected_lunii, ConnectedLuniiOutcome};
use rustory_lib::application::import_export::{
    accept_structured_archive_creation, export_story_archive, plan_story_archive_export,
};
use rustory_lib::application::story::node::{
    attach_node_media, save_node_content, store_node_media, AttachNodeMediaInput,
    SaveNodeContentInput,
};
use rustory_lib::application::story::presentation::{
    attach_recorded_announcement, set_layout, AnnouncementTarget,
};
use rustory_lib::application::story::structure::{add_node, add_option, set_option_link};
use rustory_lib::application::story::{create_story, CreateStoryInput};
use rustory_lib::domain::device::StoryLayout;
use rustory_lib::domain::shared::AppErrorCode;
use rustory_lib::domain::transfer::{pack_uuid_bytes, short_id_from_pack_uuid};
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::device::{
    audio_is_device_ready, v3_decipher_in_place, v3_story_key_iv, SystemDeviceScanner,
    SystemDeviceV3PackWriter,
};
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

/// A 16-bit PCM mono WAV of a sine tone — real, decodable audio. A distinct
/// duration is a distinct sound (the media store names a file by content).
fn wav(seconds: f64) -> Vec<u8> {
    const RATE: u32 = 44_100;
    let frames = (f64::from(RATE) * seconds) as usize;
    let data_len = frames * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        let phase = 2.0 * std::f64::consts::PI * 440.0 * i as f64 / f64::from(RATE);
        let sample = (0.4 * phase.sin() * f64::from(i16::MAX)) as i16;
        out.extend_from_slice(&sample.to_le_bytes());
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

/// Label a node and give it a sound, the way the editor writes them.
fn voice(library: &mut Library, story_id: &str, node_id: &str, label: &str, seconds: f64) {
    save_node_content(
        &mut library.db,
        library.app_data.path(),
        SaveNodeContentInput {
            story_id: story_id.into(),
            node_id: node_id.into(),
            text: String::new(),
            label: label.into(),
        },
    )
    .expect("label the node");
    attach_node_media(
        &mut library.db,
        library.app_data.path(),
        AttachNodeMediaInput {
            story_id: story_id.into(),
            node_id: node_id.into(),
            kind: MediaKind::Audio,
            bytes: wav(seconds),
        },
    )
    .expect("attach the sound");
}

/// Another pack already on the volume (a `.pi` holds whole 16-byte entries).
const OTHER_PACK: [u8; 16] = [
    0x01, 0xa0, 0x6e, 0xd9, 0x20, 0x40, 0x77, 0xc1, 0x9e, 0x03, 0xb8, 0xf4, 0x29, 0xf4, 0xe9, 0x54,
];

/// A scratch volume carrying the markers of a Lunii V3: a `.md` of the V3
/// metadata version with a serial and a content key, a `.pi`, a `.bt`.
fn scratch_v3_volume() -> (TempDir, Vec<u8>) {
    let dir = TempDir::new().expect("volume");
    let mut md = vec![0u8; 256];
    md[0] = 7;
    md[0x1A..0x28].copy_from_slice(b"0123456789abcd");
    for (i, byte) in md.iter_mut().enumerate().skip(0x40).take(32) {
        *byte = i as u8;
    }
    std::fs::write(dir.path().join(".md"), &md).expect(".md");
    std::fs::write(dir.path().join(".pi"), OTHER_PACK).expect(".pi");
    std::fs::write(dir.path().join(".bt"), b"bt").expect(".bt");
    (dir, md)
}

/// Send `story_id` to the scratch volume through the application's own
/// flow: the plan under the library, then the live re-scan, the engine and
/// the real writer.
fn send(
    library: &Library,
    volume: &Path,
    story_id: &str,
) -> Result<
    rustory_lib::application::device::send::SentToDevice,
    rustory_lib::domain::shared::AppError,
> {
    let pack = plan_story_pack(&library.db, story_id)?;
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![volume.to_path_buf()]);
    send_story_pack_to_device(
        &scanner,
        &SystemDeviceV3PackWriter,
        &SendStoryPackRequest {
            device_identifier: device_identifier(&scanner),
            pack,
            media_dir: resolve_node_media_dir(library.app_data.path()),
        },
        Duration::from_secs(30),
        &|_| {},
    )
}

/// The identifier the device panel holds: read from the volume itself (it
/// follows the `.pi`, so it is read again before every send).
fn device_identifier(scanner: &SystemDeviceScanner) -> String {
    match read_connected_lunii(scanner, Duration::from_secs(30)).expect("scan the volume") {
        ConnectedLuniiOutcome::Supported(profile) => profile.device_identifier,
        other => panic!("the scratch volume is not a supported device: {other:?}"),
    }
}

/// One stage as the device's node index records it.
#[derive(Debug, PartialEq, Eq)]
struct DeviceStage {
    image: i32,
    audio: i32,
    /// (first entry in the option lists, number of entries, entry selected).
    ok: (i32, i32, i32),
    home: (i32, i32, i32),
    /// wheel, ok, home, pause, autoplay.
    controls: (bool, bool, bool, bool, bool),
}

const NONE: (i32, i32, i32) = (-1, -1, -1);
const COVER: (bool, bool, bool, bool, bool) = (true, true, false, false, false);
const WHEEL_ENTRY: (bool, bool, bool, bool, bool) = (true, true, true, false, false);
const PLAYED: (bool, bool, bool, bool, bool) = (false, false, true, true, true);

/// Read a pack back from the volume: its stages (the cleartext `ni`), its
/// option lists (`li`, deciphered with the volume's key) and how many
/// pictures and sounds its index announces.
fn read_device_pack(content: &Path, md: &[u8]) -> (Vec<DeviceStage>, Vec<i32>, (u32, u32)) {
    let ni = std::fs::read(content.join("ni")).expect("ni");
    let u32_at = |at: usize| u32::from_le_bytes(ni[at..at + 4].try_into().expect("u32"));
    let i32_at = |at: usize| i32::from_le_bytes(ni[at..at + 4].try_into().expect("i32"));
    let flag_at = |at: usize| i16::from_le_bytes(ni[at..at + 2].try_into().expect("i16")) != 0;
    let (offset, record, count) = (u32_at(4) as usize, u32_at(8) as usize, u32_at(12) as usize);
    assert_eq!((offset, record), (512, 44), "the node index layout");
    assert_eq!(ni.len(), offset + record * count);
    let stages: Vec<DeviceStage> = (0..count)
        .map(|index| {
            let at = offset + index * record;
            DeviceStage {
                image: i32_at(at),
                audio: i32_at(at + 4),
                ok: (i32_at(at + 8), i32_at(at + 12), i32_at(at + 16)),
                home: (i32_at(at + 20), i32_at(at + 24), i32_at(at + 28)),
                controls: (
                    flag_at(at + 32),
                    flag_at(at + 34),
                    flag_at(at + 36),
                    flag_at(at + 38),
                    flag_at(at + 40),
                ),
            }
        })
        .collect();

    let (key, iv) = v3_story_key_iv(md).expect("the volume's key");
    let mut li = std::fs::read(content.join("li")).expect("li");
    v3_decipher_in_place(&mut li, &key, &iv);
    // A short list file is zero-padded to a whole cipher block: the node
    // index is the authority on how many entries the lists hold.
    let entries = stages
        .iter()
        .flat_map(|stage| [stage.ok, stage.home])
        .filter(|(first, _, _)| *first >= 0)
        .map(|(first, count, _)| (first + count) as usize)
        .max()
        .unwrap_or(0);
    let li = li
        .chunks_exact(4)
        .take(entries)
        .map(|entry| i32::from_le_bytes(entry.try_into().expect("i32")))
        .collect();
    (stages, li, (u32_at(16), u32_at(20)))
}

/// The on-device names of the pack's pictures (`ri`) or sounds (`si`), in
/// index order: 12-byte `000\XXXXXXXX` entries, deciphered.
fn read_device_asset_names(content: &Path, index: &str, count: usize, md: &[u8]) -> Vec<String> {
    let (key, iv) = v3_story_key_iv(md).expect("the volume's key");
    let mut table = std::fs::read(content.join(index)).expect("asset index");
    v3_decipher_in_place(&mut table, &key, &iv);
    table
        .chunks_exact(12)
        .take(count)
        .map(|entry| {
            assert_eq!(&entry[..4], b"000\\");
            String::from_utf8(entry[4..].to_vec()).expect("an ascii name")
        })
        .collect()
}

#[test]
fn a_story_with_choices_written_in_the_editor_is_sent_and_plays_by_its_options() {
    let mut library = library();
    let id = create_story(
        &mut library.db,
        CreateStoryInput {
            title: "Le Chemin".into(),
        },
    )
    .expect("create story")
    .id;
    // The crossroads offers two ways; each option is created and linked to
    // a new node in one gesture, like the editor's `Créer et lier`.
    add_option(&mut library.db, &id, "n1", "À gauche").expect("first option");
    let left = add_node(&mut library.db, &id, Some(("n1", 0)))
        .expect("create and link")
        .structure
        .nodes[1]
        .id
        .clone();
    add_option(&mut library.db, &id, "n1", "À droite").expect("second option");
    let right = add_node(&mut library.db, &id, Some(("n1", 1)))
        .expect("create and link")
        .structure
        .nodes[2]
        .id
        .clone();
    // The river leads to a tale…
    add_option(&mut library.db, &id, &right, "Écouter le conte").expect("third option");
    let tale = add_node(&mut library.db, &id, Some((right.as_str(), 0)))
        .expect("create and link")
        .structure
        .nodes[3]
        .id
        .clone();
    // …which will offer the two ways again — not linked yet.
    add_option(&mut library.db, &id, &tale, "À gauche").expect("fourth option");
    add_option(&mut library.db, &id, &tale, "À droite").expect("fifth option");
    voice(&mut library, &id, "n1", "Le carrefour", 0.30);
    voice(&mut library, &id, &left, "La forêt", 0.35);
    voice(&mut library, &id, &right, "La rivière", 0.40);
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

    let (volume, md) = scratch_v3_volume();
    let short_id = short_id_from_pack_uuid(&id).expect("a short id");
    let content = volume.path().join(".content").join(&short_id);

    // An option that leads nowhere: refused with its reason, by node,
    // before the volume is touched.
    let err = send(&library, volume.path(), &id).expect_err("refused");
    assert_eq!(err.code, AppErrorCode::DeviceWriteFailed);
    let wire = serde_json::to_value(&err).expect("serialize");
    assert_eq!(wire["details"]["source"], "story_pack");
    assert_eq!(wire["details"]["cause"], "option_without_destination");
    assert_eq!(wire["details"]["node"], tale.as_str());
    set_option_link(&mut library.db, &id, &tale, 0, Some(left.as_str())).expect("link the option");
    set_option_link(&mut library.db, &id, &tale, 1, Some(right.as_str())).expect("link the option");

    // A node without a sound: the device would stay mute on it — refused
    // with its reason, by node, the volume still untouched.
    let err = send(&library, volume.path(), &id).expect_err("refused");
    assert_eq!(err.code, AppErrorCode::DeviceWriteFailed);
    let wire = serde_json::to_value(&err).expect("serialize");
    assert_eq!(wire["details"]["cause"], "node_without_audio");
    assert_eq!(wire["details"]["node"], tale.as_str());
    assert!(!volume.path().join(".content").exists(), "nothing written");
    assert_eq!(
        std::fs::read(volume.path().join(".pi")).expect(".pi"),
        OTHER_PACK,
        "the device index is untouched"
    );
    voice(&mut library, &id, &tale, "Le conte", 0.45);

    // Every option leads to a node, every node has a sound: the story is
    // written as a pack filed under its own id.
    let sent = send(&library, volume.path(), &id).expect("send the story");
    assert_eq!(sent.pack_uuid, id);
    assert_eq!(sent.short_id, short_id);
    assert_eq!((sent.image_count, sent.audio_count), (1, 4));

    // What the device reads: one stage per node, the start node first.
    let (stages, option_lists, (image_count, audio_count)) = read_device_pack(&content, &md);
    assert_eq!((image_count, audio_count), (1, 4));
    // Two option lists: the two ways (offered by the crossroads AND by the
    // tale, which share them), then the tale alone.
    assert_eq!(option_lists, vec![1, 2, 3]);
    let two_ways = |selected: i32| (0, 2, selected);
    assert_eq!(
        stages,
        vec![
            // The crossroads is the cover: OK opens the wheel of the two
            // ways, on the first one.
            DeviceStage {
                image: 0,
                audio: 0,
                ok: two_ways(0),
                home: NONE,
                controls: COVER,
            },
            // The forest, an entry of the wheel: it waits for the listener
            // and ends the pack. Home has no wheel above to return to.
            DeviceStage {
                image: -1,
                audio: 1,
                ok: NONE,
                home: NONE,
                controls: WHEEL_ENTRY,
            },
            // The river, the other entry: OK plays the tale.
            DeviceStage {
                image: -1,
                audio: 2,
                ok: (2, 1, 0),
                home: NONE,
                controls: WHEEL_ENTRY,
            },
            // The tale plays through, then returns to the wheel ON THE
            // RIVER — the entry the listener came through; home leads to
            // that same spot.
            DeviceStage {
                image: -1,
                audio: 3,
                ok: two_ways(1),
                home: two_ways(1),
                controls: PLAYED,
            },
        ]
    );

    // Every sound the index names is on the volume, in the device format.
    let (key, iv) = v3_story_key_iv(&md).expect("the volume's key");
    let sounds = read_device_asset_names(&content, "si", 4, &md);
    assert_eq!(sounds.len(), 4);
    for name in &sounds {
        let mut sound = std::fs::read(content.join("sf/000").join(name)).expect("a sound");
        v3_decipher_in_place(&mut sound, &key, &iv);
        assert!(audio_is_device_ready(&sound), "{name} is a device sound");
    }
    let pictures = read_device_asset_names(&content, "ri", 1, &md);
    assert_eq!(pictures.len(), 1);
    assert!(content.join("rf/000").join(&pictures[0]).is_file());
    assert_eq!(std::fs::read(content.join("bt")).expect("bt").len(), 32);
    // The pack is indexed once, after the one that was already there.
    let story_entry = pack_uuid_bytes(&id).expect("uuid bytes");
    let pi = std::fs::read(volume.path().join(".pi")).expect(".pi");
    assert_eq!(pi, [OTHER_PACK, story_entry].concat());

    // The story changes — the forest now leads to the tale too — and is
    // sent again: its pack is REPLACED, never added beside the first one.
    add_option(&mut library.db, &id, &left, "Écouter le conte").expect("another option");
    set_option_link(&mut library.db, &id, &left, 0, Some(tale.as_str())).expect("link the option");
    send(&library, volume.path(), &id).expect("send again");
    let (stages, option_lists, _) = read_device_pack(&content, &md);
    assert_eq!(
        option_lists,
        vec![1, 2, 3],
        "the forest shares the tale's list"
    );
    assert_eq!(stages[1].ok, (2, 1, 0));
    let pi = std::fs::read(volume.path().join(".pi")).expect(".pi");
    assert_eq!(pi, [OTHER_PACK, story_entry].concat());
}

/// Give `target` its spoken announcement, recorded — the way the editor's
/// microphone gesture writes it.
fn announce(library: &mut Library, story_id: &str, target: AnnouncementTarget, seconds: f64) {
    let prepared = store_node_media(library.app_data.path(), MediaKind::Audio, &wav(seconds))
        .expect("store the recording");
    attach_recorded_announcement(&mut library.db, story_id, &target, prepared)
        .expect("attach the announcement");
}

/// One stage as the listener meets it on the device: what it shows and
/// plays (the deciphered files), and where OK and HOME lead — the entries
/// of the wheel entered (stage indices) and the entry it is entered on.
struct PlayedStage {
    image: Option<Vec<u8>>,
    audio: Option<Vec<u8>>,
    ok: Option<(Vec<i32>, i32)>,
    home: Option<(Vec<i32>, i32)>,
    controls: (bool, bool, bool, bool, bool),
}

/// Read a pack back from the volume as the listener meets it.
fn played_stages(content: &Path, md: &[u8]) -> Vec<PlayedStage> {
    let (stages, option_lists, (image_count, audio_count)) = read_device_pack(content, md);
    let pictures = read_device_asset_names(content, "ri", image_count as usize, md);
    let sounds = read_device_asset_names(content, "si", audio_count as usize, md);
    let (key, iv) = v3_story_key_iv(md).expect("the volume's key");
    let asset = |dir: &str, names: &[String], index: i32| -> Option<Vec<u8>> {
        let name = names.get(usize::try_from(index).ok()?)?;
        let mut bytes = std::fs::read(content.join(dir).join(name)).expect("an asset");
        v3_decipher_in_place(&mut bytes, &key, &iv);
        Some(bytes)
    };
    let wheel = |(first, count, selected): (i32, i32, i32)| -> Option<(Vec<i32>, i32)> {
        let first = usize::try_from(first).ok()?;
        let entries = option_lists[first..first + count as usize].to_vec();
        Some((entries, selected))
    };
    stages
        .iter()
        .map(|stage| PlayedStage {
            image: asset("rf/000", &pictures, stage.image),
            audio: asset("sf/000", &sounds, stage.audio),
            ok: wheel(stage.ok),
            home: wheel(stage.home),
            controls: stage.controls,
        })
        .collect()
}

#[test]
fn a_collection_created_from_a_pack_whose_archive_is_gone_is_sent_as_that_pack() {
    // Another library shares a collection presented `Au choix`: a spoken
    // question, then its three stories on the wheel.
    let mut other = library();
    let source = create_story(
        &mut other.db,
        CreateStoryInput {
            title: "La Collection".into(),
        },
    )
    .expect("create story")
    .id;
    let mut episodes = vec!["n1".to_string()];
    for _ in 0..2 {
        let added = add_node(&mut other.db, &source, None).expect("add an episode");
        episodes.push(added.structure.nodes.last().expect("a node").id.clone());
    }
    for (index, node) in episodes.iter().enumerate() {
        let label = format!("Histoire {}", index + 1);
        voice(
            &mut other,
            &source,
            node,
            &label,
            0.30 + 0.05 * index as f64,
        );
    }
    attach_node_media(
        &mut other.db,
        other.app_data.path(),
        AttachNodeMediaInput {
            story_id: source.clone(),
            node_id: "n1".into(),
            kind: MediaKind::Image,
            bytes: png(),
        },
    )
    .expect("attach the picture");
    set_layout(&other.db, &source, StoryLayout::Menu).expect("present it `Au choix`");
    announce(&mut other, &source, AnnouncementTarget::Question, 0.50);
    for (index, node) in episodes.iter().enumerate() {
        let target = AnnouncementTarget::Chapter {
            node_id: node.clone(),
        };
        announce(&mut other, &source, target, 0.55 + 0.05 * index as f64);
    }
    let out = TempDir::new().expect("out");
    let archive = out.path().join("La Collection.zip");
    let plan = plan_story_archive_export(&other.db, &source, None).expect("plan the export");
    export_story_archive(
        &plan,
        &resolve_node_media_dir(other.app_data.path()),
        &archive,
        &|_| {},
    )
    .expect("export the collection");

    // This library creates a story from that pack: its stages are nodes,
    // its wheels are options — a story with choices.
    let mut here = library();
    let card = accept_structured_archive_creation(&mut here.db, here.app_data.path(), &archive)
        .expect("create the story from the pack");
    let retained = resolve_source_archive_path(here.app_data.path(), &card.id);
    assert!(retained.is_file(), "the source archive is retained");

    // THE REFERENCE: the pack sent from its retained archive, as it is.
    let (reference_volume, md) = scratch_v3_volume();
    let scanner =
        SystemDeviceScanner::with_explicit_mount_roots(vec![reference_volume.path().to_path_buf()]);
    let from_archive = send_archive_to_device(
        &scanner,
        &SystemDeviceV3PackWriter,
        &SendArchiveRequest {
            device_identifier: device_identifier(&scanner),
            archive_path: retained.clone(),
        },
        Duration::from_secs(30),
        &|_| {},
    )
    .expect("send the retained archive");
    assert_eq!(
        from_archive.pack_uuid, source,
        "the pack keeps its identity"
    );

    // The retained archive goes away (a library moved without it): the
    // story is now sent by its own graph, to a volume of the same key.
    std::fs::remove_file(&retained).expect("lose the retained archive");
    let (volume, same_md) = scratch_v3_volume();
    assert_eq!(same_md, md);
    let from_story = send(&here, volume.path(), &card.id).expect("send the story");
    assert_eq!(from_story.pack_uuid, from_archive.pack_uuid);
    assert_eq!(from_story.short_id, from_archive.short_id);
    assert_eq!(
        (from_story.image_count, from_story.audio_count),
        (from_archive.image_count, from_archive.audio_count)
    );

    // The device meets the same pack: stage for stage, the same picture,
    // the same sound, the same wheels entered on the same entries.
    let content = |root: &Path| root.join(".content").join(&from_archive.short_id);
    let reference = played_stages(&content(reference_volume.path()), &md);
    let sent = played_stages(&content(volume.path()), &md);
    // Cover, question, three wheel entries, three stories.
    assert_eq!(reference.len(), 8);
    assert_eq!(sent.len(), reference.len());
    for (index, (sent, reference)) in sent.iter().zip(&reference).enumerate() {
        assert!(sent.image == reference.image, "stage {index}: the picture");
        assert!(sent.audio == reference.audio, "stage {index}: the sound");
        assert_eq!(sent.ok, reference.ok, "stage {index}: where OK leads");
        assert_eq!(sent.home, reference.home, "stage {index}: where HOME leads");
        if index == 1 {
            // The one thing a story does not carry: the question played
            // with every control off. It comes back as a node that plays
            // through — pause and home on, which takes nothing away.
            assert_eq!(reference.controls, (false, false, false, false, true));
            assert_eq!(sent.controls, PLAYED);
        } else {
            assert_eq!(sent.controls, reference.controls, "stage {index}: controls");
        }
    }
    // The pack is a real menu: the cover leads to the question, the
    // question opens the wheel of the three stories, a story returns to the
    // wheel on its own entry.
    assert!(sent.iter().all(|stage| stage.audio.is_some()));
    assert_eq!(sent[1].ok, Some((vec![2, 3, 4], 0)));
    assert_eq!(sent[6].ok, Some((vec![2, 3, 4], 1)));
    assert_eq!(sent[6].home, Some((vec![2, 3, 4], 1)));
}
