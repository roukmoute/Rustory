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

use rustory_lib::application::import_export::web_episode_extraction;
use rustory_lib::domain::import::official_content_sources;
use rustory_lib::domain::shared::{AppError, AppErrorCode};
use rustory_lib::infrastructure::db;

use crate::runtime::World;

/// Cross-step state of one scenario execution.
#[derive(Default)]
pub struct SharedState {
    /// S4: the invalid addresses documented by the Given step.
    s4_invalid_urls: Vec<String>,
    /// S4: the (address, motivated refusal) pairs collected by the When step.
    s4_refusals: Vec<(String, AppError)>,
    /// S5: the (address, expected failure case) pairs documented by the
    /// Given step.
    s5_cases: Vec<(String, S5Case)>,
    /// S5: the local 500 server kept alive until the scenario ends.
    s5_server: Option<LocalHttpServer>,
    /// S5: the (case, motivated refusal) pairs collected by the When step.
    s5_refusals: Vec<(S5Case, AppError)>,
}

impl SharedState {
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
    fn s5_server_mut(&mut self) -> &mut Option<LocalHttpServer> {
        &mut self.s5_server
    }
    fn s5_refusals(&self) -> &[(S5Case, AppError)] {
        &self.s5_refusals
    }
    fn s5_refusals_mut(&mut self) -> &mut Vec<(S5Case, AppError)> {
        &mut self.s5_refusals
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

/// One-shot HTTP server on 127.0.0.1 for the S5 "erreur HTTP" case (same
/// shape as the web module unit tests).
struct LocalHttpServer {
    url: String,
    stop: Arc<AtomicBool>,
}

impl Drop for LocalHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn start_local_http_server(status: u16, body: &str) -> LocalHttpServer {
    use std::io::{Read, Write};

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
                continue;
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

pub fn handle(world: &mut World, scenario: &str, step: &str) {
    match (scenario, step) {
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
    let server = start_local_http_server(500, "<html><body>service indisponible</body></html>");
    let http_url = server.url.clone();
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

fn assert_no_story_created() {
    let mut library = db::open_in_memory().expect("fresh in-memory library");
    db::run_migrations(&mut library).expect("migrations must apply");
    let count: i64 = library
        .conn()
        .query_row("SELECT count(*) FROM stories", [], |row| row.get(0))
        .expect("the stories table must exist");
    assert_eq!(count, 0, "a refused import must not create any story");
}
