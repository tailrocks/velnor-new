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
#[path = "tap_merge_protection_observer.rs"]
mod protection_observer;
#[path = "review_completion.rs"]
pub(crate) mod review_completion;
#[path = "tap_merge_tree.rs"]
mod tree;

use std::{
    env,
    time::{Duration, Instant},
};
use velnor_actions_contract::{
    ContractError,
    homebrew_merge_gate::{
        TapMergeObservation, TapMergePolicy, TapServerMergeProtection, validate_tap_merge_gate,
    },
};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

pub const MERGE_HOMEBREW_TAP_PR_OP: &str = "merge-homebrew-tap-pr-v1";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapMergeOutcome {
    Pending,
    Merged,
    AlreadyPromoted,
    SkippedStaleSource,
}

pub fn merge_homebrew_tap_from_env() -> Result<TapMergeOutcome, OrchestratorError> {
    crate::package_release_publish::artifact_zip::within_operation(merge_once)
}

fn merge_once() -> Result<TapMergeOutcome, OrchestratorError> {
    let candidate = candidate::load_candidate()?;
    if candidate.plan.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Preview
        && !crate::tap_source_freshness::verify_preview_source_is_current(
            &candidate.plan.repository,
            &candidate.plan.default_branch,
            candidate.release.receipt.repository.id,
            &candidate.release.receipt.source.commit_sha,
        )?
    {
        return Ok(TapMergeOutcome::SkippedStaleSource);
    }
    let (api, authority, observer) = authenticate_merge(&candidate)?;
    if candidate
        .selection()
        .ok_or_else(|| internal("tap_merge_skipped_promotion_invalid"))?
        .merged
        .is_some()
    {
        completed::verify_already_promoted(&candidate, &authority, &api, &observer)?;
        return Ok(TapMergeOutcome::AlreadyPromoted);
    }
    attempt_once(
        &authority.policy,
        || {
            crate::package_release_publish::verify_tap_update_environment(
                &candidate.plan.repository,
                &candidate.plan.default_branch,
            )?;
            observation::observe(&candidate, &authority, &api, &observer)
        },
        || {
            let signed = crate::package_release_signer::verify_attested_signed_package_from_env()?;
            crate::package_release_publish::verify_published_release(&signed)?;
            if signed.receipt().canonical_digest().ok().as_deref()
                != Some(candidate.release.receipt_digest.as_str())
            {
                return Err(internal("tap_merge_release_changed"));
            }
            if candidate.plan.channel
                == velnor_actions_contract::package_release::PackageReleaseChannel::Preview
                && !crate::tap_source_freshness::verify_preview_source_is_current(
                    &candidate.plan.repository,
                    &candidate.plan.default_branch,
                    candidate.release.receipt.repository.id,
                    &candidate.release.receipt.source.commit_sha,
                )?
            {
                return Err(internal("tap_merge_source_superseded_during_operation"));
            }
            Ok(())
        },
        |observed| merge_validated_head(&candidate, &api, observed),
    )
}

fn attempt_once(
    policy: &TapMergePolicy,
    mut observe: impl FnMut() -> Result<Option<TapMergeObservation>, OrchestratorError>,
    refresh_source: impl FnOnce() -> Result<(), OrchestratorError>,
    merge: impl FnOnce(&TapMergeObservation) -> Result<(), OrchestratorError>,
) -> Result<TapMergeOutcome, OrchestratorError> {
    let Some(prior) = observe()? else {
        return Ok(TapMergeOutcome::Pending);
    };
    if !gate_ready_or_pending(policy, &prior)? {
        return Ok(TapMergeOutcome::Pending);
    }
    let Some(final_observation) = observe()? else {
        return Ok(TapMergeOutcome::Pending);
    };
    if !gate_ready_or_pending(policy, &final_observation)? {
        return Ok(TapMergeOutcome::Pending);
    }
    if final_observation.current_head_sha != prior.current_head_sha
        || final_observation.current_base_sha != prior.current_base_sha
        || final_observation.server_protection != TapServerMergeProtection::PullRequestProtected
    {
        return Err(internal("tap_merge_final_state_changed"));
    }
    refresh_source()?;
    merge(&final_observation)?;
    Ok(TapMergeOutcome::Merged)
}

fn merge_validated_head(
    candidate: &candidate::MergeCandidate,
    api: &github::TapMergeGitHub,
    observed: &TapMergeObservation,
) -> Result<(), OrchestratorError> {
    let merged: serde_json::Value = api.merge(
        &candidate.policy.repository,
        candidate
            .selection()
            .ok_or_else(|| internal("tap_merge_selection_missing"))?
            .pull_request,
        &observed.current_head_sha,
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

fn authenticate_merge(
    candidate: &candidate::MergeCandidate,
) -> Result<
    (
        github::TapMergeGitHub,
        observation::MergeAuthority,
        protection_observer::ProtectionObserver,
    ),
    OrchestratorError,
> {
    let merge_token = required_token("VELNOR_TAP_MERGE_TOKEN")?;
    let source_token = required_token("GH_TOKEN")?;
    let observer_token = required_token("VELNOR_TAP_PROTECTION_TOKEN")?;
    protection_observer::validate_distinct_tokens(&observer_token, &merge_token, &source_token)?;
    if merge_token == source_token
        || env::var("VELNOR_TAP_TOKEN").is_ok_and(|writer| writer == merge_token)
    {
        return Err(internal("tap_merge_credentials_reused"));
    }
    let budget = Duration::from_secs(
        u64::from(velnor_actions_contract::workflow::JobTimeout::DEFAULT_MINUTES) * 60,
    );
    let remaining = CheckDeadline::after(budget)
        .and_then(CheckDeadline::remaining)
        .map_err(|_| internal("tap_merge_deadline_elapsed"))?;
    let api = github::TapMergeGitHub::with_deadline(merge_token, Instant::now() + remaining);
    let authority = observation::load_merge_authority(candidate, &api)?;
    let observer = protection_observer::ProtectionObserver::authenticate(
        candidate,
        &authority,
        observer_token,
        api.deadline(),
    )?;
    Ok((api, authority, observer))
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

fn required_token(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("tap_merge_token_missing"))
}
