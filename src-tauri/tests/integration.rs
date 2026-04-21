// Integration tests root — Cargo only auto-discovers `tests/*.rs`; this file
// wires up `tests/integration/*.rs` modules so they compile and run.
#[path = "integration/storage_init.rs"]
mod storage_init;
