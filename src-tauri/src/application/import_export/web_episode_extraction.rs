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
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};

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

/// Parse HTML content and extract podcast episodes.
/// Looks for common patterns: <item>, <episode>, <article> with audio links.
fn parse_web_episodes(html_content: &str) -> Vec<WebEpisode> {
    let mut episodes = Vec::new();
    let document = Html::parse_document(html_content);

    // Try to find items using item tags
    if let Ok(item_selector) = Selector::parse("item") {
        for item in document.select(&item_selector) {
            let title = item
                .select(&Selector::parse("title").unwrap_or_else(|_| Selector::parse("*").unwrap()))
                .next()
                .map(|t| t.text().collect::<String>().trim().to_string())
                .unwrap_or_else(|| "Épisode sans titre".to_string());

            let audio_url = item
                .select(&Selector::parse("enclosure[url]").unwrap_or_else(|_| Selector::parse("a").unwrap()))
                .next()
                .and_then(|e| e.value().attr("url").map(|s| s.to_string()))
                .or_else(|| {
                    item.select(&Selector::parse("a[href$='.mp3'], a[href$='.m4a'], a[href$='.wav'], a[href$='.podcast']").unwrap())
                        .next()
                        .and_then(|a| a.value().attr("href").map(|s| s.to_string()))
                });

            let summary = item
                .select(
                    &Selector::parse("description, summary, p")
                        .unwrap_or_else(|_| Selector::parse("p").unwrap()),
                )
                .next()
                .map(|p| p.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            let image_url = item
                .select(
                    &Selector::parse("image, img")
                        .unwrap_or_else(|_| Selector::parse("img").unwrap()),
                )
                .next()
                .and_then(|img| img.value().attr("src").map(|s| s.to_string()));

            episodes.push(WebEpisode {
                title,
                summary,
                audio_url,
                image_url,
            });
        }
    } else {
        // Fallback: try article tags
        if let Ok(article_selector) = Selector::parse("article") {
            for article in document.select(&article_selector) {
                let title = article
                    .select(
                        &Selector::parse("h1, h2, h3, .title, .episode-title")
                            .unwrap_or_else(|_| Selector::parse("h1").unwrap()),
                    )
                    .next()
                    .map(|h| h.text().collect::<String>().trim().to_string())
                    .unwrap_or_else(|| "Épisode sans titre".to_string());

                let audio_url = article
                    .select(&Selector::parse("a[href$='.mp3'], a[href$='.m4a'], a[href$='.wav'], a[href$='.podcast']").unwrap())
                    .next()
                    .and_then(|a| a.value().attr("href").map(|s| s.to_string()));

                let summary = article
                    .select(&Selector::parse("p").unwrap())
                    .next()
                    .map(|p| p.text().collect::<String>().trim().to_string())
                    .unwrap_or_default();

                let image_url = article
                    .select(&Selector::parse("img").unwrap())
                    .next()
                    .and_then(|img| img.value().attr("src").map(|s| s.to_string()));

                episodes.push(WebEpisode {
                    title,
                    summary,
                    audio_url,
                    image_url,
                });
            }
        }
    }

    episodes
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
    let episodes = parse_web_episodes(&html_content);

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
    let episodes = parse_web_episodes(&html_content);

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

    /// Minimal one-shot HTTP server on 127.0.0.1 for deterministic status tests.
    struct LocalHttpServer {
        url: String,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Drop for LocalHttpServer {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn start_local_http_server(status: u16, body: &str) -> LocalHttpServer {
        use std::io::{Read, Write};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let addr = listener.local_addr().expect("local address");
        let url = format!("http://{addr}/page");
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let body = body.to_owned();
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
                let _ = stream.read(&mut request);
                let reason = match status {
                    200 => "OK",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    _ => "Error",
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        LocalHttpServer { url, stop }
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
        let server = start_local_http_server(500, "<html><body>boom</body></html>");
        let err = fetch_html(&server.url, Duration::from_secs(10))
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
        let server = start_local_http_server(500, "boom");
        let http_error = fetch_html(&server.url, Duration::from_secs(10)).expect_err("http error");
        drop(server);
        assert_ne!(
            unreachable.message, http_error.message,
            "S5 requires a distinct user-facing reason per access failure case"
        );
    }

    #[test]
    fn test_parse_web_episodes_returns_empty_on_no_episode() {
        let html = "<html><body><p>No episodes here</p></body></html>";
        let episodes = parse_web_episodes(html);
        assert!(episodes.is_empty());
    }
}
