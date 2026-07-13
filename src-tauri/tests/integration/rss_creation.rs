//! End-to-end journeys of the RSS external-source creation flow against a
//! REAL SQLite database and a scripted feed source (no network): preview
//! purity, the re-proven accept, the durable provenance + review lifecycle,
//! and every honest refusal (transport, blocked feed, diverged source) —
//! plus the REAL transport cap of the production HTTP source against a
//! local listener.

use std::io::{Read as _, Write as _};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustory_lib::application::import_export::{
    accept_rss_story_creation, preview_rss_source, RssCreationOutcome,
};
use rustory_lib::application::story::{node, scope};
use rustory_lib::domain::import::{parse_rss, rss_item_fingerprint, ImportState, RssItemRef};
use rustory_lib::domain::shared::{AppError, AppErrorCode};
use rustory_lib::infrastructure::db::{self, DbHandle};
use rustory_lib::infrastructure::device::{
    HttpRssFeedSource, RssFeedSource, MAX_RSS_RESPONSE_BYTES,
};
use tempfile::TempDir;

const BUDGET: Duration = Duration::from_secs(30);
const FEED_URL: &str = "https://exemple.fr/flux.xml";

/// One programmed fetch response: raw body bytes or a typed error.
type ScriptedFetch = Result<Vec<u8>, AppError>;

/// Scripted feed source for the integration crate (the lib's recorder mock
/// is `cfg(test)`-gated): pops the next programmed response (FIFO) and
/// records every requested URL — the proof of the accept's re-fetch.
#[derive(Clone, Default)]
struct ScriptedRssSource {
    queue: Arc<Mutex<Vec<ScriptedFetch>>>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl ScriptedRssSource {
    fn new() -> Self {
        Self::default()
    }

    fn enqueue_body(&self, body: impl Into<Vec<u8>>) {
        self.queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Ok(body.into()));
    }

    fn enqueue_failure(&self, err: AppError) {
        self.queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Err(err));
    }

    fn request_count(&self) -> usize {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len()
    }
}

impl RssFeedSource for ScriptedRssSource {
    fn fetch(&self, url: &str, _budget: Duration) -> Result<Vec<u8>, AppError> {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(url.to_string());
        let mut queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        if queue.is_empty() {
            Ok(Vec::new())
        } else {
            queue.remove(0)
        }
    }
}

fn transport_error(stage: &'static str) -> AppError {
    AppError::rss_source_unreachable(
        "Récupération du flux impossible: la source est injoignable.",
        "Vérifie l'adresse du flux et ta connexion, puis réessaie.",
    )
    .with_details(serde_json::json!({ "source": "network", "stage": stage }))
}

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

fn count(db: &DbHandle, table: &str) -> u32 {
    db.conn()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .expect("count")
}

fn provenance_row(db: &DbHandle, story_id: &str) -> (String, String, String, Option<String>) {
    db.conn()
        .query_row(
            "SELECT source_format, source_name, import_state, findings_summary \
             FROM story_local_imports WHERE story_id = ?1",
            rusqlite::params![story_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("provenance row")
}

#[test]
fn journey_1_preview_is_pure_then_the_accept_lands_a_reviewable_draft() {
    let db_dir = TempDir::new().expect("tmp");
    let app_data = TempDir::new().expect("app data");
    let db_path = db_dir.path().join("rustory.sqlite");
    let mut db = db::open_at(&db_path).expect("open");
    db::run_migrations(&mut db).expect("migrate");
    let source = ScriptedRssSource::new();
    source.enqueue_body(nominal_feed());
    source.enqueue_body(nominal_feed());

    // Phase 1 — the preview parses the feed and mutates NOTHING.
    let preview = preview_rss_source(&source, FEED_URL, BUDGET).expect("preview");
    assert_eq!(preview.source_host, "exemple.fr");
    assert_eq!(preview.analysis.items.len(), 2);
    assert_eq!(preview.analysis.state, ImportState::NeedsReview);
    assert_eq!(count(&db, "stories"), 0);
    assert_eq!(count(&db, "story_local_imports"), 0);

    // Phase 2 — the accept RE-FETCHES (2nd request) and commits ONE story
    // + its provenance atomically.
    let outcome = accept_rss_story_creation(
        &mut db,
        &source,
        FEED_URL,
        &RssItemRef::Guid("g-1".into()),
        &fingerprint_in(&nominal_feed(), "g-1"),
        BUDGET,
    )
    .expect("accept");
    let RssCreationOutcome::Created { story } = outcome else {
        panic!("expected a creation");
    };
    assert_eq!(source.request_count(), 2, "preview + accept re-fetch");
    assert_eq!(story.title, "Episode 1");

    let (format, name, state, summary) = provenance_row(&db, &story.id);
    assert_eq!(format, "rss");
    assert_eq!(name, "exemple.fr", "host only — never the full address");
    assert_eq!(state, "needs_review", "the ingestion floor");
    assert!(summary.is_some(), "an rss summary is never NULL");
    let checksum: String = db
        .conn()
        .query_row(
            "SELECT artifact_checksum FROM story_local_imports WHERE story_id = ?1",
            rusqlite::params![&story.id],
            |r| r.get(0),
        )
        .expect("checksum");
    assert_eq!(checksum.len(), 64);
    assert!(checksum.bytes().all(|b| b.is_ascii_hexdigit()));

    // FULL edit scope BY CONSTRUCTION — the draft opens like a native one,
    // with the ingested text pre-filled on the start node.
    assert_eq!(
        scope::story_edit_scope(db.conn(), &story.id),
        scope::StoryEditScope::Full
    );
    let detail =
        rustory_lib::application::story::get_story_detail(&db, app_data.path(), &story.id, None)
            .expect("read detail")
            .expect("present");
    assert!(detail.editable);
    assert_eq!(detail.edit_scope, "full");
    assert_eq!(detail.import_state.as_deref(), Some("needsReview"));
    let start = detail.node.expect("current node");
    assert_eq!(start.text, "Premier texte.");

    // The created card carries the chip + the durable on-demand report
    // (the RSS wording) — straight from the creation AND from a fresh
    // provenance re-read.
    let card_state = story.import_state.expect("chip state");
    assert_eq!(format!("{card_state:?}"), "NeedsReview");
    let report = story.import_report.expect("report");
    assert!(report.iter().any(|f| f
        .message
        .starts_with("Contenu ingéré depuis une source externe (RSS).")));

    // Review resolution REUSED without modification: a sound editor write
    // settles the pending review (`resolved`), the trace stays in base.
    let ack = node::save_node_content(
        &mut db,
        app_data.path(),
        node::SaveNodeContentInput {
            story_id: story.id.clone(),
            node_id: "n1".into(),
            text: "Texte relu et corrigé.".into(),
            label: String::new(),
        },
    )
    .expect("editor write");
    assert_eq!(
        ack.import_state.as_deref(),
        Some("resolved"),
        "the acknowledgement carries the settled review"
    );
    let (_, _, state, summary) = provenance_row(&db, &story.id);
    assert_eq!(state, "resolved");
    assert!(summary.is_some(), "the findings trace is never erased");
}

#[test]
fn an_enclosure_item_lands_partial_with_the_missing_media_finding() {
    let mut db = fresh_db();
    let source = ScriptedRssSource::new();
    let body = feed_xml(
        "<item><title>Podcast</title><description>Episode audio.</description><guid>g-p</guid>\
         <enclosure url=\"https://exemple.fr/ep.mp3\" length=\"1\" type=\"audio/mpeg\"/></item>",
    );
    source.enqueue_body(body.clone());
    let outcome = accept_rss_story_creation(
        &mut db,
        &source,
        FEED_URL,
        &RssItemRef::Guid("g-p".into()),
        &fingerprint_in(&body, "g-p"),
        BUDGET,
    )
    .expect("accept");
    let RssCreationOutcome::Created { story } = outcome else {
        panic!("expected a creation");
    };
    let (_, _, state, summary) = provenance_row(&db, &story.id);
    assert_eq!(state, "partial", "a non-downloaded enclosure is honest");
    assert!(summary.expect("summary").contains("\"aspect\":\"media\""));
    let report = story.import_report.expect("report");
    assert!(report.iter().any(|f| f.message
        == "Le média distant référencé par la source n'a pas été récupéré. Ajoute le média manuellement dans l'éditeur."));
    // No media pipeline ran: zero asset row, text-only ingestion.
    assert_eq!(count(&db, "assets"), 0);
}

#[test]
fn transport_failures_reject_and_create_nothing_on_both_phases() {
    let mut db = fresh_db();
    let source = ScriptedRssSource::new();
    // Preview transport failure.
    source.enqueue_failure(transport_error("request"));
    let err = preview_rss_source(&source, FEED_URL, BUDGET).expect_err("preview transport");
    assert_eq!(err.code, AppErrorCode::RssSourceUnreachable);
    // Accept transport failure (including the oversize refusal shape).
    source.enqueue_failure(transport_error("response_oversize"));
    let err = accept_rss_story_creation(
        &mut db,
        &source,
        FEED_URL,
        &RssItemRef::Guid("g-1".into()),
        &"0".repeat(64),
        BUDGET,
    )
    .expect_err("accept transport");
    assert_eq!(err.code, AppErrorCode::RssSourceUnreachable);
    let details = serde_json::to_value(&err).expect("ser");
    assert_eq!(details["details"]["stage"], "response_oversize");
    assert_eq!(count(&db, "stories"), 0, "nothing was created");
    assert_eq!(count(&db, "story_local_imports"), 0);
}

#[test]
fn blocked_feeds_are_typed_verdicts_and_create_nothing() {
    let mut db = fresh_db();
    let source = ScriptedRssSource::new();
    for (body, expect_blocked_aspect) in [
        // Unreadable XML → envelope.
        (b"pas du xml".to_vec(), "envelope"),
        // An Atom feed → formatVersion.
        (
            b"<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\"><title>Atom</title></feed>"
                .to_vec(),
            "formatVersion",
        ),
        // A well-formed feed with zero exploitable item → structure.
        (
            feed_xml("<item><guid>seulement-un-guid</guid></item>").into_bytes(),
            "structure",
        ),
    ] {
        source.enqueue_body(body.clone());
        let preview = preview_rss_source(&source, FEED_URL, BUDGET).expect("typed verdict");
        assert!(preview.analysis.is_blocked(), "{expect_blocked_aspect}");

        // The SAME feed at accept time is the honest recoverable refusal.
        source.enqueue_body(body);
        let outcome = accept_rss_story_creation(
            &mut db,
            &source,
            FEED_URL,
            &RssItemRef::Guid("g-1".into()),
            &"0".repeat(64),
            BUDGET,
        )
        .expect("a refusal, not an error");
        assert!(matches!(outcome, RssCreationOutcome::SourceChanged));
    }
    assert_eq!(count(&db, "stories"), 0);
    assert_eq!(count(&db, "story_local_imports"), 0);
}

#[test]
fn a_source_that_changed_between_preview_and_accept_refuses_honestly() {
    let mut db = fresh_db();
    let source = ScriptedRssSource::new();
    // The preview serves an item…
    source.enqueue_body(nominal_feed());
    let preview = preview_rss_source(&source, FEED_URL, BUDGET).expect("preview");
    assert_eq!(preview.analysis.items.len(), 2);
    // …the accept re-fetches a DIFFERENT feed where the item is gone.
    source.enqueue_body(feed_xml(
        "<item><title>Autre épisode</title><description>Nouveau.</description><guid>g-z</guid></item>",
    ));
    let outcome = accept_rss_story_creation(
        &mut db,
        &source,
        FEED_URL,
        &RssItemRef::Guid("g-1".into()),
        &fingerprint_in(&nominal_feed(), "g-1"),
        BUDGET,
    )
    .expect("a refusal, not an error");
    assert!(matches!(outcome, RssCreationOutcome::SourceChanged));
    assert_eq!(count(&db, "stories"), 0, "zero mutation on the refusal");
    assert_eq!(source.request_count(), 2);
}

#[test]
fn a_resolvable_item_whose_content_diverged_refuses_honestly() {
    // The guid still resolves on the fresh fetch, but the item TEXT was
    // rewritten since the preview: the previewed-content proof no longer
    // matches — the honest refusal, zero DB row (never a creation from
    // content the user never reread).
    let mut db = fresh_db();
    let source = ScriptedRssSource::new();
    let previewed = nominal_feed();
    source.enqueue_body(previewed.clone());
    let preview = preview_rss_source(&source, FEED_URL, BUDGET).expect("preview");
    assert_eq!(preview.analysis.items.len(), 2);
    source.enqueue_body(feed_xml(
        "<item><title>Episode 1</title><description>Texte RÉÉCRIT depuis la preview.</description><guid>g-1</guid></item>",
    ));
    let outcome = accept_rss_story_creation(
        &mut db,
        &source,
        FEED_URL,
        &RssItemRef::Guid("g-1".into()),
        &fingerprint_in(&previewed, "g-1"),
        BUDGET,
    )
    .expect("a refusal, not an error");
    assert!(matches!(outcome, RssCreationOutcome::SourceChanged));
    assert_eq!(count(&db, "stories"), 0, "zero mutation on the refusal");
    assert_eq!(count(&db, "story_local_imports"), 0);
}

#[test]
fn the_created_card_surfaces_on_the_overview_projection_with_the_rss_report() {
    // `load_overview` needs a Tauri AppHandle; the projection itself is
    // proven through the SAME SQL the overview runs (LEFT JOIN + state +
    // summary), re-rendered with the RSS copy by `source_format = 'rss'`.
    let mut db = fresh_db();
    let source = ScriptedRssSource::new();
    source.enqueue_body(nominal_feed());
    let outcome = accept_rss_story_creation(
        &mut db,
        &source,
        FEED_URL,
        &RssItemRef::Guid("g-2".into()),
        &fingerprint_in(&nominal_feed(), "g-2"),
        BUDGET,
    )
    .expect("accept");
    let RssCreationOutcome::Created { story } = outcome else {
        panic!("expected a creation");
    };

    let (import_state, findings_summary, source_format): (String, String, String) = db
        .conn()
        .query_row(
            "SELECT li.import_state, li.findings_summary, li.source_format \
             FROM stories s LEFT JOIN story_local_imports li ON li.story_id = s.id \
             WHERE s.id = ?1",
            rusqlite::params![&story.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("joined projection row");
    assert_eq!(import_state, "needs_review");
    assert_eq!(source_format, "rss");
    let report =
        rustory_lib::ipc::dto::import_export::rss_import_findings_from_summary(&findings_summary);
    assert!(
        report.iter().any(|f| f
            .message
            .starts_with("Contenu ingéré depuis une source externe (RSS).")),
        "the durable card report re-renders with the feed wording"
    );
}

#[test]
fn the_http_source_refuses_an_oversize_response_at_cap_plus_one() {
    // A REAL local HTTP exchange (no external network): the listener
    // streams one byte past the cap; the production source must refuse
    // with the `response_oversize` stage instead of buffering it whole.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let serve = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        // Drain the request head (best effort — the client sends a small GET).
        let mut buf = [0u8; 4096];
        let _ = socket.read(&mut buf);
        let body_len = (MAX_RSS_RESPONSE_BYTES + 1) as usize;
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n"
        );
        socket.write_all(head.as_bytes()).expect("head");
        // Stream the oversize body in chunks.
        let chunk = vec![b'a'; 64 * 1024];
        let mut sent = 0usize;
        while sent < body_len {
            let take = chunk.len().min(body_len - sent);
            if socket.write_all(&chunk[..take]).is_err() {
                break; // The client may hang up right at the cap.
            }
            sent += take;
        }
    });

    let source = HttpRssFeedSource::default();
    let err = source
        .fetch(&format!("http://127.0.0.1:{port}/feed.xml"), BUDGET)
        .expect_err("the cap+1 read must refuse the oversize body");
    assert_eq!(err.code, AppErrorCode::RssSourceUnreachable);
    let v = serde_json::to_value(&err).expect("ser");
    assert_eq!(v["details"]["stage"], "response_oversize");
    let _ = serve.join();
}
