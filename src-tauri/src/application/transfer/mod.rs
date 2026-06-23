//! Transfer application services.
//!
//! Orchestrates the transfer flow on top of the device + filesystem
//! infrastructure. The first occupant is story preparation (story 3.x); the
//! actual write + verification steps land in later stories. Stays Tauri-free:
//! the runtime (event emission, `AppHandle`) lives in the command layer; this
//! module only sees the [`prepare::PreparationEventEmitter`] trait.

pub mod outcome;
pub mod prepare;
// Shares the parent module's name on purpose: it owns the device-write
// orchestration, kept separate from the preparation service.
#[allow(clippy::module_inception)]
pub mod transfer;

pub use outcome::{
    discard_transfer_outcome, read_transfer_outcome, record_transfer_outcome, StoredTransferOutcome,
};
pub use prepare::{
    prepare_story, read_preparation_state, PreparationEventEmitter, PreparationOutcome,
    PreparationStateView,
};
pub use transfer::{read_transfer_state, transfer_story, TransferOutcome, TransferStateView};
