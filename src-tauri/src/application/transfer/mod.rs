//! Transfer application services.
//!
//! Orchestrates the transfer flow on top of the device + filesystem
//! infrastructure. The first occupant is story preparation (story 3.x); the
//! actual write + verification steps land in later stories. Stays Tauri-free:
//! the runtime (event emission, `AppHandle`) lives in the command layer; this
//! module only sees the [`prepare::PreparationEventEmitter`] trait.

pub mod prepare;

pub use prepare::{
    prepare_story, read_preparation_state, PreparationEventEmitter, PreparationOutcome,
    PreparationStateView,
};
