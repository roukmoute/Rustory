//! Settings-context commands: the pure reads behind the
//! `Profil de support` screen. No `application/settings/` layer — the
//! command delegates straight to the domain accessors + DTO mapping
//! (build-time constants, no observed conflict justifies the
//! abstraction — the exact choice of `read_content_source_policy`).

use crate::domain::device::official_device_support_matrix;
use crate::domain::import::official_local_artifacts;
use crate::ipc::dto::settings::SupportProfileDto;

/// Read the official support profile: the device support matrix and
/// the local-artifact registry of this distribution, with their frozen
/// labels and per-limit reasons (`Support Profile Screen Contract`). A
/// PURE, synchronous read of the domain matrices — zero network, zero
/// DB, zero lock: the frontend renders what Rust declares and never
/// hardcodes a family, cohort, kind, label or reason. Infallible by
/// construction (the matrices are build-time constants), hence no
/// `Result`. The content sources are NOT served here —
/// `read_content_source_policy` stays their single truth and the
/// screen reads both independently (fail-closed per section).
#[tauri::command]
pub fn read_support_profile() -> SupportProfileDto {
    SupportProfileDto::from_matrices(official_device_support_matrix(), official_local_artifacts())
}
