//! Symlink-safe streaming of declared build outputs into upload-ready files.

use std::io::Write;
use std::path::{Path, PathBuf};

use velnor_actions_contract::canonical::{Blake3Accumulator, canonical_json_bytes};
use velnor_actions_contract::{parse_strict_json, validate_run_key};
use velnor_actions_contract_config::ArtifactBuildTask;
use velnor_actions_contract_workflow::{
    ArtifactBuildFile, ArtifactBuildIdentity, ArtifactBuildProvider, ArtifactBuildResult,
    ArtifactBuildRunContext, Plan, artifact_name, canonical_plan_digest, expected_artifact_builds,
};
use velnor_actions_orchestrator_core::exclusive_write::{
    create_dir_no_symlink, write_exclusive, write_exclusive_with,
};
use velnor_actions_orchestrator_core::safe_read::stream_repo_file;
use velnor_actions_orchestrator_core::{OrchestratorError, internal, internal_contract};

/// Runtime values emitted by the plan matrix and GitHub's immutable job context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactExportInvocation {
    /// GitHub repository numeric identity.
    pub repository_id: String,
    /// GitHub `owner/repository` slug.
    pub repository: String,
    /// GitHub event source SHA.
    pub github_sha: String,
    /// Source SHA copied from the plan-produced matrix.
    pub source_sha: String,
    /// Plan digest copied from the plan-produced matrix.
    pub plan_digest: String,
    /// GitHub run ID.
    pub run_id: String,
    /// GitHub run attempt.
    pub run_attempt: u32,
    /// Actual workflow job key (`GITHUB_JOB`).
    pub workflow_job_id: String,
    /// Actual runner environment from Actions (`github-hosted` or `self-hosted`).
    pub runner_environment: String,
    /// Actual runner operating system.
    pub runner_os: String,
    /// Actual runner architecture.
    pub runner_arch: String,
    /// Provider token from the plan-produced matrix.
    pub provider: ArtifactBuildProvider,
    /// Task ID from the plan-produced matrix.
    pub task_id: String,
    /// Mise task name from the plan-produced matrix.
    pub mise_task: String,
    /// Exact artifact name from the plan-produced matrix.
    pub artifact_name: String,
}

/// Result manifest filename included in every build artifact.
pub const ARTIFACT_RESULT_FILENAME: &str = "artifact-result.json";
/// Directory containing output files in one upload artifact.
pub const ARTIFACT_OUTPUTS_DIRECTORY: &str = "outputs";

/// One fully staged artifact result, ready for the pinned upload action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedArtifact {
    /// Run/task-specific directory to upload.
    pub directory: PathBuf,
    /// Exact result manifest written to that directory.
    pub result: ArtifactBuildResult,
}

/// Stream all declared outputs into a fresh, run-scoped upload directory.
///
/// Reads remain under the checkout, reject symlinks and non-files, and stop at
/// each declaration's byte bound. Only output files and the identity manifest
/// are staged; no repository-root paths or secrets are copied implicitly.
///
/// # Errors
/// Fails on invalid task or identity, absent/unsafe/oversize/empty output,
/// duplicate destination, or unwritable runner temporary storage.
pub fn materialize_artifact(
    repository_root: &Path,
    runner_temp: &Path,
    task: &ArtifactBuildTask,
    identity: ArtifactBuildIdentity,
) -> Result<MaterializedArtifact, OrchestratorError> {
    task.validate("plan").map_err(internal_contract)?;
    if identity.task_id != task.id {
        return Err(internal("artifact_task_identity_mismatch"));
    }
    let artifact_name =
        velnor_actions_contract_workflow::artifact_name(&identity).map_err(internal_contract)?;
    let directory = runner_temp
        .join("velnor")
        .join("artifact-builds")
        .join(&artifact_name);
    create_dir_no_symlink(runner_temp, &directory)?;
    let outputs_dir = directory.join(ARTIFACT_OUTPUTS_DIRECTORY);
    create_dir_no_symlink(runner_temp, &outputs_dir)?;

    let mut files = Vec::with_capacity(task.outputs.len());
    for declaration in &task.outputs {
        let path = outputs_dir.join(format!("{}.bin", declaration.id));
        let (size_bytes, digest) = write_exclusive_with(&path, "artifact_output", |destination| {
            let mut digest = Blake3Accumulator::new();
            let size_bytes = stream_repo_file(
                repository_root,
                &declaration.path,
                declaration.max_bytes,
                |chunk| {
                    destination.write_all(chunk).map_err(|error| {
                        OrchestratorError::io(path.display().to_string(), error.to_string())
                    })?;
                    digest.update(chunk);
                    Ok(())
                },
            )?;
            if size_bytes == 0 {
                return Err(internal(&format!(
                    "artifact_empty_output:{}",
                    declaration.id
                )));
            }
            Ok((size_bytes, digest.finalize()))
        })?;
        files.push(ArtifactBuildFile {
            output_id: declaration.id.clone(),
            path: declaration.path.clone(),
            size_bytes,
            digest,
        });
    }

    let result = ArtifactBuildResult {
        schema: ArtifactBuildResult::SCHEMA,
        artifact_name,
        identity,
        outputs: files,
    };
    result
        .validate_for(task, &result.identity)
        .map_err(internal_contract)?;
    let manifest = canonical_json_bytes(&result).map_err(internal_contract)?;
    write_exclusive(
        &directory.join(ARTIFACT_RESULT_FILENAME),
        &manifest,
        "artifact_result",
    )?;
    Ok(MaterializedArtifact { directory, result })
}

/// Verify GitHub and matrix context against a downloaded, validated plan,
/// then stage exactly the task's declared output files.
///
/// The plan digest binds this step to the earlier plan job even though the
/// build task itself executes between plan download and export. The runner
/// context verifies that the hosted lane remains genuinely GitHub-hosted and
/// the Velnor lane remains self-hosted Linux/x64.
///
/// # Errors
/// Fails closed when any environment, matrix, plan, task, provider, or output
/// identity differs from the authoritative plan.
pub fn materialize_planned_artifact(
    repository_root: &Path,
    runner_temp: &Path,
    plan_text: &str,
    invocation: ArtifactExportInvocation,
) -> Result<MaterializedArtifact, OrchestratorError> {
    let value = parse_strict_json(plan_text).map_err(internal_contract)?;
    let plan: Plan = serde_json::from_value(value).map_err(|_| internal("bad_artifact_plan"))?;
    plan.validate().map_err(internal_contract)?;

    let run_key = format!("r{}-a{}", invocation.run_id, invocation.run_attempt);
    validate_run_key(&run_key).map_err(internal_contract)?;
    if plan.run_key != run_key
        || plan.head != invocation.source_sha
        || invocation.source_sha != invocation.github_sha
        || canonical_plan_digest(&plan).map_err(internal_contract)? != invocation.plan_digest
    {
        return Err(internal("artifact_plan_identity_mismatch"));
    }
    if invocation.runner_os != "Linux" || invocation.runner_arch != "X64" {
        return Err(internal("artifact_runner_platform_mismatch"));
    }
    let expected_environment = match invocation.provider {
        ArtifactBuildProvider::GithubHosted => "github-hosted",
        ArtifactBuildProvider::VelnorScaleSet => "self-hosted",
    };
    if invocation.runner_environment != expected_environment {
        return Err(internal("artifact_runner_provider_mismatch"));
    }

    let context = ArtifactBuildRunContext {
        repository_id: invocation.repository_id,
        repository: invocation.repository,
        run_id: invocation.run_id,
        run_attempt: invocation.run_attempt,
    };
    let providers = plan
        .artifact_tasks
        .first()
        .map(|task| task.providers.as_slice())
        .ok_or_else(|| internal("artifact_plan_has_no_tasks"))?;
    let expected =
        expected_artifact_builds(&plan, &context, providers).map_err(internal_contract)?;
    let identity = expected
        .into_iter()
        .find(|item| {
            item.identity.provider == invocation.provider
                && item.identity.task_id == invocation.task_id
        })
        .map(|item| item.identity)
        .ok_or_else(|| internal("artifact_task_not_in_plan"))?;
    if identity.workflow_job_id != invocation.workflow_job_id {
        return Err(internal("artifact_job_identity_mismatch"));
    }
    let task = plan
        .artifact_tasks
        .iter()
        .find(|item| item.task.id == invocation.task_id)
        .map(|item| &item.task)
        .ok_or_else(|| internal("artifact_task_not_in_plan"))?;
    if task.mise_task != invocation.mise_task
        || artifact_name(&identity).map_err(internal_contract)? != invocation.artifact_name
    {
        return Err(internal("artifact_matrix_identity_mismatch"));
    }
    materialize_artifact(repository_root, runner_temp, task, identity)
}

/// Load GitHub's immutable environment plus the matrix values from the
/// current attempt, then invoke the plan-bound exporter.
///
/// # Errors
/// Missing or malformed environment, unsafe/missing plan bytes, and any
/// mismatch fail closed before output files are read.
pub fn materialize_artifact_from_environment(
    velnor_dir: &Path,
) -> Result<MaterializedArtifact, OrchestratorError> {
    let runner_temp = required_path("RUNNER_TEMP")?;
    if velnor_dir != runner_temp.join("velnor") {
        return Err(internal("bad_artifact_temp_scope"));
    }
    let run_id = required_env("GITHUB_RUN_ID")?;
    let run_attempt = required_env("GITHUB_RUN_ATTEMPT")?
        .parse::<u32>()
        .ok()
        .filter(|attempt| *attempt > 0)
        .ok_or_else(|| internal("bad_run_attempt"))?;
    let run_key = format!("r{run_id}-a{run_attempt}");
    validate_run_key(&run_key).map_err(internal_contract)?;
    let plan_path = runner_temp
        .join("velnor")
        .join(&run_key)
        .join(velnor_actions_orchestrator_core::decisions::PLAN_JSON_NAME);
    let plan_text = velnor_actions_orchestrator_core::safe_read::read_event_file(
        &plan_path,
        velnor_actions_orchestrator_core::safe_read::MAX_REPO_FILE_BYTES,
    )?;
    let invocation = ArtifactExportInvocation {
        repository_id: required_env("GITHUB_REPOSITORY_ID")?,
        repository: required_env("GITHUB_REPOSITORY")?,
        github_sha: required_env("GITHUB_SHA")?,
        source_sha: required_env(velnor_actions_contract_workflow::ARTIFACT_SOURCE_SHA_ENV)?,
        plan_digest: required_env(velnor_actions_contract_workflow::ARTIFACT_PLAN_DIGEST_ENV)?,
        run_id,
        run_attempt,
        workflow_job_id: required_env("GITHUB_JOB")?,
        runner_environment: required_env("RUNNER_ENVIRONMENT")?,
        runner_os: required_env("RUNNER_OS")?,
        runner_arch: required_env("RUNNER_ARCH")?,
        provider: provider_from_env()?,
        task_id: required_env(velnor_actions_contract_workflow::ARTIFACT_TASK_ID_ENV)?,
        mise_task: required_env(velnor_actions_contract_workflow::ARTIFACT_MISE_TASK_ENV)?,
        artifact_name: required_env(velnor_actions_contract_workflow::ARTIFACT_NAME_ENV)?,
    };
    let repository_root = std::env::current_dir()
        .map_err(|error| OrchestratorError::io("current_dir", error.to_string()))?;
    materialize_planned_artifact(&repository_root, &runner_temp, &plan_text, invocation)
}

fn required_env(key: &str) -> Result<String, OrchestratorError> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("missing_artifact_env:{key}")))
}

fn required_path(key: &str) -> Result<PathBuf, OrchestratorError> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal(&format!("missing_artifact_env:{key}")))
}

fn provider_from_env() -> Result<ArtifactBuildProvider, OrchestratorError> {
    match required_env(velnor_actions_contract_workflow::ARTIFACT_PROVIDER_ENV)?.as_str() {
        "github_hosted" => Ok(ArtifactBuildProvider::GithubHosted),
        "velnor_scale_set" => Ok(ArtifactBuildProvider::VelnorScaleSet),
        _ => Err(internal("bad_artifact_provider")),
    }
}
