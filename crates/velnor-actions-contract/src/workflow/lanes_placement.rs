use super::hosted_tasks;
use super::{LaneClass, Placement};
use crate::config::{
    CheckExecutor, CheckPlatform, ExecutionConfig, ExecutionMode, ExecutionRole,
    RUNNER_LABEL_CATALOG, RunnerPlatform, RunsOn, VelnorConfig, VerificationRunner,
    WORKFLOW_TASK_JOB_PREFIX, WorkflowTask, runner_platform,
};
use crate::errors::ContractError;
use crate::workflow::ir::Job;

pub(in crate::workflow) fn placement_for(
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
    class: LaneClass,
    job_id: &str,
    job: &Job,
) -> Result<Placement, ContractError> {
    placement_for_source(config, execution, dispatch, class, job_id, job_id, job)
}

pub(super) fn placement_for_executor(
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
    class: LaneClass,
    job_id: &str,
    job: &Job,
    qualification_sources: &std::collections::BTreeMap<String, String>,
) -> Result<Placement, ContractError> {
    let source_id = qualification_sources.get(job_id).cloned();
    match source_id {
        Some(source_id) => {
            placement_for_source(config, execution, dispatch, class, job_id, &source_id, job)
        }
        None => placement_for(config, execution, dispatch, class, job_id, job),
    }
}

pub(in crate::workflow) fn placement_for_source(
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
    class: LaneClass,
    job_id: &str,
    override_id: &str,
    job: &Job,
) -> Result<Placement, ContractError> {
    if let Some(over) = execution.overrides.get(override_id)
        && !role_matches(over.role, class)
    {
        return Err(ContractError::config(
            "config.toml",
            format!("execution.overrides.{override_id}.role"),
            "role_not_eligible",
        ));
    }
    let (is_image_job, supports_scale_set) =
        if let Some(checked) = config.check_image_job_runner(job_id, &job.runs_on) {
            checked?;
            (
                true,
                runner_platform(&job.runs_on) == Some(RunnerPlatform::LinuxAmd64),
            )
        } else {
            (false, job_supports_scale_set(job_id, job, config)?)
        };
    if !is_image_job
        && let Some(runner) = hosted_tasks::declared_hosted_macos_task_runner(config, job_id)
    {
        if execution
            .overrides
            .get(override_id)
            .is_some_and(|over| over.profile == execution.scale_set_profile)
        {
            return Err(ContractError::config(
                "config.toml",
                format!("execution.overrides.{override_id}.profile"),
                "workflow_task_hosted_only_scale_set_override",
            ));
        }
        if job.runs_on == runner {
            return Ok(Placement::HostedOnly);
        }
    }
    let capabilities = PlacementCapabilities {
        supports_scale_set,
        is_image_job,
    };
    placement_for_mode_or_profile(
        execution,
        dispatch,
        class,
        job_id,
        override_id,
        job,
        capabilities,
    )
}

#[derive(Clone, Copy)]
struct PlacementCapabilities {
    supports_scale_set: bool,
    is_image_job: bool,
}

fn placement_for_mode_or_profile(
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
    class: LaneClass,
    job_id: &str,
    override_id: &str,
    job: &Job,
    capabilities: PlacementCapabilities,
) -> Result<Placement, ContractError> {
    if let Some(mode) = dispatch.or(execution.mode) {
        return match mode {
            ExecutionMode::Hosted => Ok(Placement::HostedOnly),
            ExecutionMode::ScaleSet => {
                require_scale_set_support(
                    capabilities.supports_scale_set,
                    execution,
                    job_id,
                    override_id,
                    job,
                )?;
                Ok(Placement::ScaleSetOnly)
            }
            ExecutionMode::Both => Ok(
                if !capabilities.is_image_job
                    && class == LaneClass::Verification
                    && capabilities.supports_scale_set
                    && supports_hosted_scale_set_pair(job)
                {
                    Placement::Both
                } else {
                    Placement::HostedOnly
                },
            ),
        };
    }
    let key = execution
        .overrides
        .get(override_id)
        .map_or(execution.default_profile.as_str(), |over| {
            over.profile.as_str()
        });
    if key == execution.hosted_profile {
        Ok(Placement::HostedOnly)
    } else if key == execution.scale_set_profile {
        require_scale_set_support(
            capabilities.supports_scale_set,
            execution,
            job_id,
            override_id,
            job,
        )?;
        Ok(Placement::ScaleSetOnly)
    } else {
        Err(ContractError::config(
            "config.toml",
            format!("execution.overrides.{job_id}.profile"),
            format!("unknown_profile:{key}"),
        ))
    }
}

/// Make sure that a job can run on the Linux/amd64 Scale Set.
fn job_supports_scale_set(
    job_id: &str,
    job: &Job,
    config: &VelnorConfig,
) -> Result<bool, ContractError> {
    if let Some(check) = &job.check_runner {
        return Ok(check.platform == CheckPlatform::LinuxX64);
    }
    let Some(task_id) = job_id.strip_prefix(WORKFLOW_TASK_JOB_PREFIX) else {
        return Ok(match RunsOn::parse(&job.runs_on) {
            Ok(RunsOn::ScaleSet(_)) => true,
            Ok(RunsOn::Hosted(label)) => {
                RUNNER_LABEL_CATALOG.contains(&label.as_str()) && !label.ends_with("-arm")
            }
            Err(_) => false,
        });
    };
    let tasks = &config.workflow.tasks;
    let Some(task) = tasks.iter().find(|task| task.id() == task_id) else {
        return Err(ContractError::config(
            "config.toml",
            format!("workflow.tasks.id:{task_id}"),
            "task_job_without_declaration",
        ));
    };
    let (expected_runner, supports_scale_set) = match task {
        WorkflowTask::Verification(verification) => (
            verification.runner.runs_on(),
            verification.runner == VerificationRunner::LinuxX64,
        ),
        WorkflowTask::Build(build) => (build.runner.runs_on(), false),
        WorkflowTask::NativeImage(image) => (image.platform.runs_on(), false),
    };
    if job.runs_on != expected_runner {
        return Err(ContractError::config(
            "config.toml",
            format!("workflow.tasks.runner:{job_id}"),
            format!("runner_mismatch:{expected_runner}:{}", job.runs_on),
        ));
    }
    Ok(supports_scale_set)
}

fn supports_hosted_scale_set_pair(job: &Job) -> bool {
    let hosted_label = match RunsOn::parse(&job.runs_on) {
        Ok(RunsOn::Hosted(_)) => true,
        Ok(RunsOn::ScaleSet(_)) | Err(_) => false,
    };
    hosted_label
        && job
            .check_runner
            .as_ref()
            .is_none_or(|runner| runner.executor == CheckExecutor::Hosted)
}

fn require_scale_set_support(
    supports_scale_set: bool,
    execution: &ExecutionConfig,
    job_id: &str,
    override_id: &str,
    job: &Job,
) -> Result<(), ContractError> {
    if supports_scale_set {
        return Ok(());
    }
    let issue = if job.runs_on.starts_with("macos-")
        || job
            .check_runner
            .as_ref()
            .is_some_and(|runner| runner.platform != CheckPlatform::LinuxX64)
    {
        format!("unsupported_darwin_workload:{job_id}:{}", job.runs_on)
    } else {
        format!(
            "unsupported_workload_for_linux_amd64_scale_set:{job_id}:{}",
            job.runs_on
        )
    };
    let key = if execution
        .overrides
        .get(override_id)
        .is_some_and(|over| over.profile == execution.scale_set_profile)
    {
        format!("execution.overrides.{job_id}.profile")
    } else if execution.default_profile == execution.scale_set_profile {
        "execution.default_profile".to_owned()
    } else {
        "execution.mode".to_owned()
    };
    Err(ContractError::config("config.toml", key, issue))
}

fn role_matches(role: ExecutionRole, class: LaneClass) -> bool {
    matches!(
        (role, class),
        (ExecutionRole::Control, LaneClass::Control)
            | (ExecutionRole::Release, LaneClass::SingleWriter)
            | (ExecutionRole::Verification, LaneClass::Verification)
    )
}
