//! Settings-context commands: the pure reads behind the
//! `Profil de support` screen. No `application/settings/` layer — the
//! command delegates straight to the domain accessors + DTO mapping
//! (build-time constants, no observed conflict justifies the
//! abstraction — the exact choice of `read_content_source_policy`).

use crate::domain::device::official_device_support_matrix;
use crate::domain::import::official_local_artifacts;
#[cfg(not(target_os = "linux"))]
use crate::domain::import::LinuxInstallKind;
#[cfg(target_os = "linux")]
use crate::domain::import::{classify_linux_install, LinuxInstallKind, LINUX_PACKAGE_MIME_XML};
use crate::ipc::dto::settings::SupportProfileDto;

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
