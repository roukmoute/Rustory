//! Update-availability orchestration (`Update Availability Contract`):
//! the once-per-launch consultation — decision, fetch, pure resolution,
//! diagnostics line, session memo. The catalog pattern: this layer
//! consumes the infrastructure trait so the whole sequence is testable
//! with a mock source and no network; the command stays a THIN frontier.
//!
//! The `apply` submodule carries the SEPARATE orchestration of the
//! update GESTURE (`Update Apply Contract`) — same testability
//! discipline, its own session state and event family.

pub mod apply;

pub use apply::{
    run_update_apply, run_update_apply_supervised, start_update_apply, StartUpdateApplyOutcome,
    UpdateApplyEventEmitter, UpdateApplySession, UpdateApplySessionSnapshot,
};

use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::domain::update::{
    format_release_version, resolve_availability, ReleaseVersion, UpdateAvailability,
    UpdateCheckDecision,
};
use crate::infrastructure::diagnostics::update_log;
use crate::infrastructure::updates::UpdateReleaseSource;

/// The session's single-flight memo: the "one check per launch" bound,
/// STRICT under concurrency. Three sealed phases behind ONE mutex —
/// idle, in-flight, settled — plus a condvar so concurrent callers WAIT
/// for the settled verdict instead of racing a second consultation. No
/// `MutexGuard` is ever held across the network: the guard only covers
/// the phase transitions (`Condvar::wait` releases the lock while
/// parked). Holds the DOMAIN type — this layer never depends on `ipc`.
#[derive(Default)]
pub struct UpdateCheckMemo {
    state: Mutex<MemoPhase>,
    settled: Condvar,
}

/// The memo's sealed lifecycle. `InFlight` is claimed by EXACTLY one
/// caller; everyone else parks on the condvar until `Settled`.
#[derive(Default)]
enum MemoPhase {
    #[default]
    Idle,
    InFlight,
    Settled(UpdateAvailability),
}

impl UpdateCheckMemo {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Re-arms the memo if the flight ends without settling (a panicking
/// mock source, a poisoned path): the phase returns to `Idle` and the
/// waiters are woken to retry — parked callers must never wait forever
/// on a flight that died.
struct FlightGuard<'a> {
    memo: &'a UpdateCheckMemo,
    settled: bool,
}

impl FlightGuard<'_> {
    /// Publish the settled verdict and wake every parked caller.
    fn settle(mut self, availability: UpdateAvailability) {
        let mut phase = self
            .memo
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *phase = MemoPhase::Settled(availability);
        self.settled = true;
        self.memo.settled.notify_all();
    }
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        if !self.settled {
            let mut phase = self
                .memo
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *phase = MemoPhase::Idle;
            self.memo.settled.notify_all();
        }
    }
}

/// Resolve THE launch's update-availability verdict — EXACTLY one
/// consultation per launch, single-flight under concurrency:
///
/// 1. a settled memo returns as-is; an in-flight consultation PARKS the
///    caller until the shared verdict settles (one fetch, one
///    diagnostics line, one verdict for everyone — never a duplicate);
/// 2. a `Skip` decision settles on `CheckNotRun` and traces its motive
///    (`update_check_skipped`) — the wire never carries the motive;
/// 3. a `Run` decision fetches within `budget` — NO lock held across
///    the network — resolves PURELY against the running version (a
///    transport failure maps to the calm `CheckUnavailable`, never an
///    error), traces the settled line (`update_check_completed` /
///    `update_check_unreachable`) and settles the memo.
///
/// The trace is best-effort by contract (`let _ = …`): losing a line
/// never degrades the verdict. `log_path` is `None` when no diagnostics
/// home exists (the app-data dir could not be resolved).
pub fn ensure_update_availability(
    source: &dyn UpdateReleaseSource,
    decision: UpdateCheckDecision,
    current: ReleaseVersion,
    budget: Duration,
    memo: &UpdateCheckMemo,
    log_path: Option<&Path>,
) -> UpdateAvailability {
    // Phase transition under the lock: claim the flight, or return /
    // park on the already-known outcome. `Condvar::wait` RELEASES the
    // lock while parked — no guard ever survives into the network.
    {
        let mut phase = memo
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            match *phase {
                MemoPhase::Settled(availability) => return availability,
                MemoPhase::InFlight => {
                    phase = memo
                        .settled
                        .wait(phase)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                MemoPhase::Idle => {
                    *phase = MemoPhase::InFlight;
                    break;
                }
            }
        }
    }
    let flight = FlightGuard {
        memo,
        settled: false,
    };

    let availability = match decision {
        UpdateCheckDecision::Skip { reason } => {
            record(
                log_path,
                update_log::Event::UpdateCheckSkipped {
                    reason: reason.log_token(),
                },
            );
            UpdateAvailability::CheckNotRun
        }
        UpdateCheckDecision::Run => match source.fetch_latest(budget) {
            Ok(probe) => {
                let availability = resolve_availability(current, &probe);
                record(
                    log_path,
                    update_log::Event::UpdateCheckCompleted {
                        result: availability.wire_tag(),
                        latest: match availability {
                            UpdateAvailability::UpdateAvailable { latest } => {
                                Some(format_release_version(latest))
                            }
                            _ => None,
                        },
                    },
                );
                availability
            }
            Err(stage) => {
                record(
                    log_path,
                    update_log::Event::UpdateCheckUnreachable {
                        stage: stage.token(),
                    },
                );
                UpdateAvailability::CheckUnavailable
            }
        },
    };

    flight.settle(availability);
    availability
}

/// Best-effort trace line — a diagnostics failure never degrades the
/// verdict, and no diagnostics home simply skips the line.
fn record(log_path: Option<&Path>, event: update_log::Event) {
    if let Some(path) = log_path {
        let _ = update_log::record_event_at_path(path, event);
    }
}
