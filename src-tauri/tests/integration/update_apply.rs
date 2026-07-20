//! Integration of the update-apply orchestration (`Update Apply
//! Contract`): start decision → worker transitions → sampled events →
//! terminal session state + diagnostics, exercised end-to-end with a
//! programmable gateway mock and a recording emitter (the
//! `MockUpdateReleaseSource` pattern) — zero plugin, zero network, zero
//! Tauri runtime.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use rustory_lib::application::update::{
    run_update_apply, run_update_apply_supervised, start_update_apply, StartUpdateApplyOutcome,
    UpdateApplyEventEmitter, UpdateApplySession,
};
use rustory_lib::domain::update::{
    update_apply_failed_headline, update_apply_failed_notice, ManualUpdateReason,
    UpdateApplyFailureStage, UpdateApplyMode, UpdateApplyPhase, UpdateApplyState,
};
use rustory_lib::infrastructure::updates::{UpdateApplyGateway, UpdateApplyProgressTick};
use tempfile::TempDir;

/// Programmable gateway mock: replays its scripted progress ticks, then
/// pops the next queued outcome (FIFO — an empty queue answers success)
/// and counts every attempt. An optional delay widens the concurrency
/// window for the single-flight proof.
#[derive(Default)]
struct MockUpdateApplyGateway {
    ticks: Mutex<Vec<UpdateApplyProgressTick>>,
    outcomes: Mutex<Vec<Result<(), UpdateApplyFailureStage>>>,
    calls: AtomicU32,
    delay: Mutex<Option<Duration>>,
}

impl MockUpdateApplyGateway {
    fn new() -> Self {
        Self::default()
    }

    fn script_ticks(&self, ticks: Vec<UpdateApplyProgressTick>) {
        *self
            .ticks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = ticks;
    }

    fn enqueue(&self, outcome: Result<(), UpdateApplyFailureStage>) {
        self.outcomes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(outcome);
    }

    fn set_delay(&self, delay: Duration) {
        *self
            .delay
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(delay);
    }

    fn call_count(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }
}

impl UpdateApplyGateway for MockUpdateApplyGateway {
    fn check_and_apply(
        &self,
        on_progress: &(dyn Fn(UpdateApplyProgressTick) + Send + Sync),
    ) -> Result<(), UpdateApplyFailureStage> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let delay = *self
            .delay
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(delay) = delay {
            std::thread::sleep(delay);
        }
        let ticks = self
            .ticks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        for tick in ticks {
            on_progress(tick);
        }
        let mut outcomes = self
            .outcomes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if outcomes.is_empty() {
            Ok(())
        } else {
            outcomes.remove(0)
        }
    }
}

/// One recorded emission — the emitter's whole surface, verbatim.
#[derive(Debug, Clone, PartialEq)]
enum Recorded {
    Progress {
        job_id: String,
        phase: UpdateApplyPhase,
        percent: Option<u8>,
        sequence: u64,
    },
    Completed {
        job_id: String,
        sequence: u64,
    },
    Failed {
        job_id: String,
        sequence: u64,
        stage: UpdateApplyFailureStage,
        headline: String,
        notice: String,
    },
}

#[derive(Default)]
struct RecordingEmitter {
    events: Mutex<Vec<Recorded>>,
}

impl RecordingEmitter {
    fn new() -> Self {
        Self::default()
    }

    fn events(&self) -> Vec<Recorded> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl UpdateApplyEventEmitter for RecordingEmitter {
    fn progress(&self, job_id: &str, phase: UpdateApplyPhase, percent: Option<u8>, sequence: u64) {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(Recorded::Progress {
                job_id: job_id.to_string(),
                phase,
                percent,
                sequence,
            });
    }

    fn completed(&self, job_id: &str, sequence: u64) {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(Recorded::Completed {
                job_id: job_id.to_string(),
                sequence,
            });
    }

    fn failed(
        &self,
        job_id: &str,
        sequence: u64,
        stage: UpdateApplyFailureStage,
        headline: &str,
        notice: &str,
    ) {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(Recorded::Failed {
                job_id: job_id.to_string(),
                sequence,
                stage,
                headline: headline.to_string(),
                notice: notice.to_string(),
            });
    }
}

fn accepted_job_id(session: &UpdateApplySession) -> String {
    match start_update_apply(UpdateApplyMode::Integrated, session) {
        StartUpdateApplyOutcome::Started { job_id } => job_id,
        other => panic!("expected an accepted start, got {other:?}"),
    }
}

fn sequences(events: &[Recorded]) -> Vec<u64> {
    events
        .iter()
        .map(|event| match event {
            Recorded::Progress { sequence, .. }
            | Recorded::Completed { sequence, .. }
            | Recorded::Failed { sequence, .. } => *sequence,
        })
        .collect()
}

#[test]
fn the_nominal_flow_walks_the_phases_to_ready_to_restart() {
    let dir = TempDir::new().expect("tempdir");
    let log = dir.path().join("update.jsonl");
    let gateway = MockUpdateApplyGateway::new();
    gateway.script_ticks(vec![
        UpdateApplyProgressTick::Downloading { percent: Some(0) },
        UpdateApplyProgressTick::Downloading { percent: Some(50) },
        UpdateApplyProgressTick::Downloading { percent: Some(100) },
        UpdateApplyProgressTick::Installing,
    ]);
    let emitter = RecordingEmitter::new();
    let session = UpdateApplySession::new();

    let job_id = accepted_job_id(&session);
    assert_eq!(
        session.snapshot().state,
        UpdateApplyState::Running {
            phase: UpdateApplyPhase::Checking,
            percent: None
        },
        "an accepted start claims the checking state under the lock"
    );
    assert_eq!(
        session.snapshot().job_id.as_deref(),
        Some(job_id.as_str()),
        "the accepted start mints the correlation id INTO the authoritative session"
    );

    run_update_apply(&gateway, &emitter, &session, Some(&log), &job_id);

    let events = emitter.events();
    assert_eq!(
        events,
        vec![
            Recorded::Progress {
                job_id: job_id.clone(),
                phase: UpdateApplyPhase::Checking,
                percent: None,
                sequence: 0
            },
            Recorded::Progress {
                job_id: job_id.clone(),
                phase: UpdateApplyPhase::Downloading,
                percent: Some(0),
                sequence: 1
            },
            Recorded::Progress {
                job_id: job_id.clone(),
                phase: UpdateApplyPhase::Downloading,
                percent: Some(50),
                sequence: 2
            },
            Recorded::Progress {
                job_id: job_id.clone(),
                phase: UpdateApplyPhase::Downloading,
                percent: Some(100),
                sequence: 3
            },
            Recorded::Progress {
                job_id: job_id.clone(),
                phase: UpdateApplyPhase::Installing,
                percent: None,
                sequence: 4
            },
            Recorded::Completed {
                job_id: job_id.clone(),
                sequence: 5
            },
        ],
        "the phases walk in order, one event per named transition"
    );
    let seqs = sequences(&events);
    assert!(
        seqs.windows(2).all(|pair| pair[1] > pair[0]),
        "sequences are strictly increasing across progress and terminal"
    );
    assert_eq!(session.snapshot().state, UpdateApplyState::ReadyToRestart);

    let content = std::fs::read_to_string(&log).expect("log written");
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "one started line + one terminal line — never a line per chunk"
    );
    assert!(lines[0].contains("\"category\":\"update_apply_started\""));
    assert!(lines[1].contains("\"category\":\"update_apply_completed\""));
}

#[test]
fn every_failure_stage_lands_on_its_terminal_with_the_frozen_copies() {
    for stage in [
        UpdateApplyFailureStage::Feed,
        UpdateApplyFailureStage::NotApplicable,
        UpdateApplyFailureStage::Download,
        UpdateApplyFailureStage::Verification,
        UpdateApplyFailureStage::Install,
    ] {
        let dir = TempDir::new().expect("tempdir");
        let log = dir.path().join("update.jsonl");
        let gateway = MockUpdateApplyGateway::new();
        gateway.enqueue(Err(stage));
        let emitter = RecordingEmitter::new();
        let session = UpdateApplySession::new();

        let job_id = accepted_job_id(&session);
        run_update_apply(&gateway, &emitter, &session, Some(&log), &job_id);

        assert_eq!(
            session.snapshot().state,
            UpdateApplyState::Failed { stage },
            "stage {stage:?} must settle the failed session state"
        );
        // The installation stays INTACT: one attempt, zero retry — the
        // gateway is never re-consulted by the worker itself.
        assert_eq!(gateway.call_count(), 1);
        let events = emitter.events();
        assert_eq!(
            events.last(),
            Some(&Recorded::Failed {
                job_id: job_id.clone(),
                sequence: 1,
                stage,
                headline: update_apply_failed_headline(stage).to_string(),
                notice: update_apply_failed_notice(stage).to_string(),
            }),
            "the terminal event carries the domain's frozen couple for {stage:?}"
        );
        let content = std::fs::read_to_string(&log).expect("log written");
        assert!(content.contains("\"category\":\"update_apply_failed\""));
        assert!(content.contains(&format!("\"stage\":\"{}\"", stage.token())));
    }
}

#[test]
fn a_second_start_during_a_flight_is_refused_with_one_single_gateway_attempt() {
    let gateway = MockUpdateApplyGateway::new();
    gateway.set_delay(Duration::from_millis(100));
    let emitter = RecordingEmitter::new();
    let session = UpdateApplySession::new();

    let job_id = accepted_job_id(&session);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            run_update_apply(&gateway, &emitter, &session, None, &job_id);
        });
        // Give the worker time to enter the gateway, then race a second
        // start into the flight window.
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            start_update_apply(UpdateApplyMode::Integrated, &session),
            StartUpdateApplyOutcome::AlreadyInFlight,
            "one gesture per session: a running flight refuses a new start"
        );
        worker.join().expect("worker thread");
    });

    assert_eq!(gateway.call_count(), 1, "exactly ONE gateway attempt");
    assert_eq!(session.snapshot().state, UpdateApplyState::ReadyToRestart);
}

#[test]
fn a_ready_to_restart_gesture_still_refuses_a_new_start() {
    let gateway = MockUpdateApplyGateway::new();
    let emitter = RecordingEmitter::new();
    let session = UpdateApplySession::new();

    let job_id = accepted_job_id(&session);
    run_update_apply(&gateway, &emitter, &session, None, &job_id);
    assert_eq!(session.snapshot().state, UpdateApplyState::ReadyToRestart);

    assert_eq!(
        start_update_apply(UpdateApplyMode::Integrated, &session),
        StartUpdateApplyOutcome::AlreadyInFlight,
        "an applied update waiting for its restart never re-downloads"
    );
    assert_eq!(gateway.call_count(), 1);
}

#[test]
fn a_failed_gesture_reopens_the_right_to_start_with_a_new_job() {
    let gateway = MockUpdateApplyGateway::new();
    gateway.enqueue(Err(UpdateApplyFailureStage::Download));
    let emitter = RecordingEmitter::new();
    let session = UpdateApplySession::new();

    let first_job = accepted_job_id(&session);
    run_update_apply(&gateway, &emitter, &session, None, &first_job);
    assert_eq!(
        session.snapshot().state,
        UpdateApplyState::Failed {
            stage: UpdateApplyFailureStage::Download
        }
    );

    // The failure re-opens the right: a NEW job, a NEW flight (the
    // emptied queue answers success this time).
    let second_job = accepted_job_id(&session);
    assert_ne!(first_job, second_job, "a retry is a NEW job");
    assert_eq!(
        session.snapshot().job_id.as_deref(),
        Some(second_job.as_str()),
        "the re-accepted start replaces the session's correlation id"
    );
    run_update_apply(&gateway, &emitter, &session, None, &second_job);

    assert_eq!(gateway.call_count(), 2);
    assert_eq!(session.snapshot().state, UpdateApplyState::ReadyToRestart);
}

#[test]
fn a_manual_plan_never_starts_nor_touches_the_session() {
    let gateway = MockUpdateApplyGateway::new();
    let session = UpdateApplySession::new();

    for reason in [
        ManualUpdateReason::DevelopmentBuild,
        ManualUpdateReason::UnofficialInstall,
        ManualUpdateReason::PackageManagerOwned,
        ManualUpdateReason::ChannelUnproven,
        ManualUpdateReason::TrustChainNotConfigured,
    ] {
        assert_eq!(
            start_update_apply(UpdateApplyMode::Manual { reason }, &session),
            StartUpdateApplyOutcome::NotEligible,
            "a manual copy never mutates itself ({reason:?})"
        );
    }
    // Fail-closed and side-effect-free: zero gateway attempt, zero
    // transition — the session stays idle.
    assert_eq!(gateway.call_count(), 0);
    assert_eq!(session.snapshot().state, UpdateApplyState::Idle);
    assert_eq!(
        session.snapshot().job_id,
        None,
        "a refused start never mints a correlation id"
    );
}

#[test]
fn an_unknown_content_length_keeps_the_percent_absent_end_to_end() {
    let gateway = MockUpdateApplyGateway::new();
    gateway.script_ticks(vec![
        UpdateApplyProgressTick::Downloading { percent: None },
        UpdateApplyProgressTick::Downloading { percent: None },
        UpdateApplyProgressTick::Installing,
    ]);
    let emitter = RecordingEmitter::new();
    let session = UpdateApplySession::new();

    let job_id = accepted_job_id(&session);
    run_update_apply(&gateway, &emitter, &session, None, &job_id);

    let events = emitter.events();
    // Checking, ONE percentless downloading (the identical repeat is
    // sampled out), installing, completed — and never an invented digit.
    assert_eq!(
        events,
        vec![
            Recorded::Progress {
                job_id: job_id.clone(),
                phase: UpdateApplyPhase::Checking,
                percent: None,
                sequence: 0
            },
            Recorded::Progress {
                job_id: job_id.clone(),
                phase: UpdateApplyPhase::Downloading,
                percent: None,
                sequence: 1
            },
            Recorded::Progress {
                job_id: job_id.clone(),
                phase: UpdateApplyPhase::Installing,
                percent: None,
                sequence: 2
            },
            Recorded::Completed {
                job_id: job_id.clone(),
                sequence: 3
            },
        ]
    );
}

#[test]
fn progress_is_sampled_one_event_per_integer_percent_never_a_duplicate() {
    let gateway = MockUpdateApplyGateway::new();
    // A chunk storm: repeated couples must collapse to ONE event each.
    gateway.script_ticks(vec![
        UpdateApplyProgressTick::Downloading { percent: Some(0) },
        UpdateApplyProgressTick::Downloading { percent: Some(0) },
        UpdateApplyProgressTick::Downloading { percent: Some(0) },
        UpdateApplyProgressTick::Downloading { percent: Some(1) },
        UpdateApplyProgressTick::Downloading { percent: Some(1) },
        UpdateApplyProgressTick::Installing,
        UpdateApplyProgressTick::Installing,
    ]);
    let emitter = RecordingEmitter::new();
    let session = UpdateApplySession::new();

    let job_id = accepted_job_id(&session);
    run_update_apply(&gateway, &emitter, &session, None, &job_id);

    let events = emitter.events();
    let progress_couples: Vec<(UpdateApplyPhase, Option<u8>)> = events
        .iter()
        .filter_map(|event| match event {
            Recorded::Progress { phase, percent, .. } => Some((*phase, *percent)),
            _ => None,
        })
        .collect();
    assert_eq!(
        progress_couples,
        vec![
            (UpdateApplyPhase::Checking, None),
            (UpdateApplyPhase::Downloading, Some(0)),
            (UpdateApplyPhase::Downloading, Some(1)),
            (UpdateApplyPhase::Installing, None),
        ],
        "one event per changed (phase, integer percent) couple"
    );
    assert!(
        progress_couples.windows(2).all(|pair| pair[0] != pair[1]),
        "never two consecutive identical progress events"
    );
}

/// A gateway whose flight DIES: the panic models the plugin's
/// `block_on` (network, URL parse, zip extraction) or an emitter
/// blowing up inside the worker.
struct PanickingGateway;

impl UpdateApplyGateway for PanickingGateway {
    fn check_and_apply(
        &self,
        _on_progress: &(dyn Fn(UpdateApplyProgressTick) + Send + Sync),
    ) -> Result<(), UpdateApplyFailureStage> {
        panic!("worker death mid-gateway");
    }
}

#[test]
fn a_dead_worker_settles_the_defensive_terminal_and_reopens_the_right_to_start() {
    let dir = TempDir::new().expect("tempdir");
    let log = dir.path().join("update.jsonl");
    let emitter = RecordingEmitter::new();
    let session = UpdateApplySession::new();

    let job_id = accepted_job_id(&session);
    run_update_apply_supervised(&PanickingGateway, &emitter, &session, Some(&log), &job_id);

    // The supervisor settled the honest defensive terminal: a worker
    // death must never leave the session `Running` (the single-flight
    // would refuse every later start until the app restarts).
    assert_eq!(
        session.snapshot().state,
        UpdateApplyState::Failed {
            stage: UpdateApplyFailureStage::Install
        },
        "a dead worker settles Failed, never a forever-running session"
    );
    assert_eq!(
        emitter.events().last(),
        Some(&Recorded::Failed {
            job_id: job_id.clone(),
            sequence: u64::MAX,
            stage: UpdateApplyFailureStage::Install,
            headline: update_apply_failed_headline(UpdateApplyFailureStage::Install).to_string(),
            notice: update_apply_failed_notice(UpdateApplyFailureStage::Install).to_string(),
        }),
        "the defensive terminal rides the always-winning sequence"
    );
    let content = std::fs::read_to_string(&log).expect("log written");
    assert!(content.contains("\"category\":\"update_apply_failed\""));
    assert!(content.contains("\"stage\":\"install\""));

    // The failure re-opens the right: a NEW start is accepted, and the
    // supervised nominal path still walks to its regular terminal.
    let second_job = accepted_job_id(&session);
    assert_ne!(job_id, second_job, "the retry is a NEW job");
    let recovered = MockUpdateApplyGateway::new();
    run_update_apply_supervised(&recovered, &emitter, &session, None, &second_job);
    assert_eq!(session.snapshot().state, UpdateApplyState::ReadyToRestart);
}

#[test]
fn a_missing_diagnostics_home_never_degrades_the_gesture() {
    let gateway = MockUpdateApplyGateway::new();
    let emitter = RecordingEmitter::new();
    let session = UpdateApplySession::new();

    let job_id = accepted_job_id(&session);
    run_update_apply(&gateway, &emitter, &session, None, &job_id);

    assert_eq!(session.snapshot().state, UpdateApplyState::ReadyToRestart);
}
