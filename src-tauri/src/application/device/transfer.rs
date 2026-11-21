//! Pre-transfer comparison application service.
//!
//! Composes, in Rust, the read-only comparison shown before a transfer:
//! given a selected local story and a connected supported Lunii, does the
//! story's pack already live on the device (a send would REPLACE it) or not
//! (a send would ADD it), and how many other device stories stay untouched.
//!
//! The flow mirrors the deliberate `alreadyImported` precedent: the
//! local↔device membership is composed HERE, never recomputed by the
//! frontend. It is an authoritative snapshot: the live device is re-scanned
//! through the proven [`read_device_library`] pipeline (re-scan + identity
//! guard + `ReadLibrary` gate + `.pi` read), and the local truth (the
//! story's title and the packs it is linked to) is read AFTER the device
//! I/O under a scoped DB lock — never held across the scan.
//!
//! "The story's pack" is every pack the local library ties to the story —
//! the three links the device inventory already names a pack's local story
//! by ([`resolve_local_truth`](super::title::resolve_local_truth)), read
//! here from the story's side:
//!
//! - its IMPORT provenance (`story_imports.pack_uuid`): the pack it was
//!   copied from, which the round-trip writes back under that identity;
//! - its SEND link (`story_device_packs.pack_uuid`): the pack the pack
//!   engine last wrote it as — a retained archive's own entry uuid, or the
//!   identity of a synthesized pack — recorded at every successful send;
//! - its own ID: a synthesized pack is named by its story id, which covers a
//!   story sent before the send link existed.
//!
//! The comparison was first written when a copy from a device was the only
//! way a local story could own a pack, hence a read once limited to
//! `story_imports`. A story sent by the pack engine then read "nouvelle"
//! right after its own send, while sending it again REPLACES that pack. The
//! regular flows give a story one of the two stored links, never both (the
//! pack engine refuses a device-copied story, the round-trip takes nothing
//! else), but the tables do not forbid it: the membership is their union.
//!
//! Read-only by contract: nothing is written, and the send capability stays
//! governed by the `WriteStory` gate (always `false` in MVP). Stays
//! Tauri-free: tests inject a [`MockDeviceScanner`] + [`MockDeviceLibraryReader`]
//! and a temp DB.

use std::sync::Mutex;
use std::time::Duration;

use rusqlite::OptionalExtension;

use crate::domain::device::UnsupportedReason;
use crate::domain::shared::AppError;
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::device::{DeviceLibraryReader, DeviceScanner};

use super::library::{read_device_library, DeviceLibraryOutcome};

/// Result of [`read_transfer_preview`]. Mapped 1-to-1 by the IPC layer to
/// `TransferPreviewDto`. Recoverable failures (device unplugged mid-read,
/// FS error, identity changed, local store unavailable, selected story
/// vanished) propagate as `Err(AppError)` — they are not comparison states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferPreviewOutcome {
    /// No supported device is connected anymore.
    NoDevice,
    /// A device is present but its profile is not in the allow-list, or more
    /// than one supported Lunii is connected (cannot bind the comparison).
    Unsupported { reason: UnsupportedReason },
    /// The comparison was composed.
    Ready {
        device_identifier: String,
        story_id: String,
        story_title: String,
        /// A pack the local library ties to the selected story (import
        /// provenance, send link, or the story's own id) is already on the
        /// device — a send would REPLACE it. `false` ⇒ a send would ADD it
        /// ("Nouvelle sur l'appareil").
        on_device: bool,
        /// How many OTHER device stories a send would leave untouched: the
        /// inventory minus every entry that is the selected story's own.
        unchanged_count: u32,
        /// Whether a transfer is allowed (the `WriteStory` capability).
        /// Always `false` in MVP Phase 1 — the preview is read-only.
        transferable: bool,
    },
}

/// Local facts about the selected story, read under the caller's scoped DB
/// lock: its title and the identity of every pack the library ties to it.
struct TransferLocalFacts {
    title: String,
    /// The story's own id (a synthesized pack is named by it), then the
    /// pack of its import provenance and the pack of its send link, when it
    /// has them. See the module doc.
    own_pack_uuids: Vec<String>,
}

impl TransferLocalFacts {
    /// Whether the device pack `pack_uuid` is the selected story's own.
    /// Keyed on the pack UUID, never the title or the device identifier:
    /// the same pack seen from another Lunii resolves to the same identity.
    fn owns(&self, pack_uuid: &str) -> bool {
        self.own_pack_uuids.iter().any(|own| own == pack_uuid)
    }
}

/// Compose the pre-transfer comparison for `story_id` against the supported
/// Lunii whose identifier equals `requested_device_identifier`.
pub fn read_transfer_preview(
    db: &Mutex<DbHandle>,
    scanner: &dyn DeviceScanner,
    library_reader: &dyn DeviceLibraryReader,
    story_id: &str,
    requested_device_identifier: &str,
    budget: Duration,
) -> Result<TransferPreviewOutcome, AppError> {
    // Authoritative device read FIRST — re-scan + identity guard + ReadLibrary
    // gate + `.pi` inventory — reusing the proven device-library pipeline. The
    // DB mutex is NOT held during this I/O.
    let outcome =
        read_device_library(scanner, library_reader, requested_device_identifier, budget)?;

    match outcome {
        DeviceLibraryOutcome::None => Ok(TransferPreviewOutcome::NoDevice),
        DeviceLibraryOutcome::Unsupported { reason, .. } => {
            Ok(TransferPreviewOutcome::Unsupported { reason })
        }
        DeviceLibraryOutcome::Readable {
            device_identifier,
            library,
            ..
        } => {
            // Compose local truth AFTER the device I/O, under a scoped DB lock
            // (taken here, never held across the scan/read), keyed by the
            // story id. Fail-closed: a local-store read failure surfaces a
            // recoverable error rather than a misleading "nouvelle" verdict.
            let facts = {
                let db = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                resolve_transfer_local_facts(&db, story_id)?
            };

            // Membership: the inventory entries that are the story's OWN pack
            // (see the module doc). Count EVERY matching entry, not just the
            // first: a pack listed in both `.pi` and `.pi.hidden` (a
            // duplicated inventory) yields two entries for the same content,
            // and a story tied to two packs owns both — none of them is
            // "another story", so none may be counted as one that "stays
            // inchangée".
            let own_count = library
                .entries
                .iter()
                .filter(|entry| facts.owns(&entry.uuid))
                .count();
            let on_device = own_count > 0;

            // Every other device story stays untouched by a send. Saturating so
            // it never underflows even on a degenerate inventory.
            let unchanged_count = (library.entries.len() as u32).saturating_sub(own_count as u32);

            Ok(TransferPreviewOutcome::Ready {
                device_identifier,
                story_id: story_id.to_string(),
                story_title: facts.title,
                on_device,
                unchanged_count,
                // Read-only preview: this 3.1 flag stays `false` for every
                // cohort. The actual send is gated by the `WriteStory`
                // capability + the prepared-artifact check in the transfer
                // flow, never by this preview flag. A `Readable` outcome only
                // ever arises for a supported profile.
                transferable: false,
            })
        }
    }
}

/// Read the selected story's title and the packs the library ties to it
/// (its import provenance, its send link) under the caller's scoped DB lock.
fn resolve_transfer_local_facts(
    db: &DbHandle,
    story_id: &str,
) -> Result<TransferLocalFacts, AppError> {
    let title: Option<String> = db
        .conn()
        .query_row(
            "SELECT title FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| local_read_error("select_title"))?;
    // A vanished story is a race (deleted between selection and this read):
    // recoverable LIBRARY_INCONSISTENT, not a fabricated comparison.
    let Some(title) = title else {
        return Err(story_missing_error());
    };

    // The two stored links, each at most one row per story. Fail-closed like
    // the title read: an unreadable link would otherwise turn a replacement
    // into a "nouvelle" verdict.
    let mut own_pack_uuids = vec![story_id.to_string()];
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT pack_uuid FROM story_imports WHERE story_id = ?1 \
             UNION \
             SELECT pack_uuid FROM story_device_packs WHERE story_id = ?1",
        )
        .map_err(|_| local_read_error("select_pack_uuid"))?;
    let rows = stmt
        .query_map(rusqlite::params![story_id], |row| row.get::<_, String>(0))
        .map_err(|_| local_read_error("select_pack_uuid"))?;
    for row in rows {
        own_pack_uuids.push(row.map_err(|_| local_read_error("select_pack_uuid"))?);
    }

    Ok(TransferLocalFacts {
        title,
        own_pack_uuids,
    })
}

fn story_missing_error() -> AppError {
    AppError::library_inconsistent(
        "Comparaison impossible: histoire introuvable dans la bibliothèque locale.",
        "Recharge la bibliothèque puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "transfer_preview",
        "cause": "story_missing",
    }))
}

fn local_read_error(stage: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Comparaison indisponible: vérifie le disque local et réessaie.",
        "Réessaie ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "transfer_preview",
        "stage": stage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::device::title::record_sent_pack;
    use crate::domain::device::{DeviceLibrary, DeviceStoryEntry};
    use crate::infrastructure::db;
    use crate::infrastructure::device::{
        compute_device_identifier, MockDeviceLibraryReader, MockDeviceScanner,
    };

    const UUID_A: &str = "11111111-1111-1111-1111-1111111111aa";
    const UUID_B: &str = "22222222-2222-2222-2222-2222222222bb";
    const UUID_C: &str = "33333333-3333-3333-3333-3333333333cc";
    const UUID_ABSENT: &str = "ffffffff-ffff-ffff-ffff-ffffffffffff";

    fn budget() -> Duration {
        Duration::from_secs(5)
    }

    /// The identifier the mock scanner's `enqueue_supported_lunii` volume
    /// hashes to (`.pi` = `MOCK_PI`, serial = `MOCK_SERIAL`).
    fn mock_identifier() -> String {
        compute_device_identifier(b"MOCK_PI", Some("MOCK_SERIAL"))
    }

    fn fresh_db() -> Mutex<DbHandle> {
        let mut handle = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut handle).expect("migrate");
        Mutex::new(handle)
    }

    fn insert_story(db: &Mutex<DbHandle>, id: &str, title: &str) {
        db.lock()
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES (?1, ?2, 3, '{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}', \
                 '65d663fd2180630fa24693a5ccaee6d663b7a0f78b7d44b0e5ef07adc3f293b2', \
                 '2026-06-16T00:00:00.000Z', '2026-06-16T00:00:00.000Z')",
                rusqlite::params![id, title],
            )
            .expect("insert story");
    }

    fn insert_import(db: &Mutex<DbHandle>, story_id: &str, pack_uuid: &str) {
        db.lock()
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
                 VALUES (?1, ?2, '0123456789abcdef0123456789abcdef', '2026-06-16T00:00:00.000Z', 5, 18, ?3, 'lunii')",
                rusqlite::params![story_id, pack_uuid, "ab".repeat(32)],
            )
            .expect("insert provenance");
    }

    /// The send link of `story_id`, written exactly as a settled send
    /// writes it (the last send wins).
    fn insert_send_link(db: &Mutex<DbHandle>, story_id: &str, pack_uuid: &str) {
        record_sent_pack(&db.lock().unwrap(), story_id, pack_uuid).expect("record the sent pack");
    }

    fn entry(uuid: &str, short_id: &str) -> DeviceStoryEntry {
        DeviceStoryEntry {
            uuid: uuid.into(),
            short_id: short_id.into(),
            hidden: false,
            content_present: true,
            cover_png: None,
        }
    }

    fn library(entries: Vec<DeviceStoryEntry>) -> DeviceLibrary {
        DeviceLibrary {
            entries,
            had_trailing_bytes: false,
        }
    }

    fn three_pack_inventory() -> DeviceLibrary {
        library(vec![
            entry(UUID_A, "111111AA"),
            entry(UUID_B, "222222BB"),
            entry(UUID_C, "333333CC"),
        ])
    }

    #[test]
    fn ready_new_when_story_has_no_pack_uuid() {
        let db = fresh_db();
        insert_story(&db, "s1", "Mon histoire");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                story_title,
                on_device,
                unchanged_count,
                transferable,
                ..
            } => {
                assert_eq!(story_title, "Mon histoire");
                assert!(
                    !on_device,
                    "a story no device pack is tied to is new on the device"
                );
                assert_eq!(unchanged_count, 3);
                assert!(!transferable);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn ready_new_when_pack_uuid_absent_from_inventory() {
        let db = fresh_db();
        insert_story(&db, "s1", "Importée d'ailleurs");
        // The story carries a pack link, but that pack is NOT on this device.
        insert_import(&db, "s1", UUID_ABSENT);
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(library(vec![
            entry(UUID_A, "111111AA"),
            entry(UUID_B, "222222BB"),
        ])));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                on_device,
                unchanged_count,
                ..
            } => {
                assert!(!on_device);
                assert_eq!(unchanged_count, 2);
            }
            other => panic!("expected Ready(new), got {other:?}"),
        }
    }

    #[test]
    fn ready_replace_when_pack_uuid_present_in_inventory() {
        let db = fresh_db();
        insert_story(&db, "s1", "Déjà transférée");
        insert_import(&db, "s1", UUID_B);
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                on_device,
                unchanged_count,
                transferable,
                ..
            } => {
                assert!(on_device, "the pack is in the inventory → replacement");
                assert_eq!(
                    unchanged_count, 2,
                    "the matched pack is excluded from the count"
                );
                assert!(!transferable);
            }
            other => panic!("expected Ready(replace), got {other:?}"),
        }
    }

    #[test]
    fn ready_replace_when_the_sent_pack_is_in_the_inventory() {
        // A story the pack engine sent to this device. An archive pack keeps
        // its own entry uuid, unrelated to the story id: the send link is
        // the only thing that ties them, and sending again replaces the pack.
        let db = fresh_db();
        insert_story(&db, "s1", "Envoyée par le moteur de packs");
        insert_send_link(&db, "s1", UUID_B);
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(7);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                on_device,
                unchanged_count,
                ..
            } => {
                assert!(on_device, "the sent pack is in the inventory → replacement");
                assert_eq!(
                    unchanged_count, 2,
                    "the story's own pack is not one of the others"
                );
            }
            other => panic!("expected Ready(replace), got {other:?}"),
        }
    }

    #[test]
    fn ready_new_when_the_sent_pack_is_absent_from_the_inventory() {
        // The send link outlives the pack it names: deleted from the device
        // since, or sent to another Lunii than the one plugged in. A send
        // would ADD the story here, and every listed pack is another story.
        let db = fresh_db();
        insert_story(&db, "s1", "Envoyée ailleurs");
        insert_send_link(&db, "s1", UUID_ABSENT);
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(7);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                on_device,
                unchanged_count,
                ..
            } => {
                assert!(!on_device, "the sent pack is not on this device");
                assert_eq!(unchanged_count, 3);
            }
            other => panic!("expected Ready(new), got {other:?}"),
        }
    }

    #[test]
    fn only_the_last_sent_pack_is_the_story_s_own() {
        // The send link names the LAST pack the story was written as. An
        // earlier one still on the device is no longer tied to the story: a
        // send leaves it untouched, so it counts among the others.
        let db = fresh_db();
        insert_story(&db, "s1", "Envoyée deux fois");
        insert_send_link(&db, "s1", UUID_A);
        insert_send_link(&db, "s1", UUID_B);
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(7);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                on_device,
                unchanged_count,
                ..
            } => {
                assert!(on_device);
                assert_eq!(unchanged_count, 2, "the earlier pack stays untouched");
            }
            other => panic!("expected Ready(replace), got {other:?}"),
        }
    }

    #[test]
    fn ready_replace_when_a_pack_named_by_the_story_id_is_in_the_inventory() {
        // A synthesized pack is named by its story id. A story sent before
        // the send link existed carries no link at all, yet its pack is on
        // the device and a send replaces it — the same recognition the
        // device inventory makes of that pack.
        let db = fresh_db();
        insert_story(&db, UUID_C, "Envoyée avant le lien d'envoi");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(7);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, UUID_C, &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                on_device,
                unchanged_count,
                ..
            } => {
                assert!(on_device, "the pack named by the story id is listed");
                assert_eq!(unchanged_count, 2);
            }
            other => panic!("expected Ready(replace), got {other:?}"),
        }
    }

    #[test]
    fn a_story_tied_to_two_packs_owns_both() {
        // Not a state the regular flows reach (the pack engine refuses a
        // device-copied story), but the two link tables do not forbid it.
        // Both packs are the story's own: neither is "another story".
        let db = fresh_db();
        insert_story(&db, "s1", "Copiée puis envoyée");
        insert_import(&db, "s1", UUID_A);
        insert_send_link(&db, "s1", UUID_C);
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(7);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                on_device,
                unchanged_count,
                ..
            } => {
                assert!(on_device);
                assert_eq!(unchanged_count, 1, "only the third pack is another story");
            }
            other => panic!("expected Ready(replace), got {other:?}"),
        }
    }

    #[test]
    fn a_story_tied_to_two_packs_is_on_the_device_through_either() {
        // Same double link, but only ONE of the two packs is listed — once
        // the copied one, once the sent one.
        for listed in [UUID_A, UUID_C] {
            let db = fresh_db();
            insert_story(&db, "s1", "Copiée puis envoyée");
            insert_import(&db, "s1", UUID_A);
            insert_send_link(&db, "s1", UUID_C);
            let scanner = MockDeviceScanner::new();
            scanner.enqueue_supported_lunii(7);
            let reader = MockDeviceLibraryReader::new();
            reader.enqueue(Ok(library(vec![
                entry(listed, "LISTED00"),
                entry(UUID_B, "222222BB"),
            ])));

            let outcome =
                read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                    .expect("preview");
            match outcome {
                TransferPreviewOutcome::Ready {
                    on_device,
                    unchanged_count,
                    ..
                } => {
                    assert!(on_device, "{listed} is the story's own pack");
                    assert_eq!(unchanged_count, 1);
                }
                other => panic!("expected Ready(replace) for {listed}, got {other:?}"),
            }
        }
    }

    #[test]
    fn an_unreadable_send_link_fails_closed_instead_of_reading_new() {
        // A local store that cannot answer must never be read as "no link":
        // that would announce an addition where a send replaces.
        let db = fresh_db();
        insert_story(&db, "s1", "Mon histoire");
        db.lock()
            .unwrap()
            .conn()
            .execute("DROP TABLE story_device_packs", [])
            .expect("drop the link table");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(7);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let err = read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
            .expect_err("an unreadable link must fail recoverably");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "LOCAL_STORAGE_UNAVAILABLE");
        assert_eq!(v["details"]["source"], "transfer_preview");
        assert_eq!(v["details"]["stage"], "select_pack_uuid");
    }

    #[test]
    fn unchanged_count_is_zero_when_the_only_pack_is_the_selected_one() {
        let db = fresh_db();
        insert_story(&db, "s1", "Seule à bord");
        insert_import(&db, "s1", UUID_A);
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(library(vec![entry(UUID_A, "111111AA")])));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                on_device,
                unchanged_count,
                ..
            } => {
                assert!(on_device);
                assert_eq!(unchanged_count, 0);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn unchanged_count_excludes_every_occurrence_of_a_duplicated_pack() {
        // A pack listed in BOTH `.pi` and `.pi.hidden` produces two inventory
        // entries for the same content. A send touches that content once, so
        // both entries must be excluded from "resterait inchangé" — counting
        // only the first would over-report the unchanged set by one.
        let db = fresh_db();
        insert_story(&db, "s1", "Doublon visible + masqué");
        insert_import(&db, "s1", UUID_A);
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(library(vec![
            entry(UUID_A, "111111AA"),
            entry(UUID_A, "111111AA"),
            entry(UUID_B, "222222BB"),
            entry(UUID_C, "333333CC"),
        ])));

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        match outcome {
            TransferPreviewOutcome::Ready {
                on_device,
                unchanged_count,
                ..
            } => {
                assert!(on_device);
                assert_eq!(
                    unchanged_count, 2,
                    "both occurrences of the selected pack are excluded"
                );
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn transferable_is_false_for_every_supported_mvp_cohort() {
        // V1 (md v3), V2 (md v6), V3 (md v7) are the supported MVP cohorts.
        for version in [3u8, 6, 7] {
            let db = fresh_db();
            insert_story(&db, "s1", "Mon histoire");
            let scanner = MockDeviceScanner::new();
            scanner.enqueue_supported_lunii(version);
            let reader = MockDeviceLibraryReader::new();
            reader.enqueue(Ok(three_pack_inventory()));

            let outcome =
                read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                    .expect("preview");
            match outcome {
                TransferPreviewOutcome::Ready { transferable, .. } => {
                    assert!(
                        !transferable,
                        "md v{version} must not be transferable in MVP"
                    );
                }
                other => panic!("expected Ready for md v{version}, got {other:?}"),
            }
        }
    }

    #[test]
    fn missing_story_yields_recoverable_library_inconsistent() {
        let db = fresh_db();
        // No story seeded — the selection vanished between pick and read.
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let err = read_transfer_preview(
            &db,
            &scanner,
            &reader,
            "ghost",
            &mock_identifier(),
            budget(),
        )
        .expect_err("a vanished story must fail recoverably");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "LIBRARY_INCONSISTENT");
        assert_eq!(v["details"]["source"], "transfer_preview");
        assert_eq!(v["details"]["cause"], "story_missing");
    }

    #[test]
    fn identifier_mismatch_propagates_recoverable_device_changed() {
        let db = fresh_db();
        insert_story(&db, "s1", "Mon histoire");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_supported_lunii(3);
        let reader = MockDeviceLibraryReader::new();
        reader.enqueue(Ok(three_pack_inventory()));

        let err = read_transfer_preview(
            &db,
            &scanner,
            &reader,
            "s1",
            "deadbeefdeadbeefdeadbeefdeadbeef",
            budget(),
        )
        .expect_err("identity mismatch must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "DEVICE_SCAN_FAILED");
        assert_eq!(v["details"]["source"], "device_changed");
    }

    #[test]
    fn no_device_when_scanner_finds_nothing() {
        let db = fresh_db();
        insert_story(&db, "s1", "Mon histoire");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_no_device();
        let reader = MockDeviceLibraryReader::new();

        let outcome =
            read_transfer_preview(&db, &scanner, &reader, "s1", &mock_identifier(), budget())
                .expect("preview");
        assert_eq!(outcome, TransferPreviewOutcome::NoDevice);
    }

    #[test]
    fn unsupported_when_metadata_unsupported() {
        let db = fresh_db();
        insert_story(&db, "s1", "Mon histoire");
        let scanner = MockDeviceScanner::new();
        scanner.enqueue_unsupported_metadata(99);
        let reader = MockDeviceLibraryReader::new();

        let outcome = read_transfer_preview(&db, &scanner, &reader, "s1", "whatever", budget())
            .expect("preview");
        assert_eq!(
            outcome,
            TransferPreviewOutcome::Unsupported {
                reason: UnsupportedReason::MetadataUnsupported,
            }
        );
    }
}
