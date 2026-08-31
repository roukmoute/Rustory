//! APS acceptance harness for the Rustory Tauri backend.
//!
//! The generated entry points (`acceptance/generated/entry_points.rs`,
//! produced by `src-tauri/tools/acceptance_entrypoint_generator.py`) call
//! `runtime::run_execution(scenario, example)`, which replays the
//! scenario steps of the Gherkin IR — env `RUSTORY_ACCEPTANCE_IR` when
//! set, otherwise the committed `acceptance/ir/base.json` — against the
//! closed vocabulary of `acceptance/handlers.rs`.
//!
//! Generated acceptance tests are kept separate from the unit tests:
//! neither replaces the other.

#[path = "acceptance/runtime.rs"]
mod runtime;

#[path = "acceptance/handlers.rs"]
mod handlers;

#[path = "acceptance/generated/entry_points.rs"]
mod generated;
