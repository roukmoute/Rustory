//! RSS external-source creation application service (the FR31 flow).
//!
//! Two phases, NO mutation before acceptance:
//!
//! 1. [`preview_rss_source`] — validates the address (Rust-authoritative),
//!    fetches the feed through the injected [`RssFeedSource`] (bounded,
//!    explicit-action-only) and runs the bounded domain parse. PURE: zero
//!    byte written, zero DB row, zero store file — AC2 is structurally
//!    guaranteed before acceptance.
//! 2. [`accept_rss_story_creation`] — RE-FETCHES and RE-PARSES from zero
//!    (**the source is the authority**, the network equivalent of the
//!    folder flow's "the disk is the authority"; the frontend never
//!    re-submits content). The chosen item is resolved by STRICT `guid`
//!    (else exact `title`+`link`); a missing/ambiguous item or a feed
//!    turned blocked is the honest recoverable refusal
//!    [`RssCreationOutcome::SourceChanged`] with ZERO mutation — NEVER a
//!    creation from the stale preview data. Otherwise ONE `BEGIN
//!    IMMEDIATE` transaction inserts the canonical `stories` row (fresh
//!    UUIDv7, `created_at = updated_at = now` — a BIRTH, exactly like the
//!    structured-folder creation)
//!    and the provenance row (`source_format = 'rss'`, host-only source
//!    name, checksum of the SECOND fetch's bytes — the bytes actually
//!    ingested). No media is ever downloaded, so there is nothing to
//!    promote and nothing to compensate: a failed transaction rolls back
//!    fully and leaves NOTHING.

use std::time::Duration;

use crate::application::story::now_iso_ms;
use crate::domain::import::{
    content_source_activation, feed_url_host, parse_rss, resolve_rss_item, rss_import_state,
    rss_item_findings, rss_item_fingerprint, ContentSourceActivation, ContentSourceKind,
    ContentSourceLine, RssAnalysis, RssItemRef, RSS_FALLBACK_TITLE_PREFIX,
    RSS_SOURCE_FORMAT_VERSION,
};
use crate::domain::shared::AppError;
use crate::domain::story::{
    canonical_structure_json, content_checksum, content_checksum_bytes, normalize_title,
    validate_title, CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::device::RssFeedSource;
use crate::ipc::dto::import_export::{
    rss_import_report_dto, serialize_findings_summary, state_db_tag, state_dto,
};
use crate::ipc::dto::StoryCardDto;

/// The application-level outcome of previewing a feed: the HOST (the only
/// address fragment that ever crosses further), the SHA-256 fingerprint of
/// the fetched bytes and the typed domain analysis.
#[derive(Debug, Clone)]
pub struct RssPreviewOutcome {
    pub source_host: String,
    pub feed_checksum: String,
    pub analysis: RssAnalysis,
}

/// The content-source policy gate, consulted by BOTH facades (preview AND
/// accept) BEFORE the address validation and BEFORE any network dispatch:
/// a policy refusal never produces a byte of traffic (the recording mock
/// proves zero fetch). The matrix travels as a parameter — the commands
/// hand `official_content_sources()`, tests inject custom distributions —
/// so the policy stays consulted in one place per flow. Fail-closed: a
/// kind missing from the received matrix refuses exactly like a
/// `NotActivated` one (the gate never enables by default).
fn ensure_rss_source_enabled(sources: &[ContentSourceLine]) -> Result<(), AppError> {
    match content_source_activation(sources, ContentSourceKind::Rss) {
        ContentSourceActivation::Enabled => Ok(()),
        ContentSourceActivation::NotActivated | ContentSourceActivation::BlockedByPolicy => {
            Err(AppError::content_source_unavailable(ContentSourceKind::Rss))
        }
    }
}

/// Phase 1 — fetch + parse with ZERO mutation. Only TRANSPORT failures
/// (invalid address, unreachable source, over-cap response) reject; every
/// feed-CONTENT problem is a typed verdict inside the analysis. The
/// content-source policy is consulted FIRST: a non-enabled `rss` line in
/// `sources` refuses with `CONTENT_SOURCE_UNAVAILABLE` before any I/O.
pub fn preview_rss_source(
    sources: &[ContentSourceLine],
    source: &dyn RssFeedSource,
    url: &str,
    budget: Duration,
) -> Result<RssPreviewOutcome, AppError> {
    ensure_rss_source_enabled(sources)?;
    let source_host = feed_url_host(url).ok_or_else(invalid_feed_url_error)?;
    let bytes = source.fetch(url, budget)?;
    let feed_checksum = content_checksum_bytes(&bytes);
    let analysis = parse_rss(&bytes);
    Ok(RssPreviewOutcome {
        source_host,
        feed_checksum,
        analysis,
    })
}

/// The typed outcome of an accept: the created card + its report, or the
/// honest recoverable refusal (the source diverged since the preview —
/// nothing was mutated). The refusal is a VERDICT, never an `AppError`.
#[derive(Debug, Clone)]
pub enum RssCreationOutcome {
    Created { story: StoryCardDto },
    SourceChanged,
}

/// The fully re-proven, ready-to-commit ingestion — everything the atomic
/// DB transaction needs, produced WITHOUT any DB access
/// ([`prepare_rss_story_creation`]) so the network fetch never serializes
/// other commands behind the DB lock.
#[derive(Debug)]
pub struct PreparedRssCreation {
    title: String,
    structure_json: String,
    checksum: String,
    now_iso: String,
    source_host: String,
    feed_checksum: String,
    state: crate::domain::import::ImportState,
    findings: Vec<crate::domain::import::RecognitionFinding>,
}

/// The typed outcome of the DB-free accept phase: the honest refusal, or
/// the prepared creation to hand to [`commit_rss_story_creation`].
#[derive(Debug)]
pub enum RssAcceptPhase {
    SourceChanged,
    Prepared(Box<PreparedRssCreation>),
}

/// Phase 2a — RE-fetch, re-parse and re-prove the chosen item, with NO DB
/// access at all: the command runs this BEFORE taking the DB lock, so the
/// (up to 30 s) network fetch never holds it. `expected_fingerprint` is
/// the canonical proof of the PREVIEWED item: the fresh item must match
/// it EXACTLY — a resolvable reference (same guid) whose content diverged
/// is the honest `SourceChanged` refusal, never a creation from content
/// the user never reread. The accept re-proves EVERYTHING, the policy
/// included: the gate runs FIRST, so a direct command call can never
/// bypass the distribution's content-source matrix.
pub fn prepare_rss_story_creation(
    sources: &[ContentSourceLine],
    source: &dyn RssFeedSource,
    url: &str,
    item_ref: &RssItemRef,
    expected_fingerprint: &str,
    budget: Duration,
) -> Result<RssAcceptPhase, AppError> {
    ensure_rss_source_enabled(sources)?;
    let source_host = feed_url_host(url).ok_or_else(invalid_feed_url_error)?;
    // RE-fetch + re-parse from zero: the reference is a pointer, never an
    // authority; the checksum persisted below fingerprints THESE bytes.
    let bytes = source.fetch(url, budget)?;
    let feed_checksum = content_checksum_bytes(&bytes);
    let analysis = parse_rss(&bytes);
    if analysis.is_blocked() {
        // The feed turned blocked between the preview and the accept.
        return Ok(RssAcceptPhase::SourceChanged);
    }
    let Some(item) = resolve_rss_item(&analysis.items, item_ref) else {
        // Missing or ambiguous — an approximate match is never taken.
        return Ok(RssAcceptPhase::SourceChanged);
    };
    if rss_item_fingerprint(item) != expected_fingerprint {
        // The reference still resolves but the CONTENT diverged since the
        // preview (same guid, different text/title/link/enclosure).
        return Ok(RssAcceptPhase::SourceChanged);
    }

    // The ingested item's findings and durable state (never `recognized`;
    // an enclosure derives `partial`).
    let findings = rss_item_findings(item);
    let state = rss_import_state(&findings);

    // Title: the cleaned candidate when it survives the canonical
    // validation, else the `Histoire de {hôte}` fallback (valid by
    // construction — the address gate proved it).
    let candidate = normalize_title(&item.title);
    let title = if !item.title.is_empty() && validate_title(&candidate).is_ok() {
        candidate
    } else {
        format!("{RSS_FALLBACK_TITLE_PREFIX}{source_host}")
    };

    // A BIRTH: the canonical v3 minimal structure whose start node carries
    // the cleaned item text. `canonical_structure_json` keeps the bytes
    // deterministic, so the checksum covers the ingested text exactly like
    // any other canonical byte.
    let mut structure = CanonicalStructure::minimal();
    structure.nodes[0].text = item.text.clone();
    let structure_json = canonical_structure_json(&structure);
    let checksum = content_checksum(&structure_json);
    let now_iso = now_iso_ms().map_err(|_| clock_unavailable_error())?;

    Ok(RssAcceptPhase::Prepared(Box::new(PreparedRssCreation {
        title,
        structure_json,
        checksum,
        now_iso,
        source_host,
        feed_checksum,
        state,
        findings,
    })))
}

/// Phase 2b — the single atomic transaction (`stories` + provenance).
/// This is the ONLY part of the accept that needs the DB lock. A failed
/// transaction rolls back fully: nothing remains (no media was ever
/// downloaded, so there is nothing to compensate).
pub fn commit_rss_story_creation(
    db: &mut DbHandle,
    prepared: PreparedRssCreation,
) -> Result<StoryCardDto, AppError> {
    let PreparedRssCreation {
        title,
        structure_json,
        checksum,
        now_iso,
        source_host,
        feed_checksum,
        state,
        findings,
    } = prepared;
    let findings_summary = serialize_findings_summary(&findings);
    let story_id = uuid::Uuid::now_v7().to_string();

    let tx = db
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|err| db_commit_error(&err, "begin_transaction"))?;
    tx.execute(
        "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![
            &story_id,
            &title,
            CANONICAL_STORY_SCHEMA_VERSION,
            &structure_json,
            &checksum,
            &now_iso,
        ],
    )
    .map_err(|err| db_commit_error(&err, "insert_story"))?;
    tx.execute(
        "INSERT INTO story_local_imports (story_id, source_format, source_format_version, source_name, artifact_checksum, import_state, findings_summary, imported_at) \
         VALUES (?1, 'rss', ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            &story_id,
            RSS_SOURCE_FORMAT_VERSION,
            &source_host,
            &feed_checksum,
            state_db_tag(state),
            &findings_summary,
            &now_iso,
        ],
    )
    .map_err(|err| db_commit_error(&err, "insert_provenance"))?;
    // Persist → VERIFY → report (the P1 guardrail): re-read both rows
    // INSIDE the transaction before composing the success DTO — a success
    // is never composed from data that was not proven committed-to-be.
    let verified: (String, String) = tx
        .query_row(
            "SELECT s.title, li.import_state FROM stories s \
             JOIN story_local_imports li ON li.story_id = s.id \
             WHERE s.id = ?1 AND li.source_format = 'rss'",
            rusqlite::params![&story_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|err| db_commit_error(&err, "verify_rows"))?;
    if verified.0 != title || verified.1 != state_db_tag(state) {
        return Err(db_commit_error(
            &rusqlite::Error::QueryReturnedNoRows,
            "verify_rows",
        ));
    }
    tx.commit().map_err(|err| db_commit_error(&err, "commit"))?;

    let import_report = rss_import_report_dto(&findings);
    Ok(StoryCardDto {
        id: story_id,
        title,
        import_state: Some(state_dto(state)),
        import_report: if import_report.is_empty() {
            None
        } else {
            Some(import_report)
        },
        transferable: false,
    })
}

/// Convenience: prepare + commit under the SAME borrowed handle (tests and
/// single-threaded callers). The IPC command does NOT use this — it runs
/// [`prepare_rss_story_creation`] before taking the DB lock and only locks
/// for [`commit_rss_story_creation`].
pub fn accept_rss_story_creation(
    db: &mut DbHandle,
    sources: &[ContentSourceLine],
    source: &dyn RssFeedSource,
    url: &str,
    item_ref: &RssItemRef,
    expected_fingerprint: &str,
    budget: Duration,
) -> Result<RssCreationOutcome, AppError> {
    match prepare_rss_story_creation(sources, source, url, item_ref, expected_fingerprint, budget)?
    {
        RssAcceptPhase::SourceChanged => Ok(RssCreationOutcome::SourceChanged),
        RssAcceptPhase::Prepared(prepared) => commit_rss_story_creation(db, *prepared)
            .map(|story| RssCreationOutcome::Created { story }),
    }
}

// ===== Closed user-facing copy — sober, PII-free (no URL, no host). =====

/// The provided address is not a supported feed address (`http`/`https`
/// only, no userinfo, a sober host…). Frozen copy (`product-language.md`).
pub fn invalid_feed_url_error() -> AppError {
    AppError::rss_source_unreachable(
        "Récupération du flux impossible: l'adresse du flux n'est pas valide.",
        "Saisis une adresse http(s) complète puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "network",
        "stage": "url_invalid",
    }))
}

/// The blocking worker task could not be joined (command layer).
pub fn spawn_blocking_join_error() -> AppError {
    AppError::import_failed(
        "Création interrompue de façon inattendue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({ "source": "spawn_blocking_join" }))
}

/// The system clock could not produce the birth timestamp. Same closed
/// `IMPORT_FAILED` taxonomy as the sibling creation flows — the network
/// code stays STRICTLY transport.
fn clock_unavailable_error() -> AppError {
    AppError::import_failed(
        "Création impossible: l'horloge système est indisponible.",
        "Vérifie la date et l'heure de ton ordinateur puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "other",
        "cause": "system_clock_invalid",
    }))
}

fn db_commit_error(err: &rusqlite::Error, stage: &'static str) -> AppError {
    let kind = match err {
        rusqlite::Error::SqliteFailure(code, _) => match code.code {
            rusqlite::ErrorCode::ConstraintViolation => "constraint_violation",
            rusqlite::ErrorCode::DatabaseBusy => "busy",
            rusqlite::ErrorCode::DatabaseLocked => "locked",
            _ => "other",
        },
        _ => "other",
    };
    AppError::import_failed(
        "Création impossible: enregistrement local refusé.",
        "Réessaie ; si le problème persiste, consulte les traces locales.",
    )
    .with_details(serde_json::json!({
        "source": "db_commit",
        "stage": stage,
        "kind": kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::import::{official_content_sources, rss_item_ref, ImportState};
    use crate::domain::shared::AppErrorCode;
    use crate::infrastructure::db;
    use crate::infrastructure::device::MockRssFeedSource;
    use crate::ipc::dto::import_export::ImportStateDto;

    const BUDGET: Duration = Duration::from_secs(30);
    const FEED_URL: &str = "https://exemple.fr/flux.xml";

    fn fresh_db() -> DbHandle {
        let mut handle = db::open_in_memory().expect("open in-memory db");
        db::run_migrations(&mut handle).expect("migrate");
        handle
    }

    fn feed_xml(items: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\"><channel><title>Mon flux</title>{items}</channel></rss>"
        )
    }

    fn nominal_feed() -> String {
        feed_xml(
            "<item><title>Episode 1</title><description>Premier texte.</description><guid>g-1</guid></item>\
             <item><title>Episode 2</title><description>Deuxième texte.</description><guid>g-2</guid></item>",
        )
    }

    /// The previewed-content proof of one item of `feed`, exactly as the
    /// preview DTO would carry it.
    fn fingerprint_in(feed: &str, guid: &str) -> String {
        let analysis = parse_rss(feed.as_bytes());
        let item = analysis
            .items
            .iter()
            .find(|item| item.guid.as_deref() == Some(guid))
            .expect("previewed item");
        rss_item_fingerprint(item)
    }

    /// A custom distribution whose `rss` line is NOT enabled — the
    /// injected matrix that proves the refusal paths.
    fn rss_disabled_matrix() -> [ContentSourceLine; 1] {
        [ContentSourceLine {
            kind: ContentSourceKind::Rss,
            activation: ContentSourceActivation::NotActivated,
        }]
    }

    fn assert_policy_refusal(err: &AppError) {
        assert_eq!(err.code, AppErrorCode::ContentSourceUnavailable);
        let v = serde_json::to_value(err).expect("ser");
        assert_eq!(v["details"]["source"], "content_source_policy");
        assert_eq!(v["details"]["kind"], "rss");
    }

    // ===== the content-source policy gate (before ANY I/O) =====

    #[test]
    fn preview_refuses_a_not_enabled_source_before_any_dispatch() {
        let source = MockRssFeedSource::new();
        let err = preview_rss_source(&rss_disabled_matrix(), &source, FEED_URL, BUDGET)
            .expect_err("policy must refuse");
        assert_policy_refusal(&err);
        assert_eq!(source.fetch_count(), 0, "zero network dispatch");
    }

    #[test]
    fn preview_policy_gate_runs_before_the_address_validation() {
        // An INVALID address with a disabled source: the refusal is the
        // POLICY one, never `url_invalid` — the gate sits upstream of the
        // whole flow, address validation included.
        let source = MockRssFeedSource::new();
        let err = preview_rss_source(
            &rss_disabled_matrix(),
            &source,
            "ftp://exemple.fr/flux.xml",
            BUDGET,
        )
        .expect_err("policy must refuse first");
        assert_policy_refusal(&err);
        assert_eq!(source.fetch_count(), 0);
    }

    #[test]
    fn preview_fails_closed_on_an_empty_matrix() {
        let source = MockRssFeedSource::new();
        let err = preview_rss_source(&[], &source, FEED_URL, BUDGET)
            .expect_err("an absent line refuses like a not-activated one");
        assert_policy_refusal(&err);
        assert_eq!(source.fetch_count(), 0);
    }

    #[test]
    fn accept_refuses_a_not_enabled_source_with_zero_dispatch_and_zero_mutation() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        let err = accept_rss_story_creation(
            &mut db,
            &rss_disabled_matrix(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-1".into()),
            &"0".repeat(64),
            BUDGET,
        )
        .expect_err("policy must refuse");
        assert_policy_refusal(&err);
        assert_eq!(source.fetch_count(), 0, "zero network dispatch");
        assert_eq!(count_stories(&db), 0, "nothing is created");
    }

    #[test]
    fn accept_refuses_a_blocked_by_policy_source_identically() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        let blocked = [ContentSourceLine {
            kind: ContentSourceKind::Rss,
            activation: ContentSourceActivation::BlockedByPolicy,
        }];
        let err = accept_rss_story_creation(
            &mut db,
            &blocked,
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-1".into()),
            &"0".repeat(64),
            BUDGET,
        )
        .expect_err("policy must refuse");
        assert_policy_refusal(&err);
        assert_eq!(source.fetch_count(), 0);
        assert_eq!(count_stories(&db), 0);
    }

    // ===== preview =====

    #[test]
    fn preview_returns_host_checksum_and_analysis_with_zero_db_access() {
        let source = MockRssFeedSource::new();
        source.enqueue_body(nominal_feed());
        let outcome = preview_rss_source(official_content_sources(), &source, FEED_URL, BUDGET)
            .expect("preview");
        assert_eq!(outcome.source_host, "exemple.fr");
        assert_eq!(outcome.feed_checksum.len(), 64);
        assert_eq!(outcome.analysis.items.len(), 2);
        assert_eq!(outcome.analysis.state, ImportState::NeedsReview);
        // The recorder proves exactly ONE dispatch with the full URL and
        // the caller's budget.
        assert_eq!(source.requests(), vec![(FEED_URL.to_string(), BUDGET)]);
    }

    #[test]
    fn preview_refuses_an_invalid_address_without_any_dispatch() {
        let source = MockRssFeedSource::new();
        let err = preview_rss_source(
            official_content_sources(),
            &source,
            "ftp://exemple.fr/flux.xml",
            BUDGET,
        )
        .expect_err("must refuse");
        assert_eq!(err.code, AppErrorCode::RssSourceUnreachable);
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["stage"], "url_invalid");
        assert_eq!(source.fetch_count(), 0, "no network dispatch");
    }

    #[test]
    fn preview_propagates_a_transport_failure_verbatim() {
        let source = MockRssFeedSource::new();
        source.enqueue_failure(crate::infrastructure::device::rss_source::fetch_error(
            "request",
        ));
        let err = preview_rss_source(official_content_sources(), &source, FEED_URL, BUDGET)
            .expect_err("transport");
        assert_eq!(err.code, AppErrorCode::RssSourceUnreachable);
    }

    #[test]
    fn preview_maps_a_blocked_feed_to_the_typed_verdict_never_an_error() {
        let source = MockRssFeedSource::new();
        source.enqueue_body("pas du xml");
        let outcome = preview_rss_source(official_content_sources(), &source, FEED_URL, BUDGET)
            .expect("verdict, not error");
        assert!(outcome.analysis.is_blocked());
    }

    // ===== accept =====

    #[test]
    fn accept_refetches_from_zero_and_commits_story_plus_provenance() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        source.enqueue_body(nominal_feed());
        let fingerprint = fingerprint_in(&nominal_feed(), "g-2");
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-2".into()),
            &fingerprint,
            BUDGET,
        )
        .expect("accept");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        assert_eq!(story.title, "Episode 2");
        assert_eq!(story.import_state, Some(ImportStateDto::NeedsReview));
        assert!(story.import_report.is_some());
        // ONE dispatch — the accept's own re-fetch (no preview ran here).
        assert_eq!(source.fetch_count(), 1);

        // The committed rows: canonical story + rss provenance.
        let (title, text): (String, String) = db
            .conn()
            .query_row(
                "SELECT title, structure_json FROM stories WHERE id = ?1",
                rusqlite::params![&story.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("story row");
        assert_eq!(title, "Episode 2");
        assert!(text.contains("Deuxième texte."));
        let (format, name, state, summary): (String, String, String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT source_format, source_name, import_state, findings_summary \
                 FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![&story.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("provenance row");
        assert_eq!(format, "rss");
        assert_eq!(name, "exemple.fr");
        assert_eq!(state, "needs_review");
        assert!(summary.is_some(), "an rss summary is never NULL");
    }

    #[test]
    fn accept_persists_the_checksum_of_the_second_fetch() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        // Two DIFFERENT bodies that both carry the same resolvable item:
        // the persisted fingerprint must be the SECOND fetch's bytes.
        let first = nominal_feed();
        let second = feed_xml(
            "<item><title>Episode 2</title><description>Deuxième texte.</description><guid>g-2</guid></item>",
        );
        source.enqueue_body(first.clone());
        source.enqueue_body(second.clone());
        let preview = preview_rss_source(official_content_sources(), &source, FEED_URL, BUDGET)
            .expect("preview");
        let previewed = preview
            .analysis
            .items
            .iter()
            .find(|item| item.guid.as_deref() == Some("g-2"))
            .expect("previewed item");
        // The item content is IDENTICAL across the two bodies, so the
        // previewed proof still matches the second fetch (only unrelated
        // parts of the feed changed).
        let fingerprint = rss_item_fingerprint(previewed);
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-2".into()),
            &fingerprint,
            BUDGET,
        )
        .expect("accept");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        let stored: String = db
            .conn()
            .query_row(
                "SELECT artifact_checksum FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![&story.id],
                |row| row.get(0),
            )
            .expect("checksum");
        assert_eq!(stored, content_checksum_bytes(second.as_bytes()));
        assert_ne!(stored, content_checksum_bytes(first.as_bytes()));
        assert_eq!(source.fetch_count(), 2, "preview + accept re-fetch");
    }

    #[test]
    fn accept_refuses_honestly_when_the_item_disappeared() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        source.enqueue_body(nominal_feed());
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("disparu".into()),
            &"0".repeat(64),
            BUDGET,
        )
        .expect("a refusal, not an error");
        assert!(matches!(outcome, RssCreationOutcome::SourceChanged));
        assert_eq!(count_stories(&db), 0, "zero mutation on a refusal");
    }

    #[test]
    fn a_guid_less_item_stays_creatable_next_to_a_guid_twin_sharing_its_title() {
        // A guid-carrying item and a guid-less one share the same (title,
        // link): the TitleLink resolution only considers guid-less items,
        // so the second one CREATES instead of dead-ending on a lying
        // « La source a changé » refusal.
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        let body = feed_xml(
            "<item><title>T</title><description>Premier.</description><guid>g</guid></item>\
             <item><title>T</title><description>Deuxième texte.</description></item>",
        );
        source.enqueue_body(body.clone());
        let analysis = parse_rss(body.as_bytes());
        let guid_less = analysis
            .items
            .iter()
            .find(|item| item.guid.is_none())
            .expect("guid-less item");
        let reference = rss_item_ref(guid_less);
        let fingerprint = rss_item_fingerprint(guid_less);
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &reference,
            &fingerprint,
            BUDGET,
        )
        .expect("accept");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation, not a refusal");
        };
        let text: String = db
            .conn()
            .query_row(
                "SELECT structure_json FROM stories WHERE id = ?1",
                rusqlite::params![&story.id],
                |row| row.get(0),
            )
            .expect("story row");
        assert!(text.contains("Deuxième texte."));
    }

    #[test]
    fn accept_refuses_a_resolvable_item_whose_content_diverged() {
        // The reference still resolves (same guid) but the CONTENT changed
        // between the preview and the accept: the previewed proof no longer
        // matches — the honest refusal, never a creation from content the
        // user never reread.
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        let previewed_body = nominal_feed();
        let diverged_body = feed_xml(
            "<item><title>Episode 1</title><description>Texte RÉÉCRIT depuis la preview.</description><guid>g-1</guid></item>",
        );
        source.enqueue_body(diverged_body);
        let previewed_fingerprint = fingerprint_in(&previewed_body, "g-1");
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-1".into()),
            &previewed_fingerprint,
            BUDGET,
        )
        .expect("a refusal, not an error");
        assert!(matches!(outcome, RssCreationOutcome::SourceChanged));
        assert_eq!(count_stories(&db), 0, "zero mutation on the refusal");
    }

    #[test]
    fn accept_refuses_honestly_when_the_feed_turned_blocked() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        source.enqueue_body("<feed>atom désormais</feed>");
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-1".into()),
            &"0".repeat(64),
            BUDGET,
        )
        .expect("a refusal, not an error");
        assert!(matches!(outcome, RssCreationOutcome::SourceChanged));
        assert_eq!(count_stories(&db), 0);
    }

    #[test]
    fn accept_propagates_a_transport_failure_and_mutates_nothing() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        source.enqueue_failure(crate::infrastructure::device::rss_source::fetch_error(
            "request",
        ));
        let err = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-1".into()),
            &"0".repeat(64),
            BUDGET,
        )
        .expect_err("transport");
        assert_eq!(err.code, AppErrorCode::RssSourceUnreachable);
        assert_eq!(count_stories(&db), 0);
    }

    #[test]
    fn an_enclosure_item_persists_partial_with_the_missing_media_finding() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        let body = feed_xml(
            "<item><title>Podcast</title><description>Audio distant.</description><guid>g-a</guid>\
             <enclosure url=\"https://exemple.fr/ep.mp3\" length=\"1\" type=\"audio/mpeg\"/></item>",
        );
        source.enqueue_body(body.clone());
        let fingerprint = fingerprint_in(&body, "g-a");
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-a".into()),
            &fingerprint,
            BUDGET,
        )
        .expect("accept");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        assert_eq!(story.import_state, Some(ImportStateDto::Partial));
        let state: String = db
            .conn()
            .query_row(
                "SELECT import_state FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![&story.id],
                |row| row.get(0),
            )
            .expect("state");
        assert_eq!(state, "partial");
        let report = story.import_report.expect("report");
        assert!(report
            .iter()
            .any(|f| f.message
                == "Le média distant référencé par la source n'a pas été récupéré. Ajoute le média manuellement dans l'éditeur."));
    }

    #[test]
    fn a_titleless_item_falls_back_to_histoire_de_hote() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        let body =
            feed_xml("<item><description>Texte sans titre.</description><guid>g-n</guid></item>");
        source.enqueue_body(body.clone());
        let fingerprint = fingerprint_in(&body, "g-n");
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-n".into()),
            &fingerprint,
            BUDGET,
        )
        .expect("accept");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        assert_eq!(story.title, "Histoire de exemple.fr");
        // The fallback is a review step: the Title ambiguity is persisted.
        let summary: String = db
            .conn()
            .query_row(
                "SELECT findings_summary FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![&story.id],
                |row| row.get(0),
            )
            .expect("summary");
        assert!(summary.contains("\"aspect\":\"title\""));
    }

    #[test]
    fn the_created_structure_is_canonical_v3_with_the_text_prefilled() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        source.enqueue_body(nominal_feed());
        let fingerprint = fingerprint_in(&nominal_feed(), "g-1");
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-1".into()),
            &fingerprint,
            BUDGET,
        )
        .expect("accept");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        let (schema_version, structure_json, checksum): (u32, String, String) = db
            .conn()
            .query_row(
                "SELECT schema_version, structure_json, content_checksum FROM stories WHERE id = ?1",
                rusqlite::params![&story.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("story row");
        assert_eq!(schema_version, CANONICAL_STORY_SCHEMA_VERSION);
        let mut expected = CanonicalStructure::minimal();
        expected.nodes[0].text = "Premier texte.".into();
        assert_eq!(structure_json, canonical_structure_json(&expected));
        assert_eq!(checksum, content_checksum(&structure_json));
    }

    #[test]
    fn accept_refuses_an_invalid_address_without_any_dispatch() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        let err = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            "file:///etc/passwd",
            &RssItemRef::Guid("g".into()),
            &"0".repeat(64),
            BUDGET,
        )
        .expect_err("must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["stage"], "url_invalid");
        assert_eq!(source.fetch_count(), 0);
    }

    fn count_stories(db: &DbHandle) -> i64 {
        db.conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count")
    }
}
