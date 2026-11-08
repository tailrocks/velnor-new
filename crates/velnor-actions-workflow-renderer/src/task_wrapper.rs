// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::cache_family::S256;
use velnor_actions_contract::{
    Job, ScaleSetSelector, Step, StepKind, TASK_EXECUTION_MANIFEST_PATH,
    TASK_EXECUTION_MANIFEST_SCHEMA, TaskExecutionManifestEntryV1, TaskExecutionManifestV1,
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

#[path = "task_wrapper_validation.rs"]
mod validation;
use validation::{supports_task_runner, validate_task_checkout, validate_task_job_scope};

use self::declared::{declared_task_call, declared_task_file, validate_task_fields};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Shape {
    helper_version: String,
    preview_loader: Option<String>,
}

struct TaskFactorContext<'a> {
    checkout_uses: &'a str,
    report_helper_version: &'a str,
    controller: &'a crate::render::ControllerLocator,
    workflow_tasks: &'a [crate::verification_jobs::WorkflowTaskPolicy],
    lane_plan: Option<&'a velnor_actions_contract::LanePlan>,
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
    producer_tasks: BTreeMap<String, BTreeMap<String, String>>,
}

pub(crate) fn factor_obligation_steps(
    jobs: &BTreeMap<String, Job>,
    ctx: &crate::render::RenderContext,
) -> Result<(BTreeMap<String, Job>, Vec<RenderedFile>), RenderError> {
    let context = TaskFactorContext {
        checkout_uses: &ctx.checkout_uses,
        report_helper_version: ctx.controller.report_helper_version(),
        controller: &ctx.controller,
        workflow_tasks: &ctx.workflow_tasks,
        lane_plan: ctx.lane_plan.as_ref(),
        scale_set_selector: ctx.scale_set_selector.as_ref(),
    };
    let collected = collect_task_executions(jobs, &context)?;
    let shape_ids = index_shapes(collected.shapes);
    let preview = matches!(
        ctx.controller,
        crate::render::ControllerLocator::ProtectedBaseDispatch { .. }
    );
    let factored = if preview {
        jobs.clone()
    } else {
        replace_task_steps(jobs, &collected.eligible, &shape_ids)?
    };
    let files = render_task_files(
        &shape_ids,
        collected.manifest_tasks,
        collected.producer_tasks,
        &ctx.generator_version,
        !preview,
    )?;
    Ok((factored, files))
}

pub fn declared_task_wrapper_digest(
    helper_version: &str,
    generator_version: &str,
) -> Result<S256, RenderError> {
    let shape_ids = index_shapes(BTreeSet::from([Shape {
        helper_version: helper_version.to_owned(),
        preview_loader: None,
    }]));
    let files = render_task_files(
        &shape_ids,
        BTreeMap::new(),
        BTreeMap::new(),
        generator_version,
        true,
    )?;
    let bytes: String = files.into_iter().map(|file| file.bytes).collect();
    Ok(S256::of(bytes.as_bytes()))
}

pub fn preview_task_wrapper_digest(
    preview: &velnor_actions_contract::source_preview::SourcePreviewLock,
    generator_version: &str,
) -> Result<S256, RenderError> {
    let loader = crate::commands::join_validated_argv_for_run(
        &crate::preview_setup::loader_command(preview),
    )?;
    let script = task_script(&preview.receipt.version, Some(&loader));
    let environment = declared::inline_task_environment(
        "b3-0000000000000000000000000000000000000000000000000000000000000000",
        generator_version,
    )?;
    let template = Yaml::Map(vec![
        ("shell".to_owned(), Yaml::str("bash")),
        ("env".to_owned(), environment),
        ("run".to_owned(), Yaml::str(script)),
    ]);
    let bytes = marker::with_marker(generator_version, &crate::yaml::render_yaml(&template))?;
    Ok(S256::of(bytes.as_bytes()))
}

fn collect_task_executions<'a>(
    jobs: &'a BTreeMap<String, Job>,
    context: &TaskFactorContext<'_>,
) -> Result<CollectedTaskExecutions<'a>, RenderError> {
    let mut eligible = BTreeMap::<(String, usize), TaskExecutionRef<'_>>::new();
    let mut shapes = BTreeSet::new();
    let mut manifest_tasks = BTreeMap::<String, TaskExecutionManifestEntryV1>::new();
    let mut producer_tasks = BTreeMap::<String, BTreeMap<String, String>>::new();
    for (job_id, job) in jobs {
        for (step_index, step) in job.steps.iter().enumerate() {
            let Some(prepared) = prepare_task_execution(job_id, job, step_index, step, context)?
            else {
                continue;
            };
            crate::task_producer_manifest::record(
                &mut producer_tasks,
                job_id,
                job,
                &prepared.record,
            )?;
            let paired = context
                .lane_plan
                .and_then(|plan| plan.route_for_output(job_id))
                .is_some_and(|route| route.variant.is_some());
            let key = if paired {
                prepared.record.matrix_id.clone()
            } else {
                prepared.record.task_id.clone()
            };
            match manifest_tasks.entry(key) {
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
                preview_loader: None,
            };
            shapes.insert(shape);
            eligible.insert((job_id.clone(), step_index), prepared.reference);
        }
    }
    Ok(CollectedTaskExecutions {
        eligible,
        shapes,
        manifest_tasks,
        producer_tasks,
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
        ..
    } = &step.kind
    else {
        return Ok(None);
    };
    if !supports_task_runner(
        job_id,
        &job.runs_on,
        context.workflow_tasks,
        context.lane_plan,
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
        context.controller,
    )?;
    validate_task_fields(argv, env, task_id, task_digest, matrix_id, matrix_key)?;
    let route = context
        .lane_plan
        .and_then(|plan| plan.route_for_output(job_id));
    if !velnor_actions_contract::workflow::matrix_entry::task_matrix_identity_matches(
        task_id,
        matrix_id,
        matrix_key,
        route.and_then(|route| route.variant),
    ) {
        return Err(RenderError::InvalidWorkflow(format!(
            "declared_task_route_mismatch:{job_id}"
        )));
    }
    let record = task_record(step)?;
    let reference = TaskExecutionRef {
        execution_digest: record.execution_digest.clone(),
        helper_version: task_helper_version,
    };
    Ok(Some(PreparedTaskExecution { record, reference }))
}

fn task_record(step: &Step) -> Result<TaskExecutionManifestEntryV1, RenderError> {
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
        return Err(RenderError::InvalidWorkflow(
            "declared_task_required".to_owned(),
        ));
    };
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
    Ok(record)
}

pub(crate) fn preview_task_step(
    step: &Step,
    ctx: &crate::render::RenderContext,
) -> Result<Yaml, RenderError> {
    let crate::render::ControllerLocator::ProtectedBaseDispatch { preview, .. } = &ctx.controller
    else {
        return Err(RenderError::InvalidWorkflow(
            "preview_task_requires_dispatch".to_owned(),
        ));
    };
    let record = task_record(step)?;
    let loader = crate::commands::join_validated_argv_for_run(
        &crate::preview_setup::loader_command(preview),
    )?;
    let shape = Shape {
        helper_version: record.report_helper_version.clone(),
        preview_loader: Some(loader),
    };
    declared::inline_task_step(
        step,
        &shape,
        &record.execution_digest,
        &ctx.generator_version,
    )
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
    for ((job_id, step_index), task) in eligible {
        let shape = Shape {
            helper_version: task.helper_version.clone(),
            preview_loader: None,
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
    producer_tasks: BTreeMap<String, BTreeMap<String, String>>,
    generator_version: &str,
    emit_actions: bool,
) -> Result<Vec<RenderedFile>, RenderError> {
    let mut files = Vec::with_capacity(shape_ids.len() + usize::from(!manifest_tasks.is_empty()));
    if emit_actions {
        for (shape, action_id) in shape_ids {
            files.push(declared_task_file(*action_id, shape, generator_version)?);
        }
    }
    if !manifest_tasks.is_empty() {
        let manifest = TaskExecutionManifestV1 {
            schema: TASK_EXECUTION_MANIFEST_SCHEMA,
            generator_version: generator_version.to_owned(),
            tasks: manifest_tasks,
        };
        let bytes = manifest.marked_json().map_err(RenderError::Contract)?;
        steps::scan_for_private_subcommands(&bytes)?;
        if let Some(file) = crate::task_producer_manifest::render(
            &manifest,
            &bytes,
            producer_tasks,
            generator_version,
        )? {
            files.push(file);
        }
        files.push(RenderedFile {
            path: TASK_EXECUTION_MANIFEST_PATH.to_owned(),
            bytes,
        });
    }
    Ok(files)
}
struct TaskExecutionRef<'a> {
    execution_digest: String,
    helper_version: &'a String,
}
