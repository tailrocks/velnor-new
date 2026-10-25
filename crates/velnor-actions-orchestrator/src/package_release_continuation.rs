use super::{
    github::GitHubApi,
    identity::read_regular,
    recovery::promotion_context::{Invocation, Purpose},
    run_identity::RunSource,
};
use crate::{
    OrchestratorError, internal::internal, package_release_signer::VerifiedAttestedSignedPackage,
};
use std::{env, path::PathBuf, time::Duration};
use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePolicy, package_release::PackageReleaseChannel,
    parse_package_release_policy, preview_identity::PreviewIdentity,
};
use velnor_actions_mise::CheckDeadline;

#[path = "package_release_continuation_envelope.rs"]
mod envelope;
#[path = "package_release_continuation_source.rs"]
mod source;
pub(crate) use source::verify_current_source;

pub const SELECT_RELEASE_TAP_CONTINUATION_OP: &str = "select-release-tap-continuation-v1";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTapContinuationSelection {
    pub source_sha: String,
    pub original_run_id: u64,
    pub original_run_attempt: u32,
    pub ci_run_id: u64,
    pub ci_attempt: u32,
    pub commit_count: u64,
}

pub fn select_release_tap_continuation_from_env()
-> Result<Option<ReleaseTapContinuationSelection>, OrchestratorError> {
    super::artifact_zip::within_operation(select_from_env)
}

fn select_from_env() -> Result<Option<ReleaseTapContinuationSelection>, OrchestratorError> {
    let root = PathBuf::from(required("RUNNER_TEMP")?).join("velnor");
    let path = root.join("package-release-policy.json");
    if !root.is_absolute() || std::path::Path::new(&required("VELNOR_REQUEST_FILE")?) != path {
        return Err(invalid("request"));
    }
    let meta = std::fs::symlink_metadata(&path).map_err(|_| invalid("request"))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > 1024 * 1024 {
        return Err(invalid("request"));
    }
    let policy_bytes = read_regular(&path)?;
    let policy = parse_package_release_policy(&policy_bytes).map_err(|_| invalid("policy"))?;
    if policy.canonical_bytes().map_err(|_| invalid("policy"))? != policy_bytes
        || env::var("VELNOR_RELEASE_POLICY").ok().as_deref()
            != std::str::from_utf8(&policy_bytes).ok()
    {
        return Err(invalid("policy"));
    }
    let invocation = Invocation::from_env()?;
    let (resolved_policy, plan) =
        super::release_policy_admission::resolve_policy_from_env(&invocation.source_sha)?;
    if resolved_policy != policy {
        return Err(invalid("policy"));
    }
    invocation.validate(&plan, Purpose::TapPromotion)?;
    if !invocation.continuation(&plan) {
        return Err(invalid("context"));
    }
    let api = GitHubApi::new(plan.repository.clone());
    let repo_id = source::repository_id(&api, &plan.repository)?;
    invocation.authenticate_continuation(&plan, &api, repo_id)?;
    match plan.channel {
        PackageReleaseChannel::Preview => {
            let workspace = PathBuf::from(required("GITHUB_WORKSPACE")?);
            if !workspace.is_absolute() {
                return Err(invalid("workspace"));
            }
            let deadline =
                CheckDeadline::after(Duration::from_secs(300)).map_err(|_| invalid("deadline"))?;
            let count = crate::preview_producer::source_commit_count(
                &workspace,
                &invocation.source_sha,
                deadline,
            )?;
            if count == 0
                || required("VELNOR_RELEASE_COMMIT_COUNT")?.parse::<u64>().ok() != Some(count)
            {
                return Err(invalid("history"));
            }
            select_preview(&api, &plan, &invocation.source_sha, repo_id, count)
        }
        PackageReleaseChannel::Stable => select_stable(&api, &policy, &plan, repo_id),
    }
}

fn select_preview(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
    sha: &str,
    repo_id: u64,
    count: u64,
) -> Result<Option<ReleaseTapContinuationSelection>, OrchestratorError> {
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
    let Some(release) = api.release_by_tag(&identity.tag)? else {
        return Ok(None);
    };
    let tag = api
        .tag_identity(&identity.tag)?
        .ok_or_else(|| invalid("tag"))?;
    if tag.source_sha != sha {
        return Err(invalid("tag"));
    }
    let tag_message = tag.message.as_deref().ok_or_else(|| invalid("tag"))?;
    let receipt = envelope::preview_receipt_hint(&release, plan, repo_id, sha, count, tag_message)?;
    if source::head(api, &plan.default_branch)? != sha {
        return Ok(None);
    }
    Ok(Some(ReleaseTapContinuationSelection {
        source_sha: sha.to_owned(),
        original_run_id: receipt.workflow.run_id,
        original_run_attempt: receipt.workflow.run_attempt,
        ci_run_id,
        ci_attempt,
        commit_count: count,
    }))
}

fn select_stable(
    api: &GitHubApi,
    policy: &PackageReleasePolicy,
    current_plan: &PackageReleasePlan,
    repo_id: u64,
) -> Result<Option<ReleaseTapContinuationSelection>, OrchestratorError> {
    let tag_name = format!("v{}", current_plan.base_version);
    let Some(tag) = api.tag_identity(&tag_name)? else {
        return Ok(None);
    };
    let exact_plan =
        super::release_policy_admission::resolve_policy_at_source(policy, &tag.source_sha)?;
    if exact_plan.base_version != current_plan.base_version
        || exact_plan.channel != PackageReleaseChannel::Stable
    {
        return Ok(None);
    }
    let Some((ci_run_id, ci_attempt)) = source::latest_ci(
        api,
        RunSource {
            repository_id: repo_id,
            sha: &tag.source_sha,
            branch: &exact_plan.default_branch,
        },
    )?
    else {
        return Ok(None);
    };
    let Some(release) = api.release_by_tag(&tag_name)? else {
        return Ok(None);
    };
    let receipt = envelope::stable_receipt_hint(
        &release,
        &exact_plan,
        repo_id,
        tag.message.as_deref().ok_or_else(|| invalid("tag"))?,
    )?;
    if receipt.source.commit_sha != tag.source_sha {
        return Err(invalid("stable_source"));
    }
    if source::head(api, &current_plan.default_branch)? != required("GITHUB_SHA")? {
        return Ok(None);
    }
    Ok(Some(ReleaseTapContinuationSelection {
        source_sha: tag.source_sha,
        original_run_id: receipt.workflow.run_id,
        original_run_attempt: receipt.workflow.run_attempt,
        ci_run_id,
        ci_attempt,
        commit_count: 0,
    }))
}

pub(super) fn verify_published_selection(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    let plan = signed.plan();
    let invocation = Invocation::from_env()?;
    let api = GitHubApi::new(plan.repository.clone());
    let repo_id = source::repository_id(&api, &plan.repository)?;
    invocation.authenticate_continuation(plan, &api, repo_id)?;
    let (policy, current_plan) =
        super::release_policy_admission::resolve_policy_from_env(&invocation.source_sha)?;
    if policy.channel != plan.channel
        || current_plan != *plan
        || source::head(&api, &plan.default_branch)? != invocation.source_sha
    {
        return Err(invalid("current_source"));
    }
    let receipt = signed.receipt();
    let selected_source = required("VELNOR_RELEASE_SOURCE_SHA")?;
    if !super::identity::is_git_sha(&selected_source) {
        return Err(invalid("source"));
    }
    let selected_plan =
        super::release_policy_admission::resolve_policy_at_source(&policy, &selected_source)?;
    let count = required("VELNOR_RELEASE_COMMIT_COUNT")?.parse::<u64>().ok();
    let selected_run = required("VELNOR_RELEASE_ORIGINAL_RUN_ID")?
        .parse::<u64>()
        .ok();
    let selected_attempt = required("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT")?
        .parse::<u32>()
        .ok();
    if receipt.source.commit_sha != selected_source
        || selected_plan != *plan
        || receipt.repository.id != repo_id
        || receipt.workflow.run_id != selected_run.unwrap_or_default()
        || receipt.workflow.run_attempt != selected_attempt.unwrap_or_default()
        || receipt.version.commit_count.unwrap_or_default() != count.unwrap_or_default()
        || (receipt.channel == PackageReleaseChannel::Stable
            && (receipt.version.commit_count.is_some() || count != Some(0)))
        || (receipt.channel == PackageReleaseChannel::Preview
            && (receipt.version.commit_count.is_none() || count != receipt.version.commit_count))
    {
        return Err(invalid("source"));
    }
    let (current_ci_id, current_ci_attempt) = super::current_ci_from_env()?;
    api.verify_ci_identity(
        current_ci_id,
        current_ci_attempt,
        RunSource {
            repository_id: repo_id,
            sha: &selected_source,
            branch: &plan.default_branch,
        },
        receipt.channel == PackageReleaseChannel::Preview,
    )?;
    super::published::verify_published_release(signed)
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
