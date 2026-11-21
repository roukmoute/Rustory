//! Media whose names END ALIKE, sent to a device, end to end through the
//! public send services. A device file is named after the last 8 characters
//! of its source name: two different pictures (or two different sounds) that
//! share those 8 characters must still land on two files, every stage showing
//! and playing its own — for a pack sent from its archive and for a pack laid
//! out from the library alike. The volume is written by the real pack writer
//! and read back the way the device reads it: the cleartext node index, the
//! deciphered picture and sound indices, the deciphered files.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use rustory_lib::application::device::send::{
    send_archive_to_device, send_story_pack_to_device, SendArchiveRequest, SendStoryPackRequest,
};
use rustory_lib::application::device::{read_connected_lunii, ConnectedLuniiOutcome};
use rustory_lib::domain::device::{synthesize_sequential_pack, DeviceFamily, EpisodeAssets};
use rustory_lib::infrastructure::device::{
    to_device_audio, to_device_image, v3_decipher_in_place, v3_story_key_iv, DeviceLibraryReader,
    SystemDeviceLibraryReader, SystemDeviceScanner, SystemDeviceV3PackWriter,
};
use tempfile::TempDir;

/// The identity of the pack sent in both tests.
const PACK: &str = "6f0e3b1a-52c4-4d7e-9a8b-0c1d2e3f4a5b";
const SHORT_ID: &str = "2E3F4A5B";

/// The 8 characters three pictures end with, and the 8 characters two sounds
/// end with — as the device writes them.
const PICTURE_SUFFIX: &str = "1234ABCD";
const SOUND_SUFFIX: &str = "5678EF01";

/// A real picture, 96×64, shaded in `tint`: two tints, two different device
/// bitmaps.
fn png(tint: [u8; 3]) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(96, 64, |x, _| {
        let level = |channel: u8| ((u32::from(channel) * (x + 32)) / 128) as u8;
        image::Rgba([level(tint[0]), level(tint[1]), level(tint[2]), 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode png");
    out.into_inner()
}

/// A 16-bit PCM mono WAV of a sine tone — real, decodable audio. A distinct
/// duration is a distinct sound.
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

/// The identifier the device panel holds, read from the volume itself.
fn device_identifier(scanner: &SystemDeviceScanner) -> String {
    match read_connected_lunii(scanner, Duration::from_secs(30)).expect("scan the volume") {
        ConnectedLuniiOutcome::Supported(profile) => profile.device_identifier,
        other => panic!("the scratch volume is not a supported device: {other:?}"),
    }
}

/// What one stage shows or plays, as the device finds it: the name its index
/// entry carries and the deciphered file of that name.
#[derive(Debug)]
struct DeviceMedia {
    name: String,
    bytes: Vec<u8>,
}

/// Read a pack back from the volume, stage by stage: the picture and the
/// sound each one's node-index record points at, through the deciphered
/// `ri` / `si` entry of that index and the deciphered file it names.
fn read_stages(content: &Path, md: &[u8]) -> Vec<(Option<DeviceMedia>, Option<DeviceMedia>)> {
    let (key, iv) = v3_story_key_iv(md).expect("the volume's key");
    let ni = std::fs::read(content.join("ni")).expect("ni");
    let u32_at = |at: usize| u32::from_le_bytes(ni[at..at + 4].try_into().expect("u32"));
    let i32_at = |at: usize| i32::from_le_bytes(ni[at..at + 4].try_into().expect("i32"));
    let (offset, record, count) = (u32_at(4) as usize, u32_at(8) as usize, u32_at(12) as usize);
    assert_eq!((offset, record), (512, 44), "the node index layout");
    assert_eq!(ni.len(), offset + record * count);

    // An asset index: 12-byte `000\XXXXXXXX` entries, deciphered. A short
    // index file is zero-padded to a whole cipher block: the node index is
    // the authority on how many entries it holds.
    let names = |index: &str, count: usize| -> Vec<String> {
        let mut table = std::fs::read(content.join(index)).expect("asset index");
        v3_decipher_in_place(&mut table, &key, &iv);
        let names: Vec<String> = table
            .chunks_exact(12)
            .take(count)
            .map(|entry| {
                assert_eq!(&entry[..4], b"000\\", "{index}: an entry's folder");
                String::from_utf8(entry[4..].to_vec()).expect("an ascii name")
            })
            .collect();
        assert_eq!(names.len(), count, "{index}: one entry per asset");
        names
    };
    let pictures = names("ri", u32_at(16) as usize);
    let sounds = names("si", u32_at(20) as usize);

    let media = |dir: &str, names: &[String], index: i32| -> Option<DeviceMedia> {
        let name = names.get(usize::try_from(index).ok()?)?.clone();
        let mut bytes = std::fs::read(content.join(dir).join(&name))
            .unwrap_or_else(|_| panic!("{dir}/{name} is on the volume"));
        v3_decipher_in_place(&mut bytes, &key, &iv);
        Some(DeviceMedia { name, bytes })
    };
    (0..count)
        .map(|stage| {
            let at = offset + stage * record;
            (
                media("rf/000", &pictures, i32_at(at)),
                media("sf/000", &sounds, i32_at(at + 4)),
            )
        })
        .collect()
}

/// A media on the device is the device form of its source: the very bytes
/// the conversion yields (a file shorter than the ciphered region carries a
/// few bytes of zero padding after them).
fn assert_is_the_device_form_of(media: &DeviceMedia, device_form: &[u8], what: &str) {
    assert!(
        media.bytes.starts_with(device_form) && media.bytes.len() - device_form.len() < 16,
        "{what}: the file {} does not hold its own media ({} bytes on the device, {} expected)",
        media.name,
        media.bytes.len(),
        device_form.len()
    );
}

/// The names of the files actually present in one asset folder of the pack.
fn files_in(content: &Path, dir: &str) -> HashSet<String> {
    std::fs::read_dir(content.join(dir))
        .expect("an asset folder")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// Whether `name` is a device file name: 8 uppercase hexadecimal characters.
fn is_device_name(name: &str) -> bool {
    name.len() == 8
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
}

/// The pixels of an encoded picture.
fn pixels(encoded: &[u8]) -> image::RgbaImage {
    image::load_from_memory(encoded)
        .expect("a decodable picture")
        .to_rgba8()
}

#[test]
fn a_pack_archive_whose_media_names_end_alike_puts_every_media_on_its_own_device_file() {
    // Three pictures whose names end with the same 8 characters, two sounds
    // likewise — and a third sound ending like the PICTURES do, which is no
    // clash at all: pictures and sounds live in two folders.
    let picture_names = ["a", "b", "c"].map(|lead| format!("{}1234abcd.png", lead.repeat(32)));
    let sound_names = [
        format!("{}5678ef01.wav", "d".repeat(32)),
        format!("{}5678ef01.wav", "e".repeat(32)),
        format!("{}1234abcd.wav", "f".repeat(32)),
    ];
    let pictures = [png([255, 40, 40]), png([40, 255, 40]), png([40, 40, 255])];
    let sounds = [wav(0.30), wav(0.35), wav(0.40)];

    // A pack of three stages played one after the other, each with its own
    // picture and its own sound.
    let stage = |index: usize, uuid: &str, ok: &str| {
        format!(
            r#"{{"uuid":"{uuid}","squareOne":{},"image":"{}","audio":"{}",
                "okTransition":{ok},"homeTransition":null,
                "controlSettings":{{"wheel":true,"ok":true,"home":false,"pause":false,"autoplay":false}}}}"#,
            index == 0,
            picture_names[index],
            sound_names[index]
        )
    };
    let descriptor = format!(
        r#"{{"format":"v1","version":1,"nightModeAvailable":false,
            "stageNodes":[{},{},{}],
            "actionNodes":[
              {{"id":"to-second","options":["second"]}},
              {{"id":"to-third","options":["third"]}}
            ]}}"#,
        stage(0, PACK, r#"{"actionNode":"to-second","optionIndex":0}"#),
        stage(1, "second", r#"{"actionNode":"to-third","optionIndex":0}"#),
        stage(2, "third", "null"),
    );
    let dir = TempDir::new().expect("dir");
    let archive = dir.path().join("pack.zip");
    {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).expect("archive"));
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("story.json", options)
            .expect("descriptor");
        writer.write_all(descriptor.as_bytes()).expect("descriptor");
        for (name, bytes) in picture_names
            .iter()
            .zip(&pictures)
            .chain(sound_names.iter().zip(&sounds))
        {
            writer
                .start_file(format!("assets/{name}"), options)
                .expect("asset");
            writer.write_all(bytes).expect("asset");
        }
        writer.finish().expect("finish the archive");
    }

    let (volume, md) = scratch_v3_volume();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![volume.path().to_path_buf()]);
    let sent = send_archive_to_device(
        &scanner,
        &SystemDeviceV3PackWriter,
        &SendArchiveRequest {
            device_identifier: device_identifier(&scanner),
            archive_path: archive,
        },
        Duration::from_secs(30),
        &|_| {},
    )
    .expect("send the pack archive");
    assert_eq!(sent.pack_uuid, PACK);
    assert_eq!(sent.short_id, SHORT_ID);
    assert_eq!((sent.image_count, sent.audio_count), (3, 3));

    // What every stage must show and play: the device form of ITS media.
    let device_pictures: Vec<Vec<u8>> = pictures
        .iter()
        .map(|picture| to_device_image(picture).expect("a device bitmap"))
        .collect();
    let device_sounds: Vec<Vec<u8>> = sounds
        .iter()
        .map(|sound| to_device_audio(sound).expect("a device sound"))
        .collect();
    assert!(
        device_pictures[0] != device_pictures[1] && device_pictures[1] != device_pictures[2],
        "the pictures differ"
    );
    assert!(
        device_sounds[0] != device_sounds[1] && device_sounds[1] != device_sounds[2],
        "the sounds differ"
    );

    let content = volume.path().join(".content").join(SHORT_ID);
    let stages = read_stages(&content, &md);
    assert_eq!(stages.len(), 3);
    for (index, (picture, sound)) in stages.iter().enumerate() {
        let picture = picture.as_ref().expect("a picture");
        let sound = sound.as_ref().expect("a sound");
        assert_is_the_device_form_of(
            picture,
            &device_pictures[index],
            &format!("the picture of stage {index}"),
        );
        assert_is_the_device_form_of(
            sound,
            &device_sounds[index],
            &format!("the sound of stage {index}"),
        );
    }

    // The names: the first media to bear a name keeps it, the others get a
    // device name of their own — and the sound that ends like the pictures
    // keeps its name too, in the sounds' folder.
    let picture_files: Vec<&str> = stages
        .iter()
        .map(|(picture, _)| picture.as_ref().expect("a picture").name.as_str())
        .collect();
    let sound_files: Vec<&str> = stages
        .iter()
        .map(|(_, sound)| sound.as_ref().expect("a sound").name.as_str())
        .collect();
    assert_eq!(picture_files[0], PICTURE_SUFFIX);
    assert_eq!(sound_files[0], SOUND_SUFFIX);
    assert_eq!(sound_files[2], PICTURE_SUFFIX);
    for name in picture_files.iter().chain(&sound_files) {
        assert!(is_device_name(name), "{name} is a device file name");
    }
    assert_eq!(
        picture_files.iter().collect::<HashSet<_>>().len(),
        3,
        "three pictures, three files: {picture_files:?}"
    );
    assert_eq!(
        sound_files.iter().collect::<HashSet<_>>().len(),
        3,
        "three sounds, three files: {sound_files:?}"
    );
    // The folders hold those files and nothing else.
    assert_eq!(
        files_in(&content, "rf/000"),
        picture_files.iter().map(|name| name.to_string()).collect()
    );
    assert_eq!(
        files_in(&content, "sf/000"),
        sound_files.iter().map(|name| name.to_string()).collect()
    );

    // The cover the device panel shows for the pack is read back through the
    // same index: it is the FIRST stage's picture, not one written over it.
    let library = SystemDeviceLibraryReader
        .read_library(volume.path(), DeviceFamily::Lunii, Duration::from_secs(30))
        .expect("read the device");
    let entry = library
        .entries
        .iter()
        .find(|entry| entry.uuid == PACK)
        .expect("the pack is listed");
    let cover = entry.cover_png.as_deref().expect("a cover");
    assert!(
        pixels(cover) == pixels(&device_pictures[0]),
        "the cover is the first stage's picture"
    );
}

#[test]
fn a_library_story_whose_media_names_end_alike_puts_every_media_on_its_own_device_file() {
    // Two episodes of a library story: the media store names a file after its
    // content, and here the two pictures' names end with the same 8
    // characters, the two sounds' names likewise.
    let media = TempDir::new().expect("media store");
    let picture_names = ["a", "b"].map(|lead| format!("{}1234abcd.png", lead.repeat(56)));
    let sound_names = ["c", "d"].map(|lead| format!("{}5678ef01.wav", lead.repeat(56)));
    let pictures = [png([255, 200, 40]), png([40, 200, 255])];
    let sounds = [wav(0.30), wav(0.35)];
    for (name, bytes) in picture_names
        .iter()
        .zip(&pictures)
        .chain(sound_names.iter().zip(&sounds))
    {
        std::fs::write(media.path().join(name), bytes).expect("a stored media");
    }
    let pack = synthesize_sequential_pack(
        PACK,
        &[
            EpisodeAssets {
                audio_ref: sound_names[0].clone(),
                image_ref: Some(picture_names[0].clone()),
            },
            EpisodeAssets {
                audio_ref: sound_names[1].clone(),
                image_ref: Some(picture_names[1].clone()),
            },
        ],
    );

    let (volume, md) = scratch_v3_volume();
    let scanner = SystemDeviceScanner::with_explicit_mount_roots(vec![volume.path().to_path_buf()]);
    let sent = send_story_pack_to_device(
        &scanner,
        &SystemDeviceV3PackWriter,
        &SendStoryPackRequest {
            device_identifier: device_identifier(&scanner),
            pack,
            media_dir: media.path().to_path_buf(),
        },
        Duration::from_secs(30),
        &|_| {},
    )
    .expect("send the story");
    assert_eq!(sent.short_id, SHORT_ID);
    assert_eq!((sent.image_count, sent.audio_count), (2, 2));

    let device_pictures: Vec<Vec<u8>> = pictures
        .iter()
        .map(|picture| to_device_image(picture).expect("a device bitmap"))
        .collect();
    let device_sounds: Vec<Vec<u8>> = sounds
        .iter()
        .map(|sound| to_device_audio(sound).expect("a device sound"))
        .collect();
    assert!(
        device_pictures[0] != device_pictures[1],
        "the pictures differ"
    );
    assert!(device_sounds[0] != device_sounds[1], "the sounds differ");

    // The cover and the first episode share the first episode's media; the
    // second episode has its own.
    let content = volume.path().join(".content").join(SHORT_ID);
    let stages = read_stages(&content, &md);
    assert_eq!(stages.len(), 3, "the cover and two episodes");
    for (stage, episode) in [(0, 0), (1, 0), (2, 1)] {
        let (picture, sound) = &stages[stage];
        assert_is_the_device_form_of(
            picture.as_ref().expect("a picture"),
            &device_pictures[episode],
            &format!("the picture of stage {stage}"),
        );
        assert_is_the_device_form_of(
            sound.as_ref().expect("a sound"),
            &device_sounds[episode],
            &format!("the sound of stage {stage}"),
        );
    }

    // The first episode's media keep the names the device has always given
    // them; the second episode's get names of their own.
    let name_of = |media: &Option<DeviceMedia>| media.as_ref().expect("a media").name.clone();
    let (first_picture, first_sound) = (name_of(&stages[0].0), name_of(&stages[0].1));
    let (second_picture, second_sound) = (name_of(&stages[2].0), name_of(&stages[2].1));
    assert_eq!(first_picture, PICTURE_SUFFIX);
    assert_eq!(first_sound, SOUND_SUFFIX);
    assert_ne!(second_picture, first_picture);
    assert_ne!(second_sound, first_sound);
    assert!(is_device_name(&second_picture), "{second_picture}");
    assert!(is_device_name(&second_sound), "{second_sound}");
    assert_eq!(
        files_in(&content, "rf/000"),
        HashSet::from([first_picture, second_picture])
    );
    assert_eq!(
        files_in(&content, "sf/000"),
        HashSet::from([first_sound, second_sound])
    );
}
