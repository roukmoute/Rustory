//! Transfer domain layer.
//!
//! Canonical, framework-free types for the transfer flow: the story-preparation
//! model (assembles LOCALLY what a transfer would need) and the story-transfer
//! model (writes the prepared pack back to the device). The verification phase
//! lands in a later story. Strictly independent of `infrastructure/`,
//! `application/` and `tauri::*`.

pub mod preparation;
// The submodule shares the parent module's name on purpose: it owns the
// transfer-specific pure rules, kept separate from the preparation model.
#[allow(clippy::module_inception)]
pub mod transfer;

pub use preparation::{
    ensure_descriptor_coherent, gate_prepare, verify_aggregate, PreparationFailureCause,
    PreparationPhase, PreparedArtifact, PreparedArtifactKind, TransferArtifactDescriptor,
    PREPARATION_PIPELINE_VERSION,
};
pub use transfer::{
    append_pack_uuid, build_write_plan, classify, classify_verify, compose_verified_summary,
    ensure_cohort_coherent, failure_copy, pack_uuid_bytes, short_id_from_pack_uuid, ChecksumProbe,
    PackWriteFile, PackWritePlan, TransferCompleteness, TransferFailureCause, VerifiedSummary,
    VerifyVerdict,
};
