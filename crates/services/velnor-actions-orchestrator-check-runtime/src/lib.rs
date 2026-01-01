//! Event-time `execute-check-v1` entrypoint over the runtime-execute crate.
//!
//! Extracted from the orchestrator hub as a dependency-free leaf: the
//! module binds the six check identity env vars plus the run key, then
//! delegates to [`execute_check_to`](velnor_actions_orchestrator_runtime_execute::execute::execute_check_to).
//! The hub keeps the entrypoint byte-identical through its `api` re-export.

pub mod check_runtime;

pub use check_runtime::{EXECUTE_CHECK_OP, execute_check};
