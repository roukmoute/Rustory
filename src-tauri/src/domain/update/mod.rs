//! Update domain — pure, framework-free, zero I/O. Two sealed
//! capabilities, deliberately DISTINCT: `availability` (the `Update
//! Availability Contract` — knowing that a newer OFFICIAL version exists,
//! information only) and `apply` (the `Update Apply Contract` — the
//! user-triggered GESTURE of installing it, gated by a stricter per-copy
//! plan). The information verdict decides WHETHER the gesture surface
//! exists; the signed feed alone decides WHAT the gesture applies.

pub mod apply;
pub mod availability;

pub use apply::{
    decide_update_apply, update_apply_failed_headline, update_apply_failed_notice,
    update_apply_plan_guidance, update_apply_plan_headline, update_apply_ready_headline,
    update_apply_ready_notice, update_apply_running_headline, update_apply_running_notice,
    ManualUpdateReason, UpdateApplyFailureStage, UpdateApplyMode, UpdateApplyPhase,
    UpdateApplyState,
};
pub use availability::{
    decide_update_check, format_release_version, parse_release_version, resolve_availability,
    update_headline, update_notice, ReleaseProbe, ReleaseVersion, UpdateAvailability,
    UpdateCheckDecision, UpdateCheckSkipReason,
};
