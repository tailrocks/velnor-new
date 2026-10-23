//! Credential-free verification and import of the current-run snapshot bundle.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use velnor_actions_mise::CheckDeadline;

use velnor_actions_contract::GitHubEvidenceFetchSpecV1;

#[path = "github_evidence_import_files.rs"]
mod files;

use crate::OrchestratorError;
use crate::internal::internal;

const SPEC_LIMIT: usize = 64 * 1024;

#[cfg(test)]
#[path = "github_evidence_import_tests.rs"]
mod tests;

pub(super) fn run() -> Result<(), OrchestratorError> {
    let deadline = files::new_deadline()?;
    let spec = read_spec()?;
    files::check_deadline(deadline)?;
    let workspace = workspace_root(deadline)?;
    let bundle = bundle_root(deadline)?;
    let snapshot = files::load_snapshot(&bundle, &spec, deadline)?;
    let context = ImportContext::from_env()?;
    import_snapshot(&spec, &bundle, &workspace, &snapshot, &context, deadline)
}

#[cfg(test)]
pub(super) fn import_with_context(
    spec: &GitHubEvidenceFetchSpecV1,
    bundle: &Path,
    workspace: &Path,
    context: &ImportContext,
) -> Result<(), OrchestratorError> {
    let deadline = files::new_deadline()?;
    let snapshot = files::load_snapshot(bundle, spec, deadline)?;
    import_snapshot(spec, bundle, workspace, &snapshot, context, deadline)
}

fn import_snapshot(
    spec: &GitHubEvidenceFetchSpecV1,
    bundle: &Path,
    workspace: &Path,
    snapshot: &velnor_actions_contract::GitHubEvidenceSnapshotV1,
    context: &ImportContext,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    files::check_deadline(deadline)?;
    validate_context(spec, snapshot, context)?;
    files::check_deadline(deadline)?;
    files::stage_bundle(bundle, workspace, snapshot, deadline)
}

#[cfg(test)]
pub(super) fn verify_test_bundle(
    root: &Path,
    spec: &GitHubEvidenceFetchSpecV1,
) -> Result<velnor_actions_contract::GitHubEvidenceSnapshotV1, OrchestratorError> {
    let deadline = files::new_deadline()?;
    let snapshot = files::load_snapshot(root, spec, deadline)?;
    files::verify_bundle(root, &snapshot, deadline)?;
    Ok(snapshot)
}

fn read_spec() -> Result<GitHubEvidenceFetchSpecV1, OrchestratorError> {
    if env::var("VELNOR_INTERNAL_OP").as_deref()
        != Ok(velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION)
    {
        return Err(internal("github_evidence_operation_identity"));
    }
    let text = required_env(velnor_actions_contract::GITHUB_EVIDENCE_SPEC_ENV)?;
    if text.len() > SPEC_LIMIT {
        return Err(internal("github_evidence_spec_size"));
    }
    let spec = GitHubEvidenceFetchSpecV1::parse_canonical_json(&text)
        .map_err(|_| internal("github_evidence_spec_invalid"))?;
    Ok(spec)
}

pub(super) struct ImportContext {
    pub(super) repository_owner: String,
    pub(super) repository_name: String,
    pub(super) repository_id: u64,
    pub(super) run_id: u64,
    pub(super) run_attempt: u32,
    pub(super) event_name: String,
    pub(super) job: String,
    pub(super) reference: String,
    pub(super) sha: String,
    pub(super) workflow_repository: String,
    pub(super) workflow_path: String,
    pub(super) workflow_reference: String,
    pub(super) evidence_token_present: bool,
}

impl ImportContext {
    fn from_env() -> Result<Self, OrchestratorError> {
        let repository = required_env("GITHUB_REPOSITORY")?;
        let repository_id = positive_env::<u64>("GITHUB_REPOSITORY_ID")?;
        let run_id = positive_env::<u64>("GITHUB_RUN_ID")?;
        let run_attempt = positive_env::<u32>("GITHUB_RUN_ATTEMPT")?;
        let Some((repository_owner, repository_name)) = repository.split_once('/') else {
            return Err(internal("github_evidence_repository_identity"));
        };
        let workflow_ref = required_env("GITHUB_WORKFLOW_REF")?;
        let Some((workflow, workflow_reference)) = workflow_ref.rsplit_once('@') else {
            return Err(internal("github_evidence_workflow_identity"));
        };
        let Some((workflow_repository, workflow_path)) = workflow.split_once("/.github/workflows/")
        else {
            return Err(internal("github_evidence_workflow_identity"));
        };
        Ok(Self {
            repository_owner: repository_owner.to_owned(),
            repository_name: repository_name.to_owned(),
            repository_id,
            run_id,
            run_attempt,
            event_name: env::var("GITHUB_EVENT_NAME").unwrap_or_default(),
            job: env::var("GITHUB_JOB").unwrap_or_default(),
            reference: env::var("GITHUB_REF").unwrap_or_default(),
            sha: env::var("GITHUB_SHA").unwrap_or_default(),
            workflow_repository: workflow_repository.to_owned(),
            workflow_path: workflow_path.to_owned(),
            workflow_reference: workflow_reference.to_owned(),
            evidence_token_present: env::var_os(velnor_actions_contract::GITHUB_EVIDENCE_TOKEN_ENV)
                .is_some(),
        })
    }
}

fn validate_context(
    spec: &GitHubEvidenceFetchSpecV1,
    snapshot: &velnor_actions_contract::GitHubEvidenceSnapshotV1,
    context: &ImportContext,
) -> Result<(), OrchestratorError> {
    let expected_job = format!("mise-task-{}", spec.collector_suite_id);
    let expected_ref = format!("refs/heads/{}", spec.default_branch);
    let snapshot_repository = format!("{}/{}", snapshot.repository.owner, snapshot.repository.name);
    if context.event_name != "schedule"
        || context.job != expected_job
        || context.reference != expected_ref
        || context.sha != snapshot.acquisition.collector_sha
        || !is_lower_hex(&snapshot.acquisition.collector_sha, 40)
        || snapshot.acquisition.run_id != context.run_id
        || snapshot.acquisition.run_attempt != context.run_attempt
        || snapshot.repository.id != context.repository_id
        || !repository_matches(
            &context.repository_owner,
            &context.repository_name,
            &snapshot_repository,
        )
        || !repository_matches(
            &context.repository_owner,
            &context.repository_name,
            &context.workflow_repository,
        )
        || context.workflow_path
            != spec
                .collector_workflow_path
                .trim_start_matches(".github/workflows/")
        || context.workflow_reference != expected_ref
        || snapshot.acquisition.collector_ref != expected_ref
        || context.evidence_token_present
    {
        return Err(internal("github_evidence_consumer_context"));
    }
    Ok(())
}

fn repository_matches(owner: &str, name: &str, candidate: &str) -> bool {
    candidate.eq_ignore_ascii_case(&format!("{owner}/{name}"))
        && velnor_actions_contract::config::is_valid_github_owner(owner)
        && velnor_actions_contract::config::is_valid_github_repo_name(name)
}

fn workspace_root(deadline: CheckDeadline) -> Result<PathBuf, OrchestratorError> {
    files::check_deadline(deadline)?;
    let path = PathBuf::from(required_env("GITHUB_WORKSPACE")?);
    if !path.is_absolute() || !real_directory(&path) {
        return Err(internal("github_evidence_workspace"));
    }
    let path = fs::canonicalize(path).map_err(|_| internal("github_evidence_workspace"))?;
    files::check_deadline(deadline)?;
    Ok(path)
}

fn bundle_root(deadline: CheckDeadline) -> Result<PathBuf, OrchestratorError> {
    files::check_deadline(deadline)?;
    let runner_temp = PathBuf::from(required_env("RUNNER_TEMP")?);
    let root = runner_temp.join("velnor/github-evidence-bundle");
    if !runner_temp.is_absolute() || !real_directory(&runner_temp) || !real_directory(&root) {
        return Err(internal("github_evidence_bundle_root"));
    }
    let root = fs::canonicalize(root).map_err(|_| internal("github_evidence_bundle_root"))?;
    files::check_deadline(deadline)?;
    Ok(root)
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn required_env(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("github_evidence_environment"))
}

fn positive_env<T>(name: &str) -> Result<T, OrchestratorError>
where
    T: std::str::FromStr + PartialEq + Default,
{
    required_env(name)?
        .parse::<T>()
        .ok()
        .filter(|value| *value != T::default())
        .ok_or_else(|| internal("github_evidence_environment_number"))
}

fn is_lower_hex(value: &str, expected: usize) -> bool {
    value.len() == expected
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
