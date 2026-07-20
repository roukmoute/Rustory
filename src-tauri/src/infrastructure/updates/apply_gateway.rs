//! Applies an official update through the Tauri updater plugin
//! (`Update Apply Contract`).
//!
//! The gateway is the THIN seam between the session job orchestration
//! (`application::update::apply`) and the plugin: a mockable trait whose
//! production impl drives the async plugin to completion INSIDE the
//! blocking worker (`tauri::async_runtime::block_on` — never on the IPC
//! thread, never in an already-async context). Every failure maps to the
//! CLOSED, PII-free [`UpdateApplyFailureStage`] set — never a URL, never
//! a raw network message.
//!
//! The REAL updater client is deliberately not exercised by the suites
//! (network is forbidden in CI — the assumed thin-frontier limit, proven
//! at the native smoke): only the pure percent math and the
//! error-to-stage mapping are unit-tested here.

use crate::domain::update::UpdateApplyFailureStage;

/// The canonical signed feed of the `stable` channel: `latest.json`
/// attached to the latest PUBLISHED release (a draft never appears
/// there — publishing the draft is what promotes the feed). The
/// production constant — locked by a contract test.
pub const UPDATE_FEED_ENDPOINT: &str =
    "https://github.com/roukmoute/Rustory/releases/latest/download/latest.json";

/// Environment override of the feed endpoint — a smoke/local tool (the
/// product precedent: the availability check's endpoint override), read
/// ONCE at gateway construction. The production constant stays the wire
/// truth.
pub const UPDATE_FEED_ENDPOINT_ENV: &str = "RUSTORY_UPDATER_FEED_ENDPOINT";

/// Wall-clock budget of the FEED CONSULTATION (the plugin's `check`) —
/// one small manifest, connection to last body byte; aligned with the
/// availability check's budget. DELIBERATELY per-step: the download that
/// may follow carries NO wall-clock budget (the plugin builds its
/// `Update` with no timeout) — an artifact of tens of MiB takes the time
/// it takes, the sampled progress is the visibility. No custom size cap
/// either (the plugin exposes none; the MANDATORY signature verification
/// carries the integrity) — the deliberate inverse of the product's
/// bounded 1 MiB informational reads.
pub const UPDATE_FEED_CHECK_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

/// One progress observation of the gesture, pushed by the gateway while
/// it works. `Downloading.percent` is the integer 0..=100 derived from
/// the received bytes IFF the content length is known — never invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateApplyProgressTick {
    Downloading { percent: Option<u8> },
    Installing,
}

/// One complete check-download-verify-install attempt. Blocking by
/// contract (driven from a `spawn_blocking` worker); `on_progress` is
/// invoked from inside the attempt — implementations must never hold a
/// lock across it. `Ok(())` means the update is APPLIED (ready for
/// restart); every failure names the closed stage where the attempt
/// stopped, with the current installation intact.
pub trait UpdateApplyGateway: Send + Sync {
    fn check_and_apply(
        &self,
        on_progress: &(dyn Fn(UpdateApplyProgressTick) + Send + Sync),
    ) -> Result<(), UpdateApplyFailureStage>;
}

/// Integer percentage of `received` bytes against the announced total:
/// `None` while no reliable total is known (absent or zero
/// `Content-Length`) — never an invented fraction; clamped to 100 when a
/// lying server streams past its announced total. The u128 widening puts
/// the multiplication out of overflow's reach for any real transfer.
pub fn download_percent(received: u64, content_length: Option<u64>) -> Option<u8> {
    let total = content_length?;
    if total == 0 {
        return None;
    }
    Some(((received as u128) * 100 / (total as u128)).min(100) as u8)
}

/// Map a plugin failure DURING THE CHECK to its closed stage: a manifest
/// that answered without covering this target is the honest
/// `NotApplicable` ("not yet offered for this installation"); everything
/// else — network, hostile status, unreadable manifest, an invalid
/// endpoint — is the `Feed` consultation failure. Catch-all DELIBERATE:
/// the plugin's error set is wider than the check path (installer-side
/// variants can never surface here), and `Feed` is the most honest stage
/// for anything unclassified that did.
pub fn map_check_error(error: &tauri_plugin_updater::Error) -> UpdateApplyFailureStage {
    match error {
        tauri_plugin_updater::Error::TargetNotFound(_)
        | tauri_plugin_updater::Error::TargetsNotFound(_) => UpdateApplyFailureStage::NotApplicable,
        _ => UpdateApplyFailureStage::Feed,
    }
}

/// Map a plugin failure DURING THE DOWNLOAD to its closed stage: the
/// plugin verifies the artifact's signature at the end of its download
/// step, so the three authenticity refusals map to `Verification` (the
/// artifact is NEVER applied); everything else — transport, stream,
/// buffering — is the `Download` stage. Catch-all deliberate, same
/// honesty rule as the check mapping.
pub fn map_download_error(error: &tauri_plugin_updater::Error) -> UpdateApplyFailureStage {
    match error {
        tauri_plugin_updater::Error::Minisign(_)
        | tauri_plugin_updater::Error::Base64(_)
        | tauri_plugin_updater::Error::SignatureUtf8(_) => UpdateApplyFailureStage::Verification,
        _ => UpdateApplyFailureStage::Download,
    }
}

/// Production gateway: the official Tauri updater, configured 100% at
/// runtime (no static `plugins.updater` endpoints) — the canonical feed
/// endpoint (+ env override, resolved ONCE at construction) and the
/// compile-time public key the command frontier resolved. Only ever
/// constructed when the trust chain exists; the plan decision guards
/// every start, so a keyless copy never reaches this code.
pub struct TauriUpdaterGateway {
    app: tauri::AppHandle,
    endpoint: String,
    pubkey: String,
}

impl TauriUpdaterGateway {
    pub fn new(app: tauri::AppHandle, pubkey: String) -> Self {
        let endpoint = std::env::var(UPDATE_FEED_ENDPOINT_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| UPDATE_FEED_ENDPOINT.to_string());
        Self {
            app,
            endpoint,
            pubkey,
        }
    }
}

impl UpdateApplyGateway for TauriUpdaterGateway {
    fn check_and_apply(
        &self,
        on_progress: &(dyn Fn(UpdateApplyProgressTick) + Send + Sync),
    ) -> Result<(), UpdateApplyFailureStage> {
        // The plugin is async; this gateway is ALWAYS driven from a
        // `spawn_blocking` worker, where parking the thread on the
        // shared runtime is the documented pattern (never the IPC
        // thread, never an already-async context).
        tauri::async_runtime::block_on(async {
            use tauri_plugin_updater::UpdaterExt;

            let endpoint: tauri::Url = self
                .endpoint
                .parse()
                .map_err(|_| UpdateApplyFailureStage::Feed)?;
            let updater = self
                .app
                .updater_builder()
                .endpoints(vec![endpoint])
                .map_err(|error| map_check_error(&error))?
                .pubkey(self.pubkey.as_str())
                .timeout(UPDATE_FEED_CHECK_BUDGET)
                .build()
                .map_err(|error| map_check_error(&error))?;

            // `check` answers `None` when the feed offers nothing newer
            // for this copy — with the target-absent manifest error
            // mapped alongside, BOTH forms of "not applicable" land on
            // the same honest stage (the divergence window between the
            // information verdict and the feed is a state, never a lie).
            let update = updater
                .check()
                .await
                .map_err(|error| map_check_error(&error))?;
            let Some(update) = update else {
                return Err(UpdateApplyFailureStage::NotApplicable);
            };

            let mut received: u64 = 0;
            let bytes = update
                .download(
                    |chunk_length, content_length| {
                        received = received.saturating_add(chunk_length as u64);
                        on_progress(UpdateApplyProgressTick::Downloading {
                            percent: download_percent(received, content_length),
                        });
                    },
                    || {},
                )
                .await
                .map_err(|error| map_download_error(&error))?;

            on_progress(UpdateApplyProgressTick::Installing);
            update
                .install(bytes)
                .map_err(|_| UpdateApplyFailureStage::Install)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Honest percent math =====

    #[test]
    fn percent_is_absent_while_no_content_length_is_known() {
        assert_eq!(download_percent(0, None), None);
        assert_eq!(download_percent(123_456, None), None);
    }

    #[test]
    fn percent_is_absent_on_a_zero_announced_total() {
        // A zero total gives no reliable fraction (and no division).
        assert_eq!(download_percent(0, Some(0)), None);
        assert_eq!(download_percent(10, Some(0)), None);
    }

    #[test]
    fn percent_walks_the_integer_bounds() {
        assert_eq!(download_percent(0, Some(100)), Some(0));
        assert_eq!(download_percent(1, Some(200)), Some(0));
        assert_eq!(download_percent(50, Some(100)), Some(50));
        assert_eq!(download_percent(99, Some(100)), Some(99));
        assert_eq!(download_percent(100, Some(100)), Some(100));
    }

    #[test]
    fn percent_clamps_a_lying_server_at_100() {
        // More bytes than announced: clamp, never 101, never a panic.
        assert_eq!(download_percent(150, Some(100)), Some(100));
    }

    #[test]
    fn percent_survives_the_improbable_u64_extremes() {
        // The u128 widening keeps the multiplication out of overflow's
        // reach even at u64::MAX received.
        assert_eq!(download_percent(u64::MAX, Some(u64::MAX)), Some(100));
        assert_eq!(download_percent(u64::MAX / 2, Some(u64::MAX)), Some(49));
    }

    // ===== Error-to-stage mapping (closed, unit-locked) =====

    #[test]
    fn a_target_absent_from_the_manifest_maps_to_not_applicable() {
        assert_eq!(
            map_check_error(&tauri_plugin_updater::Error::TargetNotFound(
                "linux-x86_64".to_string()
            )),
            UpdateApplyFailureStage::NotApplicable
        );
        assert_eq!(
            map_check_error(&tauri_plugin_updater::Error::TargetsNotFound(vec![
                "linux-x86_64-appimage".to_string(),
                "linux-x86_64".to_string(),
            ])),
            UpdateApplyFailureStage::NotApplicable
        );
    }

    #[test]
    fn every_other_check_failure_maps_to_the_feed_stage() {
        for error in [
            tauri_plugin_updater::Error::ReleaseNotFound,
            tauri_plugin_updater::Error::EmptyEndpoints,
            tauri_plugin_updater::Error::Network("connection refused".to_string()),
            tauri_plugin_updater::Error::InsecureTransportProtocol,
            tauri_plugin_updater::Error::Io(std::io::Error::other("io")),
        ] {
            assert_eq!(
                map_check_error(&error),
                UpdateApplyFailureStage::Feed,
                "check failure {error:?} must land on the feed stage"
            );
        }
    }

    #[test]
    fn an_authenticity_refusal_maps_to_the_verification_stage() {
        // The constructible face of the plugin's verify-step refusals;
        // the `Minisign(_) | Base64(_)` arms share the same mapping arm,
        // locked by the match shape itself.
        assert_eq!(
            map_download_error(&tauri_plugin_updater::Error::SignatureUtf8(
                "rotten".to_string()
            )),
            UpdateApplyFailureStage::Verification
        );
    }

    #[test]
    fn every_other_download_failure_maps_to_the_download_stage() {
        for error in [
            tauri_plugin_updater::Error::Network("reset by peer".to_string()),
            tauri_plugin_updater::Error::Io(std::io::Error::other("stream")),
        ] {
            assert_eq!(
                map_download_error(&error),
                UpdateApplyFailureStage::Download,
                "download failure {error:?} must land on the download stage"
            );
        }
    }

    // ===== Engraved constants =====

    #[test]
    fn the_feed_endpoint_and_its_override_are_locked() {
        assert_eq!(
            UPDATE_FEED_ENDPOINT,
            "https://github.com/roukmoute/Rustory/releases/latest/download/latest.json"
        );
        assert_eq!(UPDATE_FEED_ENDPOINT_ENV, "RUSTORY_UPDATER_FEED_ENDPOINT");
        assert_eq!(UPDATE_FEED_CHECK_BUDGET.as_secs(), 10);
    }
}
