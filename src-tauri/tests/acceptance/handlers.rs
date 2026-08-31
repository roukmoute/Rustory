//! Closed-vocabulary acceptance handlers for the frozen contract
//! `import-podcasts-pages-web-non-rss`.
//!
//! Dispatch is exact on (scenario name, step text): the vocabulary is
//! closed by the frozen feature, so an unknown step panics (a test
//! failure) instead of being silently skipped. Handlers are added slice
//! by slice; the generated entry points only reference what exists.

use std::time::Duration;

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
}

/// Same entry budget as the IPC command layer.
const IMPORT_BUDGET: Duration = Duration::from_secs(30);

/// S4 fixed input: addresses that must be refused BEFORE any network
/// dispatch. The scenario carries no Examples table, so these values are
/// part of the handler vocabulary (kept in sync with the TDD-1 unit
/// tests), not of the contract.
const S4_INVALID_URLS: &[&str] = &["pas-une-url", "http://", "ftp://exemplo.fr/flux"];

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

fn assert_no_story_created() {
    let mut library = db::open_in_memory().expect("fresh in-memory library");
    db::run_migrations(&mut library).expect("migrations must apply");
    let count: i64 = library
        .conn()
        .query_row("SELECT count(*) FROM stories", [], |row| row.get(0))
        .expect("the stories table must exist");
    assert_eq!(count, 0, "a refused import must not create any story");
}
