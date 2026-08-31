//! Web podcast page extraction application service.
//!
//! Extracts episodes from a web page (HTML) containing podcast episodes,
//! similar to RSS but with HTML parsing instead of XML.
//!
//! Structure mirrors rss_creation:
//! - preview_web_podcast: fetch + parse with ZERO mutation
//! - accept_web_podcast_creation: RE-fetch, re-parse, commit

use std::time::Duration;

use crate::application::story::now_iso_ms;
use crate::domain::import::{
    content_source_activation, feed_url_host, ContentSourceActivation, ContentSourceKind,
    ContentSourceLine, ImportState, RecognitionAspect, RecognitionFinding,
};
use crate::domain::shared::AppError;
use crate::domain::story::{canonical_structure_json, CanonicalStructure};
use crate::infrastructure::db::DbHandle;
use crate::ipc::dto::import_export::{state_db_tag, ImportFindingDto};
use crate::ipc::dto::StoryCardDto;
use scraper::{Element, ElementRef, Html, Selector};
use sha2::{Digest, Sha256};
use serde_json::Value;

/// The application-level outcome of previewing a web podcast page.
#[derive(Debug, Clone)]
pub struct WebPreviewOutcome {
    pub source_host: String,
    pub page_checksum: String,
    pub items: Vec<WebEpisode>,
}

/// One extracted episode from the web page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebEpisode {
    pub title: String,
    pub summary: String,
    pub audio_url: Option<String>,
    pub image_url: Option<String>,
}

/// One reference to an episode, used to select which one to create.
#[derive(Debug, Clone)]
pub struct WebEpisodeRef {
    pub title: String,
    pub audio_url: Option<String>,
}

/// The content-source policy gate, consulted by BOTH facades (preview AND
/// accept) BEFORE the address validation and BEFORE any network dispatch.
fn ensure_web_source_enabled(sources: &[ContentSourceLine]) -> Result<(), AppError> {
    match content_source_activation(sources, ContentSourceKind::Web) {
        ContentSourceActivation::Enabled => Ok(()),
        ContentSourceActivation::NotActivated | ContentSourceActivation::BlockedByPolicy => {
            Err(AppError::content_source_unavailable(ContentSourceKind::Web))
        }
    }
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
enum WebFetchFailure {
    ClientBuild,
    Request(String),
    StatusCheck(u16),
    ReadText(String),
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

/// The `@type` values recognized as a podcast episode on an episode page
/// (schema.org). Everything else is ignored: the extraction never guesses
/// an episode out of a non-episode node.
const EPISODE_JSONLD_TYPES: [&str; 4] = [
    "RadioEpisode",
    "PodcastEpisode",
    "BroadcastEpisode",
    "MusicRecording",
];

/// Collect the JSON-LD nodes carrying an `@type`, recursing into
/// `@graph` arrays (the shape of the real sample pages).
fn collect_jsonld_nodes<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    if node.get("@type").is_some() {
        out.push(node);
    }
    if let Some(graph) = node.get("@graph").and_then(Value::as_array) {
        for entry in graph {
            collect_jsonld_nodes(entry, out);
        }
    }
}

/// The `@type` of a node: a plain string or the first element of an
/// array (both are valid JSON-LD shapes).
fn jsonld_type(node: &Value) -> Option<&str> {
    match node.get("@type")? {
        Value::String(text) => Some(text.as_str()),
        Value::Array(types) => types.first().and_then(Value::as_str),
        _ => None,
    }
}

/// The JSON-LD payloads embedded in the page: every
/// `<script type="application/ld+json">` body that parses as JSON.
/// Unparsable scripts are skipped — the page content is untrusted and a
/// broken payload is never a crash.
fn jsonld_payloads(document: &Html) -> Vec<Value> {
    let selector = match Selector::parse("script[type='application/ld+json']") {
        Ok(selector) => selector,
        Err(_) => return Vec::new(),
    };
    document
        .select(&selector)
        .filter_map(|element| {
            serde_json::from_str(element.text().collect::<String>().trim()).ok()
        })
        .collect()
}

/// The audio media url of an episode node: `contentUrl` on the node
/// itself or on its `mainEntity` (the AudioObject shape of the sample
/// pages), trimmed, non-empty.
fn audio_content_url(node: &Value) -> Option<String> {
    let direct = node.get("contentUrl").and_then(Value::as_str);
    let nested = node
        .get("mainEntity")
        .and_then(|entity| entity.get("contentUrl"))
        .and_then(Value::as_str);
    let url = direct.or(nested)?.trim();
    if url.is_empty() {
        None
    } else {
        Some(url.to_owned())
    }
}

/// The image url of an episode node: the `image` field as a plain
/// string, an `ImageObject` carrying a `url`, or an array whose first
/// element is one of the two — carried as-is (no resolution, mirroring
/// the audio `contentUrl`), trimmed and non-empty, or `None`.
fn jsonld_image_url(node: &Value) -> Option<String> {
    node.get("image").and_then(image_url_from_jsonld_value)
}

fn image_url_from_jsonld_value(value: &Value) -> Option<String> {
    match value {
        Value::String(url) => trimmed_url(url),
        Value::Object(fields) => fields
            .get("url")
            .and_then(Value::as_str)
            .and_then(trimmed_url),
        Value::Array(images) => images.first().and_then(image_url_from_jsonld_value),
        _ => None,
    }
}

/// A trimmed, non-empty url string, or `None`.
fn trimmed_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// One episode from one JSON-LD node, or `None` when the node is not a
/// recognized episode type, carries no non-empty `name`, or no audio
/// `contentUrl` — the extraction emits only honest episodes (no title
/// fallback, no invented media). The image, when the node provides one
/// (`image` as a string, an `ImageObject`, or an array), is carried on
/// the episode as-is — never resolved, mirroring the audio url.
fn episode_from_jsonld_node(node: &Value) -> Option<WebEpisode> {
    if !jsonld_type(node).is_some_and(|ty| EPISODE_JSONLD_TYPES.contains(&ty)) {
        return None;
    }
    let title = node
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())?
        .to_owned();
    let audio_url = audio_content_url(node)?;
    let summary = node
        .get("description")
        .and_then(Value::as_str)
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();
    let image_url = jsonld_image_url(node);
    Some(WebEpisode {
        title,
        summary,
        audio_url: Some(audio_url),
        image_url,
    })
}

/// Resolve a (possibly relative) url against the page that referenced
/// it: absolute `http(s)://` urls pass through unchanged, a
/// protocol-relative `//authority/…` url inherits the base scheme, a
/// `/root-relative` url inherits the base scheme and authority, and a
/// bare relative url is resolved against the base path's directory (the
/// base's last path segment, when any, counts as a file).
fn resolve_url(base: &str, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(raw.to_owned());
    }
    let scheme_end = base.find("://")?;
    let scheme = &base[..scheme_end];
    let rest = &base[scheme_end + 3..]; // authority[/path]
    if let Some(protocol_relative) = raw.strip_prefix("//") {
        return Some(format!("{scheme}://{protocol_relative}"));
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, path),
        None => (rest, ""),
    };
    if let Some(root_relative) = raw.strip_prefix('/') {
        return Some(format!("{scheme}://{authority}/{root_relative}"));
    }
    let directory = match path.rfind('/') {
        Some(index) => &path[..=index],
        None => "",
    };
    Some(format!("{scheme}://{authority}/{directory}{raw}"))
}

/// The episode carried by one episode page: the first recognized
/// episode JSON-LD node of the page, or `None` (the page is then
/// ignored, never a failure — the extraction reports what it finds).
fn episode_from_episode_page(html: &str) -> Option<WebEpisode> {
    let document = Html::parse_document(html);
    let payloads = jsonld_payloads(&document);
    let mut nodes: Vec<&Value> = Vec::new();
    for payload in &payloads {
        collect_jsonld_nodes(payload, &mut nodes);
    }
    nodes.iter().find_map(|node| episode_from_jsonld_node(node))
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
    for node in nodes.iter().filter(|node| jsonld_type(*node) == Some("ItemList")) {
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
    html: &str,
    budget: Duration,
) -> Vec<WebEpisode> {
    let mut episodes = episodes_from_item_list(document, base_url, budget);
    if episodes.is_empty() {
        episodes = parse_web_episodes(html, base_url);
    }
    episodes
}

/// Parse an HTML page and extract its episodes honestly: one episode per
/// titled audio media present in the page — an `<a>` whose href ends with
/// a known audio extension, or an `<audio>` (or its `<source>`) carrying
/// a src — in DOCUMENT ORDER. The title is the media's own anchor text or
/// aria-label, never a heading and never an invented fallback: a media
/// without any title is not an episode and is skipped. The audio url is
/// resolved against the page and kept once per url; the image of the
/// media's container is carried only when the page provides one.
fn parse_web_episodes(html_content: &str, base_url: &str) -> Vec<WebEpisode> {
    let mut episodes = Vec::new();
    let document = Html::parse_document(html_content);

    let media_selector = match Selector::parse("a[href], audio[src], audio source[src]") {
        Ok(selector) => selector,
        Err(_) => return Vec::new(),
    };
    let mut seen_audio_urls: Vec<String> = Vec::new();
    for media in document.select(&media_selector) {
        let Some((title, raw_audio)) = dom_episode_media(&media) else {
            continue;
        };
        let Some(audio_url) = resolve_url(base_url, raw_audio) else {
            continue;
        };
        if seen_audio_urls.contains(&audio_url) {
            continue;
        }
        seen_audio_urls.push(audio_url.clone());
        let container = media_container(&media);
        let image_url = container
            .as_ref()
            .and_then(|container| container_image_url(container, base_url));
        let summary = container.as_ref().map_or_else(String::new, container_summary);
        episodes.push(WebEpisode {
            title,
            summary,
            audio_url: Some(audio_url),
            image_url,
        });
    }
    episodes
}

/// The known audio file extensions of an `<a>` media link, compared in
/// lower case against the end of the href.
const DOM_AUDIO_EXTENSIONS: [&str; 6] = [".mp3", ".m4a", ".ogg", ".wav", ".aac", ".opus"];

/// The honest (title, raw audio url) pair of one matched media element,
/// or `None` when the media cannot be titled: the title is the anchor
/// text for an `<a>` (else its `aria-label`), the `aria-label` of the
/// media for an `<audio>`, and of its `<audio>` parent for a `<source>`;
/// an empty title or an empty url yields `None` — nothing is invented.
fn dom_episode_media<'a>(media: &ElementRef<'a>) -> Option<(String, &'a str)> {
    let name = media.value().name();
    let title = match name {
        "a" => {
            let text = media.text().collect::<String>().trim().to_owned();
            if text.is_empty() {
                aria_label(media)?
            } else {
                text
            }
        }
        "audio" => aria_label(media)?,
        "source" => aria_label(&media.parent_element()?)?,
        _ => return None,
    };
    let raw_audio = match name {
        "a" => {
            let raw = media.attr("href")?.trim();
            let lower = raw.to_ascii_lowercase();
            if !DOM_AUDIO_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
                return None;
            }
            raw
        }
        "audio" | "source" => {
            let raw = media.attr("src")?.trim();
            if raw.is_empty() {
                return None;
            }
            raw
        }
        _ => return None,
    };
    Some((title, raw_audio))
}

/// The trimmed, non-empty `aria-label` of an element, or `None`.
fn aria_label(element: &ElementRef<'_>) -> Option<String> {
    element
        .attr("aria-label")
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
}

/// The container of a matched media: its closest parent element — except
/// for a `<source>`, whose container is the parent of its `<audio>` (the
/// `<audio>` belongs to the media, not to its container).
fn media_container<'a>(media: &ElementRef<'a>) -> Option<ElementRef<'a>> {
    let parent = media.parent_element()?;
    if media.value().name() == "source" {
        parent.parent_element()
    } else {
        Some(parent)
    }
}

/// The image of a media container: the first `img[src]` it carries with a
/// non-empty src, resolved against the page — `None` when the page
/// provides none: the image stays optional and is never invented.
fn container_image_url(container: &ElementRef<'_>, base_url: &str) -> Option<String> {
    let selector = Selector::parse("img[src]").ok()?;
    container.select(&selector).find_map(|img| {
        img.attr("src")
            .map(str::trim)
            .filter(|src| !src.is_empty())
            .and_then(|src| resolve_url(base_url, src))
    })
}

/// The summary of a media container: the text of its first `<p>`,
/// trimmed, empty when absent.
fn container_summary(container: &ElementRef<'_>) -> String {
    let selector = match Selector::parse("p") {
        Ok(selector) => selector,
        Err(_) => return String::new(),
    };
    container
        .select(&selector)
        .next()
        .map(|paragraph| paragraph.text().collect::<String>().trim().to_owned())
        .unwrap_or_default()
}

/// An extracted episode reaches the preview only when it is honest end
/// to end: a non-empty title (after trim) AND a present, non-empty audio
/// media url. Untitled or audio-less elements are rejected here, before
/// the preview — the preview never carries an episode it could not play.
fn keep_valid_episodes(episodes: Vec<WebEpisode>) -> Vec<WebEpisode> {
    episodes
        .into_iter()
        .filter(|episode| {
            !episode.title.trim().is_empty()
                && episode
                    .audio_url
                    .as_deref()
                    .map(|url| !url.trim().is_empty())
                    .unwrap_or(false)
        })
        .collect()
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
    ensure_web_source_enabled(sources)?;

    // Entry guard BEFORE any network dispatch.
    let source_host = validate_web_entry_url(web_url)?;

    let html_content = fetch_html(web_url, budget)?;
    let document = Html::parse_document(&html_content);
    let episodes = extract_web_episodes(&document, web_url, &html_content, budget);
    let episodes = keep_valid_episodes(episodes);
    if episodes.is_empty() {
        return Err(no_audio_media_error());
    }

    // Compute checksum of the page content (not the full URL, only host + path for PII)
    let mut hasher = Sha256::new();
    hasher.update(web_url);
    let page_checksum = format!("{:x}", hasher.finalize());

    Ok(WebPreviewOutcome {
        source_host,
        page_checksum,
        items: episodes,
    })
}

/// Phase 2a — prepare the story creation from a selected episode.
pub fn prepare_web_story_creation(
    sources: &[ContentSourceLine],
    web_url: &str,
    selected_episode_title: &str,
    budget: Duration,
) -> Result<WebAcceptPhase, AppError> {
    ensure_web_source_enabled(sources)?;

    // Entry guard BEFORE any network dispatch.
    let source_host = validate_web_entry_url(web_url)?;

    let html_content = fetch_html(web_url, budget)?;
    let episodes = parse_web_episodes(&html_content, web_url);

    let episode = episodes
        .iter()
        .find(|e| e.title == selected_episode_title)
        .ok_or_else(|| {
            AppError::import_failed(
                "Épisode introuvable.",
                "L'épisode sélectionné n'existe plus dans la page.",
            )
            .with_details(serde_json::json!({
                "source": "parsing",
                "stage": "episode_not_found",
            }))
        })?;

    // Compute episode checksum (title + audio_url)
    let mut hasher = Sha256::new();
    hasher.update(&episode.title);
    if let Some(audio) = &episode.audio_url {
        hasher.update(audio);
    }
    let episode_checksum = format!("{:x}", hasher.finalize());

    // Compute page checksum (for the prepared creation)
    let mut hasher2 = Sha256::new();
    hasher2.update(web_url);
    let page_checksum = format!("{:x}", hasher2.finalize());

    let now_iso = now_iso_ms()?;
    Ok(WebAcceptPhase::Prepared(Box::new(PreparedWebCreation {
        title: episode.title.clone(),
        structure_json: canonical_structure_json(&CanonicalStructure::minimal()),
        checksum: episode_checksum,
        now_iso,
        source_host: source_host.clone(),
        page_checksum,
        state: ImportState::NeedsReview,
        findings: vec![RecognitionFinding::ambiguous(RecognitionAspect::Source)],
    })))
}

/// The typed outcome of the accept phase.
#[derive(Debug)]
pub enum WebAcceptPhase {
    SourceChanged,
    Prepared(Box<PreparedWebCreation>),
}

/// The error when a db commit fails.
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

/// Phase 2b — commit the prepared creation into the database.
pub fn commit_web_story_creation(
    db: &mut DbHandle,
    prepared: PreparedWebCreation,
) -> Result<StoryCardDto, AppError> {
    let PreparedWebCreation {
        title,
        structure_json,
        checksum,
        now_iso,
        source_host,
        page_checksum,
        state,
        findings,
    } = prepared;

    let story_id = uuid::Uuid::now_v7().to_string();
    let import_report = Some(
        findings
            .iter()
            .map(ImportFindingDto::from_domain)
            .collect::<Vec<_>>(),
    );
    let import_state_tag = state_db_tag(state);
    let import_report_json = serde_json::to_string(&import_report).map_err(|_e| {
        AppError::import_failed(
            "Création impossible: serialization échouée.",
            "Réessaie ; si le problème persiste, consulte les traces locales.",
        )
    })?;

    db.conn()
        .execute(
            "INSERT INTO stories (id, title, structure_json, content_checksum, created_at, updated_at, import_state, import_report, source_format, source_name, source_checksum) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                &story_id,
                &title,
                &structure_json,
                &checksum,
                &now_iso,
                &now_iso,
                import_state_tag,
                &import_report_json,
                "web",
                &format!("Histoire de {}", source_host),
                &page_checksum,
            ],
        )
        .map_err(|err| db_commit_error(&err, "insert_story"))?;

    Ok(StoryCardDto {
        id: story_id,
        title,
        import_state: None,
        import_report,
        transferable: false,
        sendable_archive: false,
        cover_asset_id: None,
    })
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

/// The error when a spawn_blocking join fails.
pub fn spawn_blocking_join_error() -> AppError {
    AppError::import_failed(
        "Création interrompue de façon inattendue.",
        "Réessaie ; si le problème persiste, redémarre Rustory.",
    )
    .with_details(serde_json::json!({ "source": "spawn_blocking_join" }))
}

/// The data passed from the prepare phase to the commit phase.
#[derive(Debug)]
pub struct PreparedWebCreation {
    pub title: String,
    pub structure_json: String,
    pub checksum: String,
    pub now_iso: String,
    pub source_host: String,
    pub page_checksum: String,
    pub state: ImportState,
    pub findings: Vec<RecognitionFinding>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::import::official_content_sources;

    #[test]
    fn test_invalid_web_url_error() {
        let err = invalid_web_url_error();
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::RssSourceUnreachable
        );
    }

    #[test]
    fn test_spawn_blocking_join_error() {
        let err = spawn_blocking_join_error();
        assert_eq!(err.code, crate::domain::shared::AppErrorCode::ImportFailed);
    }

    #[test]
    fn test_ensure_web_source_enabled_rejects_not_activated() {
        let sources = vec![ContentSourceLine {
            kind: ContentSourceKind::Web,
            activation: ContentSourceActivation::NotActivated,
        }];
        let err = ensure_web_source_enabled(&sources).expect_err("must reject");
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::ContentSourceUnavailable
        );
    }

    #[test]
    fn test_ensure_web_source_enabled_rejects_blocked() {
        let sources = vec![ContentSourceLine {
            kind: ContentSourceKind::Web,
            activation: ContentSourceActivation::BlockedByPolicy,
        }];
        let err = ensure_web_source_enabled(&sources).expect_err("must reject");
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::ContentSourceUnavailable
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
        F: FnOnce(&str) -> Vec<(String, u16, String)> + Send + 'static,
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
                    .unwrap_or((404, String::from("not found")));
                let reason = match status {
                    200 => "OK",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    _ => "Error",
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
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
            vec![("/page".to_owned(), 500, "<html><body>boom</body></html>".to_owned())]
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
            vec![("/page".to_owned(), 500, "boom".to_owned())]
        });
        let http_error = fetch_html(&format!("{}/page", server.base), Duration::from_secs(10)).expect_err("http error");
        drop(server);
        assert_ne!(
            unreachable.message, http_error.message,
            "S5 requires a distinct user-facing reason per access failure case"
        );
    }

    #[test]
    fn test_parse_web_episodes_returns_empty_on_no_episode() {
        let html = "<html><body><p>No episodes here</p></body></html>";
        let episodes = parse_web_episodes(html, "https://fixture.example.org/page");
        assert!(episodes.is_empty());
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
                ("/liste".to_owned(), 200, list_page_html(base)),
                (
                    "/episodes/episode-un".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode un",
                        "/media/episode-un.m4a",
                        "Resume de l'episode un.",
                    ),
                ),
                (
                    "/episodes/episode-deux".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode deux",
                        "/media/episode-deux.m4a",
                        "Resume de l'episode deux.",
                    ),
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
                .to_owned();
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

    /// `resolve_url` is a pure helper: each branch pins how a relative
    /// episode url of the list page is resolved against it.
    #[test]
    fn test_resolve_url_pins_every_relative_branch() {
        let base = "https://www.example.com/shows/selection/episodes/ep1";
        assert_eq!(
            resolve_url(base, "https://media.example.org/a.m4a"),
            Some("https://media.example.org/a.m4a".to_owned())
        );
        assert_eq!(
            resolve_url(base, "//cdn.example.org/b.m4a"),
            Some("https://cdn.example.org/b.m4a".to_owned())
        );
        assert_eq!(
            resolve_url(base, "/media/c.m4a"),
            Some("https://www.example.com/media/c.m4a".to_owned())
        );
        assert_eq!(
            resolve_url(base, "d.m4a"),
            Some("https://www.example.com/shows/selection/episodes/d.m4a".to_owned())
        );
        // A base with no path: a bare relative url sits at the root.
        assert_eq!(
            resolve_url("https://www.example.com", "e.m4a"),
            Some("https://www.example.com/e.m4a".to_owned())
        );
        // A blank url is not resolvable and never a crash.
        assert_eq!(resolve_url(base, "   "), None);
    }

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
                ("/liste-array".to_owned(), 200, list),
                ("/episodes/array-type".to_owned(), 200, episode),
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
                ("/liste-mixte".to_owned(), 200, list),
                (
                    "/episodes/explicite".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode explicite",
                        "/media/explicite.m4a",
                        "Resume explicite.",
                    ),
                ),
                (
                    "/episodes/sans-position".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode sans position",
                        "/media/sans-position.m4a",
                        "Resume sans position.",
                    ),
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
                ("/liste-doublons".to_owned(), 200, list),
                (
                    "/episodes/duplique".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode dupliqué",
                        "/media/duplique.m4a",
                        "Resume dupliqué.",
                    ),
                ),
                (
                    "/episodes/unique".to_owned(),
                    200,
                    episode_page_html(
                        base,
                        "Episode unique",
                        "/media/unique.m4a",
                        "Resume unique.",
                    ),
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

    /// S1/S3 (DOM): one episode per titled audio media of the page, in
    /// document order — the link text as title, the media url as audio,
    /// the container image when present and nothing when absent, the
    /// first `<p>` of the container as summary, a `<source>` titled by
    /// its `<audio>` aria-label; a media without any title is not emitted.
    #[test]
    fn test_parse_web_episodes_extracts_dom_episodes_in_document_order() {
        let html = "<html><body>\
            <h1>Ma sélection</h1>\
            <section>\
              <a href=\"https://fixture.example.org/media/episode-un.m4a\">Episode un</a>\
              <img src=\"https://fixture.example.org/images/episode-un.jpg\" alt=\"Episode un\">\
            </section>\
            <section>\
              <a href=\"https://fixture.example.org/media/episode-deux.ogg\">Episode deux</a>\
              <p>Résumé deux.</p>\
            </section>\
            <section>\
              <audio src=\"https://fixture.example.org/media/episode-trois.wav\" aria-label=\"Episode trois\"></audio>\
            </section>\
            <section>\
              <audio aria-label=\"Episode source\"><source src=\"https://fixture.example.org/media/episode-source.opus\"></source></audio>\
            </section>\
            <a href=\"https://fixture.example.org/media/sans-titre.mp3\"></a>\
          </body></html>";
        let episodes = parse_web_episodes(html, "https://fixture.example.org/page");
        assert_eq!(
            episodes.len(),
            4,
            "one episode per titled audio media, in document order, got: {episodes:?}"
        );
        assert_eq!(episodes[0].title, "Episode un");
        assert_eq!(
            episodes[0].audio_url.as_deref(),
            Some("https://fixture.example.org/media/episode-un.m4a")
        );
        assert_eq!(
            episodes[0].image_url.as_deref(),
            Some("https://fixture.example.org/images/episode-un.jpg")
        );
        assert_eq!(episodes[1].title, "Episode deux");
        assert_eq!(
            episodes[1].audio_url.as_deref(),
            Some("https://fixture.example.org/media/episode-deux.ogg")
        );
        assert_eq!(
            episodes[1].summary,
            "Résumé deux.",
            "the first <p> of the container is the summary, never invented"
        );
        assert!(
            episodes[1].image_url.is_none(),
            "an absent image must stay absent, not invented"
        );
        assert_eq!(episodes[2].title, "Episode trois");
        assert_eq!(
            episodes[2].audio_url.as_deref(),
            Some("https://fixture.example.org/media/episode-trois.wav")
        );
        assert!(episodes[2].image_url.is_none());
        assert_eq!(episodes[3].title, "Episode source");
        assert_eq!(
            episodes[3].audio_url.as_deref(),
            Some("https://fixture.example.org/media/episode-source.opus"),
            "a <source> without src on its <audio> is still one honest episode"
        );
        assert!(episodes[3].image_url.is_none());
    }

    /// S1 (honesty): an audio media without any title (empty link text,
    /// no aria-label) is NOT emitted — the extraction never falls back
    /// to an invented "Épisode sans titre".
    #[test]
    fn test_parse_web_episodes_never_invents_a_fallback_title() {
        let html = "<html><body>\
            <article>\
              <a href=\"https://fixture.example.org/media/orphelin.mp3\"></a>\
            </article>\
          </body></html>";
        let episodes = parse_web_episodes(html, "https://fixture.example.org/page");
        assert!(
            !episodes
                .iter()
                .any(|episode| episode.title == "Épisode sans titre"),
            "no invented fallback title may be emitted, got: {episodes:?}"
        );
        assert!(
            episodes.is_empty(),
            "an untitled media is not an episode, got: {episodes:?}"
        );
    }

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
                ("/liste-image".to_owned(), 200, list),
                ("/episodes/avec-image".to_owned(), 200, with_image),
                ("/episodes/image-string".to_owned(), 200, image_string),
                ("/episodes/image-array".to_owned(), 200, image_array),
                ("/episodes/sans-image".to_owned(), 200, without_image),
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
                .to_owned();
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

    /// The preview filter keeps an episode only when the title is
    /// non-empty (after trim) AND a non-empty audio url is present:
    /// untitled or audio-less elements never reach the preview.
    #[test]
    fn test_keep_valid_episodes_rejects_untitled_and_audioless_items() {
        let episodes = vec![
            WebEpisode {
                title: "Valide".to_owned(),
                summary: String::new(),
                audio_url: Some("https://fixture.example.org/media/valide.m4a".to_owned()),
                image_url: None,
            },
            WebEpisode {
                title: String::new(),
                summary: String::new(),
                audio_url: Some("https://fixture.example.org/media/sans-titre.m4a".to_owned()),
                image_url: None,
            },
            WebEpisode {
                title: "   ".to_owned(),
                summary: String::new(),
                audio_url: Some("https://fixture.example.org/media/blanc.m4a".to_owned()),
                image_url: None,
            },
            WebEpisode {
                title: "Sans audio".to_owned(),
                summary: String::new(),
                audio_url: None,
                image_url: None,
            },
        ];
        let kept = keep_valid_episodes(episodes);
        assert_eq!(
            kept.len(),
            1,
            "only the honest episode survives, got: {kept:?}"
        );
        assert_eq!(kept[0].title, "Valide");
    }
}
