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
//!    re-submits content). EVERY selected item is resolved by STRICT
//!    `guid` (else exact `title`+`link`) and re-proven against its
//!    previewed fingerprint; a missing/ambiguous item, a diverged one or a
//!    feed turned blocked is the honest recoverable refusal
//!    [`RssCreationOutcome::SourceChanged`] with ZERO mutation — NEVER a
//!    creation from the stale preview data. Otherwise the WHOLE selection
//!    becomes ONE story: one node per item in the reviewed (listening)
//!    order, each carrying the item's cleaned text, its title as label,
//!    its downloaded enclosure and its artwork (the item's own, else the
//!    channel's), then ONE `BEGIN IMMEDIATE` transaction inserts the
//!    canonical `stories` row (fresh UUIDv7, `created_at = updated_at =
//!    now` — a BIRTH, exactly like the structured-folder creation), the
//!    provenance row (`source_format = 'rss'`, host-only source name,
//!    checksum of the SECOND fetch's bytes — the bytes actually ingested)
//!    and every promoted media's `assets` row. A failed audio download is
//!    a CONTENT verdict (`(Media, Missing)`, état `partial`), never a
//!    refusal; a failed artwork download degrades silently (optional). A
//!    failed transaction rolls back fully and compensates the promoted
//!    files best-effort (refcounted): nothing durable remains.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::application::story::node::gc_unreferenced_media_file;
use crate::application::story::now_iso_ms;
use crate::domain::import::{
    feed_url_host, parse_rss, resolve_rss_item, rss_feed_findings, rss_import_state,
    rss_item_fingerprint, ContentSourceKind, ContentSourceLine, RssAnalysis, RssItem, RssItemRef,
    MAX_RSS_ITEMS, RSS_FALLBACK_TITLE_PREFIX, RSS_SOURCE_FORMAT_VERSION,
};
use crate::domain::shared::AppError;
use crate::domain::story::{
    canonical_structure_json, content_checksum, content_checksum_bytes, normalize_title,
    validate_title, CanonicalNode, CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION,
    START_NODE_ID,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::device::RssFeedSource;
use crate::infrastructure::filesystem::{
    ensure_node_media_store, store_media_capped, MediaKind, StoredMedia, WEB_MAX_MEDIA_BYTES,
};
use crate::ipc::dto::import_export::rss_import_report_dto;
use crate::ipc::dto::StoryCardDto;

use super::creation_common::{
    commit_story_creation, ensure_source_enabled, PromotedAsset, StoryCreationCommit,
};

/// The application-level outcome of previewing a feed: the HOST (the only
/// address fragment that ever crosses further), the SHA-256 fingerprint of
/// the fetched bytes and the typed domain analysis.
#[derive(Debug, Clone)]
pub struct RssPreviewOutcome {
    pub source_host: String,
    pub feed_checksum: String,
    pub analysis: RssAnalysis,
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
    ensure_source_enabled(sources, ContentSourceKind::Rss)?;
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

/// ONE accepted item of the previewed feed: its round-tripped reference
/// (a pointer, re-resolved from zero) and the previewed-content proof the
/// fresh item must match exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RssItemSelection {
    pub reference: RssItemRef,
    pub fingerprint: String,
}

/// The fully re-proven, ready-to-commit ingestion — everything the atomic
/// DB transaction needs, produced WITHOUT any DB access
/// ([`prepare_rss_story_creation`]) so the network fetches never serialize
/// other commands behind the DB lock.
#[derive(Debug)]
pub struct PreparedRssCreation {
    commit: StoryCreationCommit,
    /// The downloaded-and-promoted media (audio and artwork, one `assets`
    /// row each) — empty when the caller gave no store root or every
    /// download degraded to its verdict.
    assets: Vec<PromotedAsset>,
}

/// The typed outcome of the DB-free accept phase: the honest refusal, or
/// the prepared creation to hand to [`commit_rss_story_creation`].
#[derive(Debug)]
pub enum RssAcceptPhase {
    SourceChanged,
    Prepared(Box<PreparedRssCreation>),
}

/// Attempts of one enclosure download: a transient network hiccup on a
/// long series must not silently orphan an episode's audio.
const ENCLOSURE_ATTEMPTS: usize = 2;

/// The node-media store of one accept: the promoted directory pair plus a
/// per-address memo of the artworks already fetched in THIS accept — a
/// series shares its channel artwork across every episode, so it is
/// downloaded (and transcoded) once, whatever the episode count; a
/// failed artwork stays failed for the accept (no retry storm).
struct AcceptStore {
    media_dir: PathBuf,
    staging_dir: PathBuf,
    artwork_memo: HashMap<String, Option<StoredMedia>>,
}

impl AcceptStore {
    fn open(app_data_dir: &Path) -> Option<Self> {
        let (media_dir, staging_dir) = ensure_node_media_store(app_data_dir).ok()?;
        Some(Self {
            media_dir,
            staging_dir,
            artwork_memo: HashMap::new(),
        })
    }

    /// Download ONE enclosure (with [`ENCLOSURE_ATTEMPTS`]) and PROMOTE it.
    /// `None` on ANY failure — transport, over-cap, unsupported bytes,
    /// store I/O: the media stays honestly « non récupéré » (a CONTENT
    /// verdict, never an `AppError` — the module's contract).
    fn promote_enclosure(
        &self,
        source: &dyn RssFeedSource,
        url: &str,
        budget: Duration,
    ) -> Option<StoredMedia> {
        let mut bytes = None;
        for _ in 0..ENCLOSURE_ATTEMPTS {
            if let Ok(fetched) = source.fetch_enclosure(url, budget) {
                bytes = Some(fetched);
                break;
            }
        }
        store_media_capped(
            &self.media_dir,
            &self.staging_dir,
            &bytes?,
            WEB_MAX_MEDIA_BYTES,
        )
        .ok()
    }

    /// The artwork at `url`, fetched at most ONCE per accept. A stored
    /// artwork that is not an image (a mislabeled audio…) is refused.
    fn artwork(
        &mut self,
        source: &dyn RssFeedSource,
        url: &str,
        budget: Duration,
    ) -> Option<StoredMedia> {
        if let Some(memo) = self.artwork_memo.get(url) {
            return memo.clone();
        }
        let stored = source
            .fetch_enclosure(url, budget)
            .ok()
            .and_then(|bytes| {
                store_media_capped(
                    &self.media_dir,
                    &self.staging_dir,
                    &bytes,
                    WEB_MAX_MEDIA_BYTES,
                )
                .ok()
            })
            .filter(|stored| stored.kind == MediaKind::Image);
        self.artwork_memo.insert(url.to_string(), stored.clone());
        stored
    }

    /// Everything ONE promoted media needs for its `assets` row, plus the
    /// promoted file path so a failed commit can compensate the store.
    fn asset_of(&self, stored: StoredMedia) -> PromotedAsset {
        PromotedAsset {
            asset_id: uuid::Uuid::now_v7().to_string(),
            content_hash: stored.content_hash,
            media_type: stored.kind.as_str(),
            media_format: stored.format,
            byte_size: stored.byte_size,
            file_name: stored.file_name.clone(),
            promoted_path: self.media_dir.join(stored.file_name),
        }
    }
}

/// Phase 2a — RE-fetch, re-parse and re-prove EVERY selected item, with NO
/// DB access at all: the command runs this BEFORE taking the DB lock, so
/// the network phase (the feed, then one download per episode) never
/// holds it. Each selection's `fingerprint` is the canonical proof of the
/// PREVIEWED item: the fresh item must match it EXACTLY — a resolvable
/// reference (same guid) whose content diverged is the honest
/// `SourceChanged` refusal, never a creation from content the user never
/// reread. The accept re-proves EVERYTHING, the policy included: the gate
/// runs FIRST, so a direct command call can never bypass the
/// distribution's content-source matrix. `on_progress` receives the
/// integer percent (0..99) of episodes settled — signal only.
#[allow(clippy::too_many_arguments)]
pub fn prepare_rss_story_creation(
    sources: &[ContentSourceLine],
    source: &dyn RssFeedSource,
    url: &str,
    selection: &[RssItemSelection],
    budget: Duration,
    media_budget: Duration,
    app_data_dir: Option<&Path>,
    on_progress: &dyn Fn(u8),
) -> Result<RssAcceptPhase, AppError> {
    ensure_source_enabled(sources, ContentSourceKind::Rss)?;
    let source_host = feed_url_host(url).ok_or_else(invalid_feed_url_error)?;
    validate_selection(selection)?;
    // RE-fetch + re-parse from zero: the references are pointers, never an
    // authority; the checksum persisted below fingerprints THESE bytes.
    let bytes = source.fetch(url, budget)?;
    let feed_checksum = content_checksum_bytes(&bytes);
    let analysis = parse_rss(&bytes);
    if analysis.is_blocked() {
        // The feed turned blocked between the preview and the accept.
        return Ok(RssAcceptPhase::SourceChanged);
    }
    let mut items: Vec<&RssItem> = Vec::with_capacity(selection.len());
    for selected in selection {
        let Some(item) = resolve_rss_item(&analysis.items, &selected.reference) else {
            // Missing or ambiguous — an approximate match is never taken.
            return Ok(RssAcceptPhase::SourceChanged);
        };
        if rss_item_fingerprint(item) != selected.fingerprint {
            // The reference still resolves but the CONTENT diverged since
            // the preview (same guid, different text/title/enclosure…).
            return Ok(RssAcceptPhase::SourceChanged);
        }
        items.push(item);
    }

    // The store root is consulted ONLY after the re-proof, so a refusal
    // never creates a directory or a file.
    let mut store = app_data_dir.and_then(AcceptStore::open);

    // A BIRTH: one node per selected item, in the reviewed (listening)
    // order — the flat ordered graph the v3 canonical model carries. Every
    // audio is downloaded and promoted NOW (the network phase); a failed
    // download leaves its node audio-less and flips the Media finding.
    let mut structure = CanonicalStructure {
        schema_version: CANONICAL_STORY_SCHEMA_VERSION,
        start_node_id: START_NODE_ID.to_owned(),
        nodes: Vec::with_capacity(items.len()),
    };
    let mut assets: Vec<PromotedAsset> = Vec::new();
    let mut audio_missing = false;
    let total = items.len();
    for (index, item) in items.iter().enumerate() {
        let mut audio_asset_id: Option<String> = None;
        let mut image_asset_id: Option<String> = None;
        if let (true, Some(enclosure_url)) = (item.has_enclosure, &item.enclosure_url) {
            match store
                .as_ref()
                .and_then(|store| store.promote_enclosure(source, enclosure_url, media_budget))
            {
                Some(stored) => {
                    let asset = store
                        .as_ref()
                        .map(|store| store.asset_of(stored))
                        .expect("a promoted media implies an open store");
                    // An enclosure is USUALLY audio; a feed shipping an
                    // image enclosure gets it as the node artwork.
                    match asset.media_type {
                        "image" => image_asset_id = Some(asset.asset_id.clone()),
                        _ => audio_asset_id = Some(asset.asset_id.clone()),
                    }
                    assets.push(asset);
                }
                None => audio_missing = true,
            }
        }
        // The artwork is OPTIONAL: the item's own, else the channel's; a
        // failed download simply leaves the node image-less — no finding,
        // no state change.
        if image_asset_id.is_none() {
            let artwork_url = item
                .image_url
                .as_deref()
                .or(analysis.channel_image_url.as_deref());
            if let (Some(store), Some(artwork_url)) = (store.as_mut(), artwork_url) {
                if let Some(stored) = store.artwork(source, artwork_url, media_budget) {
                    let asset = store.asset_of(stored);
                    image_asset_id = Some(asset.asset_id.clone());
                    assets.push(asset);
                }
            }
        }
        structure.nodes.push(CanonicalNode {
            id: format!("n{}", index + 1),
            text: item.text.clone(),
            label: item.title.clone(),
            image_asset_id,
            audio_asset_id,
            options: Vec::new(),
        });
        on_progress(((index + 1) * 99 / total) as u8);
    }

    // Title: the channel title when it survives the canonical validation
    // as-is (the podcast IS the story), else the `Histoire de {hôte}`
    // fallback (valid by construction — the address gate proved it).
    let (title, title_recognized) = match analysis.channel_title.as_deref() {
        Some(channel) => {
            let candidate = normalize_title(channel);
            if validate_title(&candidate).is_ok() {
                (candidate, true)
            } else {
                (format!("{RSS_FALLBACK_TITLE_PREFIX}{source_host}"), false)
            }
        }
        None => (format!("{RSS_FALLBACK_TITLE_PREFIX}{source_host}"), false),
    };

    // The ingestion's findings and durable state.
    let findings = rss_feed_findings(&items, title_recognized, audio_missing);
    let state = rss_import_state(&findings);

    let structure_json = canonical_structure_json(&structure);
    let checksum = content_checksum(&structure_json);
    let now_iso = now_iso_ms().map_err(|_| clock_unavailable_error())?;

    Ok(RssAcceptPhase::Prepared(Box::new(PreparedRssCreation {
        commit: StoryCreationCommit {
            title,
            structure_json,
            checksum,
            now_iso,
            source_name: source_host,
            artifact_checksum: feed_checksum,
            state,
            findings,
        },
        assets,
    })))
}

/// The selection must be non-empty, bounded and free of duplicates (one
/// node per item — a repeated reference would be a repeated episode).
fn validate_selection(selection: &[RssItemSelection]) -> Result<(), AppError> {
    if selection.is_empty() || selection.len() > MAX_RSS_ITEMS {
        return Err(invalid_selection_error("count"));
    }
    for (index, selected) in selection.iter().enumerate() {
        if selection[..index]
            .iter()
            .any(|other| other.reference == selected.reference)
        {
            return Err(invalid_selection_error("repeated"));
        }
    }
    Ok(())
}

/// Phase 2b — the single atomic transaction (`stories` + provenance + the
/// promoted media's `assets` rows). This is the ONLY part of the accept
/// that needs the DB lock. A failed transaction rolls back fully; the
/// promoted media files — the only pre-transaction mutation — are then
/// compensated best-effort, REFCOUNTED: a file another story already
/// references (content-addressed sharing) is never removed.
pub fn commit_rss_story_creation(
    db: &mut DbHandle,
    prepared: PreparedRssCreation,
) -> Result<StoryCardDto, AppError> {
    let PreparedRssCreation { commit, assets } = prepared;
    let result = commit_story_creation(
        db,
        &commit,
        "rss",
        RSS_SOURCE_FORMAT_VERSION,
        &assets,
        rss_import_report_dto,
    );
    if result.is_err() {
        for asset in &assets {
            if let Some(media_dir) = asset.promoted_path.parent() {
                gc_unreferenced_media_file(
                    db,
                    media_dir,
                    Some((asset.content_hash.clone(), asset.file_name.clone())),
                );
            }
        }
    }
    result
}

/// Convenience: prepare + commit under the SAME borrowed handle (tests and
/// single-threaded callers). The IPC command does NOT use this — it runs
/// [`prepare_rss_story_creation`] before taking the DB lock and only locks
/// for [`commit_rss_story_creation`].
#[allow(clippy::too_many_arguments)]
pub fn accept_rss_story_creation(
    db: &mut DbHandle,
    sources: &[ContentSourceLine],
    source: &dyn RssFeedSource,
    url: &str,
    selection: &[RssItemSelection],
    budget: Duration,
    media_budget: Duration,
    app_data_dir: Option<&Path>,
) -> Result<RssCreationOutcome, AppError> {
    match prepare_rss_story_creation(
        sources,
        source,
        url,
        selection,
        budget,
        media_budget,
        app_data_dir,
        &|_| {},
    )? {
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

/// The selection round-tripped by the frontend is malformed (empty, over
/// the item bound, or repeating a reference) — a boundary drift, never a
/// content verdict. Frozen copy (`product-language.md`).
pub fn invalid_selection_error(cause: &'static str) -> AppError {
    AppError::import_failed(
        "Création impossible: la sélection d'épisodes n'est pas valide.",
        "Relance la récupération du flux, puis recommence la sélection.",
    )
    .with_details(serde_json::json!({
        "source": "validation",
        "cause": cause,
    }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::import::{
        official_content_sources, rss_item_ref, ContentSourceActivation, ImportState,
    };
    use crate::domain::shared::AppErrorCode;
    use crate::domain::story::CANONICAL_STORY_SCHEMA_VERSION;
    use crate::infrastructure::db;
    use crate::infrastructure::device::MockRssFeedSource;
    use crate::ipc::dto::import_export::{ImportAspectDto, ImportCategoryDto, ImportStateDto};

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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-1".into()),
                fingerprint: "0".repeat(64),
            }],
            BUDGET,
            BUDGET,
            None,
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-1".into()),
                fingerprint: "0".repeat(64),
            }],
            BUDGET,
            BUDGET,
            None,
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-2".into()),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            None,
        )
        .expect("accept");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        // The podcast IS the story: the channel title names it, the item
        // title labels its node.
        assert_eq!(story.title, "Mon flux");
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
        assert_eq!(title, "Mon flux");
        assert!(text.contains("Deuxième texte."));
        assert!(text.contains("\"label\":\"Episode 2\""));
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-2".into()),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            None,
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("disparu".into()),
                fingerprint: "0".repeat(64),
            }],
            BUDGET,
            BUDGET,
            None,
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
            &[RssItemSelection {
                reference: reference.clone(),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            None,
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-1".into()),
                fingerprint: previewed_fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            None,
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-1".into()),
                fingerprint: "0".repeat(64),
            }],
            BUDGET,
            BUDGET,
            None,
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-1".into()),
                fingerprint: "0".repeat(64),
            }],
            BUDGET,
            BUDGET,
            None,
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-a".into()),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            None,
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
    fn a_feed_without_a_channel_title_falls_back_to_histoire_de_hote() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        let body = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\"><channel>\
                    <item><title>Episode sans série</title><description>Texte.</description><guid>g-n</guid></item>\
                    </channel></rss>"
            .to_string();
        source.enqueue_body(body.clone());
        let fingerprint = fingerprint_in(&body, "g-n");
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-n".into()),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            None,
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-1".into()),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            None,
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
        expected.nodes[0].label = "Episode 1".into();
        assert_eq!(structure_json, canonical_structure_json(&expected));
        assert_eq!(checksum, content_checksum(&structure_json));
    }

    // ===== the whole feed as ONE story =====

    fn dated_feed() -> String {
        // Listed newest-first, like a real podcast feed.
        feed_xml(
            "<item><title>Trois</title><description>Troisième.</description><guid>g-3</guid><pubDate>Wed, 03 Mar 2026 08:00:00 +0100</pubDate></item>\
             <item><title>Deux</title><description>Deuxième.</description><guid>g-2</guid><pubDate>Tue, 02 Mar 2026 08:00:00 +0100</pubDate></item>\
             <item><title>Un</title><description>Premier.</description><guid>g-1</guid><pubDate>Mon, 01 Mar 2026 08:00:00 +0100</pubDate></item>",
        )
    }

    fn selection_of(feed: &str, guids: &[&str]) -> Vec<RssItemSelection> {
        guids
            .iter()
            .map(|guid| RssItemSelection {
                reference: RssItemRef::Guid((*guid).to_string()),
                fingerprint: fingerprint_in(feed, guid),
            })
            .collect()
    }

    fn structure_of(db: &DbHandle, story_id: &str) -> CanonicalStructure {
        let json: String = db
            .conn()
            .query_row(
                "SELECT structure_json FROM stories WHERE id = ?1",
                rusqlite::params![story_id],
                |row| row.get(0),
            )
            .expect("story row");
        serde_json::from_str(&json).expect("canonical structure")
    }

    #[test]
    fn a_whole_selection_becomes_one_story_with_one_node_per_item_in_order() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        source.enqueue_body(dated_feed());
        // The selection order is the reviewed (listening) order: the
        // preview lists the dated feed oldest-first.
        let selection = selection_of(&dated_feed(), &["g-1", "g-2", "g-3"]);
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &selection,
            BUDGET,
            BUDGET,
            None,
        )
        .expect("accept");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        assert_eq!(story.title, "Mon flux");
        let structure = structure_of(&db, &story.id);
        assert_eq!(structure.start_node_id, "n1");
        let nodes: Vec<(&str, &str, &str)> = structure
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str(), n.text.as_str()))
            .collect();
        assert_eq!(
            nodes,
            [
                ("n1", "Un", "Premier."),
                ("n2", "Deux", "Deuxième."),
                ("n3", "Trois", "Troisième."),
            ]
        );
        assert_eq!(count_stories(&db), 1, "ONE story for the whole selection");
    }

    #[test]
    fn the_selection_order_is_the_node_order_even_against_the_feed() {
        // The frontend may hand a subset in its own order: the story
        // follows the SELECTION, never the feed.
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        source.enqueue_body(dated_feed());
        let selection = selection_of(&dated_feed(), &["g-3", "g-1"]);
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &selection,
            BUDGET,
            BUDGET,
            None,
        )
        .expect("accept");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        let labels: Vec<String> = structure_of(&db, &story.id)
            .nodes
            .iter()
            .map(|n| n.label.clone())
            .collect();
        assert_eq!(labels, ["Trois", "Un"]);
    }

    #[test]
    fn one_diverged_item_refuses_the_whole_selection_with_zero_mutation() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        source.enqueue_body(dated_feed());
        let mut selection = selection_of(&dated_feed(), &["g-1", "g-2", "g-3"]);
        selection[1].fingerprint = "0".repeat(64);
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &selection,
            BUDGET,
            BUDGET,
            None,
        )
        .expect("a refusal, not an error");
        assert!(matches!(outcome, RssCreationOutcome::SourceChanged));
        assert_eq!(count_stories(&db), 0);
        assert!(source.enclosure_requests().is_empty(), "nothing downloaded");
    }

    #[test]
    fn an_empty_or_repeated_selection_is_a_validation_refusal_before_any_dispatch() {
        let mut db = fresh_db();
        let source = MockRssFeedSource::new();
        let err = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &[],
            BUDGET,
            BUDGET,
            None,
        )
        .expect_err("empty selection");
        assert_eq!(err.code, AppErrorCode::ImportFailed);
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "validation");
        assert_eq!(v["details"]["cause"], "count");

        let repeated = vec![
            RssItemSelection {
                reference: RssItemRef::Guid("g-1".into()),
                fingerprint: "0".repeat(64),
            },
            RssItemSelection {
                reference: RssItemRef::Guid("g-1".into()),
                fingerprint: "1".repeat(64),
            },
        ];
        let err = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &repeated,
            BUDGET,
            BUDGET,
            None,
        )
        .expect_err("repeated reference");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["cause"], "repeated");
        assert_eq!(source.fetch_count(), 0, "validated before any dispatch");
        assert_eq!(count_stories(&db), 0);
    }

    #[test]
    fn progress_ticks_once_per_settled_episode_up_to_99() {
        let source = MockRssFeedSource::new();
        source.enqueue_body(dated_feed());
        let ticks = std::cell::RefCell::new(Vec::new());
        let phase = prepare_rss_story_creation(
            official_content_sources(),
            &source,
            FEED_URL,
            &selection_of(&dated_feed(), &["g-1", "g-2", "g-3"]),
            BUDGET,
            BUDGET,
            None,
            &|pct| ticks.borrow_mut().push(pct),
        )
        .expect("prepare");
        assert!(matches!(phase, RssAcceptPhase::Prepared(_)));
        assert_eq!(*ticks.borrow(), vec![33, 66, 99]);
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
            &[RssItemSelection {
                reference: RssItemRef::Guid("g".into()),
                fingerprint: "0".repeat(64),
            }],
            BUDGET,
            BUDGET,
            None,
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

    // ===== the enclosure download (the media completion of FR31) =====

    fn feed_with_enclosure() -> String {
        feed_xml(
            "<item><title>Episode audio</title><description>Texte.</description><guid>g-enc</guid>\
             <enclosure url=\"https://exemple.fr/ep.wav\" type=\"audio/wav\" length=\"128\"/></item>",
        )
    }

    /// A minimal RIFF/WAVE container: enough for the magic-byte sniff, and
    /// audio is stored VERBATIM (never decoded) — the smallest honest
    /// storeable media.
    fn tiny_wav() -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&[36, 0, 0, 0]);
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&[16, 0, 0, 0]);
        bytes.extend_from_slice(&[1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0]);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes
    }

    fn tiny_webp() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([12, 34, 56, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::WebP,
            )
            .expect("encode webp");
        bytes
    }

    #[test]
    fn an_accepted_enclosure_is_downloaded_promoted_and_wired() {
        let mut db = fresh_db();
        let feed = feed_with_enclosure();
        let fingerprint = fingerprint_in(&feed, "g-enc");
        let source = MockRssFeedSource::new();
        source.enqueue_body(feed.clone());
        source.enqueue_enclosure_body(tiny_wav());
        let store_root = tempfile::tempdir().expect("tempdir");

        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-enc".into()),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            Some(store_root.path()),
        )
        .expect("creation");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };

        // The enclosure was fetched THROUGH the injected source (never an
        // ambient client), with the enclosure URL exactly.
        let enclosure_requests = source.enclosure_requests();
        assert_eq!(enclosure_requests.len(), 1);
        assert_eq!(enclosure_requests[0].0, "https://exemple.fr/ep.wav");

        // One `assets` row, audio, wired into the start node of the
        // committed structure — the library derives playback from it.
        let (asset_id, media_type, file_name): (String, String, String) = db
            .conn()
            .query_row(
                "SELECT id, media_type, file_name FROM assets WHERE story_id = ?1",
                rusqlite::params![&story.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("one assets row");
        assert_eq!(media_type, "audio");
        let structure_json: String = db
            .conn()
            .query_row(
                "SELECT structure_json FROM stories WHERE id = ?1",
                rusqlite::params![&story.id],
                |row| row.get(0),
            )
            .expect("structure");
        let structure: serde_json::Value =
            serde_json::from_str(&structure_json).expect("canonical json");
        assert_eq!(
            structure["nodes"][0]["audioAssetId"].as_str(),
            Some(asset_id.as_str()),
            "the start node must reference the promoted asset"
        );

        // The promoted file exists in the node-media store.
        let promoted = store_root.path().join("node-media").join(&file_name);
        assert!(promoted.is_file(), "promoted media file must exist");

        // The media finding is `Recognized`: no `Missing` remains, so the
        // durable state climbs from `partial` to `needs_review` (the
        // ambiguity floor keeps an RSS ingestion below `recognized`).
        assert_eq!(story.import_state, Some(ImportStateDto::NeedsReview));
        let report = story.import_report.expect("report");
        assert!(
            report.iter().any(|f| f.aspect == ImportAspectDto::Media
                && f.category == ImportCategoryDto::Recognized),
            "the media finding must be recognized, got {report:?}"
        );
    }

    #[test]
    fn a_webp_enclosure_advertised_as_jpeg_is_promoted_as_png() {
        let mut db = fresh_db();
        let feed = feed_xml(
            "<item><title>Episode illustré</title><description>Texte.</description><guid>g-webp</guid>\
             <enclosure url=\"https://exemple.fr/image?webp=false\" type=\"image/jpeg\" length=\"128\"/></item>",
        );
        let fingerprint = fingerprint_in(&feed, "g-webp");
        let source = MockRssFeedSource::new();
        source.enqueue_body(feed);
        source.enqueue_enclosure_body(tiny_webp());
        let store_root = tempfile::tempdir().expect("tempdir");

        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-webp".into()),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            Some(store_root.path()),
        )
        .expect("creation");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };

        let (media_type, media_format, file_name): (String, String, String) = db
            .conn()
            .query_row(
                "SELECT media_type, media_format, file_name FROM assets WHERE story_id = ?1",
                rusqlite::params![&story.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("one assets row");
        assert_eq!(media_type, "image");
        assert_eq!(media_format, "png");
        assert!(file_name.ends_with(".png"));
        assert!(store_root
            .path()
            .join("node-media")
            .join(file_name)
            .is_file());
    }

    #[test]
    fn a_failed_enclosure_download_degrades_to_missing_media_and_still_creates() {
        let mut db = fresh_db();
        let feed = feed_with_enclosure();
        let fingerprint = fingerprint_in(&feed, "g-enc");
        let source = MockRssFeedSource::new();
        source.enqueue_body(feed.clone());
        // No programmed enclosure response: the mock refuses like the trait
        // default — the transport failure must stay a CONTENT verdict.
        let store_root = tempfile::tempdir().expect("tempdir");

        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-enc".into()),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            Some(store_root.path()),
        )
        .expect("the creation must survive a failed download");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };

        let assets: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .expect("count");
        assert_eq!(assets, 0, "no asset row without a downloaded media");
        assert_eq!(story.import_state, Some(ImportStateDto::Partial));
        let report = story.import_report.expect("report");
        assert!(
            report
                .iter()
                .any(|f| f.aspect == ImportAspectDto::Media
                    && f.category == ImportCategoryDto::Missing),
            "the media finding must stay missing, got {report:?}"
        );
    }

    #[test]
    fn a_transient_download_failure_is_retried_once_and_still_wires_the_audio() {
        let mut db = fresh_db();
        let feed = feed_with_enclosure();
        let fingerprint = fingerprint_in(&feed, "g-enc");
        let source = MockRssFeedSource::new();
        source.enqueue_body(feed.clone());
        // First attempt fails, the second delivers the bytes.
        source.enqueue_enclosure_failure(crate::infrastructure::device::rss_source::fetch_error(
            "request",
        ));
        source.enqueue_enclosure_body(tiny_wav());
        let store_root = tempfile::tempdir().expect("tempdir");
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &[RssItemSelection {
                reference: RssItemRef::Guid("g-enc".into()),
                fingerprint: fingerprint.clone(),
            }],
            BUDGET,
            BUDGET,
            Some(store_root.path()),
        )
        .expect("creation");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        assert_eq!(source.enclosure_requests().len(), 2, "one retry");
        assert_eq!(story.import_state, Some(ImportStateDto::NeedsReview));
        let audio_rows: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE media_type = 'audio'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(audio_rows, 1);
    }

    #[test]
    fn the_channel_artwork_is_downloaded_once_and_attached_to_every_episode() {
        let mut db = fresh_db();
        let feed = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\" xmlns:itunes=\"http://www.itunes.com/dtds/podcast-1.0.dtd\"><channel>\
                    <title>Série</title><itunes:image href=\"https://exemple.fr/cover.webp\"/>\
                    <item><title>Un</title><description>A.</description><guid>g-1</guid>\
                    <enclosure url=\"https://exemple.fr/1.wav\" type=\"audio/wav\"/></item>\
                    <item><title>Deux</title><description>B.</description><guid>g-2</guid>\
                    <enclosure url=\"https://exemple.fr/2.wav\" type=\"audio/wav\"/></item>\
                    </channel></rss>"
            .to_string();
        let source = MockRssFeedSource::new();
        source.enqueue_body(feed.clone());
        // Download order: audio 1, artwork (once), audio 2 — the artwork
        // memo answers the second episode without a request.
        source.enqueue_enclosure_body(tiny_wav());
        source.enqueue_enclosure_body(tiny_webp());
        source.enqueue_enclosure_body(tiny_wav());
        let store_root = tempfile::tempdir().expect("tempdir");
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &selection_of(&feed, &["g-1", "g-2"]),
            BUDGET,
            BUDGET,
            Some(store_root.path()),
        )
        .expect("creation");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        let urls: Vec<String> = source
            .enclosure_requests()
            .into_iter()
            .map(|(url, _)| url)
            .collect();
        assert_eq!(
            urls,
            [
                "https://exemple.fr/1.wav",
                "https://exemple.fr/cover.webp",
                "https://exemple.fr/2.wav",
            ]
        );
        // Every node carries its OWN image asset row (the store shares the
        // single promoted file by content hash).
        let structure = structure_of(&db, &story.id);
        let image_ids: Vec<String> = structure
            .nodes
            .iter()
            .map(|n| n.image_asset_id.clone().expect("artwork on every node"))
            .collect();
        assert_ne!(image_ids[0], image_ids[1]);
        assert!(structure.nodes.iter().all(|n| n.audio_asset_id.is_some()));
        let (image_rows, distinct_files): (i64, i64) = db
            .conn()
            .query_row(
                "SELECT COUNT(*), COUNT(DISTINCT file_name) FROM assets WHERE media_type = 'image'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("count");
        assert_eq!((image_rows, distinct_files), (2, 1));
    }

    #[test]
    fn an_item_artwork_wins_over_the_channel_artwork() {
        let feed = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\" xmlns:itunes=\"http://www.itunes.com/dtds/podcast-1.0.dtd\"><channel>\
                    <title>Série</title><itunes:image href=\"https://exemple.fr/cover.webp\"/>\
                    <item><title>Un</title><description>A.</description><guid>g-1</guid>\
                    <itunes:image href=\"https://exemple.fr/ep1.webp\"/></item>\
                    </channel></rss>"
            .to_string();
        let source = MockRssFeedSource::new();
        source.enqueue_body(feed.clone());
        source.enqueue_enclosure_body(tiny_webp());
        let store_root = tempfile::tempdir().expect("tempdir");
        let phase = prepare_rss_story_creation(
            official_content_sources(),
            &source,
            FEED_URL,
            &selection_of(&feed, &["g-1"]),
            BUDGET,
            BUDGET,
            Some(store_root.path()),
            &|_| {},
        )
        .expect("prepare");
        assert!(matches!(phase, RssAcceptPhase::Prepared(_)));
        let urls: Vec<String> = source
            .enclosure_requests()
            .into_iter()
            .map(|(url, _)| url)
            .collect();
        assert_eq!(urls, ["https://exemple.fr/ep1.webp"]);
    }

    #[test]
    fn a_shared_media_file_survives_a_failed_commit_of_a_second_story() {
        // Two stories ingest the same bytes (content-addressed sharing).
        // When the SECOND commit fails, its compensation must not remove
        // the file the FIRST story still references.
        let mut db = fresh_db();
        let feed = feed_with_enclosure();
        let store_root = tempfile::tempdir().expect("tempdir");
        let source = MockRssFeedSource::new();
        source.enqueue_body(feed.clone());
        source.enqueue_enclosure_body(tiny_wav());
        let outcome = accept_rss_story_creation(
            &mut db,
            official_content_sources(),
            &source,
            FEED_URL,
            &selection_of(&feed, &["g-enc"]),
            BUDGET,
            BUDGET,
            Some(store_root.path()),
        )
        .expect("first creation");
        let RssCreationOutcome::Created { story } = outcome else {
            panic!("expected a creation");
        };
        let file_name: String = db
            .conn()
            .query_row(
                "SELECT file_name FROM assets WHERE story_id = ?1",
                rusqlite::params![&story.id],
                |row| row.get(0),
            )
            .expect("asset");
        let promoted = store_root.path().join("node-media").join(&file_name);
        assert!(promoted.is_file());

        // Second ingestion of the same bytes, whose commit is sabotaged.
        source.enqueue_body(feed.clone());
        source.enqueue_enclosure_body(tiny_wav());
        let phase = prepare_rss_story_creation(
            official_content_sources(),
            &source,
            FEED_URL,
            &selection_of(&feed, &["g-enc"]),
            BUDGET,
            BUDGET,
            Some(store_root.path()),
            &|_| {},
        )
        .expect("prepare");
        let RssAcceptPhase::Prepared(prepared) = phase else {
            panic!("expected a prepared creation");
        };
        db.conn()
            .execute_batch("DROP TABLE story_local_imports;")
            .expect("sabotage");
        assert!(commit_rss_story_creation(&mut db, *prepared).is_err());
        assert!(
            promoted.is_file(),
            "the file referenced by the first story must survive the compensation"
        );
    }

    // ===== S7 regression lock (TDD-3) =====
    //
    // The existing RSS path must stay unchanged: a VALID local fixture feed
    // (channel title, two items, one enclosure) previews through the
    // production `HttpRssFeedSource` with no external network.

    struct FixtureHttpServer {
        base: String,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Drop for FixtureHttpServer {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn start_fixture_http_server<F>(make_routes: F) -> FixtureHttpServer
    where
        F: FnOnce(&str) -> Vec<(String, u16, Vec<u8>)> + Send + 'static,
    {
        use std::io::{Read, Write};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let addr = listener.local_addr().expect("local address");
        let base = format!("http://{addr}");
        let routes = make_routes(&base);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if worker_stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut request = [0u8; 4096];
                let read = stream.read(&mut request).unwrap_or(0);
                let path = String::from_utf8_lossy(&request[..read])
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_owned();
                let (status, body) = routes
                    .iter()
                    .find(|(route, _, _)| *route == path)
                    .map(|(_, status, body)| (*status, body.clone()))
                    .unwrap_or((404, Vec::from("not found")));
                let reason = match status {
                    200 => "OK",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    _ => "Error",
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        FixtureHttpServer { base, stop }
    }

    fn s7_fixture_feed(base: &str) -> Vec<u8> {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <rss version=\"2.0\"><channel><title>Flux fixture</title>\
             <item><title>Episode un</title><description>Premier texte de l'episode.</description>\
             <guid>fixture-1</guid>\
             <enclosure url=\"{base}/media/episode-1.wav\" type=\"audio/wav\" length=\"20\"/></item>\
             <item><title>Episode deux</title><description>Deuxieme texte de l'episode.</description>\
             <guid>fixture-2</guid></item>\
             </channel></rss>"
        )
        .into_bytes()
    }

    /// The 20-byte WAV fixture: magic `RIFF`/`WAVE` only — enough for the
    /// store's sniff to promote it as an audio media.
    fn s7_fixture_wav() -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&[0u8; 4]);
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(&[0u8; 8]);
        bytes
    }

    /// Green from the start: the RSS preview path (S7) reads a valid local
    /// fixture feed unchanged — channel title, item count, enclosure.
    #[test]
    fn test_preview_rss_source_reads_local_fixture_feed() {
        let server = start_fixture_http_server(|base| {
            vec![
                ("/flux".to_owned(), 200, s7_fixture_feed(base)),
                ("/media/episode-1.wav".to_owned(), 200, s7_fixture_wav()),
            ]
        });
        let url = format!("{}/flux", server.base);
        let source = crate::infrastructure::device::rss_source::HttpRssFeedSource::default();
        let outcome = preview_rss_source(
            official_content_sources(),
            &source,
            &url,
            Duration::from_secs(30),
        )
        .expect("the local fixture feed must preview");
        assert_eq!(outcome.source_host, "127.0.0.1");
        assert!(
            !outcome.analysis.is_blocked(),
            "the fixture feed must not be blocked"
        );
        assert_eq!(
            outcome.analysis.channel_title.as_deref(),
            Some("Flux fixture")
        );
        assert_eq!(outcome.analysis.items.len(), 2);
        assert_eq!(outcome.analysis.items[0].title, "Episode un");
        assert!(
            outcome.analysis.items[0].has_enclosure,
            "the first item must keep its enclosure reference"
        );
        assert_eq!(outcome.analysis.items[1].title, "Episode deux");
    }
}
