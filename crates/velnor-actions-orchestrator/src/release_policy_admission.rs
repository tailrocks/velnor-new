//! Source-free version data admission for a static native release policy.

use super::{
    github::{GitHubApi, GitRefResponse, sha256},
    run_identity::{CurrentRun, RunSource, release_job_name},
};
use crate::{OrchestratorError, internal::internal};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, env, path::Path, time::Duration};
use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePolicy, parse_package_release_policy,
};
use velnor_actions_contract::{
    canonical_json_bytes, package_release::PackageReleaseChannel,
    preview_production::ReleaseProductionContext,
};
use velnor_actions_mise::CheckDeadline;

#[path = "release_metadata_read.rs"]
pub(super) mod metadata;
#[path = "package_release_source_files.rs"]
mod source_files;
use metadata::read as read_regular;

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
    let (policy, plan) = resolve_policy_from_env(&required("GITHUB_SHA")?)?;
    if policy.channel != PackageReleaseChannel::Preview {
        return Err(internal("release_policy_requires_preview_admission"));
    }
    let context = crate::preview_producer::release_workflow_context(&plan, deadline)?;
    authenticate(&plan, &context)?;
    let temp = runner_root()?;
    let policy_bytes = policy
        .canonical_bytes()
        .map_err(|_| internal("release_policy_encoding"))?;
    if required("VELNOR_REQUEST_FILE")?
        != temp.join("package-release-policy.json").to_string_lossy()
        || read_regular(&temp.join("package-release-policy.json"))? != policy_bytes
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

/// Load the digest-pinned pre-checkout policy/plan without network or credentials.
pub(crate) fn admitted_policy_plan(
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<(PackageReleasePolicy, PackageReleasePlan), OrchestratorError> {
    let policy = parse_package_release_policy(required("VELNOR_RELEASE_POLICY")?.as_bytes())
        .map_err(|_| internal("release_policy_invalid"))?;
    if policy.channel == PackageReleaseChannel::Stable {
        return crate::stable_tag_origin::admitted_policy_plan(temp, deadline);
    }
    deadline
        .scope(|| {
            if temp != runner_root()? {
                return Err(internal("release_policy_admission_path"));
            }
            let bytes = read_regular(&temp.join("release-policy-admission.json"))?;
            if sha256(&bytes) != required("VELNOR_RELEASE_POLICY_ADMISSION_DIGEST")? {
                return Err(internal("release_policy_admission_digest"));
            }
            let value =
                velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, 1024 * 1024)
                    .map_err(|_| internal("release_policy_admission_json"))?;
            let admission: Admission = serde_json::from_value(value)
                .map_err(|_| internal("release_policy_admission_schema"))?;
            let plan_bytes = read_regular(&temp.join("package-release-plan.json"))?;
            let plan = velnor_actions_contract::parse_package_release_plan(&plan_bytes)
                .map_err(|_| internal("release_policy_plan_invalid"))?;
            policy
                .validate_plan(&plan)
                .map_err(|_| internal("release_policy_plan_substitution"))?;
            if canonical_json_bytes(&admission)
                .map_err(|_| internal("release_policy_admission_encoding"))?
                != bytes
                || admission.schema != "velnor.release-policy-admission.v1"
                || admission.policy_sha256
                    != sha256(
                        &policy
                            .canonical_bytes()
                            .map_err(|_| internal("release_policy_encoding"))?,
                    )
                || admission.plan_sha256 != sha256(&plan_bytes)
                || admission.context
                    != crate::preview_producer::release_workflow_context(&plan, deadline)?
            {
                return Err(internal("release_policy_admission_substitution"));
            }
            Ok((policy, plan))
        })
        .map_err(|_| internal("release_policy_deadline"))?
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

/// Resolve exact-source manifest data; callers authenticate source/workflow/CI.
pub(crate) fn resolve_policy_from_env(
    source_sha: &str,
) -> Result<(PackageReleasePolicy, PackageReleasePlan), OrchestratorError> {
    let bytes =
        env::var("VELNOR_RELEASE_POLICY").map_err(|_| internal("release_policy_missing"))?;
    let policy = parse_package_release_policy(bytes.as_bytes())
        .map_err(|_| internal("release_policy_invalid"))?;
    let plan = resolve_policy_at_source(&policy, source_sha)?;
    Ok((policy, plan))
}

pub(super) fn resolve_policy_at_source(
    policy: &PackageReleasePolicy,
    source_sha: &str,
) -> Result<PackageReleasePlan, OrchestratorError> {
    policy
        .validate()
        .map_err(|_| internal("release_policy_invalid"))?;
    let api = GitHubApi::new(policy.repository.clone());
    let mut reader = source_files::SourceFiles::new(&api, source_sha)?;
    let mut manifests = BTreeMap::new();
    for path in policy.manifest_paths() {
        manifests.insert(path.to_owned(), reader.read(path)?);
    }
    velnor_actions_rust::resolve_release_policy(policy, &manifests)
        .map_err(|_| internal("release_version_policy_resolution"))
}
