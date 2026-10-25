use super::{
    github::{GitHubApi, sha256},
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
pub(super) mod source_files;
use metadata::read as read_regular;
pub(super) use source_files::SourceFiles;
pub(crate) use source_files::git_blob_sha1;
#[path = "release_policy_admission_acquire.rs"]
mod acquire;
#[path = "preview_source_admission.rs"]
mod source;
#[path = "preview_source_policy_projection.rs"]
mod source_policy_projection;
#[path = "release_policy_admission_source_validation.rs"]
mod source_validation;
pub use source::{
    FINALIZE_PREVIEW_SOURCE_OP, finalize_preview_source_from_env, select_preview_source_from_env,
};
pub(in crate::package_release_publish) use source_policy_projection::read_preview_policy_at_source;
pub(crate) use source_validation::validate_admitted_source_checkout;

pub const SELECT_PREVIEW_SOURCE_OP: &str = "select-preview-source-v1";
pub const ACQUIRE_PREVIEW_SOURCE_OP: &str = "acquire-preview-source-v1";
const MAX_ADMISSION_BYTES: usize = 16 * 1024 * 1024;

struct SourceAdmissionBinding {
    run_id: u64,
    attempt: u32,
    artifact_id: u64,
    artifact_digest: String,
    inner_digest: String,
    repository_id: u64,
    authority: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    policy_sha256: String,
    plan_sha256: String,
    context: ReleaseProductionContext,
    source_files: Vec<source_files::SourceFileEntry>,
    producer: velnor_actions_contract::config::RustBinaryReleaseProducer,
    task_config: String,
    task_inputs: Vec<String>,
    tool_specs: Vec<String>,
    qualified_tools: Vec<velnor_actions_contract::config::QualifiedTool>,
    qualification_digest: String,
}

pub(crate) struct PublicHistoricalAdmissionProjection {
    pub(crate) repository_id: u64,
    pub(crate) head_repository_id: u64,
    pub(crate) source_sha: String,
    pub(crate) source_ref: String,
    pub(crate) workflow_authority_sha: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) ci_run_id: u64,
    pub(crate) ci_run_attempt: u32,
    pub(crate) commit_count: u64,
    pub(crate) version: String,
    pub(crate) plan_sha256: String,
    pub(crate) policy_sha256: String,
}

pub(crate) fn project_public_historical_admission(
    bytes: &[u8],
    expected_sha256: &str,
) -> Result<PublicHistoricalAdmissionProjection, OrchestratorError> {
    if !lower_sha256(expected_sha256) || sha256(bytes) != expected_sha256 {
        return Err(internal("package_public_historical_admission_digest"));
    }
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(bytes, MAX_ADMISSION_BYTES)
            .map_err(|_| internal("package_public_historical_admission_json"))?;
    let admission: Admission = serde_json::from_value(value)
        .map_err(|_| internal("package_public_historical_admission_schema"))?;
    if admission.schema != "velnor.preview-source-admission.v1"
        || canonical_json_bytes(&admission)
            .map_err(|_| internal("package_public_historical_admission_encode"))?
            != bytes
        || !lower_sha256(&admission.plan_sha256)
        || !lower_sha256(&admission.policy_sha256)
    {
        return Err(internal("package_public_historical_admission_invalid"));
    }
    let context = admission.context;
    Ok(PublicHistoricalAdmissionProjection {
        repository_id: context.repository_id,
        head_repository_id: context.head_repository_id,
        source_sha: context.source_sha,
        source_ref: context.source_ref,
        workflow_authority_sha: context.workflow_authority_sha,
        run_id: context.run_id,
        run_attempt: context.run_attempt,
        ci_run_id: context.ci_run_id,
        ci_run_attempt: context.ci_run_attempt,
        commit_count: context.commit_count,
        version: context.version,
        plan_sha256: admission.plan_sha256,
        policy_sha256: admission.policy_sha256,
    })
}

pub fn acquire_preview_source_from_env() -> Result<(), OrchestratorError> {
    let deadline = CheckDeadline::after(Duration::from_secs(300))
        .map_err(|_| internal("release_policy_deadline"))?;
    deadline
        .scope(|| acquire::acquire_source(deadline))
        .map_err(|_| internal("release_policy_deadline"))?
}

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
            let bytes = read_source_admission(&temp.join("release-policy-admission.json"))?;
            if sha256(&bytes) != required("VELNOR_RELEASE_POLICY_ADMISSION_DIGEST")? {
                return Err(internal("release_policy_admission_digest"));
            }
            let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(
                &bytes,
                MAX_ADMISSION_BYTES,
            )
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
                || admission.schema != "velnor.preview-source-admission.v1"
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

pub(crate) fn admitted_context(
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<ReleaseProductionContext, OrchestratorError> {
    let (_, plan) = admitted_policy_plan(temp, deadline)?;
    let bytes = read_source_admission(&temp.join("release-policy-admission.json"))?;
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, MAX_ADMISSION_BYTES)
            .map_err(|_| internal("release_policy_admission_json"))?;
    let admission: Admission =
        serde_json::from_value(value).map_err(|_| internal("release_policy_admission_schema"))?;
    if admission.context.source_sha != required("VELNOR_RELEASE_SOURCE_SHA")?
        || admission.context.workflow_authority_sha
            != required("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA")?
        || admission.context.run_id != positive_u64("GITHUB_RUN_ID")?
        || u64::from(admission.context.run_attempt) != positive_u64("GITHUB_RUN_ATTEMPT")?
        || admission.context.workflow_authority_sha != required("GITHUB_SHA")?
        || admission.context.workflow_authority_sha != required("GITHUB_WORKFLOW_SHA")?
        || admission.context.source_ref != format!("refs/heads/{}", plan.default_branch)
    {
        return Err(internal("release_policy_admission_context_environment"));
    }
    Ok(admission.context)
}

fn positive_u64(name: &str) -> Result<u64, OrchestratorError> {
    let value = required(name)?;
    let parsed = value
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| internal("release_policy_admission_context_environment"))?;
    if parsed.to_string() != value {
        return Err(internal("release_policy_admission_context_environment"));
    }
    Ok(parsed)
}

fn authenticate(
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
) -> Result<(), OrchestratorError> {
    let api = GitHubApi::new(plan.repository.clone());
    api.verify_repository_identity(context.repository_id)?;
    let workflow =
        api.admitted_workflow(plan.channel.workflow_path(), plan.channel.workflow_name())?;
    api.verify_preview_main_ancestry(
        context.repository_id,
        &plan.default_branch,
        &context.source_sha,
        &context.workflow_authority_sha,
    )?;
    let authority = RunSource {
        repository_id: context.repository_id,
        sha: &context.workflow_authority_sha,
        branch: &plan.default_branch,
    };
    let job = release_job_name(plan, &required("GITHUB_JOB")?)?;
    api.verify_current_role(
        &CurrentRun {
            id: context.run_id,
            attempt: context.run_attempt,
            source: authority,
            workflow: &workflow,
            event: &required("GITHUB_EVENT_NAME")?,
        },
        &job,
    )?;
    let ci_source = RunSource {
        repository_id: context.repository_id,
        sha: &context.source_sha,
        branch: &plan.default_branch,
    };
    api.verify_ci_identity(context.ci_run_id, context.ci_run_attempt, ci_source, true)
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

fn validate_admission(
    admission: &Admission,
    policy: &PackageReleasePolicy,
    plan: &PackageReleasePlan,
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let policy_bytes = policy
        .canonical_bytes()
        .map_err(|_| internal("release_policy_encoding"))?;
    let plan_bytes = plan
        .canonical_bytes()
        .map_err(|_| internal("release_plan_encoding"))?;
    if canonical_json_bytes(admission).map_err(|_| internal("release_policy_admission_encoding"))?
        != bytes
        || admission.schema != "velnor.preview-source-admission.v1"
        || admission.policy_sha256 != sha256(&policy_bytes)
        || admission.plan_sha256 != sha256(&plan_bytes)
        || admission.context != crate::preview_producer::release_workflow_context(plan, deadline)?
    {
        return Err(internal("release_policy_admission_substitution"));
    }
    Ok(())
}

fn lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn read_source_admission(path: &Path) -> Result<Vec<u8>, OrchestratorError> {
    crate::retrieve_reports::staged_reads::read_staged_bytes(path, MAX_ADMISSION_BYTES as u64)
        .map_err(|_| internal("release_policy_admission_read"))
}

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
