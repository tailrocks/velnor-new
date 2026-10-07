//! Event-time `fetch-reports-v1` entrypoint over the retrieve crate.
//!
//! Extracted from the orchestrator hub as a dependency-free leaf: the
//! module binds the run ID, run key, and run directory from the runner
//! environment, then delegates to the retrieve crate's counting core.
//! The hub keeps the entrypoint byte-identical through its `api` re-export.

pub mod retrieve_reports;

pub use retrieve_reports::{FETCH_OP, retrieve_reports};
