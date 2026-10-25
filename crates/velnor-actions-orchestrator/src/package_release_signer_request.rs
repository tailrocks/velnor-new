use super::{absolute_env, read_regular_file, require_private_directory};
use crate::{OrchestratorError, internal::internal};
use std::{env, path::Path, time::Duration};
use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePolicy, package_release::PackageReleaseChannel,
    parse_package_release_policy,
};
use velnor_actions_mise::CheckDeadline;

const POLICY_FILE: &str = "package-release-policy.json";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum VerificationRoute {
    Admitted,
    DirectTap,
    Retained,
}

pub(super) fn verification_route() -> Result<VerificationRoute, OrchestratorError> {
    route(
        env::var("VELNOR_INTERNAL_OP").ok().as_deref(),
        env::var("GITHUB_EVENT_NAME").ok().as_deref(),
        env::var("VELNOR_RELEASE_ORIGINAL_RUN_ID").ok().as_deref(),
        env::var("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT")
            .ok()
            .as_deref(),
    )
}

fn route(
    operation: Option<&str>,
    event: Option<&str>,
    original_run: Option<&str>,
    original_attempt: Option<&str>,
) -> Result<VerificationRoute, OrchestratorError> {
    if !matches!(
        operation,
        Some("admit-homebrew-tap-v1" | "write-homebrew-tap-pr-v1" | "merge-homebrew-tap-pr-v1")
    ) {
        return Ok(VerificationRoute::Admitted);
    }
    let has_run = original_run.is_some_and(|value| !value.is_empty());
    let has_attempt = original_attempt.is_some_and(|value| !value.is_empty());
    if has_run != has_attempt {
        return Err(internal("package_tap_original_identity_incomplete"));
    }
    match (event, has_run) {
        (Some("workflow_dispatch" | "schedule"), true) => Ok(VerificationRoute::Retained),
        (Some("workflow_run" | "workflow_dispatch"), false) => Ok(VerificationRoute::DirectTap),
        _ => Err(internal("package_tap_event_invalid")),
    }
}

fn policy_request(root: &Path) -> Result<PackageReleasePolicy, OrchestratorError> {
    require_private_directory(root)?;
    if absolute_env("VELNOR_REQUEST_FILE")? != root.join(POLICY_FILE) {
        return Err(internal("package_signer_request_path_mismatch"));
    }
    let bytes = read_regular_file(&root.join(POLICY_FILE), 1024 * 1024)?;
    let policy = parse_package_release_policy(&bytes)
        .map_err(|_| internal("package_signer_policy_invalid"))?;
    let canonical = policy
        .canonical_bytes()
        .map_err(|_| internal("package_signer_policy_invalid"))?;
    if bytes != canonical
        || env::var("VELNOR_RELEASE_POLICY").ok().as_deref() != std::str::from_utf8(&canonical).ok()
    {
        return Err(internal("package_signer_policy_substitution"));
    }
    Ok(policy)
}

pub(super) fn admitted_plan(root: &Path) -> Result<PackageReleasePlan, OrchestratorError> {
    let requested = policy_request(root)?;
    let deadline = CheckDeadline::after(Duration::from_secs(300))
        .map_err(|_| internal("package_signer_deadline"))?;
    let (policy, plan) = crate::release_policy_admission::admitted_policy_plan(root, deadline)?;
    if policy != requested {
        return Err(internal("package_signer_policy_substitution"));
    }
    Ok(plan)
}

pub(super) fn direct_tap_plan(root: &Path) -> Result<PackageReleasePlan, OrchestratorError> {
    if verification_route()? != VerificationRoute::DirectTap {
        return Err(internal("package_tap_read_operation_invalid"));
    }
    let requested = policy_request(root)?;
    let event = env::var("GITHUB_EVENT_NAME").unwrap_or_default();
    if event != requested.channel.event().as_str()
        || (requested.channel == PackageReleaseChannel::Stable
            && env::var("VELNOR_STABLE_OPERATION").ok().as_deref() != Some("release"))
    {
        return Err(internal("package_tap_event_invalid"));
    }
    let (policy, plan) = crate::release_policy_admission::resolve_policy_from_env(
        &env::var("GITHUB_SHA").map_err(|_| internal("package_tap_source_missing"))?,
    )?;
    if policy != requested {
        return Err(internal("package_signer_policy_substitution"));
    }
    Ok(plan)
}
