//! Update-apply orchestration (`Update Apply Contract`): the session
//! job of the gesture — start decision (eligibility + single-flight),
//! the blocking worker's transitions, the SAMPLED progress events, the
//! diagnostics lines and the session state every read re-authorizes.
//! This layer consumes the gateway trait so the whole sequence is
//! testable with a mock and no plugin; the commands stay THIN frontiers.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::domain::update::{
    update_apply_failed_headline, update_apply_failed_notice, UpdateApplyFailureStage,
    UpdateApplyMode, UpdateApplyPhase, UpdateApplyState,
};
use crate::infrastructure::diagnostics::update_log;
use crate::infrastructure::updates::{UpdateApplyGateway, UpdateApplyProgressTick};

/// The session's gesture state behind ONE mutex — the authoritative
/// truth every `read_update_apply_state` serves (events are a comfort,
/// never the truth). Holds the DOMAIN type PLUS the session job's
/// correlation id — authoritative too, so a frontend that lost its
/// tracked id (renderer reload, unmounted start resolution) can
/// re-attach to a live flight from the re-read alone. One mutex over
/// both: the single-flight claim and the id mint stay atomic. No
/// persistence by contract (after a restart the running version IS the
/// proof). The guard only ever covers a read or a single write — never
/// a network step.
#[derive(Default)]
pub struct UpdateApplySession {
    inner: Mutex<SessionInner>,
}

#[derive(Default)]
struct SessionInner {
    state: UpdateApplyState,
    job_id: Option<String>,
}

/// One authoritative snapshot of the session: the gesture state and the
/// session job's correlation id (the id of the LAST accepted start —
/// kept through its terminal so a late reader can still correlate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateApplySessionSnapshot {
    pub state: UpdateApplyState,
    pub job_id: Option<String>,
}

impl UpdateApplySession {
    pub fn new() -> Self {
        Self::default()
    }

    /// The authoritative snapshot of the gesture's session.
    pub fn snapshot(&self) -> UpdateApplySessionSnapshot {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        UpdateApplySessionSnapshot {
            state: inner.state,
            job_id: inner.job_id.clone(),
        }
    }

    /// Write one state transition (brief lock — never held across the
    /// gateway's work). The session job id is untouched: it only moves
    /// under an accepted start.
    fn write(&self, next: UpdateApplyState) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .state = next;
    }
}

/// The start decision's closed outcome — a REFUSAL is a state, never an
/// error (the wire mirrors it verbatim).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartUpdateApplyOutcome {
    /// The gesture is accepted and claimed: the session state is already
    /// `Running { Checking }` and the worker may run under `job_id`.
    Started { job_id: String },
    /// A gesture is already running or ready to restart — ONE gesture
    /// per session until a terminal; a failure re-opens the right.
    AlreadyInFlight,
    /// The re-decided plan is manual: this copy never mutates itself —
    /// fail-closed, whatever the frontend believed.
    NotEligible,
}

/// Decide ONE start attempt atomically: the RE-DECIDED plan gates first
/// (fail-closed — a manual copy never starts, whatever the frontend
/// claimed), then the single-flight bound (a running or
/// ready-to-restart gesture refuses; `Idle` and `Failed` accept — a
/// failure re-opens the right to start). An accepted start claims the
/// session state (`Running { Checking }`) AND mints the job's
/// correlation id UNDER THE SAME LOCK, so two concurrent starts can
/// never both claim the flight and the authoritative snapshot always
/// correlates the flight it describes.
pub fn start_update_apply(
    mode: UpdateApplyMode,
    session: &UpdateApplySession,
) -> StartUpdateApplyOutcome {
    if !matches!(mode, UpdateApplyMode::Integrated) {
        return StartUpdateApplyOutcome::NotEligible;
    }
    let mut inner = session
        .inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match inner.state {
        UpdateApplyState::Running { .. } | UpdateApplyState::ReadyToRestart => {
            StartUpdateApplyOutcome::AlreadyInFlight
        }
        UpdateApplyState::Idle | UpdateApplyState::Failed { .. } => {
            let job_id = uuid::Uuid::now_v7().to_string();
            inner.state = UpdateApplyState::Running {
                phase: UpdateApplyPhase::Checking,
                percent: None,
            };
            inner.job_id = Some(job_id.clone());
            StartUpdateApplyOutcome::Started { job_id }
        }
    }
}

/// Event sink of the gesture's job — the `PreparationEventEmitter`
/// pattern: the Tauri impl lives in `commands/`, the integration tests
/// record the sequence. The failure copies arrive COMPOSED (this layer
/// owns the one composition point, from the domain's pure couples) so
/// every impl transports them verbatim.
pub trait UpdateApplyEventEmitter: Send + Sync {
    fn progress(&self, job_id: &str, phase: UpdateApplyPhase, percent: Option<u8>, sequence: u64);
    fn completed(&self, job_id: &str, sequence: u64);
    fn failed(
        &self,
        job_id: &str,
        sequence: u64,
        stage: UpdateApplyFailureStage,
        headline: &str,
        notice: &str,
    );
}

/// Run ONE accepted gesture to its terminal, blocking — the
/// `spawn_blocking` worker's body. The session state is written at
/// EVERY transition BEFORE its event is emitted (the re-read is the
/// truth, the event a comfort); progress is SAMPLED — an event fires
/// when the integer percent or the phase changes, never per chunk; the
/// per-job `sequence` is strictly increasing across progress and
/// terminal alike. Diagnostics: one `update_apply_started` line at the
/// effective start, one terminal line — never a line per chunk.
pub fn run_update_apply(
    gateway: &dyn UpdateApplyGateway,
    emitter: &dyn UpdateApplyEventEmitter,
    session: &UpdateApplySession,
    log_path: Option<&Path>,
    job_id: &str,
) {
    record(log_path, update_log::Event::UpdateApplyStarted);

    let sequence = AtomicU64::new(0);
    let next_sequence = || sequence.fetch_add(1, Ordering::SeqCst);

    // The start already claimed `Running { Checking }`; this names the
    // transition on the wire so a subscribed UI sees the job breathe
    // before the first network tick.
    let last_emitted = Mutex::new((UpdateApplyPhase::Checking, None::<u8>));
    emitter.progress(job_id, UpdateApplyPhase::Checking, None, next_sequence());

    let outcome = gateway.check_and_apply(&|tick| {
        let (phase, percent) = match tick {
            UpdateApplyProgressTick::Downloading { percent } => {
                (UpdateApplyPhase::Downloading, percent)
            }
            UpdateApplyProgressTick::Installing => (UpdateApplyPhase::Installing, None),
        };
        // Sampling gate: only a CHANGED (phase, integer percent) couple
        // transitions — never an event (nor a state write) per chunk.
        {
            let mut last = last_emitted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *last == (phase, percent) {
                return;
            }
            *last = (phase, percent);
        }
        session.write(UpdateApplyState::Running { phase, percent });
        emitter.progress(job_id, phase, percent, next_sequence());
    });

    match outcome {
        Ok(()) => {
            session.write(UpdateApplyState::ReadyToRestart);
            emitter.completed(job_id, next_sequence());
            record(log_path, update_log::Event::UpdateApplyCompleted);
        }
        Err(stage) => {
            session.write(UpdateApplyState::Failed { stage });
            emitter.failed(
                job_id,
                next_sequence(),
                stage,
                update_apply_failed_headline(stage),
                update_apply_failed_notice(stage),
            );
            record(
                log_path,
                update_log::Event::UpdateApplyFailed {
                    stage: stage.token(),
                },
            );
        }
    }
}

/// Run ONE accepted gesture under a PANIC supervisor — what the
/// command's `spawn_blocking` worker actually executes. A panic inside
/// the gateway (the plugin's `block_on`: network, URL parse, zip
/// extraction) or an emitter would kill the worker WITHOUT a terminal:
/// the session would stay `Running` forever and the single-flight would
/// refuse every later start until the app restarts — « a failure
/// re-opens the right to start » must hold on this path too (the
/// transfer worker's join supervision, same defensive discipline). The
/// death is NON-CLASSIFIABLE, so the terminal claims the most honest
/// closed stage — `Install` (the current copy is intact, the retry is
/// offered) — its event rides the always-winning `u64::MAX` sequence
/// (idempotent consumers keep the highest) and the regular
/// `update_apply_failed` line is traced.
pub fn run_update_apply_supervised(
    gateway: &dyn UpdateApplyGateway,
    emitter: &dyn UpdateApplyEventEmitter,
    session: &UpdateApplySession,
    log_path: Option<&Path>,
    job_id: &str,
) {
    // AssertUnwindSafe: on a worker death the captured references are
    // only ever used to settle the defensive terminal, and every lock
    // they reach recovers from poisoning (`poisoned.into_inner()`).
    let worker = catch_unwind(AssertUnwindSafe(|| {
        run_update_apply(gateway, emitter, session, log_path, job_id);
    }));
    if worker.is_err() {
        let stage = UpdateApplyFailureStage::Install;
        session.write(UpdateApplyState::Failed { stage });
        emitter.failed(
            job_id,
            u64::MAX,
            stage,
            update_apply_failed_headline(stage),
            update_apply_failed_notice(stage),
        );
        record(
            log_path,
            update_log::Event::UpdateApplyFailed {
                stage: stage.token(),
            },
        );
    }
}

/// Best-effort trace line — a diagnostics failure never degrades the
/// gesture, and no diagnostics home simply skips the line.
fn record(log_path: Option<&Path>, event: update_log::Event) {
    if let Some(path) = log_path {
        let _ = update_log::record_event_at_path(path, event);
    }
}
