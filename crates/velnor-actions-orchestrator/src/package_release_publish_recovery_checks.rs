use super::super::github::{WorkflowArtifact, sha256};
use super::super::identity::is_git_sha;
use super::{
    GitHubApi, Invocation, MAX_ARCHIVE_BYTES, MAX_PLAN_BYTES, POLICY_FILE, Purpose, env_path,
    env_text, read_regular_bounded,
};
use crate::{OrchestratorError, internal::internal};
use std::path::Path;
use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePolicy, package_release::PackageReleaseChannel,
    parse_package_release_policy,
};

pub(super) fn recovery_policy(root: &Path) -> Result<PackageReleasePolicy, OrchestratorError> {
    let plan_path = root.join(POLICY_FILE);
    if env_path("VELNOR_REQUEST_FILE")? != plan_path {
        return Err(internal("package_recovery_request_path_mismatch"));
    }
    let plan_bytes = read_regular_bounded(&plan_path, MAX_PLAN_BYTES)?;
    let policy = parse_package_release_policy(&plan_bytes)
        .map_err(|_| internal("package_recovery_policy_invalid"))?;
    if policy
        .canonical_bytes()
        .map_err(|_| internal("package_recovery_policy_encoding"))?
        != env_text("VELNOR_RELEASE_POLICY")?.as_bytes()
    {
        return Err(internal("package_recovery_policy_substitution"));
    }
    Ok(policy)
}

pub(super) fn original_archive(
    api: &GitHubApi,
    policy: &PackageReleasePolicy,
    original_run_id: u64,
    original_attempt: u32,
    repository_id: u64,
) -> Result<(String, Vec<u8>), OrchestratorError> {
    let channel = match policy.channel {
        PackageReleaseChannel::Preview => "preview",
        PackageReleaseChannel::Stable => "stable",
    };
    let artifact_name = format!("velnor-{channel}-signed-{original_run_id}-{original_attempt}");
    let artifacts = api.workflow_artifacts(original_run_id)?;
    let matches: Vec<_> = artifacts
        .iter()
        .filter(|artifact| artifact.name == artifact_name)
        .collect();
    if matches.len() != 1 {
        return Err(internal("package_recovery_artifact_not_unique"));
    }
    let artifact = matches[0];
    let artifact_source_sha = validate_artifact(
        artifact,
        original_run_id,
        repository_id,
        &policy.default_branch,
    )?;
    let zip_bytes = api.download_artifact_archive(artifact.id)?;
    if zip_bytes.len() as u64 != artifact.size_in_bytes
        || artifact
            .digest
            .as_deref()
            .is_some_and(|digest| digest != format!("sha256:{}", sha256(&zip_bytes)))
    {
        return Err(internal("package_recovery_artifact_digest_mismatch"));
    }
    Ok((artifact_source_sha, zip_bytes))
}

pub(super) fn validate_recovery_context(
    plan: &PackageReleasePlan,
    purpose: Purpose,
) -> Result<(), OrchestratorError> {
    Invocation::from_env()?.validate(plan, purpose)
}

pub(super) fn validate_artifact(
    artifact: &WorkflowArtifact,
    run_id: u64,
    repository_id: u64,
    default_branch: &str,
) -> Result<String, OrchestratorError> {
    let Some(run) = artifact.workflow_run.as_ref() else {
        return Err(internal("package_recovery_artifact_run_missing"));
    };
    if artifact.expired
        || artifact.size_in_bytes == 0
        || artifact.size_in_bytes > MAX_ARCHIVE_BYTES
        || run.id != run_id
        || run.repository_id != repository_id
        || !is_git_sha(&run.head_sha)
        || run.head_branch != default_branch
    {
        return Err(internal("package_recovery_artifact_invalid"));
    }
    Ok(run.head_sha.clone())
}

pub(super) fn receipt_matches_original(
    receipt_run_id: u64,
    receipt_attempt: u32,
    receipt_source_sha: &str,
    version: (PackageReleaseChannel, Option<u64>),
    original_run_id: u64,
    original_attempt: u32,
    artifact_source_sha: &str,
) -> bool {
    receipt_run_id == original_run_id
        && receipt_attempt == original_attempt
        && receipt_source_sha == artifact_source_sha
        && match version {
            (PackageReleaseChannel::Preview, Some(count)) => count > 0,
            (PackageReleaseChannel::Stable, None) => true,
            _ => false,
        }
}
