//! Run-key resolution: explicit value or the GitHub environment.
//!
//! Every event-time entrypoint (plan, merge, report, retrieve,
//! publish, execute-check) keys its run directory by one run key:
//! an explicit request value wins, else `r<run-id>-a<attempt>`
//! derives from `GITHUB_RUN_ID`/`GITHUB_RUN_ATTEMPT`. Centralizing
//! the fallback here keeps the six hub call sites plus the
//! merge-request port on one derivation.

use std::env;

use velnor_actions_contract::{run_key_for_ci, validate_run_key};

use crate::{OrchestratorError, internal, internal_contract};

/// Explicit run key, else `r<run-id>-a<attempt>` from the GitHub environment.
pub fn resolve_run_key(explicit: Option<&str>) -> Result<String, OrchestratorError> {
    if let Some(key) = explicit.filter(|key| !key.trim().is_empty()) {
        validate_run_key(key).map_err(internal_contract)?;
        return Ok(key.to_owned());
    }
    let id = env::var("GITHUB_RUN_ID").ok().filter(|v| !v.is_empty());
    let attempt = env::var("GITHUB_RUN_ATTEMPT")
        .ok()
        .filter(|v| !v.is_empty());
    let (Some(id), Some(attempt)) = (id, attempt) else {
        return Err(internal("missing_run_key"));
    };
    let id: u64 = id.parse().map_err(|_| internal("bad_run_id"))?;
    let attempt: u64 = attempt.parse().map_err(|_| internal("bad_run_attempt"))?;
    Ok(run_key_for_ci(id, attempt))
}

#[cfg(test)]
mod tests;
