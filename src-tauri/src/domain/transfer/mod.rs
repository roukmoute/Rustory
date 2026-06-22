//! Transfer domain layer.
//!
//! Canonical, framework-free types for the transfer flow. The first occupant is
//! the story-preparation model (story 3.x); the actual write + verification
//! phases land in later stories. Strictly independent of `infrastructure/`,
//! `application/` and `tauri::*`.

pub mod preparation;

pub use preparation::{
    ensure_descriptor_coherent, gate_prepare, verify_aggregate, PreparationFailureCause,
    PreparationPhase, PreparedArtifact, PreparedArtifactKind, TransferArtifactDescriptor,
    PREPARATION_PIPELINE_VERSION,
};
