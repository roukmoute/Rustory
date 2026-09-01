//! Web podcast page extraction application service.
//!
//! Extracts episodes from a web page (HTML) containing podcast episodes,
//! similar to RSS but with HTML parsing instead of XML.
//!
//! Structure mirrors rss_creation:
//! - preview_web_podcast: fetch + parse with ZERO mutation
//! - accept_web_podcast_creation: RE-fetch, re-parse, commit
//!
//! Module split: `page` holds the PURE parsing — the JSON-LD and DOM
//! extraction, the honesty filter, the injective preview checksum
//! framing — with ZERO network; this root holds the orchestration: the
//! network fetches, the `ItemList` follow (the extraction's only
//! network boundary), the two facades and the accept/commit chain.

mod page;

pub use page::WebEpisode;

use std::path::Path;
use std::time::Duration;

use crate::application::story::now_iso_ms;
use crate::domain::import::{
    feed_url_host, rss_import_state, ContentSourceKind, ContentSourceLine,
    RecognitionAspect, RecognitionCategory,
    RecognitionFinding, RSS_FALLBACK_TITLE_PREFIX,
};
use crate::domain::shared::AppError;
use crate::domain::story::{
    canonical_structure_json, content_checksum, content_checksum_bytes, CanonicalNode,
    CanonicalStructure, CANONICAL_STORY_SCHEMA_VERSION, START_NODE_ID,
};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::filesystem::{
    ensure_node_media_store, store_media_capped, StoredMedia, WEB_MAX_MEDIA_BYTES,
};
use crate::ipc::dto::import_export::import_report_dto;
use crate::ipc::dto::StoryCardDto;

use page::{
    collect_jsonld_nodes, episode_from_episode_page, jsonld_payloads, jsonld_type,
    keep_valid_episodes, page_checksum_of, parse_web_episodes, resolve_url,
};
use super::creation_common::{
    commit_story_creation, compensate_promoted_assets, ensure_source_enabled, PromotedAsset,
    StoryCreationCommit,
};
use scraper::Html;
use serde_json::Value;

/// The application-level outcome of previewing a web podcast page.
#[derive(Debug, Clone)]
pub struct WebPreviewOutcome {
    pub source_host: String,
    pub page_checksum: String,
    pub items: Vec<WebEpisode>,
}

/// The entry guard, consulted by BOTH web facades (preview AND accept)
/// right after the policy gate: the address must survive the STRICT url
/// authority (http/https only, sober host, bounded) BEFORE any network
/// dispatch — a malformed address never builds a client or sends a byte.
/// Returns the sober host of the validated address.
fn validate_web_entry_url(web_url: &str) -> Result<String, AppError> {
    feed_url_host(web_url).ok_or_else(invalid_web_url_error)
}

/// Motivated access failures of the web fetch path — one variant per case
/// so each failure keeps a DISTINCT user-facing reason (S5: the system
/// states the reason and stops). The diagnostic stage is carried in the
/// error details.
#[derive(Debug)]
enum WebFetchFailure {
    ClientBuild,
    Request(String),
    StatusCheck(u16),
    ReadText(String),
    /// A downloaded media exceeded the web ceiling (content problem: the
    /// acquisition degrades to a verdict, it never blocks the creation).
    Oversize,
}

impl WebFetchFailure {
    fn into_app_error(self) -> AppError {
        match self {
            WebFetchFailure::ClientBuild => AppError::import_failed(
                "Récupération de la page impossible.",
                "Réessaie ; si le problème persiste, consulte les traces locales.",
            )
            .with_details(serde_json::json!({
                "source": "network",
                "stage": "client_build",
            })),
            WebFetchFailure::Request(error) => AppError::rss_source_unreachable(
                "La page est injoignable.",
                "Vérifie ta connexion puis réessaie.",
            )
            .with_details(serde_json::json!({
                "source": "network",
                "stage": "request",
                "error": error,
            })),
            WebFetchFailure::StatusCheck(status) => AppError::rss_source_unreachable(
                format!("Le serveur a répondu avec une erreur HTTP {status}."),
                "Réessaie plus tard ; si le problème persiste, la page est peut-être indisponible.",
            )
            .with_details(serde_json::json!({
                "source": "network",
                "stage": "status_check",
                "status": status,
            })),
            WebFetchFailure::ReadText(error) => AppError::import_failed(
                "Impossible de lire le contenu de la page.",
                "Réessaie ; si le problème persiste, consulte les traces locales.",
            )
            .with_details(serde_json::json!({
                "source": "network",
                "stage": "read_text",
                "error": error,
            })),
            WebFetchFailure::Oversize => AppError::import_failed(
                "Le média est trop volumineux.",
                "Réessaie avec une page allégée.",
            )
            .with_details(serde_json::json!({
                "source": "network",
                "stage": "oversize",
            })),
        }
    }
}

/// Fetch HTML content from a URL using reqwest blocking client.
fn fetch_html(url: &str, budget: Duration) -> Result<String, AppError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(budget)
        .build()
        .map_err(|_| WebFetchFailure::ClientBuild.into_app_error())?;

    let response = client
        .get(url)
        .send()
        .map_err(|error| WebFetchFailure::Request(error.to_string()).into_app_error())?;

    if !response.status().is_success() {
        return Err(WebFetchFailure::StatusCheck(response.status().as_u16()).into_app_error());
    }

    response
        .text()
        .map_err(|error| WebFetchFailure::ReadText(error.to_string()).into_app_error())
}

/// Download ONE episode media (audio or image) as raw bytes, bounded by the
/// web ceiling. The typed [`WebFetchFailure`] (never an `AppError`) lets the
/// accept phase degrade a failed acquisition to a CONTENT verdict — the RSS
/// precedent: a failed download is « média non récupéré », not a refusal.
fn fetch_media_bytes(url: &str, budget: Duration) -> Result<Vec<u8>, WebFetchFailure> {
    use std::io::Read;

    let client = reqwest::blocking::Client::builder()
        .timeout(budget)
        .build()
        .map_err(|_| WebFetchFailure::ClientBuild)?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| WebFetchFailure::Request(error.to_string()))?;
    if !response.status().is_success() {
        return Err(WebFetchFailure::StatusCheck(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|declared| (declared as usize) > WEB_MAX_MEDIA_BYTES)
    {
        return Err(WebFetchFailure::Oversize);
    }
    // Bounded read: one overflow byte past the ceiling is enough to refuse.
    let mut bytes: Vec<u8> = Vec::new();
    response
        .take(WEB_MAX_MEDIA_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| WebFetchFailure::Request(error.to_string()))?;
    if bytes.len() > WEB_MAX_MEDIA_BYTES {
        return Err(WebFetchFailure::Oversize);
    }
    Ok(bytes)
}

/// The episodes of a list page through its JSON-LD `ItemList`: each
/// entry's `url` is resolved, fetched (depth one), and its episode node
/// extracted. Entries are ordered by their `position` (document order on
/// ties and when absent), exact-duplicated urls are kept once, and an
/// entry whose page cannot be fetched or carries no recognized episode
/// is skipped honestly.
fn episodes_from_item_list(
    document: &Html,
    base_url: &str,
    budget: Duration,
) -> Vec<WebEpisode> {
    let payloads = jsonld_payloads(document);
    let mut nodes: Vec<&Value> = Vec::new();
    for payload in &payloads {
        collect_jsonld_nodes(payload, &mut nodes);
    }
    let mut entries: Vec<(u64, String)> = Vec::new();
    for node in nodes.iter().filter(|node| jsonld_type(node) == Some("ItemList")) {
        let Some(elements) = node.get("itemListElement").and_then(Value::as_array) else {
            continue;
        };
        for (index, element) in elements.iter().enumerate() {
            let position = element
                .get("position")
                .and_then(Value::as_u64)
                .unwrap_or(index as u64 + 1);
            let Some(url) = element.get("url").and_then(Value::as_str) else {
                continue;
            };
            entries.push((position, url.trim().to_owned()));
        }
    }
    entries.sort_by_key(|(position, _)| *position);
    let mut unique: Vec<String> = Vec::new();
    for (_, url) in entries {
        if !url.is_empty() && !unique.contains(&url) {
            unique.push(url);
        }
    }
    unique
        .iter()
        .filter_map(|url| {
            let absolute = resolve_url(base_url, url)?;
            fetch_html(&absolute, budget)
                .ok()
                .and_then(|html| episode_from_episode_page(&html))
        })
        .collect()
}

/// The extraction routing of a web page: the JSON-LD `ItemList` pass
/// first (the shape of the sample pages), falling back to the DOM pass
/// when the list page carries no resolvable episode — the order of the
/// passes is the priority, the document order inside a pass is kept.
fn extract_web_episodes(
    document: &Html,
    base_url: &str,
    budget: Duration,
) -> Vec<WebEpisode> {
    let mut episodes = episodes_from_item_list(document, base_url, budget);
    if episodes.is_empty() {
        episodes = parse_web_episodes(document, base_url);
    }
    episodes
}

/// S6 dedicated report: the page is reachable but carries no usable
/// audio media — the import stops here, no story can be built.
fn no_audio_media_error() -> AppError {
    AppError::import_failed(
        "Aucun média audio n'a été trouvé.",
        "Vérifie que la page contient des épisodes audio puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "parsing",
        "stage": "no_audio_media",
    }))
}

/// Phase 1 — preview a web podcast page and extract episode references.
pub fn preview_web_podcast(
    sources: &[ContentSourceLine],
    web_url: &str,
    budget: Duration,
) -> Result<WebPreviewOutcome, AppError> {
    // Policy gate: check if web source is enabled before any network dispatch
    ensure_source_enabled(sources, ContentSourceKind::Web)?;

    // Entry guard BEFORE any network dispatch.
    let source_host = validate_web_entry_url(web_url)?;

    let html_content = fetch_html(web_url, budget)?;
    let document = Html::parse_document(&html_content);
    let episodes = extract_web_episodes(&document, web_url, budget);
    let episodes = keep_valid_episodes(episodes);
    if episodes.is_empty() {
        return Err(no_audio_media_error());
    }

    // The fingerprint of the PREVIEWED episode set (title + resolved audio +
    // resolved image, in page order) — the accept re-proves exactly this.
    let page_checksum = page_checksum_of(&episodes, web_url);

    Ok(WebPreviewOutcome {
        source_host,
        page_checksum,
        items: episodes,
    })
}

/// The `story_local_imports.source_format_version` written for a `web`
/// provenance row (revision 1 = the first frozen-contract extraction;
/// mirrors `RSS_SOURCE_FORMAT_VERSION` — a revision of OUR reader support,
/// not a value declared inside the foreign page).
pub const WEB_SOURCE_FORMAT_VERSION: u64 = 1;

/// Phase 2a — RE-fetch, re-extract and re-prove the PREVIEWED episode set,
/// with NO DB access at all: the command runs this BEFORE taking the DB
/// lock, so the network fetch never holds it. The page is re-proven by
/// [`page_checksum_of`] against `expected_page_checksum` — a diverged page
/// is the honest [`WebAcceptPhase::SourceChanged`] refusal with ZERO
/// mutation (nothing is downloaded, promoted or written). Otherwise every
/// episode's audio (and image, when the page provides one) is DOWNLOADED
/// and PROMOTED into the node-media store during this DB-free phase, the
/// canonical ordered N-node structure is composed (page order, each node
/// with its own title), and the findings derive the durable state: a
/// failed AUDIO download is a CONTENT verdict (`(Media, Missing)`, état
/// `partial` — the RSS precedent, the story is still created), a failed
/// IMAGE download degrades silently (the image stays optional, S3).
pub fn prepare_web_story_creation(
    sources: &[ContentSourceLine],
    web_url: &str,
    expected_page_checksum: &str,
    budget: Duration,
    app_data_dir: Option<&Path>,
) -> Result<WebAcceptPhase, AppError> {
    ensure_source_enabled(sources, ContentSourceKind::Web)?;

    // Entry guard BEFORE any network dispatch.
    let source_host = validate_web_entry_url(web_url)?;

    let html_content = fetch_html(web_url, budget)?;
    let document = Html::parse_document(&html_content);
    let episodes = extract_web_episodes(&document, web_url, budget);
    let episodes = keep_valid_episodes(episodes);
    if episodes.is_empty() {
        return Err(no_audio_media_error());
    }
    if page_checksum_of(&episodes, web_url) != expected_page_checksum {
        // The page diverged since the preview — refuse honestly, promote
        // NOTHING (the store is not even touched on this path).
        return Ok(WebAcceptPhase::SourceChanged);
    }

    // The store root is consulted ONLY after the checksum re-proof, so a
    // refusal never creates a directory or a file.
    let store: Option<(std::path::PathBuf, std::path::PathBuf)> = match app_data_dir {
        Some(dir) => ensure_node_media_store(dir).ok(),
        None => None,
    };

    // One node per episode, in page order (n1..nN — the flat ordered graph
    // the v3 canonical model carries). Every audio is downloaded and
    // promoted NOW (the network phase); a failed download leaves its node
    // audio-less and flips the Media finding (the RSS partial precedent).
    let mut structure = CanonicalStructure {
        schema_version: CANONICAL_STORY_SCHEMA_VERSION,
        start_node_id: START_NODE_ID.to_owned(),
        nodes: Vec::with_capacity(episodes.len()),
    };
    let mut assets: Vec<PromotedAsset> = Vec::new();
    let mut audio_missing = false;
    for (index, episode) in episodes.iter().enumerate() {
        let (audio_asset_id, image_asset_id, audio_failed) =
            promote_episode_media(episode, web_url, budget, store.as_ref(), &mut assets);
        if audio_failed {
            audio_missing = true;
        }
        structure.nodes.push(CanonicalNode {
            id: format!("n{}", index + 1),
            text: episode.summary.clone(),
            label: episode.title.trim().to_owned(),
            image_asset_id,
            audio_asset_id,
            options: Vec::new(),
        });
    }

    // The ingestion's findings and durable state: source and title carry
    // the NOMINAL provenance ambiguity every ingestion keeps, the structure
    // is recognized, and the Media aspect is recognized only when every
    // audio was actually downloaded and promoted.
    let findings = vec![
        RecognitionFinding::ambiguous(RecognitionAspect::Source),
        RecognitionFinding::ambiguous(RecognitionAspect::Title),
        RecognitionFinding::recognized(RecognitionAspect::Structure),
        if audio_missing {
            RecognitionFinding {
                aspect: RecognitionAspect::Media,
                category: RecognitionCategory::Missing,
                message: None,
            }
        } else {
            RecognitionFinding::recognized(RecognitionAspect::Media)
        },
    ];
    let state = rss_import_state(&findings);

    // Title: the `Histoire de {hôte}` fallback, ALWAYS (a web page's
    // collection title is not a story title; the EPISODE titles are the
    // node labels, never replaced by it).
    let title = format!("{RSS_FALLBACK_TITLE_PREFIX}{source_host}");

    // Provenance: the checksum fingerprints the RE-FETCHED page bytes —
    // the bytes actually ingested (the RSS `feed_checksum` precedent).
    let artifact_checksum = content_checksum_bytes(html_content.as_bytes());

    let structure_json = canonical_structure_json(&structure);
    let checksum = content_checksum(&structure_json);
    let now_iso = now_iso_ms()?;

    Ok(WebAcceptPhase::Prepared(Box::new(PreparedWebCreation {
        commit: StoryCreationCommit {
            title,
            structure_json,
            checksum,
            now_iso,
            source_name: source_host,
            artifact_checksum,
            state,
            findings,
        },
        assets,
    })))
}

/// The typed outcome of the accept phase.
#[derive(Debug)]
pub enum WebAcceptPhase {
    SourceChanged,
    Prepared(Box<PreparedWebCreation>),
}

/// Download ONE episode media and PROMOTE it into the node-media store,
/// or `None` on ANY failure (transport, over-cap, unsupported bytes, store
/// I/O) — the media stays honestly « non récupéré ». A content problem
/// never becomes an `AppError` here (the module's contract, the RSS
/// `promote_enclosure` precedent).
fn fetch_and_promote(
    url: &str,
    budget: Duration,
    media_dir: &Path,
    staging_dir: &Path,
) -> Option<StoredMedia> {
    let bytes = fetch_media_bytes(url, budget).ok()?;
    store_media_capped(media_dir, staging_dir, &bytes, WEB_MAX_MEDIA_BYTES).ok()
}

/// Download and promote ONE episode's media — its audio, then the image
/// when the page provides one — into the node-media store: the exact
/// per-episode body of the prepare loop. A failed AUDIO download is
/// reported as `audio_failed` (the caller flips the Media finding, the
/// RSS partial precedent); a failed IMAGE download degrades silently
/// (the image stays optional, S3).
fn promote_episode_media(
    episode: &WebEpisode,
    web_url: &str,
    budget: Duration,
    store: Option<&(std::path::PathBuf, std::path::PathBuf)>,
    assets: &mut Vec<PromotedAsset>,
) -> (Option<String>, Option<String>, bool) {
    let mut audio_asset_id: Option<String> = None;
    let mut image_asset_id: Option<String> = None;
    let mut audio_failed = false;
    if let Some((media_dir, staging_dir)) = store {
        if let Some(raw_audio) = &episode.audio_url {
            let resolved = resolve_url(web_url, raw_audio)
                .unwrap_or_else(|| raw_audio.clone());
            match fetch_and_promote(&resolved, budget, media_dir, staging_dir) {
                Some(stored) => {
                    let asset = prepare_asset(stored, media_dir);
                    audio_asset_id = Some(asset.asset_id.clone());
                    assets.push(asset);
                }
                None => audio_failed = true,
            }
        } else {
            audio_failed = true;
        }
        // The image is OPTIONAL: a failed download (or unsupported
        // bytes) simply leaves the node image-less — no finding, no
        // state change (S3).
        if let Some(raw_image) = &episode.image_url {
            let resolved = resolve_url(web_url, raw_image)
                .unwrap_or_else(|| raw_image.clone());
            if let Some(stored) = fetch_and_promote(&resolved, budget, media_dir, staging_dir)
            {
                let asset = prepare_asset(stored, media_dir);
                image_asset_id = Some(asset.asset_id.clone());
                assets.push(asset);
            }
        }
    } else if episode.audio_url.is_some() {
        audio_failed = true;
    }
    (audio_asset_id, image_asset_id, audio_failed)
}

/// Everything ONE promoted episode media needs for its `assets` row, plus
/// the promoted file path so a failed commit can compensate the store.
fn prepare_asset(stored: StoredMedia, media_dir: &Path) -> PromotedAsset {
    PromotedAsset {
        asset_id: uuid::Uuid::now_v7().to_string(),
        content_hash: stored.content_hash,
        media_type: stored.kind.as_str(),
        media_format: stored.format,
        byte_size: stored.byte_size,
        file_name: stored.file_name.clone(),
        promoted_path: media_dir.join(stored.file_name),
    }
}

/// Phase 2b — the single atomic transaction (`stories` + the provenance
/// row + every promoted media's `assets` row), shared with the RSS flow
/// ([`commit_story_creation`]). This is the ONLY part of the accept that
/// needs the DB lock. A failed transaction rolls back fully; the promoted
/// media files — the only pre-transaction mutation — are then compensated
/// best-effort.
pub fn commit_web_story_creation(
    db: &mut DbHandle,
    prepared: PreparedWebCreation,
) -> Result<StoryCardDto, AppError> {
    let PreparedWebCreation { commit, assets } = prepared;
    let result = commit_story_creation(
        db,
        &commit,
        "web",
        WEB_SOURCE_FORMAT_VERSION,
        &assets,
        import_report_dto,
    );
    if result.is_err() {
        compensate_promoted_assets(&assets);
    }
    result
}

/// Convenience: prepare + commit under the SAME borrowed handle (tests and
/// single-threaded callers). The IPC command does NOT use this — it runs
/// [`prepare_web_story_creation`] before taking the DB lock and only locks
/// for [`commit_web_story_creation`].
pub fn accept_web_podcast_creation(
    db: &mut DbHandle,
    sources: &[ContentSourceLine],
    web_url: &str,
    expected_page_checksum: &str,
    budget: Duration,
    app_data_dir: Option<&Path>,
) -> Result<WebCreationOutcome, AppError> {
    match prepare_web_story_creation(
        sources,
        web_url,
        expected_page_checksum,
        budget,
        app_data_dir,
    )? {
        WebAcceptPhase::SourceChanged => Ok(WebCreationOutcome::SourceChanged),
        WebAcceptPhase::Prepared(prepared) => commit_web_story_creation(db, *prepared)
            .map(|story| WebCreationOutcome::Created { story }),
    }
}

/// The typed outcome of an accept: the created card + its report, or the
/// honest recoverable refusal (the source diverged since the preview —
/// nothing was mutated). The refusal is a VERDICT, never an `AppError`.
#[derive(Debug, Clone)]
pub enum WebCreationOutcome {
    Created { story: StoryCardDto },
    SourceChanged,
}

/// The error for an invalid web URL.
pub fn invalid_web_url_error() -> AppError {
    AppError::rss_source_unreachable(
        "Récupération de la page impossible: l'adresse n'est pas valide.",
        "Saisis une adresse http(s) complète puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "network",
        "stage": "url_invalid",
    }))
}

/// The fully re-proven, ready-to-commit ingestion — everything the atomic
/// DB transaction needs, produced WITHOUT any DB access
/// ([`prepare_web_story_creation`]) so the network fetch never serializes
/// other commands behind the DB lock.
#[derive(Debug)]
pub struct PreparedWebCreation {
    commit: StoryCreationCommit,
    /// The downloaded-and-promoted episode media (audios, then the images
    /// the page provides), ready for their `assets` rows — empty when no
    /// store root was given or every download degraded.
    assets: Vec<PromotedAsset>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::import::{official_content_sources, ImportState};


    #[test]
    fn test_invalid_web_url_error() {
        let err = invalid_web_url_error();
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::RssSourceUnreachable
        );
    }

    // ===== Motivated access failures (TDD-2) =====
    //
    // S5: a VALID address whose page is unreachable (injoignable) or that
    // answers with an HTTP error must produce a DISTINCT user-facing reason
    // per case. Neither path may create a story: the preview never touches
    // the library, and the S5 acceptance handler asserts the story count.

    /// Minimal one-shot multi-route HTTP server on 127.0.0.1 for
    /// deterministic fixtures: the routes are built AFTER the bind so the
    /// fixture documents can reference the local base URL.
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
                let Ok(mut stream) = stream else {
                    continue
                };
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
                    .unwrap_or((404, b"not found".to_vec()));
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

    fn fetch_error_stage(err: &AppError) -> String {
        serde_json::to_value(err)
            .expect("AppError serializes")
            .get("details")
            .and_then(|details| details.get("stage"))
            .and_then(|stage| stage.as_str())
            .unwrap_or("<absent>")
            .to_owned()
    }

    #[test]
    fn test_fetch_html_rejects_unreachable_host_with_distinct_reason() {
        // RFC 2606 reserved TLD: the address is syntactically valid but can
        // never be resolved — the "injoignable" variant of S5.
        let url = "https://import-test-non-rss.exemple.invalid/";
        let err = fetch_html(url, Duration::from_secs(10))
            .expect_err("an unreachable page must never yield content");
        assert_eq!(fetch_error_stage(&err), "request");
        assert!(
            err.message.contains("injoignable"),
            "the user-facing reason must say the page is unreachable, got: {}",
            err.message
        );
    }

    #[test]
    fn test_fetch_html_rejects_http_error_status_with_distinct_reason() {
        let server = start_fixture_http_server(|_| {
            vec![("/page".to_owned(), 500, "<html><body>boom</body></html>".to_owned().into_bytes())]
        });
        let url = format!("{}/page", server.base);
        let err = fetch_html(&url, Duration::from_secs(10))
            .expect_err("an HTTP error status must never yield content");
        drop(server);
        assert_eq!(fetch_error_stage(&err), "status_check");
        let value = serde_json::to_value(&err).expect("AppError serializes");
        assert_eq!(value["details"]["status"], 500);
        assert!(
            err.message.contains("erreur HTTP") && err.message.contains("500"),
            "the user-facing reason must state the HTTP error, got: {}",
            err.message
        );
    }

    #[test]
    fn test_fetch_html_failure_reasons_are_distinct_per_case() {
        let unreachable = fetch_html(
            "https://import-test-non-rss.exemple.invalid/",
            Duration::from_secs(10),
        )
        .expect_err("unreachable host");
        let server = start_fixture_http_server(|_| {
            vec![("/page".to_owned(), 500, "boom".to_owned().into_bytes())]
        });
        let http_error = fetch_html(&format!("{}/page", server.base), Duration::from_secs(10)).expect_err("http error");
        drop(server);
        assert_ne!(
            unreachable.message, http_error.message,
            "S5 requires a distinct user-facing reason per access failure case"
        );
    }

    /// Cap boundary: a media of EXACTLY WEB_MAX_MEDIA_BYTES is below the
    /// ceiling ("strictly greater" is the refusal) — it must be accepted,
    /// whatever the declared Content-Length repeats.
    #[test]
    fn test_fetch_media_bytes_accepts_a_media_of_exactly_the_web_cap() {
        let server = start_fixture_http_server(|_| {
            vec![("/media/exact".to_owned(), 200, vec![b'x'; WEB_MAX_MEDIA_BYTES])]
        });
        let url = format!("{}/media/exact", server.base);
        let bytes = fetch_media_bytes(&url, Duration::from_secs(30))
            .expect("a media of exactly the web cap must be accepted");
        assert_eq!(bytes.len(), WEB_MAX_MEDIA_BYTES);
        drop(server);
    }

    /// Cap boundary: ONE byte above the ceiling is refused (Oversize),
    /// whatever the declared Content-Length says — the bounded read is the
    /// ground truth.
    #[test]
    fn test_fetch_media_bytes_refuses_a_media_one_byte_above_the_web_cap() {
        let server = start_fixture_http_server(|_| {
            vec![("/media/over".to_owned(), 200, vec![b'x'; WEB_MAX_MEDIA_BYTES + 1])]
        });
        let url = format!("{}/media/over", server.base);
        let err = fetch_media_bytes(&url, Duration::from_secs(30))
            .expect_err("one byte above the web cap must be refused");
        assert!(matches!(err, WebFetchFailure::Oversize));
        drop(server);
    }

    /// A one-shot `Transfer-Encoding: chunked` response: no Content-Length
    /// header, so the client cannot know the size upfront — the bounded
    /// read, not a declared length, is the ground truth.
    fn start_chunked_media_server(body: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let addr = listener.local_addr().expect("local address");
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let Ok((mut stream, _)) = listener.accept() else {
                return
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
            let mut offset = 0usize;
            while offset < body.len() {
                let chunk_len = (1024 * 1024).min(body.len() - offset);
                let chunk = &body[offset..offset + chunk_len];
                let _ = stream.write_all(format!("{:x}\r\n", chunk.len()).as_bytes());
                if stream.write_all(chunk).is_err() {
                    return
                }
                let _ = stream.write_all(b"\r\n");
                offset += chunk.len();
            }
            let _ = stream.write_all(b"0\r\n\r\n");
            let _ = stream.flush();
        });
        format!("http://{addr}/media")
    }

    /// A chunked body of exactly 1000 bytes (no Content-Length) is read in
    /// full: proves the bounded read decodes chunked framing end to end.
    #[test]
    fn test_fetch_media_bytes_reads_chunked_body_without_declared_length() {
        let url = start_chunked_media_server(vec![b'x'; 1000]);
        let bytes = fetch_media_bytes(&url, Duration::from_secs(30))
            .expect("a chunked body below the cap must be read in full");
        assert_eq!(bytes.len(), 1000);
    }

    /// Cap boundary: a chunked body one byte above the ceiling (no
    /// Content-Length) is refused by the BOUNDED READ — with no declared
    /// length, shrinking the take() ceiling would silently truncate
    /// instead of refusing.
    #[test]
    fn test_fetch_media_bytes_refuses_chunked_body_above_the_web_cap() {
        let url = start_chunked_media_server(vec![b'x'; WEB_MAX_MEDIA_BYTES + 1]);
        let err = fetch_media_bytes(&url, Duration::from_secs(30))
            .expect_err("one byte above the cap must be refused without any declared length");
        assert!(matches!(err, WebFetchFailure::Oversize));
    }

    // ===== Web source recognition (TDD-3) =====
    //
    // S1: a public non-RSS HTML page previews as a web source carrying its
    // OWN host, with its episodes identified. The real sample pages (E1/E2)
    // carry their episodes only as JSON-LD: an `ItemList` of episode-page
    // URLs, each page holding a `RadioEpisode` node (`name`,
    // `mainEntity.contentUrl`, `description`). S7 keeps its own lock in
    // `rss_creation` (local fixture feed through `HttpRssFeedSource`).

    /// The fixture list page: an `ItemList` whose entries are listed in
    /// document order 2,1 — the `position` field must drive the result.
    fn list_page_html(base: &str) -> String {
        format!(
            "<html><head><script type=\"application/ld+json\">{{\
             \"@context\":\"https://schema.org\",\
             \"@graph\":[{{\"@type\":\"ItemList\",\"name\":\"Selection fixture\",\"itemListElement\":[\
             {{\"@type\":\"ListItem\",\"position\":2,\"url\":\"{base}/episodes/episode-deux\"}},\
             {{\"@type\":\"ListItem\",\"position\":1,\"url\":\"{base}/episodes/episode-un\"}}]}}]\
             }}</script></head><body><p>page sans aucun media audio dans le HTML brut</p></body></html>"
        )
    }

    /// One fixture episode page: a `RadioEpisode` node, audio through
    /// `mainEntity.contentUrl` (the shape of the real sample pages).
    fn episode_page_html(base: &str, name: &str, audio_path: &str, description: &str) -> String {
        format!(
            "<html><head><script type=\"application/ld+json\">{{\
             \"@context\":\"https://schema.org\",\
             \"@graph\":[{{\"@type\":\"RadioEpisode\",\"name\":\"{name}\",\
             \"mainEntity\":{{\"@type\":\"AudioObject\",\"contentUrl\":\"{base}{audio_path}\"}},\
             \"description\":\"{description}\"}}]\
             }}</script></head><body><h1>{name}</h1></body></html>"
        )
    }

    /// RED: the preview must follow the `ItemList` (sorted by `position`)
    /// and emit one episode per resolvable page — today the extraction
    /// yields ZERO episodes on such a page.
    #[test]
    fn test_preview_web_podcast_extracts_item_list_episodes_in_position_order() {
        let server = start_fixture_http_server(|base| {
            vec![
                ("/liste".to_owned(), 200, list_page_html(base).into_bytes()),
                (
                    "/episodes/episode-un".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode un",
                        "/media/episode-un.m4a",
                        "Resume de l'episode un.",
                    )
                    .into_bytes(),
                ),
                (
                    "/episodes/episode-deux".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode deux",
                        "/media/episode-deux.m4a",
                        "Resume de l'episode deux.",
                    )
                    .into_bytes(),
                ),
            ]
        });
        let url = format!("{}/liste", server.base);
        let outcome = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect("the fixture list page must preview");
        assert_eq!(
            outcome.items.len(),
            2,
            "the ItemList must yield exactly its two resolvable episodes, got: {outcome:?}"
        );
        assert_eq!(outcome.items[0].title, "Episode un");
        assert_eq!(
            outcome.items[0].audio_url.as_deref(),
            Some(format!("{}/media/episode-un.m4a", server.base).as_str())
        );
        assert_eq!(outcome.items[0].summary, "Resume de l'episode un.");
        assert_eq!(outcome.items[1].title, "Episode deux");
        assert_eq!(
            outcome.items[1].audio_url.as_deref(),
            Some(format!("{}/media/episode-deux.m4a", server.base).as_str())
        );
        assert_eq!(outcome.items[1].summary, "Resume de l'episode deux.");
    }

    /// Regression lock (green from the start): a plain HTML page previews
    /// as a non-RSS web source carrying its own host.
    #[test]
    fn test_preview_web_podcast_carries_page_host_as_source() {
        // TDD-5: the fixture now carries one honest episode — a reachable
        // page WITHOUT any audio media is a S6 refusal, not a preview.
        let server = start_fixture_http_server(|_| {
            let page = "<html><body><section><a href=\"https://fixture.example.org/media/episode.wav\">Episode</a></section></body></html>"
                .to_owned()
                .into_bytes();
            vec![("/page".to_owned(), 200, page)]
        });
        let url = format!("{}/page", server.base);
        let outcome =
            preview_web_podcast(official_content_sources(), &url, Duration::from_secs(30))
                .expect("a reachable local page must preview");
        assert_eq!(
            outcome.source_host,
            feed_url_host(&url).expect("the local address must be sober"),
            "the preview must carry the page's own host"
        );
    }

    // ===== Mutation pins for the honest extraction (TDD-3 hardening) =====
    //
    // Source-mutation testing over the TDD-3 slice exposed five
    // non-equivalent survivors. Each test below pins the behavior the
    // surviving mutant would change: the array shape of the JSON-LD
    // `@type`, the relative-url branches against the list page, the
    // one-based default position of an ItemList entry, and the
    // exact-dedup of duplicated urls. No new behavior is added.

    /// An episode page whose `@type` is an array (a valid JSON-LD shape)
    /// must be recognized exactly like the string shape.
    #[test]
    fn test_episode_page_with_array_type_is_recognized() {
        let server = start_fixture_http_server(|base| {
            let list = format!(
                "<html><head><script type=\"application/ld+json\">{{\
                 \"@context\":\"https://schema.org\",\
                 \"@graph\":[{{\"@type\":\"ItemList\",\"name\":\"Array type fixture\",\
                 \"itemListElement\":[{{\"@type\":\"ListItem\",\"position\":1,\
                 \"url\":\"{base}/episodes/array-type\"}}]}}]}}\
                 </script></head><body></body></html>"
            );
            let episode = format!(
                "<html><head><script type=\"application/ld+json\">{{\
                 \"@context\":\"https://schema.org\",\
                 \"@graph\":[{{\"@type\":[\"RadioEpisode\",\"CreativeWork\"],\
                 \"name\":\"Episode en type tableau\",\
                 \"mainEntity\":{{\"@type\":\"AudioObject\",\"contentUrl\":\"{base}/media/array-type.m4a\"}},\
                 \"description\":\"Description de l'episode en type tableau.\"}}]}}\
                 </script></head><body><h1>Episode en type tableau</h1></body></html>"
            );
            vec![
                ("/liste-array".to_owned(), 200, list.into_bytes()),
                ("/episodes/array-type".to_owned(), 200, episode.into_bytes()),
            ]
        });
        let url = format!("{}/liste-array", server.base);
        let outcome = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect("the fixture list page must preview");
        assert_eq!(
            outcome.items.len(),
            1,
            "the array-typed episode node must be recognized, got: {outcome:?}"
        );
        assert_eq!(outcome.items[0].title, "Episode en type tableau");
        assert_eq!(
            outcome.items[0].audio_url.as_deref(),
            Some(format!("{}/media/array-type.m4a", server.base).as_str())
        );
    }

    /// Mixed ItemList: the first entry carries the explicit position 2,
    /// the second has none (default = its one-based document position,
    /// 2). The tie keeps document order — a zero-based default would
    /// reorder the two episodes.
    #[test]
    fn test_item_list_default_position_is_one_based_document_position() {
        let server = start_fixture_http_server(|base| {
            let list = format!(
                "<html><head><script type=\"application/ld+json\">{{\
                 \"@context\":\"https://schema.org\",\
                 \"@graph\":[{{\"@type\":\"ItemList\",\"name\":\"Mixed positions fixture\",\
                 \"itemListElement\":[\
                 {{\"@type\":\"ListItem\",\"position\":2,\"url\":\"{base}/episodes/explicite\"}},\
                 {{\"@type\":\"ListItem\",\"url\":\"{base}/episodes/sans-position\"}}]}}]}}\
                 </script></head><body></body></html>"
            );
            vec![
                ("/liste-mixte".to_owned(), 200, list.into_bytes()),
                (
                    "/episodes/explicite".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode explicite",
                        "/media/explicite.m4a",
                        "Resume explicite.",
                    )
                    .into_bytes(),
                ),
                (
                    "/episodes/sans-position".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode sans position",
                        "/media/sans-position.m4a",
                        "Resume sans position.",
                    )
                    .into_bytes(),
                ),
            ]
        });
        let url = format!("{}/liste-mixte", server.base);
        let outcome = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect("the fixture list page must preview");
        assert_eq!(
            outcome.items.len(),
            2,
            "both fixture episodes must be extracted, got: {outcome:?}"
        );
        assert_eq!(
            outcome.items[0].title,
            "Episode explicite",
            "the tie between the explicit position and the one-based default keeps document order"
        );
        assert_eq!(outcome.items[1].title, "Episode sans position");
    }

    /// An exact-duplicated url in the ItemList yields its episode
    /// exactly once, and the order is kept.
    #[test]
    fn test_item_list_keeps_an_exact_duplicate_url_once() {
        let server = start_fixture_http_server(|base| {
            let list = format!(
                "<html><head><script type=\"application/ld+json\">{{\
                 \"@context\":\"https://schema.org\",\
                 \"@graph\":[{{\"@type\":\"ItemList\",\"name\":\"Duplicates fixture\",\
                 \"itemListElement\":[\
                 {{\"@type\":\"ListItem\",\"position\":1,\"url\":\"{base}/episodes/duplique\"}},\
                 {{\"@type\":\"ListItem\",\"position\":2,\"url\":\"{base}/episodes/duplique\"}},\
                 {{\"@type\":\"ListItem\",\"position\":3,\"url\":\"{base}/episodes/unique\"}}]}}]}}\
                 </script></head><body></body></html>"
            );
            vec![
                ("/liste-doublons".to_owned(), 200, list.into_bytes()),
                (
                    "/episodes/duplique".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode dupliqué",
                        "/media/duplique.m4a",
                        "Resume dupliqué.",
                    )
                    .into_bytes(),
                ),
                (
                    "/episodes/unique".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode unique",
                        "/media/unique.m4a",
                        "Resume unique.",
                    )
                    .into_bytes(),
                ),
            ]
        });
        let url = format!("{}/liste-doublons", server.base);
        let outcome = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect("the fixture list page must preview");
        assert_eq!(
            outcome.items.len(),
            2,
            "the duplicated url must yield its episode exactly once, got: {outcome:?}"
        );
        assert_eq!(outcome.items[0].title, "Episode dupliqué");
        assert_eq!(outcome.items[1].title, "Episode unique");
    }

    // ===== Honest DOM extraction + optional image (TDD-4) =====
    //
    // S1/S3: the episodes of a page are extracted IN DOCUMENT ORDER with
    // a real non-empty title, the audio media really present in the
    // page, and an OPTIONAL image — carried when the page provides it,
    // absent otherwise, never a blocker and never invented.

    /// S2/S3 (JSON-LD): the episode image is carried on its episode when
    /// the page provides it — `image` as an ImageObject url, a plain
    /// string, or an array — and stays absent otherwise; in every case
    /// the extraction succeeds.
    #[test]
    fn test_episode_page_image_is_carried_when_provided_and_absent_otherwise() {
        let server = start_fixture_http_server(|base| {
            let list = format!(
                "<html><head><script type=\"application/ld+json\">{{\
                 \"@context\":\"https://schema.org\",\
                 \"@graph\":[{{\"@type\":\"ItemList\",\"name\":\"Image fixture\",\
                 \"itemListElement\":[\
                 {{\"@type\":\"ListItem\",\"position\":1,\"url\":\"{base}/episodes/avec-image\"}},\
                 {{\"@type\":\"ListItem\",\"position\":2,\"url\":\"{base}/episodes/image-string\"}},\
                 {{\"@type\":\"ListItem\",\"position\":3,\"url\":\"{base}/episodes/image-array\"}},\
                 {{\"@type\":\"ListItem\",\"position\":4,\"url\":\"{base}/episodes/sans-image\"}}]}}]}}\
                 </script></head><body></body></html>"
            );
            let with_image = format!(
                "<html><head><script type=\"application/ld+json\">{{\
                 \"@context\":\"https://schema.org\",\
                 \"@graph\":[{{\"@type\":\"RadioEpisode\",\"name\":\"Episode avec image\",\
                 \"image\":{{\"@type\":\"ImageObject\",\"url\":\"{base}/images/avec-image.jpg\"}},\
                 \"mainEntity\":{{\"@type\":\"AudioObject\",\"contentUrl\":\"{base}/media/avec-image.m4a\"}}}}]}}\
                 </script></head><body><h1>Episode avec image</h1></body></html>"
            );
            let image_string = format!(
                "<html><head><script type=\"application/ld+json\">{{\
                 \"@context\":\"https://schema.org\",\
                 \"@graph\":[{{\"@type\":\"RadioEpisode\",\"name\":\"Episode image string\",\
                 \"image\":\"{base}/images/image-string.jpg\",\
                 \"mainEntity\":{{\"@type\":\"AudioObject\",\"contentUrl\":\"{base}/media/image-string.m4a\"}}}}]}}\
                 </script></head><body></body></html>"
            );
            let image_array = format!(
                "<html><head><script type=\"application/ld+json\">{{\
                 \"@context\":\"https://schema.org\",\
                 \"@graph\":[{{\"@type\":\"RadioEpisode\",\"name\":\"Episode image array\",\
                 \"image\":[\"{base}/images/image-array.jpg\"],\
                 \"mainEntity\":{{\"@type\":\"AudioObject\",\"contentUrl\":\"{base}/media/image-array.m4a\"}}}}]}}\
                 </script></head><body></body></html>"
            );
            let without_image =
                episode_page_html(base, "Episode sans image", "/media/sans-image.m4a", "Resume sans image.");
            vec![
                ("/liste-image".to_owned(), 200, list.into_bytes()),
                ("/episodes/avec-image".to_owned(), 200, with_image.into_bytes()),
                ("/episodes/image-string".to_owned(), 200, image_string.into_bytes()),
                ("/episodes/image-array".to_owned(), 200, image_array.into_bytes()),
                ("/episodes/sans-image".to_owned(), 200, without_image.into_bytes()),
            ]
        });
        let url = format!("{}/liste-image", server.base);
        let outcome = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect("the fixture list page must preview");
        assert_eq!(
            outcome.items.len(),
            4,
            "all four fixture episodes must be extracted, got: {outcome:?}"
        );
        assert_eq!(outcome.items[0].title, "Episode avec image");
        assert_eq!(
            outcome.items[0].image_url.as_deref(),
            Some(format!("{}/images/avec-image.jpg", server.base).as_str()),
            "the page-provided image must be carried on its episode"
        );
        assert_eq!(outcome.items[1].title, "Episode image string");
        assert_eq!(
            outcome.items[1].image_url.as_deref(),
            Some(format!("{}/images/image-string.jpg", server.base).as_str()),
            "a plain-string image must be carried as-is"
        );
        assert_eq!(outcome.items[2].title, "Episode image array");
        assert_eq!(
            outcome.items[2].image_url.as_deref(),
            Some(format!("{}/images/image-array.jpg", server.base).as_str()),
            "an array image must carry its first element"
        );
        assert_eq!(outcome.items[3].title, "Episode sans image");
        assert!(
            outcome.items[3].image_url.is_none(),
            "an episode without image must stay image-less without error"
        );
    }

    /// S6: an accessible page that carries no audio media at all must
    /// never settle as an empty preview — the dedicated report stops
    /// the import before any story could be built.
    #[test]
    fn test_preview_web_podcast_reports_no_audio_media_when_page_has_none() {
        let server = start_fixture_http_server(|_base| {
            let page = "<html><head><title>Page sans média</title></head>\
                        <body><h1>Page sans média</h1><p>Aucun épisode ici.</p></body></html>"
                .to_owned()
                .into_bytes();
            vec![("/page".to_owned(), 200, page)]
        });
        let url = format!("{}/page", server.base);
        let err = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect_err("a page without any audio media must never produce an empty preview");
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::ImportFailed
        );
        assert_eq!(err.message, "Aucun média audio n'a été trouvé.");
        let value = serde_json::to_value(&err).expect("ser");
        assert_eq!(value["details"]["source"], "parsing");
        assert_eq!(value["details"]["stage"], "no_audio_media");
    }

    // ===== Accept/commit multi-épisodes (TDD-6) =====
    //
    // S2 (+ observables S1/S3) : l'accept re-télécharge la page, re-prouve
    // l'état préviewé par checksum, télécharge et promeut les médias,
    // compose la structure canonique ordonnée (N nœuds, ordre de la page)
    // et commite l'histoire + assets + provenance `web` dans une
    // transaction atomique. Les fixtures servent de vrais octets
    // (wav / m4a / png / jpeg) : le store sniffe par magic bytes.

    /// WAV factice mais bien formé (RIFF/WAVE) : l'audio est stocké tel
    /// quel, un payload décodable n'est PAS requis. `marker` distingue les
    /// contenus (hashes distincts).
    fn wav_bytes(marker: u8) -> Vec<u8> {
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&[0u8, 0, 0, 0]);
        v.extend_from_slice(b"WAVE");
        v.extend_from_slice(&[0u8; 4]);
        v.push(marker);
        v
    }

    /// Boîte M4A factice : taille 4 octets + `ftyp` + marque `M4A `.
    fn m4a_bytes() -> Vec<u8> {
        let mut v = vec![0u8, 0, 0, 0];
        v.extend_from_slice(b"ftyp");
        v.extend_from_slice(b"M4A ");
        v.extend_from_slice(&[0u8; 8]);
        v
    }

    fn png_fixture() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 10, 10, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Png,
            )
            .expect("encode png");
        out
    }

    fn jpeg_fixture() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 200, 10, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .expect("encode jpeg");
        out
    }

    /// La page DOM d'exemple : un titre de collection (`<title>`/`<h1>`)
    /// qui ne doit JAMAIS polluer les labels, puis trois sections ordonnées
    /// — chacune porte son média audio titré, son résumé `<p>` et (épisodes
    /// 1 et 3) son image. L'épisode 2 est SANS image (S3).
    fn multi_episode_page_html(base: &str) -> String {
        format!(
            "<html><head><title>Sélection S2</title></head><body>\
             <h1>Sélection S2</h1>\
             <section><img src=\"{base}/images/ep1.png\">\
             <a href=\"{base}/media/ep1.wav\">Épisode un</a>\
             <p>Résumé un.</p></section>\
             <section><a href=\"{base}/media/ep2.m4a\">Épisode deux</a>\
             <p>Résumé deux.</p></section>\
             <section><img src=\"{base}/images/ep3.jpg\">\
             <a href=\"{base}/media/ep3.wav\">Épisode trois</a>\
             <p>Résumé trois.</p></section>\
             </body></html>"
        )
    }

    /// La même page amputée de son troisième épisode : le contenu a divergé
    /// depuis le preview (checksum de page différent).
    fn multi_episode_page_html_shortened(base: &str) -> String {
        format!(
            "<html><head><title>Sélection S2</title></head><body>\
             <h1>Sélection S2</h1>\
             <section><img src=\"{base}/images/ep1.png\">\
             <a href=\"{base}/media/ep1.wav\">Épisode un</a>\
             <p>Résumé un.</p></section>\
             <section><a href=\"{base}/media/ep2.m4a\">Épisode deux</a>\
             <p>Résumé deux.</p></section>\
             </body></html>"
        )
    }

    fn s2_media_routes() -> Vec<(String, u16, Vec<u8>)> {
        vec![
            ("/media/ep1.wav".to_owned(), 200, wav_bytes(1)),
            ("/media/ep2.m4a".to_owned(), 200, m4a_bytes()),
            ("/media/ep3.wav".to_owned(), 200, wav_bytes(2)),
            ("/images/ep1.png".to_owned(), 200, png_fixture()),
            ("/images/ep3.jpg".to_owned(), 200, jpeg_fixture()),
        ]
    }

    /// Comme [`start_fixture_http_server`] mais avec DEUX tables de routes :
    /// le corps A répond avant `switch()`, le corps B après — l'accept
    /// re-télécharge une page divergée du preview.
    struct SwitchingFixtureServer {
        base: String,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        switched: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl SwitchingFixtureServer {
        fn start(
            make_routes: impl FnOnce(
                &str,
            ) -> (Vec<(String, u16, Vec<u8>)>, Vec<(String, u16, Vec<u8>)>)
                + Send
                + 'static,
        ) -> Self {
            use std::io::{Read, Write};
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::Arc;

            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let addr = listener.local_addr().expect("local address");
            let base = format!("http://{addr}");
            let (routes_a, routes_b) = make_routes(&base);
            let stop = Arc::new(AtomicBool::new(false));
            let switched = Arc::new(AtomicBool::new(false));
            let worker_stop = stop.clone();
            let worker_switched = switched.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if worker_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(mut stream) = stream else {
                        continue
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let mut request = [0u8; 4096];
                    let read = stream.read(&mut request).unwrap_or(0);
                    let path = String::from_utf8_lossy(&request[..read])
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_owned();
                    let routes = if worker_switched.load(Ordering::SeqCst) {
                        &routes_b
                    } else {
                        &routes_a
                    };
                    let (status, body) = routes
                        .iter()
                        .find(|(route, _, _)| *route == path)
                        .map(|(_, status, body)| (*status, body.clone()))
                        .unwrap_or((404, b"not found".to_vec()));
                    let reason = match status {
                        200 => "OK",
                        404 => "Not Found",
                        500 => "Internal Server Error",
                        _ => "Error",
                    };
                    let headers = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(headers.as_bytes());
                    let _ = stream.write_all(&body);
                    let _ = stream.flush();
                }
            });
            Self { base, stop, switched }
        }

        fn switch(&self) {
            self.switched.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl Drop for SwitchingFixtureServer {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// S2 : la page complète prépare une histoire à N nœuds ordonnés, tous
    /// médias téléchargés et promus, et le commit persiste l'histoire +
    /// assets + provenance `web` avec les fichiers présents sur disque.
    #[test]
    fn test_prepare_web_story_creation_builds_an_ordered_multi_episode_story() {
        let server = start_fixture_http_server(|base| {
            let mut routes =
                vec![("/page".to_owned(), 200, multi_episode_page_html(base).into_bytes())];
            routes.extend(s2_media_routes());
            routes
        });
        let url = format!("{}/page", server.base);
        let preview = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect("the fixture page must preview");
        assert_eq!(preview.items.len(), 3);

        let store = tempfile::TempDir::new().expect("store root");
        let phase = prepare_web_story_creation(
            official_content_sources(),
            &url,
            &preview.page_checksum,
            Duration::from_secs(30),
            Some(store.path()),
        )
        .expect("an unchanged page must prepare, never fail");
        let WebAcceptPhase::Prepared(prepared) = phase else {
            panic!("an unchanged page must be Prepared, not SourceChanged");
        };

        assert_eq!(prepared.commit.source_name, "127.0.0.1");
        assert_eq!(prepared.commit.title, "Histoire de 127.0.0.1");
        assert_eq!(prepared.commit.state, ImportState::NeedsReview);
        let expected_artifact = crate::domain::story::content_checksum_bytes(
            multi_episode_page_html(&server.base).as_bytes(),
        );
        assert_eq!(
            prepared.commit.artifact_checksum, expected_artifact,
            "the provenance must fingerprint the re-fetched page bytes"
        );

        let structure: CanonicalStructure =
            serde_json::from_str(&prepared.commit.structure_json).expect("canonical structure");
        assert_eq!(structure.start_node_id, "n1");
        assert_eq!(
            structure.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["n1", "n2", "n3"],
            "the nodes must be numbered in page order"
        );
        assert_eq!(
            structure.nodes.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(),
            vec!["Épisode un", "Épisode deux", "Épisode trois"],
            "each node carries ITS OWN title — the collection title never replaces it"
        );
        assert_eq!(structure.nodes[0].text, "Résumé un.");
        assert!(
            structure.nodes[0].image_asset_id.is_some(),
            "ep1 image is provided by the page"
        );
        assert!(
            structure.nodes[1].image_asset_id.is_none(),
            "the image-less episode must stay image-less (S3)"
        );
        assert!(structure.nodes[2].image_asset_id.is_some());
        assert!(
            structure.nodes.iter().all(|n| n.audio_asset_id.is_some()),
            "every audio must be downloaded and linked"
        );

        assert_eq!(prepared.assets.len(), 5, "3 audio + 2 images");
        // Captures BEFORE the commit consumes the prepared creation.
        let structure_json = prepared.commit.structure_json.clone();
        let promoted_paths: Vec<std::path::PathBuf> =
            prepared.assets.iter().map(|a| a.promoted_path.clone()).collect();
        let audio_ids: Vec<String> =
            serde_json::from_str::<CanonicalStructure>(&structure_json)
                .expect("parse again")
                .nodes
                .iter()
                .map(|n| n.audio_asset_id.clone().expect("audio linked"))
                .collect();

        let mut db = crate::infrastructure::db::open_in_memory().expect("in-memory db");
        crate::infrastructure::db::run_migrations(&mut db).expect("migrate");
        let card = commit_web_story_creation(&mut db, *prepared).expect("commit");
        assert_eq!(card.title, "Histoire de 127.0.0.1");

        let conn = db.conn();
        let story_count: u32 = conn
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count stories");
        assert_eq!(story_count, 1);
        let (source_format, source_name, import_state): (String, String, String) = conn
            .query_row(
                "SELECT source_format, source_name, import_state \
                 FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![card.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("provenance row");
        assert_eq!((source_format.as_str(), source_name.as_str()), ("web", "127.0.0.1"));
        assert_eq!(import_state, "needs_review");
        let asset_count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE story_id = ?1",
                rusqlite::params![card.id],
                |row| row.get(0),
            )
            .expect("count assets");
        assert_eq!(asset_count, 5);
        for audio_id in &audio_ids {
            let linked: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM assets WHERE story_id = ?1 AND id = ?2)",
                    rusqlite::params![card.id, audio_id],
                    |row| row.get(0),
                )
                .expect("audio asset row");
            assert!(linked, "node audio {audio_id} must have its assets row");
        }
        for path in &promoted_paths {
            assert!(path.exists(), "promoted file missing on disk: {path:?}");
        }
        drop(server);
    }

    /// Une page divergée entre le preview et l'accept doit être refusée
    /// honnêtement (`SourceChanged`) avec ZÉRO mutation : rien n'est
    /// téléchargé, rien n'est promu, rien n'est écrit.
    #[test]
    fn test_prepare_web_story_creation_refuses_source_changed_with_zero_mutation() {
        let store = tempfile::TempDir::new().expect("store root");
        let switching = SwitchingFixtureServer::start(|base| {
            let full = vec![("/page".to_owned(), 200, multi_episode_page_html(base).into_bytes())];
            let shortened = vec![(
                "/page".to_owned(),
                200,
                multi_episode_page_html_shortened(base).into_bytes(),
            )];
            let mut routes_a = full;
            routes_a.extend(s2_media_routes());
            let mut routes_b = shortened;
            routes_b.extend(s2_media_routes());
            (routes_a, routes_b)
        });
        let url = format!("{}/page", switching.base);
        let preview = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect("preview body A");
        assert_eq!(preview.items.len(), 3);

        switching.switch();
        let phase = prepare_web_story_creation(
            official_content_sources(),
            &url,
            &preview.page_checksum,
            Duration::from_secs(30),
            Some(store.path()),
        )
        .expect("a diverged page is an honest refusal, never an error");
        assert!(
            matches!(phase, WebAcceptPhase::SourceChanged),
            "the diverged page must refuse as SourceChanged"
        );

        // Zero mutation: no promoted file may exist.
        let media_dir = store.path().join("node-media");
        let promoted: Vec<String> = std::fs::read_dir(&media_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            promoted.is_empty(),
            "a SourceChanged refusal must promote nothing, got: {promoted:?}"
        );
        drop(switching);
    }

    /// S6 côté accept : une page accessible SANS aucun média audio refuse
    /// avec le signalement dédié, avant tout téléchargement ou checksum.
    #[test]
    fn test_prepare_web_story_creation_reports_no_audio_media() {
        let server = start_fixture_http_server(|_| {
            let page = "<html><head><title>Page sans média</title></head>\
                        <body><h1>Page sans média</h1></body></html>"
                .to_owned()
                .into_bytes();
            vec![("/page".to_owned(), 200, page)]
        });
        let url = format!("{}/page", server.base);
        let err = prepare_web_story_creation(
            official_content_sources(),
            &url,
            "checksum-inutile",
            Duration::from_secs(30),
            None,
        )
        .expect_err("a page without any audio must refuse with the S6 report");
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::ImportFailed
        );
        assert_eq!(err.message, "Aucun média audio n'a été trouvé.");
        let value = serde_json::to_value(&err).expect("ser");
        assert_eq!(value["details"]["stage"], "no_audio_media");
    }

    /// Un téléchargement audio ÉCHOUÉ (404) laisse son nœud sans audio,
    /// porte le finding honnête `(Media, Missing)`, dérive l'état `partial`
    /// et l'histoire reste créée — le précédent RSS d'échec partiel.
    #[test]
    fn test_prepare_web_story_creation_reports_a_missing_media_honestly() {
        let server = start_fixture_http_server(|base| {
            let mut routes =
                vec![("/page".to_owned(), 200, multi_episode_page_html(base).into_bytes())];
            // /media/ep2.m4a n'est PAS servi → 404.
            routes.extend(
                s2_media_routes()
                    .into_iter()
                    .filter(|(route, _, _)| route != "/media/ep2.m4a"),
            );
            routes
        });
        let url = format!("{}/page", server.base);
        let preview = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect("the preview never fetches media");

        let store = tempfile::TempDir::new().expect("store root");
        let phase = prepare_web_story_creation(
            official_content_sources(),
            &url,
            &preview.page_checksum,
            Duration::from_secs(30),
            Some(store.path()),
        )
        .expect("a failed media download degrades to a verdict, never an error");
        let WebAcceptPhase::Prepared(prepared) = phase else {
            panic!("a partial media set must still be Prepared");
        };

        assert_eq!(
            prepared.commit.state,
            ImportState::Partial,
            "the honest (Media, Missing) finding must derive partial"
        );
        assert!(
            prepared.commit
                .findings
                .iter()
                .any(|f| {
                    f.aspect == RecognitionAspect::Media
                        && f.category
                            == crate::domain::import::RecognitionCategory::Missing
                }),
            "the missing media must carry its (Media, Missing) finding"
        );

        let structure: CanonicalStructure =
            serde_json::from_str(&prepared.commit.structure_json).expect("canonical structure");
        assert!(structure.nodes[0].audio_asset_id.is_some());
        assert!(
            structure.nodes[1].audio_asset_id.is_none(),
            "the failed download must leave its node audio-less"
        );
        assert!(structure.nodes[2].audio_asset_id.is_some());
        assert_eq!(prepared.assets.len(), 4, "2 audio + 2 images");

        let mut db = crate::infrastructure::db::open_in_memory().expect("in-memory db");
        crate::infrastructure::db::run_migrations(&mut db).expect("migrate");
        let card = commit_web_story_creation(&mut db, *prepared).expect("commit");
        let import_state: String = db
            .conn()
            .query_row(
                "SELECT import_state FROM story_local_imports WHERE story_id = ?1",
                rusqlite::params![card.id],
                |row| row.get(0),
            )
            .expect("provenance row");
        assert_eq!(import_state, "partial");
        drop(server);
    }

    /// L'échec d'une image ne compte dans AUCUN verdict (S3) : l'état reste
    /// `needs_review`, aucun finding `Missing` n'apparaît, le nœud perd son
    /// image et tout le reste est importé.
    #[test]
    fn test_prepare_web_story_creation_counts_no_image_failure_in_no_verdict() {
        let server = start_fixture_http_server(|base| {
            let mut routes =
                vec![("/page".to_owned(), 200, multi_episode_page_html(base).into_bytes())];
            // /images/ep1.png n'est PAS servi → 404.
            routes.extend(
                s2_media_routes()
                    .into_iter()
                    .filter(|(route, _, _)| route != "/images/ep1.png"),
            );
            routes
        });
        let url = format!("{}/page", server.base);
        let preview = preview_web_podcast(
            official_content_sources(),
            &url,
            Duration::from_secs(30),
        )
        .expect("the preview never fetches media");

        let store = tempfile::TempDir::new().expect("store root");
        let phase = prepare_web_story_creation(
            official_content_sources(),
            &url,
            &preview.page_checksum,
            Duration::from_secs(30),
            Some(store.path()),
        )
        .expect("a failed image download degrades silently, never an error");
        let WebAcceptPhase::Prepared(prepared) = phase else {
            panic!("an image failure must still be Prepared");
        };

        assert_eq!(
            prepared.commit.state,
            ImportState::NeedsReview,
            "an image failure must not degrade the state"
        );
        assert!(
            prepared.commit.findings.iter().all(|f| {
                f.category != crate::domain::import::RecognitionCategory::Missing
            }),
            "no finding may report a missing image"
        );

        let structure: CanonicalStructure =
            serde_json::from_str(&prepared.commit.structure_json).expect("canonical structure");
        assert!(structure.nodes[0].image_asset_id.is_none());
        assert!(structure.nodes[2].image_asset_id.is_some());
        assert!(structure.nodes.iter().all(|n| n.audio_asset_id.is_some()));
        assert_eq!(prepared.assets.len(), 4, "3 audio + 1 image");
        drop(server);
    }
}
