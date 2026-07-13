//! Node-content + node-media application services (schema v2).
//!
//! These services own the canonical WRITE path for a story's current node —
//! its text, its metadata label, and its image / audio source media. They are
//! the deliberate opposite of `update_story` (title only): a node write DOES
//! re-serialize `structure_json` and recompute `content_checksum`, always
//! inside a single `BEGIN IMMEDIATE` transaction so a failure leaves the
//! previous canonical body untouched (NFR9) and never touches another story's
//! canonical row (FR18). The media BYTES live in the node-media store; SQLite
//! owns the `assets` metadata; the canonical JSON only ever carries the asset
//! reference. The `Media` axis gets its first living emitter here.

use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::application::story::now_iso_ms;
use crate::application::story::review;
use crate::application::story::scope::{story_edit_scope, StoryEditScope};
use crate::domain::shared::AppError;
use crate::domain::story::{
    canonical_structure_json, content_checksum, validate_canonical, CanonicalNode,
    CanonicalStoryFacts, CanonicalStructure, MediaCause, Severity,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::filesystem::{
    ensure_node_media_store, read_media, resolve_node_media_dir, sniff_media, store_media,
    MediaKind, NodeMediaError, StoredMedia,
};
use crate::ipc::dto::{NodeContentDto, NodeMediaSlotDto, NodeWriteOutputDto};
use std::path::{Path, PathBuf};

/// Generous upper bounds (anti-DoS), mirrored by the `node_drafts` CHECK and
/// the frontend field limits.
pub const MAX_NODE_TEXT_CHARS: usize = 65536;
pub const MAX_NODE_LABEL_CHARS: usize = 4096;

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

pub struct SaveNodeContentInput {
    pub story_id: String,
    pub node_id: String,
    pub text: String,
    pub label: String,
}

pub struct AttachNodeMediaInput {
    pub story_id: String,
    pub node_id: String,
    pub kind: MediaKind,
    pub bytes: Vec<u8>,
}

pub struct RemoveNodeMediaInput {
    pub story_id: String,
    pub node_id: String,
    pub kind: MediaKind,
}

pub struct RecordNodeDraftInput {
    pub story_id: String,
    pub node_id: String,
    pub draft_text: String,
    pub draft_label: String,
}

/// A buffered node draft read back from `node_drafts`.
pub struct NodeDraftRow {
    pub node_id: String,
    pub draft_text: String,
    pub draft_label: String,
    pub draft_at: String,
}

// ---------------------------------------------------------------------------
// Projection (read side) — used by `get_story_detail`
// ---------------------------------------------------------------------------

/// Project the current node into its wire DTO. Media references are resolved
/// against the `assets` table AND the on-disk store: a present, readable asset
/// → `ready`; a dangling reference → `attention`. Pure read; never fails.
pub fn project_node_content(
    conn: &Connection,
    media_dir: &Path,
    node: &CanonicalNode,
) -> NodeContentDto {
    NodeContentDto {
        id: node.id.clone(),
        text: node.text.clone(),
        label: node.label.clone(),
        image: node
            .image_asset_id
            .as_deref()
            .map(|id| resolve_media_slot(conn, media_dir, id, "image")),
        audio: node
            .audio_asset_id
            .as_deref()
            .map(|id| resolve_media_slot(conn, media_dir, id, "audio")),
    }
}

/// Resolve a media reference into its slot DTO. `ready` requires BOTH a present
/// `assets` row AND that its promoted file still exists on disk; a dangling
/// reference (no row — e.g. an imported story whose bytes did not travel) OR a
/// missing source file degrades to `attention` (repairable) so the editor
/// surfaces "Média à corriger" up-front instead of only failing at preview.
fn resolve_media_slot(
    conn: &Connection,
    media_dir: &Path,
    asset_id: &str,
    slot_type: &str,
) -> NodeMediaSlotDto {
    let row: Option<(String, u64, String)> = conn
        .query_row(
            "SELECT media_format, byte_size, file_name FROM assets WHERE id = ?1",
            rusqlite::params![asset_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .unwrap_or(None);
    match row {
        Some((format, byte_size, file_name)) if media_dir.join(&file_name).is_file() => {
            NodeMediaSlotDto {
                asset_id: asset_id.to_string(),
                media_type: slot_type.to_string(),
                state: media_cause_slot_state(None).to_string(),
                format: Some(format),
                byte_size: Some(byte_size),
            }
        }
        // No row, or the row's promoted file is gone: the source is missing —
        // a `Fixable` `MediaCause` that the editor surfaces as "attention".
        _ => NodeMediaSlotDto {
            asset_id: asset_id.to_string(),
            media_type: slot_type.to_string(),
            state: media_cause_slot_state(Some(MediaCause::SourceMissing)).to_string(),
            format: None,
            byte_size: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Write path
// ---------------------------------------------------------------------------

/// Load the story's canonical structure inside a write transaction, locate the
/// target node, run `mutate`, re-validate, re-serialize, recompute the checksum,
/// bump `updated_at`, optionally consume the node draft, and commit. Returns the
/// write outcome plus whatever `mutate` produced. Any early return drops the
/// transaction, which rolls it back (the previous body is preserved).
fn apply_node_mutation<F, T>(
    db: &mut DbHandle,
    media_dir: &Path,
    story_id: &str,
    node_id: &str,
    consume_draft: bool,
    mutate: F,
) -> Result<(NodeWriteOutputDto, T), AppError>
where
    F: FnOnce(&mut CanonicalNode, &Transaction<'_>) -> Result<T, AppError>,
{
    let now_iso = now_iso_ms()?;
    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| transport_error(&e, "begin_transaction", story_id))?;

    // Authoritative edit-scope guard (FR21): a node write requires the FULL
    // scope — a device-pack story's content is carried by the copied pack, so
    // only its title may change (through `update_story`). The UI never offers
    // the controls, but a direct IPC call or a stale UI state must still be
    // refused here, atomically with the read. The derived scope is reused for
    // the acknowledgement's `importState` (same None-unless-Full rule as the
    // detail projection).
    let scope = story_edit_scope(&tx, story_id);
    if scope != StoryEditScope::Full {
        return Err(node_not_editable(story_id));
    }

    let row: Option<(String, u32, String, String, String)> = tx
        .query_row(
            "SELECT title, schema_version, structure_json, content_checksum, updated_at \
             FROM stories WHERE id = ?1",
            rusqlite::params![story_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(|e| transport_error(&e, "read", story_id))?;
    let (title, schema_version, structure_json, stored_checksum, stored_updated_at) = match row {
        Some(values) => values,
        None => return Err(story_missing(story_id)),
    };

    // Refuse to edit a story whose PERSISTED canonical facts carry a BLOCKING
    // issue (corrupt structure, checksum mismatch, unsupported schema): editing
    // must never silently "repair" an on-disk corruption by overwriting it with
    // a freshly computed checksum. A FIXABLE issue (e.g. an invalid persisted
    // title) does NOT block editing the node's text/media — that would conflate
    // the AC2 fixable/blocking distinction and make a renameable story
    // "unreadable".
    let persisted_facts = CanonicalStoryFacts {
        title: title.clone(),
        schema_version,
        structure_json: structure_json.clone(),
        content_checksum: stored_checksum.clone(),
    };
    if has_blocking(&persisted_facts) {
        return Err(structure_corrupt(story_id));
    }

    let mut structure: CanonicalStructure =
        serde_json::from_str(&structure_json).map_err(|_| structure_corrupt(story_id))?;
    let idx = structure
        .nodes
        .iter()
        .position(|n| n.id == node_id)
        .ok_or_else(|| node_missing(story_id, node_id))?;

    let extra = mutate(&mut structure.nodes[idx], &tx)?;

    let new_json = canonical_structure_json(&structure);

    // A mutation that lands on the EXACT same canonical bytes (e.g. saving the
    // unchanged text/label, removing a media from an already-empty slot) is
    // acknowledged WITHOUT touching the row: rewriting would bump `updated_at`
    // — a persisted, exported, user-visible metadata — for zero actual change.
    // No REAL write happened, so a pending import review is NOT resolved
    // either — but the ACK still carries the CURRENT state read in this
    // transaction (it never lies). A content save still consumes the node
    // draft: the canonical row already reflects the latest committed text.
    if new_json == structure_json {
        if consume_draft {
            tx.execute(
                "DELETE FROM node_drafts WHERE story_id = ?1 AND node_id = ?2",
                rusqlite::params![story_id, node_id],
            )
            .map_err(|e| transport_error(&e, "delete_draft", story_id))?;
        }
        let import_state = review::read_import_state(&tx, story_id, scope)?;
        let node = project_node_content(&tx, media_dir, &structure.nodes[idx]);
        tx.commit()
            .map_err(|e| transport_error(&e, "commit", story_id))?;
        return Ok((
            NodeWriteOutputDto {
                id: story_id.to_string(),
                updated_at: stored_updated_at,
                content_checksum: stored_checksum,
                node,
                import_state: import_state.map(|s| s.wire_tag().to_string()),
            },
            extra,
        ));
    }

    let new_checksum = content_checksum(&new_json);

    // Defense in depth: the mutated structure must not introduce a BLOCKING
    // incoherence (e.g. a wrong node count). A pre-existing fixable title issue
    // is independent of a node edit, so it must not block the save either.
    // The COMPLETE post-mutation blocker list is kept: `Blocking` refuses the
    // write, and the full list (any severity) is then the review-resolution
    // oracle — zero extra I/O.
    let facts = CanonicalStoryFacts {
        title,
        schema_version,
        structure_json: new_json.clone(),
        content_checksum: new_checksum.clone(),
    };
    let post_mutation_blockers = validate_canonical(&facts);
    if post_mutation_blockers
        .iter()
        .any(|b| b.severity == Severity::Blocking)
    {
        return Err(structure_corrupt(story_id));
    }

    tx.execute(
        "UPDATE stories SET structure_json = ?1, content_checksum = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![new_json, new_checksum, now_iso, story_id],
    )
    .map_err(|e| transport_error(&e, "update", story_id))?;

    // ONLY a content save (text/label) consumes the buffered node draft — a
    // media mutation re-serializes the structure from the still-old text, so
    // dropping the draft here would lose an un-flushed keystroke on a crash
    // (NFR8). Media mutations leave the draft for the next content save.
    // The DELETE is CONDITIONED on the mutated node: the single per-story
    // draft row may buffer ANOTHER node's unsaved keystrokes, and a save of
    // this node must never silently destroy that other buffer (NFR8).
    if consume_draft {
        tx.execute(
            "DELETE FROM node_drafts WHERE story_id = ?1 AND node_id = ?2",
            rusqlite::params![story_id, node_id],
        )
        .map_err(|e| transport_error(&e, "delete_draft", story_id))?;
    }

    // A real write that leaves the canonical story ENTIRELY sound settles a
    // pending import review (AC3) — inside this same transaction. The
    // acknowledgement then carries the state read POST-UPDATE, through the
    // same derivation as the detail projection.
    review::resolve_import_review_if_clean(&tx, story_id, &post_mutation_blockers)?;
    let import_state = review::read_import_state(&tx, story_id, scope)?;

    let node = project_node_content(&tx, media_dir, &structure.nodes[idx]);

    tx.commit()
        .map_err(|e| transport_error(&e, "commit", story_id))?;

    Ok((
        NodeWriteOutputDto {
            id: story_id.to_string(),
            updated_at: now_iso,
            content_checksum: new_checksum,
            node,
            import_state: import_state.map(|s| s.wire_tag().to_string()),
        },
        extra,
    ))
}

/// Write the current node's text + metadata label. `app_data_dir` locates the
/// node-media store so the re-projected node resolves its media slots.
pub fn save_node_content(
    db: &mut DbHandle,
    app_data_dir: &Path,
    input: SaveNodeContentInput,
) -> Result<NodeWriteOutputDto, AppError> {
    if input.text.chars().count() > MAX_NODE_TEXT_CHARS
        || input.label.chars().count() > MAX_NODE_LABEL_CHARS
    {
        return Err(AppError::local_storage_unavailable(
            "Enregistrement impossible: contenu du nœud trop long.",
            "Réduis la taille du texte ou du libellé puis réessaie.",
        )
        .with_details(serde_json::json!({ "source": "node_content_too_long" })));
    }
    let media_dir = resolve_node_media_dir(app_data_dir);
    let text = input.text;
    let label = input.label;
    // A content save consumes the node draft (the canonical row now reflects
    // the latest committed text).
    apply_node_mutation(
        db,
        &media_dir,
        &input.story_id,
        &input.node_id,
        true,
        move |node, _tx| {
            node.text = text;
            node.label = label;
            Ok(())
        },
    )
    .map(|(out, ())| out)
}

/// A media file already validated + promoted into the store, awaiting the DB
/// commit. Produced by [`store_node_media`] WITHOUT the DB lock so the (up to
/// 32 MiB) write never serialises other IPC commands (NFR5).
pub struct PreparedMedia {
    stored: StoredMedia,
    media_dir: PathBuf,
}

/// Validate (magic bytes + slot match) and PROMOTE the bytes into the store.
/// No DB access, so the IPC command runs this OUTSIDE the DB lock.
pub fn store_node_media(
    app_data_dir: &Path,
    kind: MediaKind,
    bytes: &[u8],
) -> Result<PreparedMedia, AppError> {
    // The file must match the requested slot: an image dropped on the audio
    // slot (or vice versa) is refused as unsupported for this slot.
    let sniffed = sniff_media(bytes)
        .ok_or_else(|| media_invalid(MediaCause::UnsupportedFormat, "unsupported_format"))?;
    if sniffed.kind != kind {
        return Err(media_invalid(
            MediaCause::UnsupportedFormat,
            "unsupported_format",
        ));
    }
    let (media_dir, staging_dir) =
        ensure_node_media_store(app_data_dir).map_err(map_store_error)?;
    let stored = store_media(&media_dir, &staging_dir, bytes).map_err(map_store_error)?;
    Ok(PreparedMedia { stored, media_dir })
}

/// Commit a PROMOTED media to the node's slot UNDER the DB lock: the edit-scope
/// guard, the asset INSERT, the previous-asset reclaim, and the file GC — but
/// NOT the file write (done by [`store_node_media`] off the lock). A commit
/// failure (e.g. a device-pack story — titleOnly scope — refused by the in-tx
/// guard) compensates the promoted file so the store never leaks a row-less file.
pub fn commit_node_media(
    db: &mut DbHandle,
    prepared: PreparedMedia,
    story_id: &str,
    node_id: &str,
    kind: MediaKind,
) -> Result<NodeWriteOutputDto, AppError> {
    let PreparedMedia { stored, media_dir } = prepared;
    let promoted = (stored.content_hash.clone(), stored.file_name.clone());
    let asset_id = uuid::Uuid::now_v7().to_string();
    let created_at = now_iso_ms()?;
    let story_id_owned = story_id.to_string();

    let result = apply_node_mutation(db, &media_dir, story_id, node_id, false, move |node, tx| {
        let story_id = story_id_owned;
        // Capture + delete the slot's PREVIOUS asset (a replacement), so a
        // replaced row + file can be GC'd after commit instead of leaking.
        let previous = match kind {
            MediaKind::Image => node.image_asset_id.take(),
            MediaKind::Audio => node.audio_asset_id.take(),
        };
        let previous_info = match previous {
            Some(old_id) => {
                let info: Option<(String, String)> = tx
                        .query_row(
                            "SELECT content_hash, file_name FROM assets WHERE id = ?1 AND story_id = ?2",
                            rusqlite::params![old_id, story_id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()
                        .map_err(|e| media_store_db_error(&e))?;
                tx.execute(
                    "DELETE FROM assets WHERE id = ?1 AND story_id = ?2",
                    rusqlite::params![old_id, story_id],
                )
                .map_err(|e| media_store_db_error(&e))?;
                info
            }
            None => None,
        };
        tx.execute(
                "INSERT INTO assets (id, story_id, content_hash, media_type, media_format, byte_size, file_name, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    asset_id,
                    story_id,
                    stored.content_hash,
                    stored.kind.as_str(),
                    stored.format,
                    stored.byte_size,
                    stored.file_name,
                    created_at,
                ],
            )
            .map_err(|e| media_store_db_error(&e))?;
        match kind {
            MediaKind::Image => node.image_asset_id = Some(asset_id),
            MediaKind::Audio => node.audio_asset_id = Some(asset_id),
        }
        Ok(previous_info)
    });

    match result {
        Ok((out, previous_info)) => {
            // Replacement GC: drop the replaced asset's file if now unreferenced.
            gc_unreferenced_file(db, &media_dir, previous_info);
            Ok(out)
        }
        Err(err) => {
            // The transaction rolled back, so the new asset row is gone but the
            // file was promoted before it — compensate it if now unreferenced.
            gc_unreferenced_file(db, &media_dir, Some(promoted));
            Err(err)
        }
    }
}

/// Convenience: [`store_node_media`] then [`commit_node_media`]. Used by tests
/// and simple callers; the IPC command splits the two so the DB lock is held
/// only for the commit, never across the file write (NFR5).
pub fn attach_node_media(
    db: &mut DbHandle,
    app_data_dir: &Path,
    input: AttachNodeMediaInput,
) -> Result<NodeWriteOutputDto, AppError> {
    let prepared = store_node_media(app_data_dir, input.kind, &input.bytes)?;
    commit_node_media(db, prepared, &input.story_id, &input.node_id, input.kind)
}

/// Remove the media from a node's slot. The asset row is deleted; the promoted
/// file is best-effort garbage-collected AFTER commit, only when no other asset
/// row still references the same content.
pub fn remove_node_media(
    db: &mut DbHandle,
    app_data_dir: &Path,
    input: RemoveNodeMediaInput,
) -> Result<NodeWriteOutputDto, AppError> {
    let media_dir = resolve_node_media_dir(app_data_dir);
    let kind = input.kind;
    let story_id = input.story_id.clone();
    let (out, removed) = apply_node_mutation(
        db,
        &media_dir,
        &input.story_id,
        &input.node_id,
        false,
        move |node, tx| {
            let asset_id = match kind {
                MediaKind::Image => node.image_asset_id.take(),
                MediaKind::Audio => node.audio_asset_id.take(),
            };
            let Some(asset_id) = asset_id else {
                return Ok(None);
            };
            let info: Option<(String, String)> = tx
                .query_row(
                    "SELECT content_hash, file_name FROM assets WHERE id = ?1 AND story_id = ?2",
                    rusqlite::params![asset_id, story_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|e| media_store_db_error(&e))?;
            tx.execute(
                "DELETE FROM assets WHERE id = ?1 AND story_id = ?2",
                rusqlite::params![asset_id, story_id],
            )
            .map_err(|e| media_store_db_error(&e))?;
            Ok(info)
        },
    )?;

    // Post-commit, best-effort file GC (refcounted by content hash).
    gc_unreferenced_file(db, &media_dir, removed);

    Ok(out)
}

/// Boot-time reconciliation of the node-media store: delete any PROMOTED file
/// that no `assets` row references. The per-action GC + attach compensation are
/// best-effort and swallow their errors, so a crash between `commit` and the
/// file delete — or a `remove_file` that failed — can leave a `<hash>.<ext>`
/// referenced by no row (a DB↔FS divergence / disk leak). This sweep reconciles
/// it at the next launch, mirroring the import store's staging sweep. The
/// `.staging` sub-directory is handled separately by `sweep_node_media_staging`.
/// Best-effort by contract — a failure never blocks the boot.
pub fn sweep_orphan_node_media(db: &DbHandle, app_data_dir: &Path) {
    let media_dir = resolve_node_media_dir(app_data_dir);
    let Ok(entries) = std::fs::read_dir(&media_dir) else {
        return;
    };
    let referenced: std::collections::HashSet<String> = {
        let Ok(mut stmt) = db.conn().prepare("SELECT file_name FROM assets") else {
            return;
        };
        let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else {
            return;
        };
        rows.flatten().collect()
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip the `.staging` sub-directory (only files are content-addressed
        // promoted media).
        if !path.is_file() {
            continue;
        }
        let is_orphan = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| !referenced.contains(name));
        if is_orphan {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Best-effort delete of a promoted media file IF no remaining `assets` row
/// references the same content (content-addressed sharing). Used by the
/// replacement GC, the removal GC, the attach-failure compensation, and the
/// structural node deletion (`structure::delete_node`).
pub(crate) fn gc_unreferenced_media_file(
    db: &DbHandle,
    media_dir: &Path,
    info: Option<(String, String)>,
) {
    gc_unreferenced_file(db, media_dir, info)
}

fn gc_unreferenced_file(db: &DbHandle, media_dir: &Path, info: Option<(String, String)>) {
    if let Some((content_hash, file_name)) = info {
        let still_used: bool = db
            .conn()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM assets WHERE content_hash = ?1)",
                rusqlite::params![content_hash],
                |r| r.get(0),
            )
            .unwrap_or(true);
        if !still_used {
            let _ = std::fs::remove_file(media_dir.join(&file_name));
        }
    }
}

/// Resolve a node media's stored file name (a short DB read). Held under the DB
/// lock by the command, which then releases the lock BEFORE the file read +
/// base64 (NFR5) — the lock must not span the I/O.
pub fn resolve_node_media_file(
    db: &DbHandle,
    story_id: &str,
    asset_id: &str,
) -> Result<String, AppError> {
    let file_name: Option<String> = db
        .conn()
        .query_row(
            "SELECT file_name FROM assets WHERE id = ?1 AND story_id = ?2",
            rusqlite::params![asset_id, story_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| media_processing("read"))?;
    file_name.ok_or_else(|| media_processing("read"))
}

/// Read a promoted media's bytes + MIME for a preview, OFF the DB lock. The
/// frontend never owns the bytes — the command wraps them in a `data:` URL.
pub fn read_node_media_file(
    media_dir: &Path,
    file_name: &str,
) -> Result<(Vec<u8>, &'static str), AppError> {
    read_media(media_dir, file_name).map_err(map_store_error)
}

/// Convenience: resolve + read in one call (DB lock spans the read). Used by
/// tests; the IPC command splits the two to keep the lock off the file I/O.
pub fn read_node_media(
    db: &DbHandle,
    media_dir: &Path,
    story_id: &str,
    asset_id: &str,
) -> Result<(Vec<u8>, &'static str), AppError> {
    let file_name = resolve_node_media_file(db, story_id, asset_id)?;
    read_node_media_file(media_dir, &file_name)
}

// ---------------------------------------------------------------------------
// Node-content recovery buffer (NFR8)
// ---------------------------------------------------------------------------

/// Buffer the in-progress node text + label. UPSERT guarded by `draft_at` so a
/// reordered IPC pair never lets a stale write clobber a fresher one (mirrors
/// `record_draft`). A non-existent story trips the FK and surfaces as a
/// recovery-channel failure.
pub fn record_node_draft(db: &mut DbHandle, input: RecordNodeDraftInput) -> Result<(), AppError> {
    if input.draft_text.chars().count() > MAX_NODE_TEXT_CHARS
        || input.draft_label.chars().count() > MAX_NODE_LABEL_CHARS
    {
        return Err(AppError::recovery_draft_unavailable(
            "Brouillon de nœud trop long pour être enregistré.",
            "Réduis la taille du texte puis réessaie.",
        )
        .with_details(serde_json::json!({ "source": "node_draft_too_long" })));
    }
    let now_iso = now_iso_ms()?;
    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| recovery_transport("begin_transaction"))?;
    tx.execute(
        "INSERT INTO node_drafts (story_id, node_id, draft_text, draft_label, draft_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(story_id) DO UPDATE \
           SET node_id = excluded.node_id, draft_text = excluded.draft_text, \
               draft_label = excluded.draft_label, draft_at = excluded.draft_at \
           WHERE excluded.draft_at >= node_drafts.draft_at",
        rusqlite::params![
            input.story_id,
            input.node_id,
            input.draft_text,
            input.draft_label,
            now_iso,
        ],
    )
    .map_err(|_| recovery_transport("upsert"))?;
    tx.commit().map_err(|_| recovery_transport("commit"))
}

/// Read the buffered node draft for a story, if any.
pub fn read_node_draft(db: &DbHandle, story_id: &str) -> Result<Option<NodeDraftRow>, AppError> {
    db.conn()
        .query_row(
            "SELECT node_id, draft_text, draft_label, draft_at FROM node_drafts WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| {
                Ok(NodeDraftRow {
                    node_id: r.get(0)?,
                    draft_text: r.get(1)?,
                    draft_label: r.get(2)?,
                    draft_at: r.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|_| recovery_transport("select"))
}

/// Drop the buffered node draft. The optional `expected_draft_at` is a CAS
/// guard so a concurrent buffer refresh is preserved.
pub fn discard_node_draft(
    db: &mut DbHandle,
    story_id: &str,
    expected_draft_at: Option<&str>,
) -> Result<(), AppError> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| recovery_transport("begin_transaction"))?;
    match expected_draft_at {
        Some(expected) => tx.execute(
            "DELETE FROM node_drafts WHERE story_id = ?1 AND draft_at = ?2",
            rusqlite::params![story_id, expected],
        ),
        None => tx.execute(
            "DELETE FROM node_drafts WHERE story_id = ?1",
            rusqlite::params![story_id],
        ),
    }
    .map_err(|_| recovery_transport("delete"))?;
    tx.commit().map_err(|_| recovery_transport("commit"))
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn story_missing(story_id: &str) -> AppError {
    AppError::library_inconsistent(
        "Histoire introuvable, recharge la bibliothèque.",
        "Retourne à la bibliothèque et recharge la liste.",
    )
    .with_details(serde_json::json!({ "source": "story_missing", "id": story_id }))
}

fn structure_corrupt(story_id: &str) -> AppError {
    AppError::library_inconsistent(
        "La structure interne de l'histoire est illisible ou incohérente.",
        "Relance Rustory pour reconstruire la vue cohérente.",
    )
    .with_details(serde_json::json!({ "source": "structure_corrupt", "id": story_id }))
}

fn node_missing(story_id: &str, node_id: &str) -> AppError {
    AppError::library_inconsistent(
        "Le nœud à modifier est introuvable dans l'histoire.",
        "Recharge l'éditeur puis réessaie.",
    )
    .with_details(
        serde_json::json!({ "source": "node_missing", "id": story_id, "nodeId": node_id }),
    )
}

/// Refusal of a node write on a story outside the FULL edit scope: a
/// device-pack story's content is carried by the pack copied from the device
/// (only its title is a local metadata) — the UI never offers the controls,
/// this is the authoritative backend guard.
fn node_not_editable(story_id: &str) -> AppError {
    AppError::library_inconsistent(
        "Le contenu de cette histoire est porté par le pack copié depuis l'appareil et ne peut pas être modifié ici.",
        "Tu peux modifier le titre depuis l'éditeur ; le contenu du pack reste celui de l'appareil.",
    )
    .with_details(serde_json::json!({ "source": "node_not_editable", "id": story_id }))
}

fn transport_error(_err: &rusqlite::Error, stage: &'static str, story_id: &str) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu enregistrer ta modification.",
        "Réessaie dans un instant ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_node_write",
        "stage": stage,
        "id": story_id,
    }))
}

/// `true` when the canonical facts carry at least one BLOCKING blocker. A
/// fixable-only set (e.g. an invalid persisted title) does not block a node
/// content edit.
fn has_blocking(facts: &CanonicalStoryFacts) -> bool {
    validate_canonical(facts)
        .iter()
        .any(|b| b.severity == Severity::Blocking)
}

/// The wire slot state derived from the node-media validation axis. This is the
/// first LIVING emitter of the `Media` axis: the projection derives the slot
/// state from a [`MediaCause`] (its frozen severity), not an ad-hoc string —
/// `ready` (no cause), `attention` (a `Fixable` cause such as a missing source),
/// or `blocked` (a `Blocking` cause).
fn media_cause_slot_state(cause: Option<MediaCause>) -> &'static str {
    match cause.map(MediaCause::severity) {
        None => "ready",
        Some(Severity::Fixable) => "attention",
        Some(Severity::Blocking) => "blocked",
    }
}

/// A node-media BLOCK (`MEDIA_INVALID`) derived from its [`MediaCause`]: the
/// cause owns the user-facing copy (its severity is always `Blocking` on this
/// path), the fine-grained `stage` is carried in `details` for support triage.
fn media_invalid(cause: MediaCause, stage: &'static str) -> AppError {
    let (message, action) = match cause {
        MediaCause::UnsupportedFormat => (
            "Ce média utilise un format non pris en charge.",
            "Choisis une image PNG ou JPEG, ou un son MP3, WAV ou OGG.",
        ),
        MediaCause::Unreadable | MediaCause::SourceMissing => (
            "Ce média est illisible ou dépasse la taille autorisée.",
            "Choisis un fichier plus léger et lisible puis réessaie.",
        ),
    };
    AppError::media_invalid(message, action)
        .with_details(serde_json::json!({ "source": "media_invalid", "stage": stage }))
}

fn media_processing(stage: &'static str) -> AppError {
    AppError::media_processing_failed(
        "Média indisponible: le stockage local a échoué.",
        "Réessaie dans un instant ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({ "source": "media_processing_failed", "stage": stage }))
}

fn map_store_error(err: NodeMediaError) -> AppError {
    match err {
        NodeMediaError::UnsupportedFormat => {
            media_invalid(MediaCause::UnsupportedFormat, "unsupported_format")
        }
        NodeMediaError::Oversize => media_invalid(MediaCause::Unreadable, "oversize"),
        NodeMediaError::Transport(stage) => media_processing(stage),
    }
}

fn media_store_db_error(_err: &rusqlite::Error) -> AppError {
    media_processing("db")
}

fn recovery_transport(stage: &'static str) -> AppError {
    AppError::recovery_draft_unavailable(
        "Récupération du nœud indisponible: vérifie le disque local et réessaie.",
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({ "source": "node_draft", "stage": stage }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::{create_story, get_story_detail, CreateStoryInput};
    use crate::domain::shared::AppErrorCode;
    use crate::domain::story::START_NODE_ID;
    use crate::infrastructure::db;
    use tempfile::TempDir;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];
    const OGG: &[u8] = b"OggS\0\0\0\0\0\0\0\0";

    fn fresh_db() -> DbHandle {
        let mut db = db::open_in_memory().expect("open");
        db::run_migrations(&mut db).expect("migrate");
        db
    }

    fn new_story(db: &mut DbHandle) -> String {
        create_story(
            db,
            CreateStoryInput {
                title: "Histoire".into(),
            },
        )
        .expect("create")
        .id
    }

    fn mark_local_import(db: &DbHandle, story_id: &str) {
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, imported_at) \
                 VALUES (?1, 'rustory', 1, 'a.rustory', ?2, 'recognized', '2026-06-27T00:00:00.000Z')",
                rusqlite::params![story_id, "a".repeat(64)],
            )
            .expect("insert provenance");
    }

    fn mark_local_import_needs_review(db: &DbHandle, story_id: &str) {
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'a.rustory', ?2, 'needs_review', '[{\"aspect\":\"timestamps\",\"category\":\"ambiguous\"}]', '2026-06-27T00:00:00.000Z')",
                rusqlite::params![story_id, "a".repeat(64)],
            )
            .expect("insert provenance");
    }

    fn mark_device_pack(db: &DbHandle, story_id: &str) {
        db.conn()
            .execute(
                "INSERT INTO story_imports (story_id, pack_uuid, source_device_identifier, imported_at, pack_file_count, pack_total_bytes, pack_checksum, source_family) \
                 VALUES (?1, '019739b2-0000-7000-8000-000000000000', '0123456789abcdef0123456789abcdef', '2026-07-06T00:00:00.000Z', 5, 18, ?2, 'lunii')",
                rusqlite::params![story_id, "ab".repeat(32)],
            )
            .expect("insert pack provenance");
    }

    fn promoted_file_count(media_dir: &Path) -> usize {
        std::fs::read_dir(media_dir)
            .map(|rd| rd.flatten().filter(|e| e.path().is_file()).count())
            .unwrap_or(0)
    }

    #[test]
    fn save_node_content_persists_text_and_label_and_rechecksums() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        let before: String = db
            .conn()
            .query_row(
                "SELECT content_checksum FROM stories WHERE id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();

        let out = save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                text: "Il était une fois".into(),
                label: "Début".into(),
            },
        )
        .expect("save");

        assert_eq!(out.node.text, "Il était une fois");
        assert_eq!(out.node.label, "Début");
        assert_ne!(out.content_checksum, before, "checksum must change");

        let detail = get_story_detail(&db, tmp.path(), &id, None)
            .unwrap()
            .unwrap();
        let node = detail.node.expect("node projected");
        assert_eq!(node.text, "Il était une fois");
        assert_eq!(node.label, "Début");
        assert!(detail.editable, "native story is editable");
    }

    #[test]
    fn save_node_content_rejects_unknown_node() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        let err = save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id,
                node_id: "ghost".into(),
                text: "x".into(),
                label: "y".into(),
            },
        )
        .expect_err("must fail");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        assert_eq!(err.details.unwrap()["source"], "node_missing");
    }

    #[test]
    fn save_node_content_on_missing_story_is_library_inconsistent() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let err = save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: "nope".into(),
                node_id: START_NODE_ID.into(),
                text: "x".into(),
                label: "y".into(),
            },
        )
        .expect_err("must fail");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        assert_eq!(err.details.unwrap()["source"], "story_missing");
    }

    #[test]
    fn save_node_content_never_touches_another_story() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let a = new_story(&mut db);
        let b = new_story(&mut db);
        let b_before: (String, String) = db
            .conn()
            .query_row(
                "SELECT structure_json, content_checksum FROM stories WHERE id = ?1",
                [&b],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: a,
                node_id: START_NODE_ID.into(),
                text: "edited".into(),
                label: "lab".into(),
            },
        )
        .expect("save a");
        let b_after: (String, String) = db
            .conn()
            .query_row(
                "SELECT structure_json, content_checksum FROM stories WHERE id = ?1",
                [&b],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(b_before, b_after, "FR18: the other story is untouched");
    }

    #[test]
    fn attach_then_remove_image_round_trip() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        let media_dir = resolve_node_media_dir(tmp.path());

        let attached = attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
                bytes: PNG.to_vec(),
            },
        )
        .expect("attach");
        let slot = attached.node.image.expect("image slot");
        assert_eq!(slot.state, "ready");
        assert_eq!(slot.format.as_deref(), Some("png"));
        assert!(attached.node.audio.is_none());

        let (count, file_name): (u32, String) = db
            .conn()
            .query_row(
                "SELECT COUNT(*), MAX(file_name) FROM assets WHERE story_id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert!(media_dir.join(&file_name).exists(), "promoted file present");
        let _ = slot;

        let removed = remove_node_media(
            &mut db,
            tmp.path(),
            RemoveNodeMediaInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
            },
        )
        .expect("remove");
        assert!(removed.node.image.is_none(), "slot cleared");
        let count_after: u32 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE story_id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count_after, 0, "asset row deleted");
        assert!(
            !media_dir.join(&file_name).exists(),
            "the unreferenced file is GC'd on remove"
        );
    }

    #[test]
    fn attach_rejects_wrong_kind_for_slot() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        // An OGG audio offered to the IMAGE slot is unsupported for that slot.
        let err = attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id,
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
                bytes: OGG.to_vec(),
            },
        )
        .expect_err("must reject");
        assert_eq!(err.code, AppErrorCode::MediaInvalid);
        assert_eq!(err.details.unwrap()["stage"], "unsupported_format");
    }

    #[test]
    fn attach_rejects_unsupported_bytes_as_media_invalid() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        let err = attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id,
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
                bytes: b"<svg>not supported</svg>".to_vec(),
            },
        )
        .expect_err("must reject");
        assert_eq!(err.code, AppErrorCode::MediaInvalid);
    }

    #[test]
    fn read_node_media_returns_promoted_bytes() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        let attached = attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
                bytes: PNG.to_vec(),
            },
        )
        .expect("attach");
        let asset_id = attached.node.image.unwrap().asset_id;
        let media_dir = resolve_node_media_dir(tmp.path());
        let (bytes, mime) = read_node_media(&db, &media_dir, &id, &asset_id).expect("read");
        assert_eq!(bytes, PNG);
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn node_draft_record_read_discard_round_trip() {
        let mut db = fresh_db();
        let id = new_story(&mut db);
        record_node_draft(
            &mut db,
            RecordNodeDraftInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                draft_text: "en cours".into(),
                draft_label: "lab".into(),
            },
        )
        .expect("record");
        let draft = read_node_draft(&db, &id).expect("read").expect("present");
        assert_eq!(draft.draft_text, "en cours");
        assert_eq!(draft.node_id, START_NODE_ID);
        discard_node_draft(&mut db, &id, None).expect("discard");
        assert!(read_node_draft(&db, &id).expect("read").is_none());
    }

    #[test]
    fn saving_node_content_consumes_the_node_draft() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        record_node_draft(
            &mut db,
            RecordNodeDraftInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                draft_text: "buffered".into(),
                draft_label: String::new(),
            },
        )
        .expect("record");
        save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                text: "saved".into(),
                label: String::new(),
            },
        )
        .expect("save");
        assert!(
            read_node_draft(&db, &id).expect("read").is_none(),
            "a successful content save drops the node draft"
        );
    }

    #[test]
    fn saving_one_node_never_consumes_another_nodes_draft() {
        // The single per-story draft row may buffer ANOTHER node's unsaved
        // keystrokes: a successful save of n1 must not silently destroy the
        // recoverable buffer of n2 (NFR8).
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        crate::application::story::structure::add_node(&mut db, &id, None).expect("n2");
        record_node_draft(
            &mut db,
            RecordNodeDraftInput {
                story_id: id.clone(),
                node_id: "n2".into(),
                draft_text: "non sauvé sur n2".into(),
                draft_label: String::new(),
            },
        )
        .expect("record");

        save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                text: "saved on n1".into(),
                label: String::new(),
            },
        )
        .expect("save n1");
        let draft = read_node_draft(&db, &id)
            .expect("read")
            .expect("n2's buffer must survive a save of n1");
        assert_eq!(draft.node_id, "n2");
        assert_eq!(draft.draft_text, "non sauvé sur n2");

        // Saving THE buffered node does consume it.
        save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: "n2".into(),
                text: "saved on n2".into(),
                label: String::new(),
            },
        )
        .expect("save n2");
        assert!(read_node_draft(&db, &id).expect("read").is_none());
    }

    // F2: a media mutation must NOT consume the buffered text draft (a kill
    // before the next debounce would otherwise lose un-flushed text — NFR8).
    #[test]
    fn media_mutation_preserves_the_text_draft() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        record_node_draft(
            &mut db,
            RecordNodeDraftInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                draft_text: "frappe non sauvée".into(),
                draft_label: String::new(),
            },
        )
        .expect("record");
        attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
                bytes: PNG.to_vec(),
            },
        )
        .expect("attach");
        let draft = read_node_draft(&db, &id).expect("read");
        assert!(
            draft.is_some_and(|d| d.draft_text == "frappe non sauvée"),
            "an attach must leave the text draft for the next content save"
        );
    }

    // F1: writing a node on a DEVICE-PACK story is refused authoritatively
    // (the pack carries the content; only the title is a local metadata).
    #[test]
    fn device_pack_node_write_is_refused() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        mark_device_pack(&db, &id);

        let text_err = save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                text: "x".into(),
                label: String::new(),
            },
        )
        .expect_err("text write refused");
        assert_eq!(text_err.code, AppErrorCode::LibraryInconsistent);
        assert_eq!(text_err.details.unwrap()["source"], "node_not_editable");
        assert_eq!(
            text_err.message,
            "Le contenu de cette histoire est porté par le pack copié depuis l'appareil et ne peut pas être modifié ici."
        );
        assert_eq!(
            text_err.user_action.as_deref(),
            Some("Tu peux modifier le titre depuis l'éditeur ; le contenu du pack reste celui de l'appareil.")
        );

        let media_err = attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
                bytes: PNG.to_vec(),
            },
        )
        .expect_err("media write refused");
        assert_eq!(media_err.details.unwrap()["source"], "node_not_editable");
        // The refusal happens BEFORE any file is promoted.
        assert_eq!(promoted_file_count(&resolve_node_media_dir(tmp.path())), 0);
    }

    // FR21: a `.rustory` import carries the FULL edit scope — its node writes
    // go through exactly like a native story's.
    #[test]
    fn rustory_import_node_write_is_accepted() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        mark_local_import(&db, &id);

        let out = save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                text: "corrigé dans l'éditeur".into(),
                label: "Début".into(),
            },
        )
        .expect("a .rustory import edits like a native story");
        assert_eq!(out.node.text, "corrigé dans l'éditeur");

        let detail = get_story_detail(&db, tmp.path(), &id, None)
            .unwrap()
            .unwrap();
        assert!(detail.editable, "full scope projects editable");
    }

    #[test]
    fn device_pack_story_is_not_editable() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        mark_device_pack(&db, &id);
        let detail = get_story_detail(&db, tmp.path(), &id, None)
            .unwrap()
            .unwrap();
        assert!(!detail.editable, "a device pack projects titleOnly");
    }

    // AC3: a clean node write settles a pending import review inside the
    // write transaction, and the ACK carries the post-UPDATE state.
    #[test]
    fn a_clean_node_write_resolves_a_pending_review_and_acks_it() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        mark_local_import_needs_review(&db, &id);

        let out = save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                text: "corrigé".into(),
                label: String::new(),
            },
        )
        .expect("save");
        assert_eq!(
            out.import_state.as_deref(),
            Some("resolved"),
            "the ACK carries the settled state"
        );

        let (state, summary): (String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT import_state, findings_summary FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("provenance row");
        assert_eq!(state, "resolved");
        assert!(summary.is_some(), "the findings trace is KEPT in base");
    }

    // An acknowledged no-op (same canonical bytes — re-saving the unchanged
    // text/label) is NOT a real write: the pending review stays, the row is
    // not rewritten (`updated_at` untouched), the ACK carries the CURRENT
    // state, and the node draft is still consumed (the canonical row already
    // reflects the latest committed text).
    #[test]
    fn a_no_op_node_save_does_not_resolve_but_acks_the_current_state() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                text: "inchangé".into(),
                label: "Début".into(),
            },
        )
        .expect("first real save");
        mark_local_import_needs_review(&db, &id);
        record_node_draft(
            &mut db,
            RecordNodeDraftInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                draft_text: "inchangé".into(),
                draft_label: "Début".into(),
            },
        )
        .expect("buffer draft");
        let (updated_before, checksum_before): (String, String) = db
            .conn()
            .query_row(
                "SELECT updated_at, content_checksum FROM stories WHERE id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row");

        std::thread::sleep(std::time::Duration::from_millis(2));
        let out = save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                text: "inchangé".into(),
                label: "Début".into(),
            },
        )
        .expect("no-op save");

        assert_eq!(
            out.import_state.as_deref(),
            Some("needsReview"),
            "the no-op ACK carries the CURRENT state"
        );
        let (updated_after, checksum_after): (String, String) = db
            .conn()
            .query_row(
                "SELECT updated_at, content_checksum FROM stories WHERE id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("row");
        assert_eq!(
            updated_after, updated_before,
            "a no-op save must not bump updated_at"
        );
        assert_eq!(checksum_after, checksum_before);
        // The ACK mirrors the STORED state, not a fresh timestamp.
        assert_eq!(out.updated_at, updated_before);
        assert_eq!(out.content_checksum, checksum_before);
        let state: String = db
            .conn()
            .query_row(
                "SELECT import_state FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .expect("provenance row");
        assert_eq!(
            state, "needs_review",
            "no real write happened, the review stays pending"
        );
        assert!(
            read_node_draft(&db, &id).expect("read").is_none(),
            "a no-op content save still consumes the node draft"
        );
    }

    // Removing a media from an already-empty slot lands on the same canonical
    // bytes: acknowledged no-op, the pending review is NOT resolved.
    #[test]
    fn removing_media_from_an_empty_slot_does_not_resolve_a_pending_review() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        mark_local_import_needs_review(&db, &id);

        let out = remove_node_media(
            &mut db,
            tmp.path(),
            RemoveNodeMediaInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
            },
        )
        .expect("no-op removal");

        assert_eq!(
            out.import_state.as_deref(),
            Some("needsReview"),
            "the no-op ACK carries the CURRENT state"
        );
        let state: String = db
            .conn()
            .query_row(
                "SELECT import_state FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .expect("provenance row");
        assert_eq!(
            state, "needs_review",
            "no real write happened, the review stays pending"
        );
    }

    // The ACK's `importState` is an explicit None for a native story (no
    // provenance row) — same None-unless-Full rule as the detail projection.
    #[test]
    fn a_native_node_write_acks_a_null_import_state() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);

        let out = save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id,
                node_id: START_NODE_ID.into(),
                text: "texte".into(),
                label: String::new(),
            },
        )
        .expect("save");
        assert_eq!(out.import_state, None);
    }

    // F4: a story whose stored checksum no longer matches its structure is
    // refused for edit and projects no node (never an implicit repair).
    #[test]
    fn checksum_mismatch_refuses_edit_and_projects_no_node() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        db.conn()
            .execute(
                "UPDATE stories SET content_checksum = ?1 WHERE id = ?2",
                rusqlite::params!["0".repeat(64), id],
            )
            .expect("corrupt checksum");

        let detail = get_story_detail(&db, tmp.path(), &id, None)
            .unwrap()
            .unwrap();
        assert!(
            detail.node.is_none(),
            "a checksum-mismatched story projects no node"
        );

        let err = save_node_content(
            &mut db,
            tmp.path(),
            SaveNodeContentInput {
                story_id: id,
                node_id: START_NODE_ID.into(),
                text: "x".into(),
                label: String::new(),
            },
        )
        .expect_err("editing a corrupt story is refused");
        assert_eq!(err.details.unwrap()["source"], "structure_corrupt");
    }

    // F5: a node referencing an asset whose source file is gone projects
    // `attention`, not `ready`.
    #[test]
    fn projection_marks_a_missing_source_file_as_attention() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
                bytes: PNG.to_vec(),
            },
        )
        .expect("attach");
        let file_name: String = db
            .conn()
            .query_row(
                "SELECT file_name FROM assets WHERE story_id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        // The source file vanishes from the store (external deletion).
        std::fs::remove_file(resolve_node_media_dir(tmp.path()).join(&file_name)).unwrap();

        let detail = get_story_detail(&db, tmp.path(), &id, None)
            .unwrap()
            .unwrap();
        let slot = detail.node.unwrap().image.expect("slot present");
        assert_eq!(slot.state, "attention");
        assert!(slot.format.is_none());
    }

    // F6: when the transaction fails AFTER the file is promoted, the orphan
    // file is compensated (no DB↔FS divergence, no disk leak).
    #[test]
    fn attach_compensates_the_promoted_file_when_the_mutation_fails() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        // A bad node id makes `apply_node_mutation` fail AFTER `store_media`.
        let err = attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id,
                node_id: "ghost".into(),
                kind: MediaKind::Image,
                bytes: PNG.to_vec(),
            },
        )
        .expect_err("mutation fails");
        assert_eq!(err.details.unwrap()["source"], "node_missing");
        assert_eq!(
            promoted_file_count(&resolve_node_media_dir(tmp.path())),
            0,
            "the promoted file is compensated after the rollback"
        );
    }

    // F7: replacing a slot's media deletes the previous asset row and GCs its
    // file when no longer referenced.
    #[test]
    fn replacing_media_gcs_the_previous_asset() {
        let mut db = fresh_db();
        let tmp = TempDir::new().unwrap();
        let id = new_story(&mut db);
        let media_dir = resolve_node_media_dir(tmp.path());

        attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
                bytes: PNG.to_vec(),
            },
        )
        .expect("attach A");
        let first_file: String = db
            .conn()
            .query_row(
                "SELECT file_name FROM assets WHERE story_id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();

        // Replace with a DIFFERENT image (JPEG) — distinct content hash.
        let replaced = attach_node_media(
            &mut db,
            tmp.path(),
            AttachNodeMediaInput {
                story_id: id.clone(),
                node_id: START_NODE_ID.into(),
                kind: MediaKind::Image,
                bytes: JPEG.to_vec(),
            },
        )
        .expect("attach B");
        assert_eq!(replaced.node.image.unwrap().format.as_deref(), Some("jpeg"));

        // Exactly one asset row remains (the old one was reclaimed).
        let count: u32 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE story_id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "the replaced asset row is removed");
        assert!(
            !media_dir.join(&first_file).exists(),
            "the replaced file is GC'd when unreferenced"
        );
    }
}
