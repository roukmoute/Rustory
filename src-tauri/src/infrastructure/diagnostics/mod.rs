//! Local diagnostics subsystem.
//!
//! For MVP Phase 1 the only producer is the recovery flow. The crate-wide
//! `tracing` setup remains deferred until a broader operational need
//! emerges (device flow, transfer pipeline). This module is intentionally
//! tiny: append-only JSONL writers with disciplined PII handling.

pub mod recovery_log;
