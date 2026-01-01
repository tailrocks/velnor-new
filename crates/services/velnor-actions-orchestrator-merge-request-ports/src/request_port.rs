//! Run-key resolution consumed by merge-request assembly, behind a port trait.
//!
//! Assembly calls exactly this core function. The hub implements
//! [`RequestPort`] by delegating to the core run-key module, so
//! merge-request assembly depends only on this contract and the direct
//! `internal_request`/`merge_request` cycle is broken.
//!
//! Staged-report reads were considered for this port but moved with
//! assembly instead: merge-request building is their sole production
//! caller, so the reader lives beside its owner rather than behind
//! the trait.

use velnor_actions_orchestrator_core::OrchestratorError;

/// Run-key resolution for request assembly.
pub trait RequestPort {
    /// Explicit run key, else `r<run-id>-a<attempt>` from the GitHub environment.
    fn resolve_run_key(&self, explicit: Option<&str>) -> Result<String, OrchestratorError>;
}
