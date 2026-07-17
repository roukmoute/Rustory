//! Settings-context commands: the pure reads behind the
//! `Profil de support` screen and the once-per-launch update-availability
//! read. No `application/settings/` layer for the PURE profile read — the
//! command delegates straight to the domain accessors + DTO mapping
//! (build-time constants, no observed conflict justifies the
//! abstraction — the exact choice of `read_content_source_policy`). The
//! update read, by contrast, orchestrates network + log + memo and goes
//! through its own `application::update` layer.

use std::path::Path;
use std::time::Duration;

use tauri::Manager;

use crate::application::update::{ensure_update_availability, UpdateCheckMemo};
use crate::domain::device::official_device_support_matrix;
use crate::domain::import::official_local_artifacts;
#[cfg(not(target_os = "linux"))]
use crate::domain::import::LinuxInstallKind;
#[cfg(target_os = "linux")]
use crate::domain::import::{classify_linux_install, LinuxInstallKind, LINUX_PACKAGE_MIME_XML};
use crate::domain::update::{decide_update_check, parse_release_version, UpdateCheckDecision};
use crate::infrastructure::diagnostics::update_log;
use crate::infrastructure::updates::UpdateReleaseSource;
use crate::ipc::dto::settings::{SupportProfileDto, UpdateAvailabilityDto};

/// Wall-clock budget of the whole update-availability consultation,
/// connection to last body byte (`Update Availability Contract`).
const UPDATE_CHECK_BUDGET: Duration = Duration::from_secs(10);

/// Thin frontier of the Linux install probe: read the raw observations
/// (`APPIMAGE` marker via `tauri::Env`, the current executable path,
/// the presence of the package's shared-mime-info XML — the package
/// artifact witnessing a declared association) and hand them to the
/// PURE `classify_linux_install` — every decision, including the
/// corroboration of a possibly inherited/polluted `APPIMAGE` marker,
/// lives in the domain; this glue only observes. The existence check
/// is infallible (`is_file` degrades to `false` on any error).
#[cfg(target_os = "linux")]
fn probe_current_install(app: &tauri::AppHandle) -> Option<LinuxInstallKind> {
    use tauri::Manager;
    let env = app.env();
    classify_linux_install(
        env.appimage.as_deref(),
        std::env::current_exe().ok().as_deref(),
        std::path::Path::new(LINUX_PACKAGE_MIME_XML).is_file(),
    )
}

/// No reliable install marker exists on Windows/macOS (no runtime
/// heuristics — the file-association lines document the channels, the
/// probe stays silent): the profile carries NO current-install claim.
#[cfg(not(target_os = "linux"))]
fn probe_current_install(_app: &tauri::AppHandle) -> Option<LinuxInstallKind> {
    None
}

/// Read the official support profile: the device support matrix, the
/// local-artifact registry and the file-association registry of this
/// distribution, with their frozen labels and per-limit reasons
/// (`Support Profile Screen Contract`). A PURE, synchronous read of
/// the domain matrices — zero network, zero DB, zero lock: the
/// frontend renders what Rust declares and never hardcodes a family,
/// cohort, kind, channel, label or reason. Infallible by construction
/// (the matrices are build-time constants and the install probe
/// degrades to "no claim"), hence no `Result`. The content sources are
/// NOT served here — `read_content_source_policy` stays their single
/// truth and the screen reads both independently (fail-closed per
/// section).
#[tauri::command]
pub fn read_support_profile(app: tauri::AppHandle) -> SupportProfileDto {
    SupportProfileDto::from_matrices(official_device_support_matrix(), official_local_artifacts())
        .with_linux_install(probe_current_install(&app))
}

/// Resolve the per-launch check decision, SHORT-CIRCUITING the install
/// probe on a development build: the contract skips a dev build "before
/// the probe is even consulted", so the probe closure must never run
/// there — the domain gate then decides on the probe's verdict for a
/// release copy. Kept separate (probe injected) so the short-circuit is
/// unit-provable without a Tauri runtime.
fn resolve_update_check_decision(
    is_debug_build: bool,
    probe: impl FnOnce() -> Option<LinuxInstallKind>,
) -> UpdateCheckDecision {
    if is_debug_build {
        return decide_update_check(true, None);
    }
    decide_update_check(false, probe())
}

/// Settle one launch's wire DTO: a CONVENTIONAL running version
/// consults (single-flight memo, budget, diagnostics — inside
/// `ensure_update_availability`) and serializes the verdict; an
/// out-of-convention one DEGRADES CALMLY — the `checkUnavailable`
/// couple with the raw version string, ZERO network dispatch (no
/// comparable referential exists, so there is nothing to consult) and
/// ZERO panic: a semver-legal but out-of-convention Cargo version in a
/// locally-built binary (the runbook's manual posture) must never
/// poison the responder and lose the whole session's verdict. The
/// domain tripwire and the three-manifest alignment lock stay the CI
/// guards of the convention. Kept separate (sync, source injected) so
/// both branches are unit-provable without a Tauri runtime.
fn settle_update_availability_dto(
    raw_current: &str,
    source: &dyn UpdateReleaseSource,
    decision: UpdateCheckDecision,
    budget: Duration,
    memo: &UpdateCheckMemo,
    log_path: Option<&Path>,
) -> UpdateAvailabilityDto {
    match parse_release_version(raw_current) {
        Some(current) => {
            let availability =
                ensure_update_availability(source, decision, current, budget, memo, log_path);
            UpdateAvailabilityDto::from_availability(availability, current)
        }
        None => UpdateAvailabilityDto::check_unavailable_with_raw_version(raw_current),
    }
}

/// Read THE launch's update-availability verdict (`Update Availability
/// Contract`). INFAILLIBLE by contract — never a `Result`, never a
/// panic: a transport failure is the `checkUnavailable` STATE of the
/// DTO, never a wire error (the absence of information is never an
/// error), and even a binary whose own version escapes the strict
/// convention degrades to the same calm state (see
/// `settle_update_availability_dto`). A THIN frontier (the assumed
/// command-level harness limit, like every command): the pure gate
/// decision rides `cfg!(debug_assertions)` + the EXISTING install probe
/// above (reused as-is, and NEVER consulted on a dev build), the
/// running version is `CARGO_PKG_VERSION` parsed by the domain parser
/// (never a second parser), and the whole consultation runs on a
/// `spawn_blocking` worker through `application::update` (memo, budget,
/// diagnostics).
///
/// The signature takes the `AppHandle` ALONE (the state is resolved in
/// the body): an async command borrowing `State<'_, …>` would force a
/// `Result` return — the exact shape this contract forbids.
#[tauri::command]
pub async fn read_update_availability(app: tauri::AppHandle) -> UpdateAvailabilityDto {
    let (source, memo) = {
        let state = app.state::<crate::AppState>();
        (
            state.update_release_source.clone(),
            state.update_availability.clone(),
        )
    };
    let decision =
        resolve_update_check_decision(cfg!(debug_assertions), || probe_current_install(&app));
    // No diagnostics home (unresolvable app-data dir) skips the trace,
    // never the verdict.
    let log_path = app
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| update_log::log_path_for(&dir));

    tauri::async_runtime::spawn_blocking(move || {
        settle_update_availability_dto(
            env!("CARGO_PKG_VERSION"),
            source.as_ref(),
            decision,
            UPDATE_CHECK_BUDGET,
            &memo,
            log_path.as_deref(),
        )
    })
    .await
    // A lost worker (join failure) settles on the calm state — the
    // command stays infallible to its last line.
    .unwrap_or_else(|_| {
        UpdateAvailabilityDto::check_unavailable_with_raw_version(env!("CARGO_PKG_VERSION"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::update::{ReleaseProbe, UpdateCheckSkipReason};
    use crate::infrastructure::updates::UpdateFetchStage;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Counting source: every dispatch is recorded — `0` proves the
    /// out-of-convention degradation never consults the network.
    #[derive(Default)]
    struct CountingSource {
        calls: AtomicU32,
    }

    impl UpdateReleaseSource for CountingSource {
        fn fetch_latest(&self, _budget: Duration) -> Result<ReleaseProbe, UpdateFetchStage> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ReleaseProbe::NoPublishedRelease)
        }
    }

    #[test]
    fn an_out_of_convention_binary_version_degrades_calmly_with_zero_dispatch() {
        // A semver-legal but out-of-convention Cargo version must never
        // panic the command nor consult the network: the calm
        // `checkUnavailable` couple with the RAW string.
        let source = CountingSource::default();
        let memo = UpdateCheckMemo::new();
        let dto = settle_update_availability_dto(
            "0.2.0-rc.1",
            &source,
            UpdateCheckDecision::Run,
            UPDATE_CHECK_BUDGET,
            &memo,
            None,
        );
        assert_eq!(dto.status, "checkUnavailable");
        assert_eq!(dto.current_version, "0.2.0-rc.1");
        assert_eq!(dto.latest_version, None);
        assert_eq!(
            source.calls.load(Ordering::SeqCst),
            0,
            "no comparable referential → zero network dispatch"
        );
    }

    #[test]
    fn a_conventional_binary_version_consults_and_serializes_the_verdict() {
        let source = CountingSource::default();
        let memo = UpdateCheckMemo::new();
        let dto = settle_update_availability_dto(
            "0.1.0",
            &source,
            UpdateCheckDecision::Run,
            UPDATE_CHECK_BUDGET,
            &memo,
            None,
        );
        // The counting source answers the no-release world → upToDate.
        assert_eq!(dto.status, "upToDate");
        assert_eq!(dto.current_version, "0.1.0");
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_debug_build_never_consults_the_install_probe() {
        // The contract's letter: a dev build skips "before the probe is
        // even consulted" — the closure must not run at all.
        let mut probed = false;
        let decision = resolve_update_check_decision(true, || {
            probed = true;
            Some(LinuxInstallKind::SystemPackage)
        });
        assert_eq!(
            decision,
            UpdateCheckDecision::Skip {
                reason: UpdateCheckSkipReason::DevelopmentBuild
            }
        );
        assert!(!probed, "the probe must never run on a debug build");
    }

    #[test]
    fn a_release_build_consults_the_probe_and_feeds_the_domain_gate() {
        let mut probed = false;
        let decision = resolve_update_check_decision(false, || {
            probed = true;
            Some(LinuxInstallKind::LocalBuild)
        });
        assert!(probed, "a release copy decides on the probe's verdict");
        assert_eq!(
            decision,
            UpdateCheckDecision::Skip {
                reason: UpdateCheckSkipReason::UnofficialInstall
            }
        );
        assert_eq!(
            resolve_update_check_decision(false, || None),
            UpdateCheckDecision::Run
        );
    }
}
