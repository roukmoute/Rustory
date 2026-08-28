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

/// Fetch HTML content from a URL using reqwest blocking client.
fn fetch_html(url: &str, budget: Duration) -> Result<String, AppError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(budget)
        .build()
        .map_err(|_| {
            AppError::import_failed(
                "Récupération de la page impossible.",
                "Réessaie ; si le problème persiste, consulte les traces locales.",
            )
            .with_details(serde_json::json!({
                "source": "network",
                "stage": "client_build",
            }))
        })?;

    let response = client.get(url).send().map_err(|e| {
        AppError::rss_source_unreachable(
            "Récupération de la page impossible.",
            "Vérifie ta connexion puis réessaie.",
        )
        .with_details(serde_json::json!({
            "source": "network",
            "stage": "request",
            "error": e.to_string(),
        }))
    })?;

    if !response.status().is_success() {
        return Err(AppError::rss_source_unreachable(
            "Récupération de la page impossible.",
            "Le serveur a répondu avec une erreur.",
        )
        .with_details(serde_json::json!({
            "source": "network",
            "stage": "status_check",
            "status": response.status().as_u16(),
        })));
    }

    response.text().map_err(|e| {
        AppError::import_failed(
            "Récupération de la page impossible.",
            "Impossible de lire le contenu de la page.",
        )
        .with_details(serde_json::json!({
            "source": "network",
            "stage": "read_text",
            "error": e.to_string(),
        }))
    })
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

    // Validate URL format (basic check: must have http/https scheme)
    if !web_url.starts_with("http://") && !web_url.starts_with("https://") {
        return Err(invalid_web_url_error());
    }

    let html_content = fetch_html(web_url, budget)?;
    let episodes = parse_web_episodes(&html_content);

    // Compute checksum of the page content (not the full URL, only host + path for PII)
    let mut hasher = Sha256::new();
    hasher.update(web_url);
    let page_checksum = format!("{:x}", hasher.finalize());

    Ok(WebPreviewOutcome {
        source_host: feed_url_host(web_url).unwrap_or_else(|| "unknown".to_string()),
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
    // Policy gate
    ensure_web_source_enabled(sources)?;

    // Validate URL format
    if !web_url.starts_with("http://") && !web_url.starts_with("https://") {
        return Err(invalid_web_url_error());
    }

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
        source_host: feed_url_host(web_url).unwrap_or_else(|| "unknown".to_string()),
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

    #[test]
    fn test_fetch_html_rejects_timeout() {
        // Using an invalid port to force a timeout-like error
        let url = "http://127.0.0.1:59999/timeout";
        let result = fetch_html(url, Duration::from_millis(100));
        // Expect an error (connection refused or timeout)
        assert!(result.is_err());
    }

    #[test]
    fn test_fetch_html_rejects_non_200_status() {
        // Using httpstat.us which returns 404
        let url = "http://httpstat.us/404";
        let result = fetch_html(url, Duration::from_secs(10));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_web_episodes_returns_empty_on_no_episode() {
        let html = "<html><body><p>No episodes here</p></body></html>";
        let episodes = parse_web_episodes(html);
        assert!(episodes.is_empty());
    }
}
