//! Source-free version data admission for a static native release policy.

use super::{
    github::{GitHubApi, sha256},
    run_identity::{CurrentRun, RunSource, release_job_name},
};
use crate::{OrchestratorError, internal::internal};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::Path,
};
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
#[path = "release_policy_acquisition.rs"]
mod acquisition;
#[path = "preview_source_admission.rs"]
mod source;
pub use acquisition::acquire_preview_source_from_env;
#[path = "release_policy_admission_closure.rs"]
mod closure;
use closure::{admission_covers_required_paths, admission_paths, admitted_preview_version_matches};
pub use source::{
    FINALIZE_PREVIEW_SOURCE_OP, finalize_preview_source_from_env, select_preview_source_from_env,
};

/// Select and retain the complete source-bound preview admission.
pub const SELECT_PREVIEW_SOURCE_OP: &str = "select-preview-source-v1";
/// Acquire and authenticate the exact admission artifact before source checkout.
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

/// Return the context bound to the canonical, digest-pinned source admission.
/// Consumers compare this value with their immutable GitHub environment before
/// using policy or payload data.
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

/// Recheck the immutable source closure and producer declaration before any
/// source checkout is used to produce a payload.
pub(crate) fn validate_admitted_source_checkout(
    temp: &Path,
    workspace: &Path,
    release: &velnor_actions_contract::config::RustBinaryReleaseConfig,
    qualified_tools: &[velnor_actions_contract::config::QualifiedTool],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let bytes = read_source_admission(&temp.join("release-policy-admission.json"))?;
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, MAX_ADMISSION_BYTES)
            .map_err(|_| internal("release_policy_admission_json"))?;
    let admission: Admission =
        serde_json::from_value(value).map_err(|_| internal("release_policy_admission_schema"))?;
    let policy = parse_package_release_policy(required("VELNOR_RELEASE_POLICY")?.as_bytes())
        .map_err(|_| internal("release_policy_invalid"))?;
    let (_, plan) = admitted_policy_plan(temp, deadline)?;
    let mut required_paths = BTreeSet::new();
    for path in policy.manifest_paths() {
        required_paths.insert(path.to_owned());
    }
    let discovered = velnor_actions_mise::discover_preview_producer(
        workspace,
        &release.producer,
        qualified_tools,
        deadline,
    )
    .map_err(|_| internal("preview_source_task_discovery"))?;
    required_paths.insert(".velnor/config.toml".to_owned());
    for path in &discovered.config_inputs {
        required_paths.insert(path.clone());
    }
    let actual_paths = admission_paths(&admission.source_files)?;
    if !admission_covers_required_paths(&required_paths, &actual_paths)
        || admission.producer != release.producer
        || admission.task_config != discovered.task_config
        || admission.task_inputs != discovered.config_inputs
        || admission.tool_specs != discovered.tool_specs
        || admission.qualified_tools != discovered.qualified_tools
        || admission.qualification_digest != discovered.qualification_digest
        || admission.context.source_sha != required("VELNOR_RELEASE_SOURCE_SHA")?
        || admission.context.workflow_authority_sha
            != required("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA")?
        || !admitted_preview_version_matches(&plan, &admission.context)
    {
        return Err(internal("release_policy_source_closure_substitution"));
    }
    verify_admitted_source_entries(workspace, &admission.source_files, deadline)?;
    Ok(())
}

fn verify_admitted_source_entries(
    workspace: &Path,
    entries: &[source_files::SourceFileEntry],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for entry in entries {
        if entry.path.starts_with('/')
            || entry
                .path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || entry.path.contains('\\')
            || !matches!(entry.mode.as_str(), "100644" | "100755")
            || !super::identity::is_git_sha(&entry.sha)
        {
            return Err(internal("release_policy_source_closure_entry"));
        }
        let metadata = std::fs::symlink_metadata(workspace.join(&entry.path))
            .map_err(|_| internal("release_policy_source_closure_file"))?;
        if !metadata.file_type().is_file() {
            return Err(internal("release_policy_source_closure_file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let executable = metadata.permissions().mode() & 0o111 != 0;
            if executable != (entry.mode == "100755") {
                return Err(internal("release_policy_source_closure_mode"));
            }
        }
        let local = crate::safe_read::read_repo_file(workspace, &entry.path, 1024 * 1024)?;
        let crate::safe_read::RepoRead::Text(local) = local else {
            return Err(internal("release_policy_source_closure_file"));
        };
        if source_files::git_blob_sha1(local.as_bytes())? != entry.sha {
            return Err(internal("release_policy_source_closure_blob"));
        }
        let object =
            velnor_actions_mise::GitRequest::rev_parse(vec![format!("HEAD:{}", entry.path).into()])
                .run_in_until(workspace, deadline)
                .map_err(|_| internal("release_policy_source_closure_blob"))?;
        if !object.success
            || object
                .stdout_text("git")
                .map_err(|_| internal("release_policy_source_closure_blob"))?
                .trim()
                != entry.sha
        {
            return Err(internal("release_policy_source_closure_blob"));
        }
    }
    Ok(())
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

#[cfg(test)]
#[path = "release_policy_admission_tests.rs"]
mod tests;

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
