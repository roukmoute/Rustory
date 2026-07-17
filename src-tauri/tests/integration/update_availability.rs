//! Integration of the update-availability orchestration
//! (`ensure_update_availability`): decision → fetch → pure resolution →
//! diagnostics line → session memo, exercised end-to-end with a
//! programmable counting source (the `MockOfficialCatalogSource`
//! pattern) — zero network, zero Tauri runtime.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use rustory_lib::application::update::{ensure_update_availability, UpdateCheckMemo};
use rustory_lib::domain::update::{
    decide_update_check, ReleaseProbe, ReleaseVersion, UpdateAvailability, UpdateCheckDecision,
    UpdateCheckSkipReason,
};
use rustory_lib::infrastructure::updates::{UpdateFetchStage, UpdateReleaseSource};
use tempfile::TempDir;

const BUDGET: Duration = Duration::from_secs(10);

fn version(major: u64, minor: u64, patch: u64) -> ReleaseVersion {
    ReleaseVersion {
        major,
        minor,
        patch,
    }
}

/// Programmable RECORDER mock: pops the next queued probe result (FIFO)
/// and counts every call — the proof of the once-per-launch memo and of
/// the Skip path's zero dispatch rides on the counter. An empty queue
/// answers the honest no-release world so a forgetful test fails on
/// assertion, not panic. An optional fetch delay widens the concurrency
/// window for the single-flight proof.
#[derive(Default)]
struct MockUpdateReleaseSource {
    queue: Mutex<Vec<Result<ReleaseProbe, UpdateFetchStage>>>,
    calls: AtomicU32,
    fetch_delay: Mutex<Option<Duration>>,
}

impl MockUpdateReleaseSource {
    fn new() -> Self {
        Self::default()
    }

    fn enqueue(&self, outcome: Result<ReleaseProbe, UpdateFetchStage>) {
        self.queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(outcome);
    }

    fn enqueue_latest_tag(&self, tag: &str) {
        self.enqueue(Ok(ReleaseProbe::Latest {
            tag: tag.to_string(),
        }));
    }

    /// Make every fetch sleep first — widens the race window so a
    /// broken single-flight would reliably double-consult.
    fn set_fetch_delay(&self, delay: Duration) {
        *self
            .fetch_delay
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(delay);
    }

    /// Number of times `fetch_latest` was invoked — `0` proves the Skip
    /// path never dispatches, `1` after two ensures proves the memo.
    fn fetch_count(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }
}

impl UpdateReleaseSource for MockUpdateReleaseSource {
    fn fetch_latest(&self, _budget: Duration) -> Result<ReleaseProbe, UpdateFetchStage> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let delay = *self
            .fetch_delay
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(delay) = delay {
            std::thread::sleep(delay);
        }
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if queue.is_empty() {
            Ok(ReleaseProbe::NoPublishedRelease)
        } else {
            queue.remove(0)
        }
    }
}

fn fresh_memo() -> UpdateCheckMemo {
    UpdateCheckMemo::new()
}

#[test]
fn a_newer_published_version_settles_on_update_available_and_traces_it() {
    let dir = TempDir::new().expect("tempdir");
    let log = dir.path().join("update.jsonl");
    let source = MockUpdateReleaseSource::new();
    source.enqueue_latest_tag("v9.9.9");
    let memo = fresh_memo();

    let availability = ensure_update_availability(
        &source,
        UpdateCheckDecision::Run,
        version(0, 1, 0),
        BUDGET,
        &memo,
        Some(&log),
    );

    assert_eq!(
        availability,
        UpdateAvailability::UpdateAvailable {
            latest: version(9, 9, 9)
        }
    );
    let content = std::fs::read_to_string(&log).expect("log written");
    assert!(content.contains("\"category\":\"update_check_completed\""));
    assert!(content.contains("\"result\":\"updateAvailable\""));
    assert!(content.contains("\"latest\":\"9.9.9\""));
}

#[test]
fn an_equal_or_older_published_version_settles_on_up_to_date() {
    for tag in ["v0.1.0", "v0.0.9"] {
        let source = MockUpdateReleaseSource::new();
        source.enqueue_latest_tag(tag);
        let memo = fresh_memo();

        let availability = ensure_update_availability(
            &source,
            UpdateCheckDecision::Run,
            version(0, 1, 0),
            BUDGET,
            &memo,
            None,
        );

        assert_eq!(
            availability,
            UpdateAvailability::UpToDate,
            "tag {tag:?} must never signal an update (equality and downgrades stay silent)"
        );
    }
}

#[test]
fn the_no_published_release_world_settles_on_up_to_date() {
    // The REAL state of the repository today: a 404 probe is an honest
    // answer, never a failure.
    let dir = TempDir::new().expect("tempdir");
    let log = dir.path().join("update.jsonl");
    let source = MockUpdateReleaseSource::new();
    source.enqueue(Ok(ReleaseProbe::NoPublishedRelease));
    let memo = fresh_memo();

    let availability = ensure_update_availability(
        &source,
        UpdateCheckDecision::Run,
        version(0, 1, 0),
        BUDGET,
        &memo,
        Some(&log),
    );

    assert_eq!(availability, UpdateAvailability::UpToDate);
    let content = std::fs::read_to_string(&log).expect("log written");
    assert!(content.contains("\"result\":\"upToDate\""));
    // Omission discipline: no newer version was found, no `latest` key.
    assert!(!content.contains("latest\""));
}

#[test]
fn a_transport_failure_settles_on_the_calm_check_unavailable() {
    let dir = TempDir::new().expect("tempdir");
    let log = dir.path().join("update.jsonl");
    let source = MockUpdateReleaseSource::new();
    source.enqueue(Err(UpdateFetchStage::Request));
    let memo = fresh_memo();

    let availability = ensure_update_availability(
        &source,
        UpdateCheckDecision::Run,
        version(0, 1, 0),
        BUDGET,
        &memo,
        Some(&log),
    );

    assert_eq!(availability, UpdateAvailability::CheckUnavailable);
    let content = std::fs::read_to_string(&log).expect("log written");
    assert!(content.contains("\"category\":\"update_check_unreachable\""));
    assert!(content.contains("\"stage\":\"request\""));
}

#[test]
fn a_tag_outside_the_convention_settles_on_check_unavailable() {
    // Fail-closed: the fetch SUCCEEDED but the tag yields no verdict —
    // a completed check whose result is "not doable", never a guess.
    let dir = TempDir::new().expect("tempdir");
    let log = dir.path().join("update.jsonl");
    let source = MockUpdateReleaseSource::new();
    source.enqueue_latest_tag("nightly-2026");
    let memo = fresh_memo();

    let availability = ensure_update_availability(
        &source,
        UpdateCheckDecision::Run,
        version(0, 1, 0),
        BUDGET,
        &memo,
        Some(&log),
    );

    assert_eq!(availability, UpdateAvailability::CheckUnavailable);
    let content = std::fs::read_to_string(&log).expect("log written");
    assert!(content.contains("\"category\":\"update_check_completed\""));
    assert!(content.contains("\"result\":\"checkUnavailable\""));
}

#[test]
fn a_skip_decision_settles_on_check_not_run_without_any_dispatch() {
    let dir = TempDir::new().expect("tempdir");
    let log = dir.path().join("update.jsonl");
    let source = MockUpdateReleaseSource::new();
    let memo = fresh_memo();

    // The REAL wiring: a debug build skips before the probe speaks.
    let decision = decide_update_check(true, None);
    assert_eq!(
        decision,
        UpdateCheckDecision::Skip {
            reason: UpdateCheckSkipReason::DevelopmentBuild
        }
    );

    let availability = ensure_update_availability(
        &source,
        decision,
        version(0, 1, 0),
        BUDGET,
        &memo,
        Some(&log),
    );

    assert_eq!(availability, UpdateAvailability::CheckNotRun);
    // The proof: ZERO network dispatch on a skipped check.
    assert_eq!(source.fetch_count(), 0);
    let content = std::fs::read_to_string(&log).expect("log written");
    assert!(content.contains("\"category\":\"update_check_skipped\""));
    assert!(content.contains("\"reason\":\"development_build\""));
}

#[test]
fn the_session_memo_makes_the_second_call_free_of_any_fetch() {
    let source = MockUpdateReleaseSource::new();
    source.enqueue_latest_tag("v9.9.9");
    let memo = fresh_memo();

    let first = ensure_update_availability(
        &source,
        UpdateCheckDecision::Run,
        version(0, 1, 0),
        BUDGET,
        &memo,
        None,
    );
    let second = ensure_update_availability(
        &source,
        UpdateCheckDecision::Run,
        version(0, 1, 0),
        BUDGET,
        &memo,
        None,
    );

    assert_eq!(first, second);
    // ONE check per launch: the second ensure returns the memo verbatim.
    assert_eq!(source.fetch_count(), 1);
}

#[test]
fn a_memoized_skip_verdict_also_short_circuits_the_second_call() {
    // StrictMode/navigation double-reads on a skipped copy must not
    // re-trace: one launch, one line.
    let dir = TempDir::new().expect("tempdir");
    let log = dir.path().join("update.jsonl");
    let source = MockUpdateReleaseSource::new();
    let memo = fresh_memo();
    let decision = UpdateCheckDecision::Skip {
        reason: UpdateCheckSkipReason::UnofficialInstall,
    };

    let first = ensure_update_availability(
        &source,
        decision,
        version(0, 1, 0),
        BUDGET,
        &memo,
        Some(&log),
    );
    let second = ensure_update_availability(
        &source,
        decision,
        version(0, 1, 0),
        BUDGET,
        &memo,
        Some(&log),
    );

    assert_eq!(first, UpdateAvailability::CheckNotRun);
    assert_eq!(second, UpdateAvailability::CheckNotRun);
    assert_eq!(source.fetch_count(), 0);
    let content = std::fs::read_to_string(&log).expect("log written");
    assert_eq!(
        content
            .lines()
            .filter(|line| line.contains("update_check_skipped"))
            .count(),
        1,
        "one launch, one trace line — the memoized answer never re-traces"
    );
}

#[test]
fn concurrent_ensures_share_one_single_flight_consultation() {
    // The single-flight proof: simultaneous callers must NEVER race a
    // second consultation — one fetch, one diagnostics line, one shared
    // verdict for everyone. The mock's fetch delay widens the window a
    // broken memo would fall into (each racer would then drain the
    // queue's single tag or read the empty-queue fallback, splitting
    // the verdicts — caught by both assertions below).
    let dir = TempDir::new().expect("tempdir");
    let log = dir.path().join("update.jsonl");
    let source = MockUpdateReleaseSource::new();
    source.enqueue_latest_tag("v9.9.9");
    source.set_fetch_delay(Duration::from_millis(50));
    let memo = fresh_memo();

    let verdicts: Vec<UpdateAvailability> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    ensure_update_availability(
                        &source,
                        UpdateCheckDecision::Run,
                        version(0, 1, 0),
                        BUDGET,
                        &memo,
                        Some(&log),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("racer thread"))
            .collect()
    });

    assert_eq!(source.fetch_count(), 1, "exactly ONE consultation");
    assert!(
        verdicts.iter().all(|verdict| *verdict
            == UpdateAvailability::UpdateAvailable {
                latest: version(9, 9, 9)
            }),
        "every concurrent caller shares the settled verdict"
    );
    let content = std::fs::read_to_string(&log).expect("log written");
    assert_eq!(
        content.lines().count(),
        1,
        "one launch, one diagnostics line — never a duplicate"
    );
}

#[test]
fn a_missing_diagnostics_home_never_degrades_the_verdict() {
    // `log_path: None` (unresolvable app-data dir): the trace is
    // skipped, the verdict stays whole.
    let source = MockUpdateReleaseSource::new();
    source.enqueue_latest_tag("v9.9.9");
    let memo = fresh_memo();

    let availability = ensure_update_availability(
        &source,
        UpdateCheckDecision::Run,
        version(0, 1, 0),
        BUDGET,
        &memo,
        None,
    );

    assert_eq!(
        availability,
        UpdateAvailability::UpdateAvailable {
            latest: version(9, 9, 9)
        }
    );
}
