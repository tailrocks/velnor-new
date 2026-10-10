//! Shared local composites for typed declared-task report envelopes.
//!
//! `TaskExecution` carries argv, task/report identity, and env as distinct
//! validated fields. This renderer never recovers task metadata from shell
//! source or accepts an environment marker as proof of eligibility.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, RunsOn, ScaleSetSelector, Step, StepKind, StepRole, TASK_EXECUTION_MANIFEST_PATH,
    TASK_EXECUTION_MANIFEST_SCHEMA, TaskExecutionManifestEntryV1, TaskExecutionManifestV1,
    VerificationRunner,
};

use crate::{
    RenderError, action_ref::DECLARED_TASK_ACTION_PREFIX, composite, marker, steps,
    tree::RenderedFile, yaml::Yaml,
};

const ACTION_NAME_PREFIX: &str = "declared-task-";
const EXECUTION_DIGEST_INPUT: &str = "digest";
const RUNTIME_RUNNER_TEMP_ENV: &str = "VELNOR_RUNTIME_RUNNER_TEMP";
const GENERATOR_VERSION_ENV: &str = "VELNOR_GENERATOR_VERSION";
const TASK_EXECUTION_DIGEST_ENV: &str = "VELNOR_TASK_EXECUTION_DIGEST";
const RUNNER_TEMP_EXPRESSION: &str = "${{ runner.temp }}";

#[path = "task_wrapper_script.rs"]
mod script;
use self::script::task_script;

#[path = "task_wrapper_declared.rs"]
mod declared;
#[cfg(test)]
use self::declared::declared_task_document;
use self::declared::{declared_task_call, declared_task_file, validate_task_fields};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Shape {
    helper_version: String,
}

struct TaskFactorContext<'a> {
    checkout_uses: &'a str,
    report_helper_version: &'a str,
    workflow_tasks: &'a [crate::verification_jobs::WorkflowTaskPolicy],
    scale_set_selector: Option<&'a ScaleSetSelector>,
}

struct PreparedTaskExecution<'a> {
    record: TaskExecutionManifestEntryV1,
    reference: TaskExecutionRef<'a>,
}

struct CollectedTaskExecutions<'a> {
    eligible: BTreeMap<(String, usize), TaskExecutionRef<'a>>,
    shapes: BTreeSet<Shape>,
    manifest_tasks: BTreeMap<String, TaskExecutionManifestEntryV1>,
}

/// Replace typed task steps and emit one composite per structural shape.
///
/// Every typed step must use a supported Linux runner and follow the
/// configured checkout. A task which violates either precondition fails
/// closed instead of silently taking a shell fallback. The caller name and
/// condition remain unchanged.
pub(crate) fn factor_obligation_steps(
    jobs: &BTreeMap<String, Job>,
    checkout_uses: &str,
    generator_version: &str,
    report_helper_version: &str,
    workflow_tasks: &[crate::verification_jobs::WorkflowTaskPolicy],
    scale_set_selector: Option<&ScaleSetSelector>,
) -> Result<(BTreeMap<String, Job>, Vec<RenderedFile>), RenderError> {
    let context = TaskFactorContext {
        checkout_uses,
        report_helper_version,
        workflow_tasks,
        scale_set_selector,
    };
    let collected = collect_task_executions(jobs, &context)?;
    let shape_ids = index_shapes(collected.shapes);
    let factored = replace_task_steps(jobs, &collected.eligible, &shape_ids)?;
    let files = render_task_files(&shape_ids, collected.manifest_tasks, generator_version)?;
    Ok((factored, files))
}

fn collect_task_executions<'a>(
    jobs: &'a BTreeMap<String, Job>,
    context: &TaskFactorContext<'_>,
) -> Result<CollectedTaskExecutions<'a>, RenderError> {
    let mut eligible = BTreeMap::<(String, usize), TaskExecutionRef<'_>>::new();
    let mut shapes = BTreeSet::new();
    let mut manifest_tasks = BTreeMap::<String, TaskExecutionManifestEntryV1>::new();
    for (job_id, job) in jobs {
        for (step_index, step) in job.steps.iter().enumerate() {
            let Some(prepared) = prepare_task_execution(job_id, job, step_index, step, context)?
            else {
                continue;
            };
            match manifest_tasks.entry(prepared.record.task_id.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(prepared.record.clone());
                }
                std::collections::btree_map::Entry::Occupied(entry)
                    if entry.get() == &prepared.record => {}
                std::collections::btree_map::Entry::Occupied(_) => {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "declared_task_id_not_unique:{}",
                        prepared.record.task_id
                    )));
                }
            }
            let shape = Shape {
                helper_version: prepared.reference.helper_version.clone(),
            };
            shapes.insert(shape);
            eligible.insert((job_id.clone(), step_index), prepared.reference);
        }
    }
    Ok(CollectedTaskExecutions {
        eligible,
        shapes,
        manifest_tasks,
    })
}

fn prepare_task_execution<'a>(
    job_id: &str,
    job: &Job,
    step_index: usize,
    step: &'a Step,
    context: &TaskFactorContext<'_>,
) -> Result<Option<PreparedTaskExecution<'a>>, RenderError> {
    let StepKind::TaskExecution {
        argv,
        env,
        task_id,
        task_digest,
        matrix_id,
        matrix_key,
        report_helper_version: task_helper_version,
        matrix_max_parallel,
        toolchain_inputs,
    } = &step.kind
    else {
        return Ok(None);
    };
    if !supports_task_runner(
        job_id,
        &job.runs_on,
        context.workflow_tasks,
        context.scale_set_selector,
    ) {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_requires_supported_linux_runner:{job_id}"
        )));
    }
    validate_task_job_scope(job_id, job, step)?;
    if task_helper_version != context.report_helper_version {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_helper_version_mismatch:{job_id}"
        )));
    }
    validate_task_checkout(
        job_id,
        job,
        step_index,
        context.checkout_uses,
        context.report_helper_version,
    )?;
    validate_task_fields(argv, env, task_id, task_digest, matrix_id, matrix_key)?;
    let mut record = TaskExecutionManifestEntryV1 {
        task_id: task_id.clone(),
        execution_digest: String::new(),
        task_digest: task_digest.clone(),
        toolchain_inputs: toolchain_inputs.clone(),
        argv: argv.clone(),
        env: env.clone(),
        matrix_id: matrix_id.clone(),
        matrix_key: matrix_key.clone(),
        report_helper_version: task_helper_version.clone(),
        matrix_max_parallel: *matrix_max_parallel,
    };
    record
        .refresh_execution_digest()
        .map_err(RenderError::Contract)?;
    let reference = TaskExecutionRef {
        execution_digest: record.execution_digest.clone(),
        helper_version: task_helper_version,
    };
    Ok(Some(PreparedTaskExecution { record, reference }))
}

fn validate_task_job_scope(job_id: &str, job: &Job, step: &Step) -> Result<(), RenderError> {
    if job.check_runner.is_some()
        || !job
            .needs
            .iter()
            .any(|need| need == crate::render::PLAN_JOB_ID)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_job_scope_mismatch:{job_id}"
        )));
    }
    if step.id.is_some() || step.role.is_some() || step.condition.is_none() {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_authority_mismatch:{job_id}"
        )));
    }
    Ok(())
}

fn validate_task_checkout(
    job_id: &str,
    job: &Job,
    step_index: usize,
    checkout_uses: &str,
    report_helper_version: &str,
) -> Result<(), RenderError> {
    let checkout_at = job.steps[..step_index].iter().position(|previous| {
        velnor_actions_contract::workflow::step_identity::is_configured_checkout(
            previous,
            checkout_uses,
        )
    });
    let Some(checkout_at) = checkout_at else {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_requires_checkout:{job_id}"
        )));
    };
    let staged_binary = format!("{}{report_helper_version}", steps::STAGED_BINARY_PREFIX);
    let helper_staged = job.steps[checkout_at + 1..step_index]
        .iter()
        .any(|previous| helper_staged_by(previous, &staged_binary));
    if !helper_staged {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_requires_staged_helper:{job_id}"
        )));
    }
    Ok(())
}

fn index_shapes(shapes: BTreeSet<Shape>) -> BTreeMap<Shape, usize> {
    shapes
        .into_iter()
        .enumerate()
        .map(|(index, shape)| (shape, index))
        .collect()
}

fn replace_task_steps(
    jobs: &BTreeMap<String, Job>,
    eligible: &BTreeMap<(String, usize), TaskExecutionRef<'_>>,
    shape_ids: &BTreeMap<Shape, usize>,
) -> Result<BTreeMap<String, Job>, RenderError> {
    let mut next = jobs.clone();
    for ((job_id, step_index), task) in &eligible {
        let shape = Shape {
            helper_version: task.helper_version.clone(),
        };
        let action_id = shape_ids.get(&shape).ok_or_else(|| {
            RenderError::InvalidWorkflow("declared_task_shape_missing".to_owned())
        })?;
        let original = jobs
            .get(job_id)
            .and_then(|job| job.steps.get(*step_index))
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_step_missing".to_owned()))?;
        let action = declared_task_call(*action_id, task, original);
        let job = next
            .get_mut(job_id)
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_job_missing".to_owned()))?;
        let caller = job
            .steps
            .get_mut(*step_index)
            .ok_or_else(|| RenderError::InvalidWorkflow("declared_task_step_missing".to_owned()))?;
        *caller = action;
    }
    Ok(next)
}

fn render_task_files(
    shape_ids: &BTreeMap<Shape, usize>,
    manifest_tasks: BTreeMap<String, TaskExecutionManifestEntryV1>,
    generator_version: &str,
) -> Result<Vec<RenderedFile>, RenderError> {
    let mut files = Vec::with_capacity(shape_ids.len() + usize::from(!manifest_tasks.is_empty()));
    for (shape, action_id) in &shape_ids {
        files.push(declared_task_file(*action_id, shape, generator_version)?);
    }
    if !manifest_tasks.is_empty() {
        let manifest = TaskExecutionManifestV1 {
            schema: TASK_EXECUTION_MANIFEST_SCHEMA,
            generator_version: generator_version.to_owned(),
            tasks: manifest_tasks,
        };
        let bytes = manifest.marked_json().map_err(RenderError::Contract)?;
        steps::scan_for_private_subcommands(&bytes)?;
        files.push(RenderedFile {
            path: TASK_EXECUTION_MANIFEST_PATH.to_owned(),
            bytes,
        });
    }
    Ok(files)
}

/// Admit a hosted Ubuntu catalog runner or the exact Scale Set resolved from
/// the validated Linux/amd64 execution profile. Crate-obligation jobs share
/// that profile without being workflow-task jobs themselves. A selector's
/// label alone never proves the platform.
fn supports_task_runner(
    job_id: &str,
    runs_on: &str,
    workflow_tasks: &[crate::verification_jobs::WorkflowTaskPolicy],
    scale_set_selector: Option<&ScaleSetSelector>,
) -> bool {
    match RunsOn::parse(runs_on) {
        Ok(RunsOn::Hosted(label)) => {
            label.starts_with("ubuntu-")
                && velnor_actions_contract::config::RUNNER_LABEL_CATALOG.contains(&label.as_str())
        }
        Ok(RunsOn::ScaleSet(selector)) => {
            let Some(configured) = scale_set_selector else {
                return false;
            };
            if configured != &selector {
                return false;
            }
            workflow_tasks
                .iter()
                .find(|task| task.owns_job_id(job_id))
                .is_none_or(|task| {
                    let crate::verification_jobs::WorkflowTaskPolicy::Verification(policy) = task
                    else {
                        return false;
                    };
                    policy.task.runner == VerificationRunner::LinuxX64
                        && policy.runner_label == VerificationRunner::LinuxX64.runs_on()
                        && policy.scale_set_token.as_deref() == Some(runs_on)
                })
        }
        Err(_) => false,
    }
}

fn helper_staged_by(step: &Step, staged_binary: &str) -> bool {
    let StepKind::Shell { run, .. } = &step.kind else {
        return false;
    };
    (crate::closure::is_acquire_step(step) || step.role == Some(StepRole::PreseedStage))
        && run.iter().any(|argument| argument.contains(staged_binary))
}

struct TaskExecutionRef<'a> {
    execution_digest: String,
    helper_version: &'a String,
}

#[cfg(test)]
#[path = "task_wrapper_tests.rs"]
mod tests;
