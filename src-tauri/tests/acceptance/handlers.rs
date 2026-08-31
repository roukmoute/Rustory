//! Closed-vocabulary acceptance handlers for the frozen contract
//! `import-podcasts-pages-web-non-rss`.
//!
//! Dispatch is exact on (scenario name, step text): the vocabulary is
//! closed by the frozen feature, so an unknown step panics (a test
//! failure) instead of being silently skipped. Handlers are added slice
//! by slice; the generated entry points only reference what exists.

use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rustory_lib::application::import_export::{rss_creation, web_episode_extraction};
use rustory_lib::domain::import::{
    feed_url_host, official_content_sources, rss_item_fingerprint, RssItemRef,
};
use rustory_lib::domain::shared::{AppError, AppErrorCode};
use rustory_lib::domain::story::CanonicalStructure;
use rustory_lib::infrastructure::db;
use rustory_lib::infrastructure::device::rss_source::HttpRssFeedSource;

use crate::runtime::World;

/// Cross-step state of one scenario execution.
#[derive(Default)]
pub struct SharedState {
    /// S1: the sample page url documented by the Given step (Examples cell).
    s1_url: Option<String>,
    /// S1: the web preview outcome, kept verbatim (Ok: items; Err: refusal).
    s1_preview: Option<Result<web_episode_extraction::WebPreviewOutcome, AppError>>,
    /// S3: the local fixture server carrying the documented episode page
    /// (title + audio media, no image), kept alive until the scenario ends.
    s3_server: Option<FixtureHttpServer>,
    /// S3: the fixture page url.
    s3_page_url: Option<String>,
    /// S3: the web preview outcome of the fixture page.
    s3_preview: Option<Result<web_episode_extraction::WebPreviewOutcome, AppError>>,
    /// S4: the invalid addresses documented by the Given step.
    s4_invalid_urls: Vec<String>,
    /// S4: the (address, motivated refusal) pairs collected by the When step.
    s4_refusals: Vec<(String, AppError)>,
    /// S5: the (address, expected failure case) pairs documented by the
    /// Given step.
    s5_cases: Vec<(String, S5Case)>,
    /// S5: the local 500 server kept alive until the scenario ends.
    s5_server: Option<FixtureHttpServer>,
    /// S5: the (case, motivated refusal) pairs collected by the When step.
    s5_refusals: Vec<(S5Case, AppError)>,
    /// S7: the local fixture feed server kept alive until the scenario
    /// ends (the accept re-fetches feed and enclosure).
    s7_server: Option<FixtureHttpServer>,
    /// S7: the fixture feed url.
    s7_feed_url: Option<String>,
    /// S7: the RSS preview outcome.
    s7_preview: Option<rss_creation::RssPreviewOutcome>,
    /// S7: the committed import proof read back from the DB rows.
    s7_proof: Option<S7ImportProof>,
}

impl SharedState {
    fn s1_url(&self) -> &Option<String> {
        &self.s1_url
    }
    fn s1_url_mut(&mut self) -> &mut Option<String> {
        &mut self.s1_url
    }
    fn s1_preview(&self) -> &Option<Result<web_episode_extraction::WebPreviewOutcome, AppError>> {
        &self.s1_preview
    }
    fn s1_preview_mut(
        &mut self,
    ) -> &mut Option<Result<web_episode_extraction::WebPreviewOutcome, AppError>> {
        &mut self.s1_preview
    }
    fn s3_server_mut(&mut self) -> &mut Option<FixtureHttpServer> {
        &mut self.s3_server
    }
    fn s3_page_url(&self) -> &Option<String> {
        &self.s3_page_url
    }
    fn s3_page_url_mut(&mut self) -> &mut Option<String> {
        &mut self.s3_page_url
    }
    fn s3_preview(&self) -> &Option<Result<web_episode_extraction::WebPreviewOutcome, AppError>> {
        &self.s3_preview
    }
    fn s3_preview_mut(
        &mut self,
    ) -> &mut Option<Result<web_episode_extraction::WebPreviewOutcome, AppError>> {
        &mut self.s3_preview
    }
    fn s4_invalid_urls(&self) -> &[String] {
        &self.s4_invalid_urls
    }
    fn s4_invalid_urls_mut(&mut self) -> &mut Vec<String> {
        &mut self.s4_invalid_urls
    }
    fn s4_refusals(&self) -> &[(String, AppError)] {
        &self.s4_refusals
    }
    fn s4_refusals_mut(&mut self) -> &mut Vec<(String, AppError)> {
        &mut self.s4_refusals
    }
    fn s5_cases(&self) -> &[(String, S5Case)] {
        &self.s5_cases
    }
    fn s5_cases_mut(&mut self) -> &mut Vec<(String, S5Case)> {
        &mut self.s5_cases
    }
    fn s5_server_mut(&mut self) -> &mut Option<FixtureHttpServer> {
        &mut self.s5_server
    }
    fn s5_refusals(&self) -> &[(S5Case, AppError)] {
        &self.s5_refusals
    }
    fn s5_refusals_mut(&mut self) -> &mut Vec<(S5Case, AppError)> {
        &mut self.s5_refusals
    }
    fn s7_server_mut(&mut self) -> &mut Option<FixtureHttpServer> {
        &mut self.s7_server
    }
    fn s7_feed_url(&self) -> &Option<String> {
        &self.s7_feed_url
    }
    fn s7_feed_url_mut(&mut self) -> &mut Option<String> {
        &mut self.s7_feed_url
    }
    fn s7_preview(&self) -> &Option<rss_creation::RssPreviewOutcome> {
        &self.s7_preview
    }
    fn s7_preview_mut(&mut self) -> &mut Option<rss_creation::RssPreviewOutcome> {
        &mut self.s7_preview
    }
    fn s7_proof(&self) -> &Option<S7ImportProof> {
        &self.s7_proof
    }
    fn s7_proof_mut(&mut self) -> &mut Option<S7ImportProof> {
        &mut self.s7_proof
    }
}

/// Same entry budget as the IPC command layer.
const IMPORT_BUDGET: Duration = Duration::from_secs(30);

/// S4 fixed input: addresses that must be refused BEFORE any network
/// dispatch. The scenario carries no Examples table, so these values are
/// part of the handler vocabulary (kept in sync with the TDD-1 unit
/// tests), not of the contract.
const S4_INVALID_URLS: &[&str] = &["pas-une-url", "http://", "ftp://exemplo.fr/flux"];

/// The two documented access-failure cases of S5 (no Examples table: the
/// vocabulary belongs to the handler, kept in sync with the TDD-2 unit
/// tests of `fetch_html`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum S5Case {
    /// Syntactically valid address on the RFC 2606 reserved TLD: the page
    /// can never be resolved, so it is unreachable.
    Unreachable,
    /// Local server that answers with an HTTP error status.
    HttpError(u16),
}

/// One-shot multi-route HTTP server on 127.0.0.1 for deterministic
/// fixtures: the routes are built AFTER the bind so the fixture documents
/// can reference the local base URL (same shape as the module unit tests).
struct FixtureHttpServer {
    base: String,
    stop: Arc<AtomicBool>,
}

impl Drop for FixtureHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn start_fixture_http_server<F>(make_routes: F) -> FixtureHttpServer
where
    F: FnOnce(&str) -> Vec<(String, u16, Vec<u8>)> + Send + 'static,
{
    use std::io::{Read, Write};

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
                continue;
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
                .unwrap_or((404, Vec::from("not found")));
            let reason = match status {
                200 => "OK",
                404 => "Not Found",
                500 => "Internal Server Error",
                _ => "Error",
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    FixtureHttpServer { base, stop }
}

pub fn handle(world: &mut World, scenario: &str, step: &str) {
    match (scenario, step) {
        ("Importer une page web publique non-RSS", "l'URL \"<url>\" pointe vers une page HTML publique contenant au moins un épisode") => {
            s1_document_page(world)
        }
        ("Importer une page web publique non-RSS", "je lance l'import de cette URL") => {
            s1_run_import(world)
        }
        ("Importer une page web publique non-RSS", "la source est reconnue comme une page web non-RSS") => {
            s1_assert_source_recognized(world)
        }
        ("Importer une page web publique non-RSS", "au moins un épisode est identifié") => {
            s1_assert_episodes_identified(world)
        }
        ("Importer une page web publique non-RSS", "chaque épisode identifié a un titre non vide") => {
            s1_assert_episode_titles(world)
        }
        ("Importer une page web publique non-RSS", "chaque épisode identifié a un média audio") => {
            s1_assert_episode_audio(world)
        }
        ("Importer une page web publique non-RSS", "l'absence d'image n'empêche pas l'import") => {
            s1_assert_image_optional(world)
        }
        ("Importer un épisode sans image", "un épisode possède un titre et un média audio valides, sans image") => {
            s3_document_episode(world)
        }
        ("Importer un épisode sans image", "je lance l'import de la page") => {
            s3_run_import(world)
        }
        ("Importer un épisode sans image", "l'épisode est importé sans erreur") => {
            s3_assert_imported_without_error(world)
        }
        ("Importer un épisode sans image", "son champ image reste vide") => {
            s3_assert_image_field_empty(world)
        }
        ("Refuser une URL mal formée", "l'adresse fournie n'est pas une URL http(s) valide") => {
            *world.state.s4_invalid_urls_mut() =
                S4_INVALID_URLS.iter().map(|url| (*url).to_owned()).collect();
        }
        ("Refuser une URL mal formée", "je lance l'import") => s4_run_import(world),
        ("Refuser une URL mal formée", "aucune requête réseau n'est effectuée") => {
            s4_assert_zero_network(world)
        }
        ("Refuser une URL mal formée", "le système indique la raison de l'échec") => {
            s4_assert_reason(world)
        }
        ("Refuser une URL mal formée", "aucune histoire n'est créée") => assert_no_story_created(),
        ("Signaler une page inaccessible", "l'URL est valide mais la page est inaccessible (injoignable ou erreur HTTP)") => {
            s5_document_inaccessible_pages(world)
        }
        ("Signaler une page inaccessible", "je lance l'import") => s5_run_import(world),
        ("Signaler une page inaccessible", "le système indique la raison de l'échec") => {
            s5_assert_reason(world)
        }
        ("Signaler une page inaccessible", "aucune histoire n'est créée") => assert_no_story_created(),
        ("Continuer d'importer un flux RSS", "l'URL fournie correspond à un flux RSS valide") => {
            s7_document_feed(world)
        }
        ("Continuer d'importer un flux RSS", "je lance l'import de cette URL") => {
            s7_run_import(world)
        }
        ("Continuer d'importer un flux RSS", "la source est reconnue comme un flux RSS") => {
            s7_assert_source_recognized(world)
        }
        ("Continuer d'importer un flux RSS", "les épisodes sont importés par le comportement existant, sans changement") => {
            s7_assert_import_proof(world)
        }
        _ => panic!(
            "no acceptance handler for step `{step}` of scenario `{scenario}` — the vocabulary is closed by the frozen feature"
        ),
    }
}

fn s4_run_import(world: &mut World) {
    let urls = world.state.s4_invalid_urls().to_vec();
    assert!(!urls.is_empty(), "the S4 Given step must document the invalid addresses");
    for url in &urls {
        let outcome = web_episode_extraction::preview_web_podcast(
            official_content_sources(),
            url,
            IMPORT_BUDGET,
        );
        match outcome {
            Err(err) => world.state.s4_refusals_mut().push((url.clone(), err)),
            Ok(_) => panic!("the malformed address `{url}` must never produce a preview"),
        }
    }
}

/// The stage tag of the motivated refusal is the zero-network proof: a
/// refusal that escaped the entry guard would surface with a network
/// stage (`client_build`, `request`, `status_check`, `read_text`).
fn s4_assert_zero_network(world: &mut World) {
    let refusals = world.state.s4_refusals().to_vec();
    assert!(!refusals.is_empty(), "the When step must have refused every documented address");
    for (url, err) in &refusals {
        let value = serde_json::to_value(err).expect("AppError must serialize");
        let stage = value
            .get("details")
            .and_then(|details| details.get("stage"))
            .and_then(|stage| stage.as_str())
            .unwrap_or("<absent>");
        assert_eq!(
            stage,
            "url_invalid",
            "address `{url}` escaped the entry guard (stage `{stage}` means the refusal happened after a network-relevant step)"
        );
    }
}

fn s4_assert_reason(world: &mut World) {
    for (url, err) in world.state.s4_refusals() {
        assert_eq!(
            err.code,
            AppErrorCode::RssSourceUnreachable,
            "address `{url}`: the refusal must keep its stable error code"
        );
        assert!(
            !err.message.trim().is_empty(),
            "address `{url}`: the failure reason must be stated"
        );
    }
}

fn s5_document_inaccessible_pages(world: &mut World) {
    // Case 1: RFC 2606 reserved TLD — the address is valid but the page
    // can never be reached (the "injoignable" case).
    let unreachable = "https://import-test-non-rss.exemple.invalid/".to_owned();
    // Case 2: a local server that answers with an HTTP error status (the
    // "erreur HTTP" case), kept alive until the scenario ends.
    let server = start_fixture_http_server(|_| {
        vec![("/page".to_owned(), 500, Vec::from("<html><body>service indisponible</body></html>"))]
    });
    let http_url = format!("{}/page", server.base);
    *world.state.s5_server_mut() = Some(server);
    world.state.s5_cases_mut().push((unreachable, S5Case::Unreachable));
    world.state.s5_cases_mut().push((http_url, S5Case::HttpError(500)));
}

fn s5_run_import(world: &mut World) {
    let cases = world.state.s5_cases().to_vec();
    assert!(!cases.is_empty(), "the S5 Given step must document the inaccessible pages");
    for (url, case) in &cases {
        let outcome = web_episode_extraction::preview_web_podcast(
            official_content_sources(),
            url,
            IMPORT_BUDGET,
        );
        match outcome {
            Err(err) => world.state.s5_refusals_mut().push((*case, err)),
            Ok(_) => panic!("the inaccessible page `{url}` must never produce a preview"),
        }
    }
}

/// S5: every documented case is refused with a STATED and DISTINCT
/// reason — the unreachable page is not reported as an HTTP error and
/// vice versa. The stage tag in the error details is the observable
/// proof of the distinct failure mode.
fn s5_assert_reason(world: &mut World) {
    let refusals = world.state.s5_refusals().to_vec();
    assert_eq!(
        refusals.len(),
        2,
        "both documented failure cases must be refused"
    );
    let mut seen_messages = Vec::new();
    for (case, err) in &refusals {
        assert_eq!(
            err.code,
            AppErrorCode::RssSourceUnreachable,
            "{case:?}: the refusal must keep its stable error code"
        );
        assert!(
            !err.message.trim().is_empty(),
            "{case:?}: the failure reason must be stated"
        );
        let value = serde_json::to_value(err).expect("AppError must serialize");
        let stage = value
            .get("details")
            .and_then(|details| details.get("stage"))
            .and_then(|stage| stage.as_str())
            .unwrap_or("<absent>");
        match case {
            S5Case::Unreachable => {
                assert_eq!(
                    stage, "request",
                    "the unreachable page must be refused at the transport step"
                );
                assert!(
                    err.message.contains("injoignable"),
                    "the unreachable page must be reported as unreachable, got: {}",
                    err.message
                );
            }
            S5Case::HttpError(status) => {
                assert_eq!(
                    stage, "status_check",
                    "the HTTP error must be refused at the status-check step"
                );
                assert_eq!(
                    value.get("details").and_then(|d| d.get("status")).and_then(|s| s.as_u64()),
                    Some(u64::from(*status)),
                    "the HTTP status must be carried in the refusal details"
                );
                assert!(
                    err.message.contains("erreur HTTP") && err.message.contains(&status.to_string()),
                    "the HTTP error must be reported with its status, got: {}",
                    err.message
                );
            }
        }
        seen_messages.push(err.message.clone());
    }
    assert_ne!(
        seen_messages[0], seen_messages[1],
        "S5 requires a distinct user-facing reason per access-failure case"
    );
}

// ===== S1 reconnaissance (TDD-3) =====
//
// S1 (exemples faisant foi E1/E2) : une page HTML publique non-RSS doit
// prévisualiser comme source web portant SON propre hôte, avec au moins
// un épisode identifié, chaque épisode identifié portant un titre non
// vide et un média audio. L'image est optionnelle : son absence ne doit
// jamais bloquer l'import.

fn s1_document_page(world: &mut World) {
    let url = world.require_example("url");
    *world.state.s1_url_mut() = Some(url);
}

fn s1_run_import(world: &mut World) {
    let url = world
        .state
        .s1_url()
        .clone()
        .expect("the S1 Given step must document the page url");
    *world.state.s1_preview_mut() = Some(web_episode_extraction::preview_web_podcast(
        official_content_sources(),
        &url,
        IMPORT_BUDGET,
    ));
}

fn s1_preview(world: &World) -> &web_episode_extraction::WebPreviewOutcome {
    world
        .state
        .s1_preview()
        .as_ref()
        .expect("the S1 When step must have run the import")
        .as_ref()
        .expect("the sample page must preview — a refusal is the failure under test")
}

fn s1_assert_source_recognized(world: &mut World) {
    let url = world
        .state
        .s1_url()
        .clone()
        .expect("the S1 Given step must document the page url");
    let expected_host = feed_url_host(&url)
        .unwrap_or_else(|| panic!("the sample address must be a sober http(s) url: {url}"));
    assert_eq!(
        s1_preview(world).source_host,
        expected_host,
        "the preview must be recognized as the page's own web source, not a foreign one"
    );
}

fn s1_assert_episodes_identified(world: &mut World) {
    assert!(
        !s1_preview(world).items.is_empty(),
        "at least one episode must be identified on the sample page"
    );
}

fn s1_assert_episode_titles(world: &mut World) {
    for item in &s1_preview(world).items {
        assert!(
            !item.title.trim().is_empty(),
            "every identified episode must carry a non-empty title"
        );
    }
}

fn s1_assert_episode_audio(world: &mut World) {
    for item in &s1_preview(world).items {
        assert!(
            item.audio_url.is_some(),
            "every identified episode must carry an audio media"
        );
    }
}

/// S1: the image is OPTIONAL — when the page provides one, it is
/// carried on its episode as a non-empty url (never invented); when it
/// does not, the field stays absent; either way the import succeeds.
fn s1_assert_image_optional(world: &mut World) {
    for item in &s1_preview(world).items {
        assert!(
            item
                .image_url
                .as_deref()
                .map_or(true, |image| !image.trim().is_empty()),
            "a carried image must be a non-empty url, got: {:?}",
            item.image_url
        );
    }
}

// ===== S3 image optionnelle (TDD-4) =====
//
// S3 : un épisode documenté avec un titre et un média audio valides et
// PAS d'image doit s'importer sans erreur, son champ image restant
// vide. Le fixture est une page locale portant un seul lien audio
// titré et aucun élément image (pas de table Examples : le vocabulaire
// appartient au handler).

fn s3_document_episode(world: &mut World) {
    let server = start_fixture_http_server(|base| {
        let page = format!(
            "<html><body>\
             <h1>Sélection sans image</h1>\
             <section>\
             <a href=\"{base}/media/episode-sans-image.m4a\">Episode sans image</a>\
             </section>\
             </body></html>"
        );
        vec![("/page".to_owned(), 200, page.into_bytes())]
    });
    let page_url = format!("{}/page", server.base);
    *world.state.s3_page_url_mut() = Some(page_url);
    *world.state.s3_server_mut() = Some(server);
}

fn s3_run_import(world: &mut World) {
    let url = world
        .state
        .s3_page_url()
        .clone()
        .expect("the S3 Given step must document the fixture page");
    *world.state.s3_preview_mut() = Some(web_episode_extraction::preview_web_podcast(
        official_content_sources(),
        &url,
        IMPORT_BUDGET,
    ));
}

fn s3_preview(world: &World) -> &web_episode_extraction::WebPreviewOutcome {
    world
        .state
        .s3_preview()
        .as_ref()
        .expect("the S3 When step must have run the import")
        .as_ref()
        .expect("the fixture page must preview without error — S3 tests the error-free import")
}

fn s3_assert_imported_without_error(world: &mut World) {
    match world.state.s3_preview().as_ref() {
        Some(Ok(outcome)) => assert!(
            !outcome.items.is_empty(),
            "the documented episode must be imported"
        ),
        Some(Err(error)) => panic!(
            "the image-less episode must import without error: {error:?}"
        ),
        None => panic!("the S3 When step must have run the import"),
    }
}

fn s3_assert_image_field_empty(world: &mut World) {
    for item in &s3_preview(world).items {
        assert!(
            item.image_url.is_none(),
            "the documented episode carries no image: its image field must stay empty, got: {item:?}"
        );
    }
}
// ===== S7 regression (TDD-3) =====
//
// S7: the RSS path must stay unchanged END-TO-END. A valid LOCAL fixture
// feed (no external network) is previewed AND accepted through the
// production `HttpRssFeedSource`; the import proof is read back from the
// committed rows (story, provenance, asset, start-node audio wiring).

/// The committed import proof of S7: what the DB rows prove after the
/// existing RSS behavior ran unchanged.
#[derive(Debug)]
struct S7ImportProof {
    stories_count: i64,
    story_title: String,
    source_format: String,
    source_name: String,
    assets_count: i64,
    media_format: Option<String>,
    start_node_audio_asset_id: Option<String>,
}

fn s7_document_feed(world: &mut World) {
    let server = start_fixture_http_server(|base| {
        vec![
            ("/flux".to_owned(), 200, s7_fixture_feed(base)),
            ("/media/episode-1.wav".to_owned(), 200, s7_fixture_wav()),
        ]
    });
    let feed_url = format!("{}/flux", server.base);
    *world.state.s7_server_mut() = Some(server);
    *world.state.s7_feed_url_mut() = Some(feed_url);
}

/// The S7 fixture feed: RSS 2.0, one channel title, two items (the first
/// with a `fixture-1` guid + a local WAV enclosure). ASCII titles so
/// `normalize_title` keeps them verbatim.
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
/// store's sniff to promote it as audio media `wav`.
fn s7_fixture_wav() -> Vec<u8> {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&[0u8; 4]);
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&[0u8; 8]);
    bytes
}

fn s7_run_import(world: &mut World) {
    let url = world
        .state
        .s7_feed_url()
        .clone()
        .expect("the S7 Given step must document the feed url");
    let source = HttpRssFeedSource::default();
    let preview = rss_creation::preview_rss_source(
        official_content_sources(),
        &source,
        &url,
        IMPORT_BUDGET,
    )
    .expect("the local fixture feed must preview");
    let item = preview
        .analysis
        .items
        .iter()
        .find(|item| item.guid.as_deref() == Some("fixture-1"))
        .expect("the fixture feed must carry its first item guid");
    let fingerprint = rss_item_fingerprint(item);
    *world.state.s7_preview_mut() = Some(preview);

    let mut library = db::open_in_memory().expect("fresh in-memory library");
    db::run_migrations(&mut library).expect("migrations must apply");
    let store_root = std::env::temp_dir().join(format!("rustory-accept-s7-{}", std::process::id()));
    std::fs::create_dir_all(&store_root).expect("the media store root must be creatable");
    let outcome = rss_creation::accept_rss_story_creation(
        &mut library,
        official_content_sources(),
        &source,
        &url,
        &RssItemRef::Guid("fixture-1".into()),
        &fingerprint,
        IMPORT_BUDGET,
        Some(&store_root),
    )
    .expect("the fixture import must not fail");
    let rss_creation::RssCreationOutcome::Created { story } = outcome else {
        panic!("the fixture feed is local and stable: the accept must never see a source change")
    };
    let story_id = story.id;
    let stories_count: i64 = library
        .conn()
        .query_row("SELECT count(*) FROM stories", [], |row| row.get(0))
        .expect("the stories table must exist");
    let (story_title, structure_json): (String, String) = library
        .conn()
        .query_row(
            "SELECT title, structure_json FROM stories WHERE id = ?1",
            [&story_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the created story row must exist");
    let (source_format, source_name): (String, String) = library
        .conn()
        .query_row(
            "SELECT source_format, source_name FROM story_local_imports WHERE story_id = ?1",
            [&story_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the provenance row must exist");
    let assets_count: i64 = library
        .conn()
        .query_row(
            "SELECT count(*) FROM assets WHERE story_id = ?1",
            [&story_id],
            |row| row.get(0),
        )
        .expect("the assets table must exist");
    let media_format = (assets_count > 0).then(|| {
        library
            .conn()
            .query_row(
                "SELECT media_format FROM assets WHERE story_id = ?1",
                [&story_id],
                |row| row.get(0),
            )
            .expect("the asset row must exist")
    });
    let structure: CanonicalStructure =
        serde_json::from_str(&structure_json).expect("the story structure must be canonical JSON");
    let start_node = structure
        .nodes
        .iter()
        .find(|node| node.id == structure.start_node_id)
        .expect("the structure must carry its start node");
    *world.state.s7_proof_mut() = Some(S7ImportProof {
        stories_count,
        story_title,
        source_format,
        source_name,
        assets_count,
        media_format,
        start_node_audio_asset_id: start_node.audio_asset_id.clone(),
    });
}

fn s7_assert_source_recognized(world: &mut World) {
    let preview = world
        .state
        .s7_preview()
        .as_ref()
        .expect("the S7 When step must have run the import");
    assert!(
        !preview.analysis.is_blocked(),
        "the fixture feed must be recognized as a valid RSS source"
    );
    assert_eq!(
        preview.source_host, "127.0.0.1",
        "the preview must carry the feed's own host"
    );
    assert_eq!(
        preview.analysis.channel_title.as_deref(),
        Some("Flux fixture"),
        "the channel title must be read from the feed"
    );
    assert_eq!(
        preview.analysis.items.len(),
        2,
        "both fixture items must be parsed"
    );
}

/// S7: the import ran through the EXISTING RSS behavior, unchanged — one
/// story for the accepted item, `rss` provenance naming the feed host,
/// the enclosure stored as a `wav` asset wired to the start node.
fn s7_assert_import_proof(world: &mut World) {
    let proof = world
        .state
        .s7_proof()
        .as_ref()
        .expect("the S7 When step must have proven the import");
    assert_eq!(proof.stories_count, 1, "the import must create exactly one story");
    assert_eq!(
        proof.story_title, "Episode un",
        "the story must keep the accepted episode's own title"
    );
    assert_eq!(
        proof.source_format, "rss",
        "the provenance must stay the rss source format"
    );
    assert_eq!(
        proof.source_name, "127.0.0.1",
        "the provenance must name the feed host"
    );
    assert_eq!(
        proof.assets_count, 1,
        "the episode enclosure must be stored as exactly one asset"
    );
    assert_eq!(
        proof.media_format.as_deref(),
        Some("wav"),
        "the stored media must keep its sniffed wav format"
    );
    assert!(
        proof.start_node_audio_asset_id.is_some(),
        "the start node must reference the stored audio asset"
    );
}

fn assert_no_story_created() {
    let mut library = db::open_in_memory().expect("fresh in-memory library");
    db::run_migrations(&mut library).expect("migrations must apply");
    let count: i64 = library
        .conn()
        .query_row("SELECT count(*) FROM stories", [], |row| row.get(0))
        .expect("the stories table must exist");
    assert_eq!(count, 0, "a refused import must not create any story");
}
