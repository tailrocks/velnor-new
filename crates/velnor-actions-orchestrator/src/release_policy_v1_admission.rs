//! Source-free version data admission for a static native release policy.

use super::{
    github::{GitHubApi, GitRefResponse, sha256},
    run_identity::{CurrentRun, RunSource, release_job_name},
};
use crate::{OrchestratorError, internal::internal};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{env, path::Path, time::Duration};
use velnor_actions_contract::{
    PackageReleasePlan, canonical_json_bytes, package_release::PackageReleaseChannel,
    preview_identity::PreviewIdentity, preview_production::ReleaseProductionContext,
};
use velnor_actions_mise::CheckDeadline;

/// Authenticate exact-source policy data before a preview producer checkout.
pub const ADMIT_RELEASE_POLICY_OP: &str = "admit-release-policy-v1";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    policy_sha256: String,
    plan_sha256: String,
    context: ReleaseProductionContext,
}

/// Admit a preview source and pin its resolved version data without write authority.
/// # Errors
/// Rejects stale or invalid workflow/CI, source, policy, files and local paths.
pub fn admit_release_policy_from_env() -> Result<(), OrchestratorError> {
    let deadline = CheckDeadline::after(Duration::from_secs(300))
        .map_err(|_| internal("release_policy_deadline"))?;
    deadline
        .scope(|| admit(deadline))
        .map_err(|_| internal("release_policy_deadline"))?
}

fn admit(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let (policy, plan) =
        super::release_policy_admission::resolve_policy_from_env(&required("GITHUB_SHA")?)?;
    if policy.channel != PackageReleaseChannel::Preview {
        return Err(internal("release_policy_requires_preview_admission"));
    }
    let context = legacy_release_workflow_context(&plan, deadline)?;
    authenticate(&plan, &context)?;
    let temp = runner_root()?;
    let policy_bytes = policy
        .canonical_bytes()
        .map_err(|_| internal("release_policy_encoding"))?;
    if required("VELNOR_REQUEST_FILE")?
        != temp.join("package-release-policy.json").to_string_lossy()
        || super::release_policy_admission::metadata::read(
            &temp.join("package-release-policy.json"),
        )? != policy_bytes
    {
        return Err(internal("release_policy_request_substitution"));
    }
    let plan_bytes = plan
        .canonical_bytes()
        .map_err(|_| internal("release_plan_encoding"))?;
    let admission = Admission {
        schema: "velnor.release-policy-admission.v1".to_owned(),
        policy_sha256: sha256(&policy_bytes),
        plan_sha256: sha256(&plan_bytes),
        context,
    };
    let bytes = canonical_json_bytes(&admission)
        .map_err(|_| internal("release_policy_admission_encoding"))?;
    crate::exclusive_write::write_exclusive(
        &temp.join("package-release-plan.json"),
        &plan_bytes,
        "release_plan",
    )?;
    crate::exclusive_write::write_exclusive(
        &temp.join("release-policy-admission.json"),
        &bytes,
        "release_policy_admission",
    )?;
    append_output(&format!("admission_digest={}\n", sha256(&bytes)))
}

fn authenticate(
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
) -> Result<(), OrchestratorError> {
    let api = GitHubApi::new(plan.repository.clone());
    api.verify_repository_identity(context.repository_id)?;
    let workflow =
        api.admitted_workflow(plan.channel.workflow_path(), plan.channel.workflow_name())?;
    let source = RunSource {
        repository_id: context.repository_id,
        sha: &context.source_sha,
        branch: &plan.default_branch,
    };
    let job = release_job_name(plan, &required("GITHUB_JOB")?)?;
    api.verify_current_role(
        &CurrentRun {
            id: context.run_id,
            attempt: context.run_attempt,
            source,
            workflow: &workflow,
            event: plan.channel.event().as_str(),
        },
        &job,
    )?;
    let head: GitRefResponse = api.get_json(&api.route(&format!(
        "git/ref/heads/{}",
        super::github::encode_segment(&plan.default_branch)
    )))?;
    if head.object.object_type != "commit" || head.object.sha != context.source_sha {
        return Err(internal("release_policy_stale_source"));
    }
    api.verify_ci_identity(context.ci_run_id, context.ci_run_attempt, source, true)
}

fn legacy_release_workflow_context(
    plan: &PackageReleasePlan,
    deadline: CheckDeadline,
) -> Result<ReleaseProductionContext, OrchestratorError> {
    if plan.channel == PackageReleaseChannel::Stable {
        return crate::stable_tag_origin::admitted_production_context(plan, deadline);
    }
    validate_legacy_workflow(plan)?;
    let source_sha = required("GITHUB_SHA")?;
    let count = positive("VELNOR_RELEASE_COMMIT_COUNT")?;
    let identity = PreviewIdentity::derive(&plan.base_version, &source_sha, count)
        .map_err(|_| internal("preview_identity_invalid"))?;
    let repository_id = positive("GITHUB_REPOSITORY_ID")?;
    let context = ReleaseProductionContext {
        repository_id,
        head_repository_id: repository_id,
        source_sha,
        source_ref: required("GITHUB_REF")?,
        workflow_authority_sha: required("GITHUB_WORKFLOW_SHA")?,
        run_id: positive("GITHUB_RUN_ID")?,
        run_attempt: u32::try_from(positive("GITHUB_RUN_ATTEMPT")?)
            .map_err(|_| internal("preview_context_number"))?,
        ci_run_id: positive("VELNOR_RELEASE_CI_RUN_ID")?,
        ci_run_attempt: u32::try_from(positive("VELNOR_RELEASE_CI_RUN_ATTEMPT")?)
            .map_err(|_| internal("preview_context_number"))?,
        commit_count: count,
        version: identity.version,
    };
    velnor_actions_contract::preview_production::validate_release_production_context(
        plan, &context,
    )
    .map_err(|_| internal("preview_context_invalid"))?;
    validate_legacy_event(plan, &context, deadline)?;
    Ok(context)
}

fn validate_legacy_workflow(plan: &PackageReleasePlan) -> Result<(), OrchestratorError> {
    let event = required("GITHUB_EVENT_NAME")?;
    let repository = required("GITHUB_REPOSITORY")?;
    let reference = required("GITHUB_REF")?;
    let sha = required("GITHUB_SHA")?;
    let authority = required("GITHUB_WORKFLOW_SHA")?;
    let workflow = required("GITHUB_WORKFLOW_REF")?;
    if !legacy_workflow_identity_matches(
        plan,
        &event,
        &repository,
        &reference,
        &sha,
        &authority,
        &workflow,
    ) {
        return Err(internal("preview_workflow_context_mismatch"));
    }
    Ok(())
}

fn legacy_workflow_identity_matches(
    plan: &PackageReleasePlan,
    event: &str,
    repository: &str,
    reference: &str,
    sha: &str,
    authority: &str,
    workflow: &str,
) -> bool {
    event == "workflow_run"
        && repository.eq_ignore_ascii_case(&plan.repository)
        && reference == format!("refs/heads/{}", plan.default_branch)
        && sha == authority
        && valid_git_sha(sha)
        && workflow == format!("{repository}/{}@{reference}", plan.channel.workflow_path())
}

fn validate_legacy_event(
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    use crate::retrieve_reports::staged_reads::read_staged_bytes_until;

    let bytes = read_staged_bytes_until(
        Path::new(&required("GITHUB_EVENT_PATH")?),
        4 * 1024 * 1024,
        || {
            deadline
                .remaining()
                .map(|_| ())
                .map_err(|_| "preview_deadline")
        },
    )
    .map_err(|_| internal("preview_event_read"))?;
    let event =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, 4 * 1024 * 1024)
            .map_err(|_| internal("preview_event_json"))?;
    if !legacy_event_matches(plan, context, &event) {
        return Err(internal("preview_event_identity"));
    }
    Ok(())
}

fn legacy_event_matches(
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
    event: &Value,
) -> bool {
    let run = &event["workflow_run"];
    event["action"].as_str() == Some("completed")
        && event["repository"]["id"].as_u64() == Some(context.repository_id)
        && event["repository"]["full_name"].as_str() == Some(plan.repository.as_str())
        && run["id"].as_u64() == Some(context.ci_run_id)
        && run["run_attempt"].as_u64() == Some(u64::from(context.ci_run_attempt))
        && run["head_repository"]["id"].as_u64() == Some(context.repository_id)
        && run["head_sha"].as_str() == Some(context.source_sha.as_str())
        && run["head_branch"].as_str() == Some(plan.default_branch.as_str())
        && run["event"].as_str() == Some("push")
        && run["status"].as_str() == Some("completed")
        && run["conclusion"].as_str() == Some("success")
        && run["path"].as_str() == Some(velnor_actions_contract::CI_WORKFLOW_PATH)
        && run["name"].as_str() == Some(velnor_actions_contract::WORKFLOW_DISPLAY_NAME)
}

fn positive(name: &str) -> Result<u64, OrchestratorError> {
    let value = required(name)?;
    value
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0 && number.to_string() == value)
        .ok_or_else(|| internal("preview_context_number"))
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn required(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("release_policy_environment"))
}

fn runner_root() -> Result<std::path::PathBuf, OrchestratorError> {
    let runner = std::path::PathBuf::from(required("RUNNER_TEMP")?);
    if !runner.is_absolute() {
        return Err(internal("release_policy_runner_root"));
    }
    let temp = runner.join("velnor");
    crate::exclusive_write::create_dir_no_symlink(&runner, &temp)?;
    Ok(temp)
}

fn append_output(value: &str) -> Result<(), OrchestratorError> {
    use std::io::Write as _;
    let mut output = std::fs::OpenOptions::new()
        .append(true)
        .open(required("GITHUB_OUTPUT")?)
        .map_err(|_| internal("release_policy_output"))?;
    output
        .write_all(value.as_bytes())
        .map_err(|_| internal("release_policy_output"))
}

#[cfg(test)]
#[path = "release_policy_v1_admission_tests.rs"]
mod tests;
