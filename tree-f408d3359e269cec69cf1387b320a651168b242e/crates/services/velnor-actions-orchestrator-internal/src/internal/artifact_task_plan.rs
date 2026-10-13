//! Resolve provider selection for planned artifact tasks.

use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_config::{
    ArtifactBuildTask, ExecutionConfig, ExecutionMode, VERIFICATION_TASK_JOB_PREFIX,
    VerificationTask,
};
use velnor_actions_contract_workflow::{
    ArtifactBuildProducer, ArtifactBuildProvider, ArtifactBuildTaskPlan, WorkflowIr,
    verification_job_mode,
};

/// Resolve the exact provider inventory for artifact tasks from effective routing.
pub(super) fn artifact_providers(
    config: &velnor_actions_contract_config::VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Vec<ArtifactBuildProvider> {
    if config.workflow.artifact_tasks.is_empty() {
        return Vec::new();
    }
    let configured = config
        .execution
        .as_ref()
        .map_or(ExecutionMode::Hosted, |execution| {
            execution.mode.unwrap_or_else(|| {
                if execution.default_profile == execution.scale_set_profile {
                    ExecutionMode::ScaleSet
                } else {
                    ExecutionMode::Hosted
                }
            })
        });
    let mode = dispatch.unwrap_or(configured);
    match mode {
        ExecutionMode::Hosted => vec![ArtifactBuildProvider::GithubHosted],
        ExecutionMode::ScaleSet => vec![ArtifactBuildProvider::VelnorScaleSet],
        ExecutionMode::Both => vec![
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ],
    }
}

/// Build plan artifact producers from one prepared config and workflow.
pub(super) fn planned_artifact_tasks_for_config(
    config: &velnor_actions_contract_config::VelnorConfig,
    dispatch: Option<ExecutionMode>,
    workflow: &WorkflowIr,
) -> Result<Vec<ArtifactBuildTaskPlan>, ContractError> {
    let matrix_providers = artifact_providers(config, dispatch);
    planned_artifact_tasks(
        &config.workflow.artifact_tasks,
        &config.workflow.tasks,
        &matrix_providers,
        config.schema,
        config.execution.as_ref(),
        dispatch,
        workflow,
    )
}

pub(super) fn planned_artifact_tasks(
    artifact_tasks: &[ArtifactBuildTask],
    verification_tasks: &[VerificationTask],
    matrix_providers: &[ArtifactBuildProvider],
    schema: u32,
    execution: Option<&ExecutionConfig>,
    dispatch: Option<ExecutionMode>,
    workflow: &WorkflowIr,
) -> Result<Vec<ArtifactBuildTaskPlan>, ContractError> {
    let mut planned = artifact_tasks
        .iter()
        .cloned()
        .map(|task| ArtifactBuildTaskPlan {
            task,
            producer: ArtifactBuildProducer::MatrixBuild,
            providers: matrix_providers.to_vec(),
        })
        .collect::<Vec<_>>();
    for task in verification_tasks
        .iter()
        .filter(|task| !task.outputs.is_empty())
    {
        let job_id = format!("{VERIFICATION_TASK_JOB_PREFIX}{}", task.id);
        let job = workflow.jobs.get(&job_id).ok_or_else(|| {
            ContractError::identity(
                "workflow.tasks",
                format!("missing_verification_job:{job_id}"),
            )
        })?;
        let providers = providers_for_verification_task(schema, execution, dispatch, &job_id, job)?;
        planned.push(ArtifactBuildTaskPlan {
            task: ArtifactBuildTask {
                id: task.id.clone(),
                mise_task: task.mise_task.clone(),
                runner: task.runner,
                timeout_minutes: task.timeout_minutes,
                outputs: task.outputs.clone(),
            },
            producer: ArtifactBuildProducer::VerificationTask,
            providers,
        });
    }
    planned.sort_by(|left, right| left.task.id.cmp(&right.task.id));
    Ok(planned)
}

fn providers_for_verification_task(
    schema: u32,
    execution: Option<&ExecutionConfig>,
    dispatch: Option<ExecutionMode>,
    job_id: &str,
    job: &velnor_actions_contract_workflow::Job,
) -> Result<Vec<ArtifactBuildProvider>, ContractError> {
    Ok(
        match verification_job_mode(schema, execution, dispatch, job_id, job)? {
            ExecutionMode::Hosted => vec![ArtifactBuildProvider::GithubHosted],
            ExecutionMode::ScaleSet => vec![ArtifactBuildProvider::VelnorScaleSet],
            ExecutionMode::Both => vec![
                ArtifactBuildProvider::GithubHosted,
                ArtifactBuildProvider::VelnorScaleSet,
            ],
        },
    )
}

#[cfg(test)]
mod tests;
