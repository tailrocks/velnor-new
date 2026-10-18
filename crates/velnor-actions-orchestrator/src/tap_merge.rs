//! Exact-head, feedback-gated Homebrew tap merge.

#[path = "tap_merge_candidate.rs"]
mod candidate;
#[path = "tap_merge_completed.rs"]
mod completed;
#[path = "tap_merge_feedback.rs"]
mod feedback;
#[path = "tap_merge_github.rs"]
mod github;
#[path = "tap_merge_history.rs"]
mod history;
#[path = "tap_merge_observation.rs"]
mod observation;
#[path = "tap_merge_protection.rs"]
mod protection;
#[path = "tap_merge_tree.rs"]
mod tree;

use std::{
    env, thread,
    time::{Duration, Instant},
};
use velnor_actions_contract::{
    ContractError,
    homebrew_merge_gate::{
        TapMergeObservation, TapMergePolicy, TapServerMergeProtection, validate_tap_merge_gate,
    },
};

use crate::{OrchestratorError, internal::internal};

/// Fixed private operation selecting the independently credentialed tap merge.
pub const MERGE_HOMEBREW_TAP_PR_OP: &str = "merge-homebrew-tap-pr-v1";
const POLL_INTERVAL: Duration = Duration::from_secs(15);
const MAX_WAIT: Duration = Duration::from_mins(45);

/// Reauthenticate candidate and PR, await exact-head gates, then squash with SHA precondition.
/// # Errors
/// Rejects stale or conflicting PRs, failed checks, incomplete feedback, and API errors.
pub fn merge_homebrew_tap_from_env() -> Result<(), OrchestratorError> {
    let candidate = candidate::load_candidate()?;
    let merge_token = required_token("VELNOR_TAP_MERGE_TOKEN")?;
    let source_token = required_token("GH_TOKEN")?;
    if merge_token == source_token
        || env::var("VELNOR_TAP_TOKEN").is_ok_and(|writer| writer == merge_token)
    {
        return Err(internal("tap_merge_credentials_reused"));
    }
    let api = github::TapMergeGitHub::with_max_wait(merge_token, MAX_WAIT);
    let authority = observation::load_merge_authority(&candidate, &api)?;
    if candidate.selection().merged.is_some() {
        return completed::verify_already_promoted(&candidate, &authority, &api);
    }
    let prior = poll_until_ready(
        || {
            crate::package_release_publish::verify_tap_update_environment(
                &candidate.plan.repository,
                &candidate.plan.default_branch,
            )?;
            if let Some(observed) = observation::observe(&candidate, &authority, &api)?
                && gate_ready_or_pending(&authority.policy, &observed)?
            {
                return Ok(Some(observed));
            }
            Ok(None)
        },
        thread::sleep,
        || internal("tap_merge_gates_timeout"),
        api.deadline(),
        Instant::now,
    )?;
    crate::package_release_publish::verify_tap_update_environment(
        &candidate.plan.repository,
        &candidate.plan.default_branch,
    )?;
    crate::tap_source_freshness::verify_preview_source_is_current(
        &candidate.plan.repository,
        &candidate.plan.default_branch,
        &candidate.release.receipt.source.commit_sha,
    )?;
    let final_observation = observation::observe(&candidate, &authority, &api)?
        .ok_or_else(|| internal("tap_merge_final_gates_pending"))?;
    validate_tap_merge_gate(&authority.policy, &final_observation)
        .map_err(|_| internal("tap_merge_final_gate_rejected"))?;
    if final_observation.current_head_sha != prior.current_head_sha
        || final_observation.current_base_sha != prior.current_base_sha
        || final_observation.server_protection
            != TapServerMergeProtection::StrictWithoutWriterBypass
    {
        return Err(internal("tap_merge_final_state_changed"));
    }
    let merged: serde_json::Value = api.merge(
        &candidate.policy.repository,
        candidate.selection().pull_request,
        &final_observation.current_head_sha,
    )?;
    if merged["merged"].as_bool() != Some(true)
        || !merged["sha"].as_str().is_some_and(|sha| {
            sha.len() == 40
                && sha
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    {
        return Err(internal("tap_merge_response_mismatch"));
    }
    Ok(())
}

fn gate_ready_or_pending(
    policy: &TapMergePolicy,
    observed: &TapMergeObservation,
) -> Result<bool, OrchestratorError> {
    match validate_tap_merge_gate(policy, observed) {
        Ok(()) => Ok(true),
        Err(ContractError::InvalidIdentity { field, problem })
            if field == "homebrew_merge_gate" && problem == "review_completion_missing"
                || field == "homebrew_merge_gate"
                    && problem == "unaddressed_feedback"
                    && observed.completion.is_none() =>
        {
            Ok(false)
        }
        Err(_) => Err(internal("tap_merge_gate_rejected")),
    }
}

fn poll_until_ready<T, E>(
    mut poll: impl FnMut() -> Result<Option<T>, E>,
    mut wait: impl FnMut(Duration),
    timeout: impl FnOnce() -> E,
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
) -> Result<T, E> {
    loop {
        if now() >= deadline {
            return Err(timeout());
        }
        let value = poll()?;
        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() {
            return Err(timeout());
        }
        if let Some(value) = value {
            return Ok(value);
        }
        wait(POLL_INTERVAL.min(remaining));
    }
}

fn required_token(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("tap_merge_token_missing"))
}

#[cfg(test)]
#[path = "tap_merge_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tap_merge_protocol_tests.rs"]
mod protocol_tests;
