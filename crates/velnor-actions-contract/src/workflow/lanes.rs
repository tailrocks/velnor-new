//! Expand one logical job graph into hosted and scale-set lanes.
//!
//! Schema 1 returns the same jobs. `hosted` keeps configured runners.
//! `scale-set` routes every supported job once. `both` duplicates eligible verification.

use super::ValidatorKind;
use super::ir::{Job, WorkflowIr};
use super::jobs::{PLAN_JOB_ID, REQUIRED_JOB_ID};
use super::named_check_lanes::add_named_check_lanes;
use super::named_check_lanes::qualification_check_source_id;
pub use super::named_check_lanes::{
    NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, NAMED_CHECK_LANES_ENV, NamedCheckLane,
    NamedCheckLaneVariant, QUALIFICATION_CHECK_JOB_PREFIX, named_check_lanes,
    qualification_job_lanes,
};
use crate::config::{
    CheckExecutor, CheckPlatform, EPHEMERAL_CHECK_ADMISSION_CONDITION, ExecutionConfig,
    ExecutionMode, ExecutionRole, NORMAL_CHECK_ADMISSION_CONDITION,
    NORMAL_EPHEMERAL_CHECK_ADMISSION_CONDITION, QUALIFICATION_CHECK_ADMISSION_CONDITION,
    RUNNER_LABEL_CATALOG, RunsOn, VelnorConfig, VerificationRunner, WORKFLOW_TASK_JOB_PREFIX,
    WorkflowTask,
};
use crate::errors::ContractError;
use std::collections::BTreeMap;

/// Suffix for the hosted copy of a verification job.
pub const HOSTED_SUFFIX: &str = "__hosted";
/// Suffix for the scale-set copy of a verification job.
pub const SCALE_SUFFIX: &str = "__local";
/// Planner class of a job id. Role overrides cannot change this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneClass {
    /// Trusted control job.
    Control,
    /// Single external writer.
    SingleWriter,
    /// Comparable verification workload.
    Verification,
}

/// Where one logical job is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
    HostedOnly,
    ScaleSetOnly,
    Both,
}

/// Which copy of a split dependency a consumer keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmitLane {
    Single,
    Hosted,
    Scale,
}

/// Classify a planner job id.
#[must_use]
pub fn lane_class(job_id: &str) -> LaneClass {
    if job_id == ValidatorKind::Actionlint.job_id()
        || matches!(
            job_id,
            PLAN_JOB_ID | REQUIRED_JOB_ID | "compare" | "queue-monitor"
        )
    {
        LaneClass::Control
    } else if job_id == "publish-baseline"
        || job_id.starts_with("release-")
        || job_id.starts_with("deploy-")
    {
        LaneClass::SingleWriter
    } else {
        LaneClass::Verification
    }
}

/// Expand jobs for schema 2. Schema 1 is returned unchanged.
///
/// # Errors
///
/// Unknown profiles, unknown override keys, or a role that disagrees
/// with [`lane_class`].
pub fn expand_workflow(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<WorkflowIr, ContractError> {
    expand_workflow_with_qualification_sources(ir, config, dispatch, &BTreeMap::new())
}

/// Expand jobs using source-job identities for per-entry Qualification jobs.
///
/// The source map binds each individual Rust or `ToFu` Qualification job to
/// its grouped verification job so existing runner overrides stay in force.
/// # Errors
pub fn expand_workflow_with_qualification_sources(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
    qualification_sources: &std::collections::BTreeMap<String, String>,
) -> Result<WorkflowIr, ContractError> {
    if config.schema != 2 {
        return Ok(ir.clone());
    }
    let execution = config
        .execution
        .as_ref()
        .ok_or_else(|| ContractError::config("config.toml", "execution", "missing_execution"))?;
    check_override_keys(ir, execution)?;
    let mut executor_lanes = named_check_lanes(ir, config, dispatch)?;
    for (job_id, lanes) in qualification_job_lanes(ir, config, dispatch, qualification_sources)? {
        if executor_lanes.insert(job_id, lanes).is_some() {
            return Err(ContractError::identity(
                "workflow.executor_lanes",
                "duplicate_executor_lane_map",
            ));
        }
    }
    let map = id_map(ir, config, execution, dispatch, qualification_sources)?;
    let mut jobs = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let class = lane_class(id);
        let placement = placement_for_executor(
            config,
            execution,
            dispatch,
            class,
            id,
            job,
            qualification_sources,
        )?;
        emit_job(&mut jobs, id, job, execution, class, placement, &map)?;
    }
    let mut expanded = ir.clone();
    expanded.jobs = jobs;
    add_named_check_lanes(&mut expanded, &executor_lanes)?;
    Ok(expanded)
}

fn check_override_keys(ir: &WorkflowIr, execution: &ExecutionConfig) -> Result<(), ContractError> {
    for key in execution.overrides.keys() {
        if !ir.jobs.contains_key(key) {
            return Err(ContractError::config(
                "config.toml",
                format!("execution.overrides.{key}"),
                "unknown_logical_job",
            ));
        }
    }
    Ok(())
}

fn id_map(
    ir: &WorkflowIr,
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
    qualification_sources: &std::collections::BTreeMap<String, String>,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, ContractError> {
    let mut map = std::collections::BTreeMap::new();
    for (id, job) in &ir.jobs {
        let placement = placement_for_executor(
            config,
            execution,
            dispatch,
            lane_class(id),
            id,
            job,
            qualification_sources,
        )?;
        let ids = match placement {
            Placement::Both => vec![
                format!("{id}{HOSTED_SUFFIX}"),
                format!("{id}{SCALE_SUFFIX}"),
            ],
            Placement::HostedOnly | Placement::ScaleSetOnly => vec![id.clone()],
        };
        map.insert(id.clone(), ids);
    }
    Ok(map)
}

pub(super) fn placement_for(
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
    class: LaneClass,
    job_id: &str,
    job: &Job,
) -> Result<Placement, ContractError> {
    let override_id = qualification_check_source_id(job_id).unwrap_or_else(|| job_id.to_owned());
    placement_for_source(
        config,
        execution,
        dispatch,
        class,
        job_id,
        &override_id,
        job,
    )
}

fn placement_for_executor(
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    dispatch: Option<ExecutionMode>,
    class: LaneClass,
    job_id: &str,
    job: &Job,
    qualification_sources: &std::collections::BTreeMap<String, String>,
) -> Result<Placement, ContractError> {
    let source_id = qualification_check_source_id(job_id)
        .or_else(|| qualification_sources.get(job_id).cloned());
    match source_id {
        Some(source_id) => {
            placement_for_source(config, execution, dispatch, class, job_id, &source_id, job)
        }
        None => placement_for(config, execution, dispatch, class, job_id, job),
    }
}

pub(super) fn placement_for_source(
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
    let supports_scale_set = job_supports_scale_set(job_id, job, config)?;
    if let Some(mode) = dispatch.or(execution.mode) {
        return Ok(match mode {
            ExecutionMode::Hosted => Placement::HostedOnly,
            ExecutionMode::ScaleSet => {
                require_scale_set_support(supports_scale_set, execution, job_id, override_id, job)?;
                Placement::ScaleSetOnly
            }
            ExecutionMode::Both => {
                if class == LaneClass::Verification
                    && supports_scale_set
                    && supports_hosted_scale_set_pair(job)
                {
                    Placement::Both
                } else {
                    Placement::HostedOnly
                }
            }
        });
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
        require_scale_set_support(supports_scale_set, execution, job_id, override_id, job)?;
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
    let hosted_label = match crate::config::RunsOn::parse(&job.runs_on) {
        Ok(crate::config::RunsOn::Hosted(_)) => true,
        Ok(crate::config::RunsOn::ScaleSet(_)) | Err(_) => false,
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

fn emit_job(
    jobs: &mut std::collections::BTreeMap<String, Job>,
    id: &str,
    job: &Job,
    execution: &ExecutionConfig,
    class: LaneClass,
    placement: Placement,
    map: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<(), ContractError> {
    match placement {
        Placement::HostedOnly => {
            insert_job(
                jobs,
                id.to_owned(),
                retarget(job, None, id, id, class, EmitLane::Single, map),
            )?;
        }
        Placement::ScaleSetOnly => {
            let token = execution.scale_selector()?.token();
            let mut next = retarget(job, Some(&token), id, id, class, EmitLane::Single, map);
            if class == LaneClass::Verification {
                next.display_name = scale_name(&job.display_name);
            }
            insert_job(jobs, id.to_owned(), next)?;
        }
        Placement::Both => {
            let token = execution.scale_selector()?.token();
            let hosted_id = format!("{id}{HOSTED_SUFFIX}");
            let scale_id = format!("{id}{SCALE_SUFFIX}");
            let mut hosted = retarget(job, None, id, &hosted_id, class, EmitLane::Hosted, map);
            hosted.display_name = hosted_name(&job.display_name);
            insert_job(jobs, hosted_id, hosted)?;
            let mut scale = retarget(
                job,
                Some(&token),
                id,
                &scale_id,
                class,
                EmitLane::Scale,
                map,
            );
            scale.display_name = scale_name(&job.display_name);
            insert_job(jobs, scale_id, scale)?;
        }
    }
    Ok(())
}

fn insert_job(jobs: &mut BTreeMap<String, Job>, id: String, job: Job) -> Result<(), ContractError> {
    if jobs.contains_key(&id) {
        return Err(ContractError::Collision(format!("expanded job id {id}")));
    }
    jobs.insert(id, job);
    Ok(())
}

fn retarget(
    job: &Job,
    runs_on: Option<&str>,
    source_id: &str,
    output_id: &str,
    class: LaneClass,
    lane: EmitLane,
    map: &std::collections::BTreeMap<String, Vec<String>>,
) -> Job {
    let mut next = job.clone();
    if let Some(token) = runs_on {
        token.clone_into(&mut next.runs_on);
        if next.check_runner.is_some() {
            next.condition = Some(if qualification_check_source_id(source_id).is_some() {
                QUALIFICATION_CHECK_ADMISSION_CONDITION.to_owned()
            } else if matches!(
                job.condition.as_deref(),
                Some(NORMAL_CHECK_ADMISSION_CONDITION | NORMAL_EPHEMERAL_CHECK_ADMISSION_CONDITION)
            ) {
                NORMAL_EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned()
            } else {
                EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned()
            });
        }
    }
    retarget_check_identity(&mut next, source_id, output_id, lane);
    next.needs = expand_needs(&job.needs, map, class, lane);
    next
}

fn retarget_check_identity(job: &mut Job, source_id: &str, output_id: &str, lane: EmitLane) {
    for step in &mut job.steps {
        match &mut step.kind {
            super::step::StepKind::Shell { env, .. } => {
                if let Some(job_id) = env.get_mut(NAMED_CHECK_JOB_ID_ENV)
                    && job_id == source_id
                {
                    output_id.clone_into(job_id);
                }
                if let Some(variant) = env.get_mut(NAMED_CHECK_LANE_VARIANT_ENV) {
                    match lane {
                        EmitLane::Hosted => "hosted".clone_into(variant),
                        EmitLane::Scale => "scale_set".clone_into(variant),
                        EmitLane::Single => {}
                    }
                }
            }
            super::step::StepKind::Action { with, .. } => {
                if let Some(name) = with.get_mut("name")
                    && name.starts_with("velnor-crate-")
                    && name.ends_with(&format!("-{source_id}"))
                {
                    let prefix_len = name.len() - source_id.len();
                    name.truncate(prefix_len);
                    name.push_str(output_id);
                }
            }
            super::step::StepKind::TaskExecution { .. }
            | super::step::StepKind::Internal { .. } => {}
        }
    }
}

fn expand_needs(
    needs: &[String],
    map: &std::collections::BTreeMap<String, Vec<String>>,
    class: LaneClass,
    lane: EmitLane,
) -> Vec<String> {
    let mut out = Vec::new();
    for need in needs {
        let targets = map.get(need).cloned().unwrap_or_else(|| vec![need.clone()]);
        if class != LaneClass::Verification || targets.len() == 1 {
            out.extend(targets);
            continue;
        }
        let suffix = match lane {
            EmitLane::Hosted => HOSTED_SUFFIX,
            EmitLane::Scale => SCALE_SUFFIX,
            EmitLane::Single => "",
        };
        if suffix.is_empty() {
            out.extend(targets);
        } else {
            out.extend(targets.into_iter().filter(|id| id.ends_with(suffix)));
        }
    }
    out
}

fn hosted_name(name: &str) -> String {
    format!("{name} / GitHub hosted / Linux x64")
}

fn scale_name(name: &str) -> String {
    format!("{name} / Velnor Scale Set / Linux x64")
}
