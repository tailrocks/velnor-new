//! Plan-bound export for static verification jobs.

use std::path::Path;

use velnor_actions_contract::{parse_strict_json, validate_run_key};
use velnor_actions_contract_workflow::{
    ArtifactBuildProducer, ArtifactBuildProvider, ArtifactBuildRunContext, Plan, artifact_name,
    artifact_plan_providers, canonical_plan_digest, expected_artifact_builds,
};
use velnor_actions_orchestrator_core::{OrchestratorError, internal, internal_contract};

use super::{
    ArtifactExportInvocation, MaterializedArtifact, materialize_planned_artifact, required_env,
    required_path,
};

/// Export one static task's declared outputs using its existing Mise job.
/// The task ID is fixed in the generated step; other values derive from the
/// downloaded plan and GitHub's immutable run/runner context.
/// # Errors
/// Missing, malformed, or mismatched plan/job/runner data fails closed.
pub fn materialize_verification_artifact_from_environment(
    velnor_dir: &Path,
) -> Result<MaterializedArtifact, OrchestratorError> {
    let (runner_temp, run_id, run_attempt, plan_text, plan) = load_plan(velnor_dir)?;
    let providers = artifact_plan_providers(&plan);
    if providers.is_empty() {
        return Err(internal("artifact_plan_has_no_tasks"));
    }
    let task_id = required_env(velnor_actions_contract_workflow::ARTIFACT_TASK_ID_ENV)?;
    let environment = required_env("RUNNER_ENVIRONMENT")?;
    let provider = provider_for_environment(&environment)?;
    let context = run_context(run_id, run_attempt)?;
    let expected = expected_artifact_builds(&plan, &context, &providers)
        .map_err(internal_contract)?
        .into_iter()
        .find(|item| {
            item.producer == ArtifactBuildProducer::VerificationTask
                && item.identity.provider == provider
                && item.identity.task_id == task_id
        })
        .ok_or_else(|| internal("artifact_task_not_in_plan"))?;
    let identity = expected.identity;
    let invocation = ArtifactExportInvocation {
        repository_id: context.repository_id,
        repository: context.repository,
        github_sha: required_env("GITHUB_SHA")?,
        source_sha: plan.head.clone(),
        plan_digest: canonical_plan_digest(&plan).map_err(internal_contract)?,
        run_id: context.run_id,
        run_attempt: context.run_attempt,
        workflow_job_id: required_env("GITHUB_JOB")?,
        runner_environment: environment,
        runner_os: required_env("RUNNER_OS")?,
        runner_arch: required_env("RUNNER_ARCH")?,
        provider,
        task_id: task_id.clone(),
        mise_task: expected_mise_task(&plan, &task_id)?,
        artifact_name: artifact_name(&identity).map_err(internal_contract)?,
        producer: ArtifactBuildProducer::VerificationTask,
    };
    let repository_root = std::env::current_dir()
        .map_err(|error| OrchestratorError::io("current_dir", error.to_string()))?;
    materialize_planned_artifact(&repository_root, &runner_temp, &plan_text, invocation)
}

fn load_plan(
    velnor_dir: &Path,
) -> Result<(std::path::PathBuf, String, u32, String, Plan), OrchestratorError> {
    let runner_temp = required_path("RUNNER_TEMP")?;
    if velnor_dir != runner_temp.join("velnor") {
        return Err(internal("bad_artifact_temp_scope"));
    }
    let run_id = required_env("GITHUB_RUN_ID")?;
    let run_attempt = required_run_attempt()?;
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
    let value = parse_strict_json(&plan_text).map_err(internal_contract)?;
    let plan: Plan = serde_json::from_value(value).map_err(|_| internal("bad_artifact_plan"))?;
    plan.validate().map_err(internal_contract)?;
    Ok((runner_temp, run_id, run_attempt, plan_text, plan))
}

fn run_context(
    run_id: String,
    run_attempt: u32,
) -> Result<ArtifactBuildRunContext, OrchestratorError> {
    Ok(ArtifactBuildRunContext {
        repository_id: required_env("GITHUB_REPOSITORY_ID")?,
        repository: required_env("GITHUB_REPOSITORY")?,
        run_id,
        run_attempt,
    })
}

fn required_run_attempt() -> Result<u32, OrchestratorError> {
    required_env("GITHUB_RUN_ATTEMPT")?
        .parse::<u32>()
        .ok()
        .filter(|attempt| *attempt > 0)
        .ok_or_else(|| internal("bad_run_attempt"))
}

fn expected_mise_task(plan: &Plan, task_id: &str) -> Result<String, OrchestratorError> {
    plan.artifact_tasks
        .iter()
        .find(|item| {
            item.producer == ArtifactBuildProducer::VerificationTask && item.task.id == task_id
        })
        .map(|item| item.task.mise_task.clone())
        .ok_or_else(|| internal("artifact_task_not_in_plan"))
}

fn provider_for_environment(environment: &str) -> Result<ArtifactBuildProvider, OrchestratorError> {
    match environment {
        "github-hosted" => Ok(ArtifactBuildProvider::GithubHosted),
        "self-hosted" => Ok(ArtifactBuildProvider::VelnorScaleSet),
        _ => Err(internal("bad_artifact_runner_environment")),
    }
}
