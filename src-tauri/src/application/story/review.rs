//! Import review RESOLUTION (FR21 / AC3): the durable `.rustory` import
//! review (`needs_review` / `partial`) is settled BY EDITING — no button,
//! no ceremony, no guided flow.
//!
//! This module is the SINGLE writer of the `resolved` import state.
//! Principle: a REAL write that leaves the canonical story ENTIRELY sound
//! settles the review. The oracle is the COMPLETE [`validate_canonical`]
//! blocker list over the post-mutation facts — any blocker of ANY severity
//! (a still-broken option link is `Fixable`) prevents resolution. Node
//! MEDIA are NEVER part of the oracle: a media `attention` slot is not a
//! `.rustory` import finding (its per-slot marker lives its own life).
//!
//! One-way by construction: the conditional UPDATE only moves
//! `needs_review` / `partial` forward, so a `resolved` row never regresses
//! (the living validation owns the present). Reading never resolves (no
//! write-on-read), `findings_summary` is KEPT as the review's trace, and
//! the transition alone never bumps `stories.updated_at`.

use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::application::story::scope::{try_story_edit_scope, StoryEditScope};
use crate::domain::shared::AppError;
use crate::domain::story::{validate_canonical, CanonicalBlocker, CanonicalStoryFacts};
use crate::ipc::dto::import_export::import_state_dto_from_tag;
use crate::ipc::dto::ImportStateDto;

/// Settle the story's import review when the post-write canonical facts are
/// ENTIRELY sound (`blockers` empty). Must run INSIDE the write transaction
/// of a real write. A no-op when any blocker remains, when the story has no
/// `.rustory` provenance row, or when the review is not pending
/// (`recognized` / `resolved` rows are left untouched — one-way).
pub fn resolve_import_review_if_clean(
    tx: &Transaction<'_>,
    story_id: &str,
    blockers: &[CanonicalBlocker],
) -> Result<(), AppError> {
    if !blockers.is_empty() {
        return Ok(());
    }
    tx.execute(
        "UPDATE story_local_imports SET import_state = 'resolved' \
         WHERE story_id = ?1 AND import_state IN ('needs_review', 'partial')",
        rusqlite::params![story_id],
    )
    .map_err(|_| review_transport("resolve", story_id))?;
    Ok(())
}

/// Read the story's durable import state for projection — the SINGLE
/// derivation shared by `get_story_detail` AND the write acknowledgements,
/// so the two can never diverge, even on forged data.
///
/// `None` unless the story carries the FULL edit scope (a device pack never
/// projects an import state — the forged two-table case is neutralized
/// here), `None` when no provenance row exists (native story), and `None`
/// when the stored tag is corrupt (degrade, never fail the read).
pub fn read_import_state(
    conn: &Connection,
    story_id: &str,
    scope: StoryEditScope,
) -> Result<Option<ImportStateDto>, AppError> {
    if scope != StoryEditScope::Full {
        return Ok(None);
    }
    let tag: Option<String> = conn
        .query_row(
            "SELECT import_state FROM story_local_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| review_transport("read_state", story_id))?;
    Ok(tag.as_deref().and_then(import_state_dto_from_tag))
}

/// Post-title-write review step shared by `update_story` and
/// `apply_recovery` (a title recovery is a real write): cheap EARLY-OUT
/// first — one provenance value read by PK, so a native title autosave
/// never pays a structure parse — then, ONLY when a review is pending on a
/// FULL-scope story (the forged two-table case must not resolve), recompute
/// the oracle over the POST-UPDATE facts and resolve if fully sound.
/// Returns the acknowledgement's `importState` wire tag, read POST-UPDATE
/// under the same None-unless-Full rule as the detail projection.
pub fn settle_review_after_title_write(
    tx: &Transaction<'_>,
    story_id: &str,
) -> Result<Option<String>, AppError> {
    let pending: Option<String> = tx
        .query_row(
            "SELECT import_state FROM story_local_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| review_transport("read_pending", story_id))?;
    let Some(pending) = pending else {
        // No provenance row: a native story or a device pack — nothing to
        // resolve, nothing to project.
        return Ok(None);
    };

    // FALLIBLE scope read: the acknowledgement built from this settle step
    // must never carry a fabricated state. Swallowing a read error here
    // (fail-closed `TitleOnly`) would skip the resolution — safe — but also
    // acknowledge `importState: null` while a review is actually pending,
    // and the frontend treats an incoming null as authoritative. Failing
    // the write (rolled back, retryable) keeps the ACK honest; the spines'
    // AUTHORIZATION guards keep the infallible fail-closed wrapper.
    let scope =
        try_story_edit_scope(tx, story_id).map_err(|_| review_transport("read_scope", story_id))?;
    if matches!(pending.as_str(), "needs_review" | "partial") && scope == StoryEditScope::Full {
        // The oracle over the POST-UPDATE facts (the title just written in
        // this same transaction). Reached only for a pending review, so a
        // native autosave never parses a structure for nothing.
        let facts: Option<CanonicalStoryFacts> = tx
            .query_row(
                "SELECT title, schema_version, structure_json, content_checksum \
                 FROM stories WHERE id = ?1",
                rusqlite::params![story_id],
                |r| {
                    Ok(CanonicalStoryFacts {
                        title: r.get(0)?,
                        schema_version: r.get(1)?,
                        structure_json: r.get(2)?,
                        content_checksum: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|_| review_transport("read_facts", story_id))?;
        if let Some(facts) = facts {
            let blockers = validate_canonical(&facts);
            resolve_import_review_if_clean(tx, story_id, &blockers)?;
        }
    }

    Ok(read_import_state(tx, story_id, scope)?.map(|s| s.wire_tag().to_string()))
}

fn review_transport(stage: &'static str, story_id: &str) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu enregistrer ta modification.",
        "Réessaie dans un instant ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_import_review",
        "stage": stage,
        "id": story_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::story::{create_story, CreateStoryInput};
    use crate::domain::story::{Axis, CanonicalCause, Severity};
    use crate::infrastructure::db::{self, DbHandle};

    const FINDINGS: &str = "[{\"aspect\":\"timestamps\",\"category\":\"ambiguous\"}]";

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

    fn mark_local_import(db: &DbHandle, story_id: &str, state: &str) {
        let summary: Option<&str> = if state == "recognized" {
            None
        } else {
            Some(FINDINGS)
        };
        db.conn()
            .execute(
                "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
                 VALUES (?1, 'rustory', 1, 'a.rustory', ?2, ?3, ?4, '2026-07-06T00:00:00.000Z')",
                rusqlite::params![story_id, "a".repeat(64), state, summary],
            )
            .expect("insert local provenance");
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

    fn provenance_row(db: &DbHandle, story_id: &str) -> (String, Option<String>) {
        db.conn()
            .query_row(
                "SELECT import_state, findings_summary FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![story_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("provenance row")
    }

    fn a_fixable_blocker() -> CanonicalBlocker {
        CanonicalBlocker {
            axis: Axis::Structure,
            cause: CanonicalCause::BrokenOptionLink,
            severity: Severity::Fixable,
        }
    }

    #[test]
    fn resolve_flips_a_pending_review_and_keeps_findings() {
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_local_import(&db, &id, "needs_review");
        let updated_before: String = db
            .conn()
            .query_row(
                "SELECT updated_at FROM stories WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .expect("updated_at");

        let tx = db.conn_mut().transaction().expect("tx");
        resolve_import_review_if_clean(&tx, &id, &[]).expect("resolve");
        tx.commit().expect("commit");

        let (state, summary) = provenance_row(&db, &id);
        assert_eq!(state, "resolved");
        assert_eq!(
            summary.as_deref(),
            Some(FINDINGS),
            "the findings trace is KEPT in base"
        );
        let updated_after: String = db
            .conn()
            .query_row(
                "SELECT updated_at FROM stories WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .expect("updated_at");
        assert_eq!(
            updated_after, updated_before,
            "the transition alone never bumps stories.updated_at"
        );
    }

    #[test]
    fn resolve_also_flips_a_partial_review() {
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_local_import(&db, &id, "partial");

        let tx = db.conn_mut().transaction().expect("tx");
        resolve_import_review_if_clean(&tx, &id, &[]).expect("resolve");
        tx.commit().expect("commit");

        assert_eq!(provenance_row(&db, &id).0, "resolved");
    }

    #[test]
    fn resolve_is_a_noop_when_any_blocker_remains() {
        // A still-broken option link is only Fixable — but ANY blocker of ANY
        // severity keeps the review pending.
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_local_import(&db, &id, "needs_review");

        let tx = db.conn_mut().transaction().expect("tx");
        resolve_import_review_if_clean(&tx, &id, &[a_fixable_blocker()]).expect("no-op");
        tx.commit().expect("commit");

        assert_eq!(provenance_row(&db, &id).0, "needs_review");
    }

    #[test]
    fn resolve_never_touches_a_recognized_or_resolved_row() {
        // One-way by construction: the conditional UPDATE only moves a
        // PENDING review forward.
        let mut db = fresh_db();
        let recognized = new_story(&mut db);
        mark_local_import(&db, &recognized, "recognized");
        let resolved = new_story(&mut db);
        mark_local_import(&db, &resolved, "resolved");

        let tx = db.conn_mut().transaction().expect("tx");
        resolve_import_review_if_clean(&tx, &recognized, &[]).expect("resolve recognized");
        resolve_import_review_if_clean(&tx, &resolved, &[]).expect("resolve resolved");
        tx.commit().expect("commit");

        assert_eq!(provenance_row(&db, &recognized).0, "recognized");
        assert_eq!(provenance_row(&db, &resolved).0, "resolved");
    }

    #[test]
    fn resolve_without_a_provenance_row_is_a_silent_noop() {
        let mut db = fresh_db();
        let id = new_story(&mut db);

        let tx = db.conn_mut().transaction().expect("tx");
        resolve_import_review_if_clean(&tx, &id, &[]).expect("no provenance row is fine");
        tx.commit().expect("commit");
    }

    #[test]
    fn read_import_state_is_none_unless_full_scope() {
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_local_import(&db, &id, "needs_review");

        // Full scope: the state is projected.
        assert_eq!(
            read_import_state(db.conn(), &id, StoryEditScope::Full).expect("read"),
            Some(ImportStateDto::NeedsReview)
        );
        // TitleOnly (e.g. the forged two-table case): NEVER projected.
        assert_eq!(
            read_import_state(db.conn(), &id, StoryEditScope::TitleOnly).expect("read"),
            None
        );
    }

    #[test]
    fn read_import_state_is_none_without_a_provenance_row() {
        let mut db = fresh_db();
        let id = new_story(&mut db);
        assert_eq!(
            read_import_state(db.conn(), &id, StoryEditScope::Full).expect("read"),
            None
        );
    }

    #[test]
    fn settle_after_title_write_early_outs_on_a_native_story() {
        // A native story has no provenance row: the settle step returns None
        // after ONE PK read — proven by corrupting the structure, which the
        // early-out must never parse or validate.
        let mut db = fresh_db();
        let id = new_story(&mut db);
        db.conn()
            .execute(
                "UPDATE stories SET structure_json = 'not json' WHERE id = ?1",
                rusqlite::params![id],
            )
            .expect("corrupt structure");

        let tx = db.conn_mut().transaction().expect("tx");
        let state = settle_review_after_title_write(&tx, &id).expect("settle");
        tx.commit().expect("commit");
        assert_eq!(state, None, "no provenance row → nothing to project");
    }

    #[test]
    fn settle_after_title_write_resolves_a_pending_review_on_a_sound_story() {
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_local_import(&db, &id, "needs_review");

        let tx = db.conn_mut().transaction().expect("tx");
        let state = settle_review_after_title_write(&tx, &id).expect("settle");
        tx.commit().expect("commit");

        assert_eq!(state.as_deref(), Some("resolved"));
        assert_eq!(provenance_row(&db, &id).0, "resolved");
    }

    #[test]
    fn settle_after_title_write_keeps_the_review_when_a_blocker_remains() {
        // The canonical facts still carry a blocker (a broken option link):
        // the review stays pending and the ACK carries the honest state.
        let mut db = fresh_db();
        let id = new_story(&mut db);
        crate::application::story::structure::add_option(&mut db, &id, "n1", "Aller")
            .expect("option");
        crate::application::story::structure::add_node(&mut db, &id, Some(("n1", 0)))
            .expect("create-and-link n2");
        crate::application::story::structure::delete_node(
            &mut db,
            std::env::temp_dir().as_path(),
            &id,
            "n2",
        )
        .expect("break the link");
        mark_local_import(&db, &id, "needs_review");

        let tx = db.conn_mut().transaction().expect("tx");
        let state = settle_review_after_title_write(&tx, &id).expect("settle");
        tx.commit().expect("commit");

        assert_eq!(state.as_deref(), Some("needsReview"));
        assert_eq!(provenance_row(&db, &id).0, "needs_review");
    }

    #[test]
    fn settle_after_title_write_never_resolves_a_forged_two_table_story() {
        // Forged rows in BOTH provenance tables: the pack takes precedence
        // (TitleOnly), so the review is NOT settled and nothing is projected.
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_device_pack(&db, &id);
        mark_local_import(&db, &id, "needs_review");

        let tx = db.conn_mut().transaction().expect("tx");
        let state = settle_review_after_title_write(&tx, &id).expect("settle");
        tx.commit().expect("commit");

        assert_eq!(state, None, "None unless the FULL edit scope");
        assert_eq!(
            provenance_row(&db, &id).0,
            "needs_review",
            "a TitleOnly story never resolves its forged local review"
        );
    }

    #[test]
    fn settle_after_title_write_carries_a_recognized_state_untouched() {
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_local_import(&db, &id, "recognized");

        let tx = db.conn_mut().transaction().expect("tx");
        let state = settle_review_after_title_write(&tx, &id).expect("settle");
        tx.commit().expect("commit");

        assert_eq!(state.as_deref(), Some("recognized"));
        assert_eq!(provenance_row(&db, &id).0, "recognized");
    }

    #[test]
    fn settle_after_title_write_fails_rather_than_acking_a_fabricated_state() {
        // A pending review whose scope read errors: the settle must FAIL the
        // write (rolled back, retryable) — never return `Ok(None)`, which
        // the acknowledgement would carry as `importState: null` and the
        // frontend would treat as authoritative (an ACK never lies).
        let mut db = fresh_db();
        let id = new_story(&mut db);
        mark_local_import(&db, &id, "needs_review");
        db.conn()
            .execute("DROP TABLE story_imports", [])
            .expect("drop the pack provenance table");

        let tx = db.conn_mut().transaction().expect("tx");
        let err = settle_review_after_title_write(&tx, &id).expect_err("must fail, not fabricate");
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "sqlite_import_review");
        assert_eq!(details["stage"], "read_scope");
        drop(tx);

        assert_eq!(
            provenance_row(&db, &id).0,
            "needs_review",
            "the pending review is untouched by the failed settle"
        );
    }
}
