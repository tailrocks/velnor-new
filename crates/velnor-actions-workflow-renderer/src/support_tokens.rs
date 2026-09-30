//! Token-hygiene gate for rendered jobs (split from `support`).
//!
//! Declared from `support.rs` (`#[path]`, no `lib.rs` edit).

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID},
};

/// Token hygiene: `${{ github.token }}` only as plan/final `GH_TOKEN`.
///
/// Parallelism §5: the planning process receives the token only as
/// `GH_TOKEN`, never printed or inherited by task execution; before any
/// repository task starts, credential variables are stripped from the
/// child environment. The final job's report fetch needs the same
/// read-only token for exact `gh` artifact downloads. Enforced here
/// against the full seven-key credential set (single source:
/// [`crate::toolchain_env::STEP_CREDENTIAL_DENYLIST`], never a local
/// copy): any nonempty credential fails everywhere, `GH_TOKEN` carries
/// the exact `${{ github.token }}` value only in the plan/final jobs,
/// empty values are the explicit scrub overlay stopping ambient
/// inheritance, and no `run:` content or action input may name a token
/// (nothing prints or forwards one).
pub(crate) fn check_token_hygiene(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    for (id, job) in jobs {
        for step in &job.steps {
            check_step_tokens(id, step)?;
        }
    }
    Ok(())
}

/// Reject credential leaks in one step's env, argv, and action inputs.
fn check_step_tokens(id: &str, step: &Step) -> Result<(), RenderError> {
    match &step.kind {
        StepKind::Shell { run, env } => {
            check_env_tokens(id, env)?;
            for arg in run {
                if names_token(arg) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_run:{id}:{}",
                        step.name
                    )));
                }
            }
        }
        StepKind::Action { with, .. } => {
            for value in with.values() {
                if names_token(value) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "token_in_action_input:{id}:{}",
                        step.name
                    )));
                }
            }
        }
        StepKind::Internal { .. } => {}
    }
    Ok(())
}

/// Reject nonempty credentials everywhere; scope `GH_TOKEN` to plan/final jobs.
///
/// Empty values on denylisted keys are the explicit scrub overlay (see
/// [`crate::toolchain_env::credential_scrub`]) and pass; anything else
/// on those keys is a leak. `MISE_GITHUB_TOKEN` contains `GITHUB_TOKEN`
/// as a substring, so it needs no separate `names_token` arm.
fn check_env_tokens(id: &str, env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    use crate::toolchain_env::STEP_CREDENTIAL_DENYLIST;
    for (key, value) in env {
        if STEP_CREDENTIAL_DENYLIST.contains(&key.as_str()) {
            if value.is_empty() {
                continue;
            }
            if key == "GH_TOKEN" {
                let scoped =
                    (id == PLAN_JOB_ID || id == FINAL_JOB_ID) && value == "${{ github.token }}";
                if scoped {
                    continue;
                }
                return Err(RenderError::InvalidWorkflow(format!(
                    "token_misplaced:{id}:GH_TOKEN"
                )));
            }
            return Err(RenderError::InvalidWorkflow(format!(
                "credential_env:{id}:{key}"
            )));
        }
        if names_token(value) {
            return Err(RenderError::InvalidWorkflow(format!(
                "token_in_env:{id}:{key}"
            )));
        }
    }
    Ok(())
}

/// True when text names a token handle (never printed or forwarded).
fn names_token(text: &str) -> bool {
    text.contains("GH_TOKEN")
        || text.contains("GITHUB_TOKEN")
        || text.contains("ACTIONS_RUNTIME_TOKEN")
        || text.contains("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        || text.contains("ACTIONS_ID_TOKEN_REQUEST_URL")
        || text.contains("CARGO_REGISTRY_TOKEN")
        || text.contains("github.token")
        || text.contains("secrets.GITHUB_TOKEN")
}
