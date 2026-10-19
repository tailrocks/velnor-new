//! Bounded discovery of an already-published current-source preview; never publication.
use super::{
    github::{GitHubApi, unique_release},
    identity::{read_regular, recovery_body},
    recovery::promotion_context::{Invocation, Purpose},
    run_identity::RunSource,
};
use crate::{
    OrchestratorError, internal::internal, package_release_signer::VerifiedAttestedSignedPackage,
};
use std::{env, path::PathBuf, time::Duration};
use velnor_actions_contract::{
    PackageReleasePlan, parse_package_release_plan, preview_identity::PreviewIdentity,
};
use velnor_actions_mise::CheckDeadline;

#[path = "package_release_continuation_envelope.rs"]
mod envelope;
#[path = "package_release_continuation_source.rs"]
mod source;
pub(crate) use source::verify_current_source;

/// Fixed private read-only selector for the generated continuation workflow.
pub const SELECT_PREVIEW_TAP_CONTINUATION_OP: &str = "select-preview-tap-continuation-v1";
/// Hints for the next independent retained-signature admission, never merge authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewTapContinuationSelection {
    /// Exact current main source commit.
    pub source_sha: String,
    /// Original signed preview workflow run.
    pub original_run_id: u64,
    /// Original signed preview workflow attempt.
    pub original_run_attempt: u32,
    /// Independently verified latest exact-source push CI.
    pub ci_run_id: u64,
    /// Current attempt of that CI.
    pub ci_attempt: u32,
    /// Complete source ancestry count.
    pub commit_count: u64,
}

/// Discover the sole canonical published current-source candidate without writes.
/// # Errors
/// Rejects ambiguous/malformed evidence, invalid caller identity and denied reads.
pub fn select_preview_tap_continuation_from_env()
-> Result<Option<PreviewTapContinuationSelection>, OrchestratorError> {
    super::artifact_zip::within_operation(select_from_env)
}

fn select_from_env() -> Result<Option<PreviewTapContinuationSelection>, OrchestratorError> {
    let root = PathBuf::from(required("RUNNER_TEMP")?).join("velnor");
    let path = root.join("package-release-plan.json");
    if !root.is_absolute() || std::path::Path::new(&required("VELNOR_REQUEST_FILE")?) != path {
        return Err(invalid("request"));
    }
    let meta = std::fs::symlink_metadata(&path).map_err(|_| invalid("request"))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > 1024 * 1024 {
        return Err(invalid("request"));
    }
    let plan = parse_package_release_plan(&read_regular(&path)?).map_err(|_| invalid("plan"))?;
    let invocation = Invocation::from_env()?;
    invocation.validate(&plan, Purpose::TapPromotion)?;
    if !invocation.continuation(&plan) {
        return Err(invalid("context"));
    }
    let api = GitHubApi::new(plan.repository.clone());
    let repo_id = source::repository_id(&api, &plan.repository)?;
    invocation.authenticate_continuation(&plan, &api, repo_id)?;
    let workspace = PathBuf::from(required("GITHUB_WORKSPACE")?);
    if !workspace.is_absolute() {
        return Err(invalid("workspace"));
    }
    let deadline =
        CheckDeadline::after(Duration::from_secs(300)).map_err(|_| invalid("deadline"))?;
    let count =
        crate::preview_producer::source_commit_count(&workspace, &invocation.source_sha, deadline)?;
    if count == 0 || required("VELNOR_RELEASE_COMMIT_COUNT")?.parse::<u64>().ok() != Some(count) {
        return Err(invalid("history"));
    }
    select(&api, &plan, &invocation.source_sha, repo_id, count)
}

fn select(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
    sha: &str,
    repo_id: u64,
    count: u64,
) -> Result<Option<PreviewTapContinuationSelection>, OrchestratorError> {
    if source::head(api, &plan.default_branch)? != sha {
        return Ok(None);
    }
    let Some((ci_run_id, ci_attempt)) = source::latest_ci(
        api,
        RunSource {
            repository_id: repo_id,
            sha,
            branch: &plan.default_branch,
        },
    )?
    else {
        return Ok(None);
    };
    let identity =
        PreviewIdentity::derive(&plan.base_version, sha, count).map_err(|_| invalid("version"))?;
    let releases = api.release_list(&identity.tag)?;
    if releases.is_empty() {
        return Ok(None);
    }
    let release = unique_release(&releases, &identity.tag)?;
    if release.draft == Some(true) {
        return Ok(None);
    }
    let tag = api
        .tag_identity(&identity.tag)?
        .ok_or_else(|| invalid("tag"))?;
    if tag.source_sha != sha {
        return Err(invalid("tag"));
    }
    let receipt = envelope::receipt_hint(
        release,
        plan,
        &identity,
        repo_id,
        tag.message.as_deref().ok_or_else(|| invalid("tag"))?,
    )?;
    if source::head(api, &plan.default_branch)? != sha {
        return Ok(None);
    }
    Ok(Some(PreviewTapContinuationSelection {
        source_sha: sha.to_owned(),
        original_run_id: receipt.workflow.run_id,
        original_run_attempt: receipt.workflow.run_attempt,
        ci_run_id,
        ci_attempt,
        commit_count: count,
    }))
}

/// Recheck current publication identity after independent retained cryptographic verification.
pub(super) fn verify_published_selection(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    let plan = signed.plan();
    let invocation = Invocation::from_env()?;
    let api = GitHubApi::new(plan.repository.clone());
    let repo_id = source::repository_id(&api, &plan.repository)?;
    invocation.authenticate_continuation(plan, &api, repo_id)?;
    let receipt = signed.receipt();
    if receipt.source.commit_sha != invocation.source_sha || receipt.repository.id != repo_id {
        return Err(invalid("source"));
    }
    if required("VELNOR_RELEASE_COMMIT_COUNT")?.parse::<u64>().ok() != receipt.version.commit_count
    {
        return Err(invalid("history"));
    }
    let (current_ci_id, current_ci_attempt) = super::current_ci_from_env()?;
    source::verify_current_source(
        &plan.repository,
        &plan.default_branch,
        &receipt.source.commit_sha,
    )?;
    api.verify_ci_identity(
        current_ci_id,
        current_ci_attempt,
        RunSource {
            repository_id: repo_id,
            sha: &receipt.source.commit_sha,
            branch: &plan.default_branch,
        },
        true,
    )?;
    let releases = api.release_list(&receipt.tag)?;
    let release = unique_release(&releases, &receipt.tag)?;
    let tag = api
        .tag_identity(&receipt.tag)?
        .ok_or_else(|| invalid("tag"))?;
    let expected = recovery_body(
        signed,
        &super::github::sha256(&receipt.canonical_bytes().map_err(|_| invalid("receipt"))?),
    )?;
    if release.draft != Some(false)
        || release.immutable != Some(true)
        || release.prerelease != Some(true)
        || release.body.as_deref() != Some(expected.as_str())
        || tag.source_sha != receipt.source.commit_sha
        || tag.message.as_deref() != Some(expected.as_str())
    {
        return Err(invalid("publication"));
    }
    let identity = super::identity::local_identity(signed)?;
    let remote_names = release
        .assets
        .iter()
        .map(|asset| &asset.name)
        .collect::<std::collections::BTreeSet<_>>();
    if release.assets.len() != identity.assets.len()
        || remote_names.len() != release.assets.len()
        || remote_names != identity.assets.keys().collect()
        || api.remote_assets(release)? != identity.assets
    {
        return Err(invalid("published_assets"));
    }
    Ok(())
}

fn required(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| invalid("environment"))
}
fn invalid(problem: &str) -> OrchestratorError {
    internal(&format!("tap_continuation_{problem}"))
}

#[cfg(test)]
#[path = "package_release_continuation_tests.rs"]
mod tests;
