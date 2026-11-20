// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::looks_github_hosted;

use crate::call::call;
use crate::cases::cases;
use crate::check_jobs::{
    check_outputs, declared_access, job_shape, needed_outputs, output, require_permissions,
};
use crate::check_step::step;
use crate::concurrency::{concurrency, refuse_shared_group};
use crate::containers::{container, services};
use crate::error::{WorkflowsError, refuse};
use crate::expression::{Level, Scope, check_condition};
use crate::job_env::{JobExtras, job_env};
use crate::job_limits;
use crate::matrix::matrix;
use crate::plan::{Concurrency, DEFAULT_RUNNER, Job, Step, Triggers, Workflow};
use crate::raw::{RawJob, RawStep, RawWorkflow};
use crate::secret_events::untrusted;
use crate::triggers::triggers;

const RESERVED_FILES: [&str; 2] = ["rust-ci", "release"];

pub(crate) struct Context<'a> {
    pub(crate) file: &'a str,
    pub(crate) condition: Option<&'a str>,
    pub(crate) inputs: &'a [&'a str],
    pub(crate) workflows: &'a [Workflow],
    pub(crate) triggers: &'a Triggers,
    pub(crate) concurrency: Option<&'a Concurrency>,
    pub(crate) runner: &'a str,
    pub(crate) hosted: bool,
}

pub(crate) fn workflow(raw: RawWorkflow, earlier: &[Workflow]) -> Result<Workflow, WorkflowsError> {
    let file = raw.file;
    let valid_file = !file.is_empty()
        && file
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !file.starts_with('-');
    if !valid_file {
        return refuse(format!(
            "the workflow file {file:?} must be lower case letters, digits and dashes"
        ));
    }
    if RESERVED_FILES.contains(&file.as_str()) {
        return refuse(format!(
            "the workflow file {file} belongs to another generator output"
        ));
    }
    if raw.name.trim().is_empty() {
        return refuse(format!("the workflow {file} needs a name"));
    }
    let runner = runner_label_of(&file, raw.runner.as_deref().unwrap_or(DEFAULT_RUNNER))?;
    let fork_runner = runner_label_of(&file, raw.fork_runner.as_deref().unwrap_or(DEFAULT_RUNNER))?;
    if !looks_github_hosted(&fork_runner) {
        return refuse(format!(
            "the fork_runner {fork_runner} of {file} must be a GitHub-hosted runner, because it runs code of pull requests from forks"
        ));
    }
    let hosted = crate::hosting::resolve(
        &format!("the workflow {file}"),
        &runner,
        raw.hosted.or(raw.runner.is_none().then_some(true)),
    )?;
    let triggers = triggers(&file, raw.on, raw.input, raw.output)?;
    let input_names: Vec<&str> = triggers
        .workflow_call
        .iter()
        .flat_map(|call| call.inputs.iter())
        .chain(triggers.workflow_dispatch.iter().flatten())
        .map(|input| input.name.as_str())
        .collect();
    let workflow_concurrency = raw
        .concurrency
        .map(|raw| concurrency(&format!("the workflow {file}"), raw))
        .transpose()?;
    let ctx = Context {
        file: &file,
        condition: raw.condition.as_deref(),
        inputs: &input_names,
        workflows: earlier,
        triggers: &triggers,
        concurrency: workflow_concurrency.as_ref(),
        runner: &runner,
        hosted,
    };
    let mut jobs: Vec<Job> = Vec::new();
    let mut raw_jobs = cases(
        &file,
        raw.hosted_runner.as_deref(),
        raw.scale_set_runner.as_deref(),
        raw.case,
        &triggers,
    )?;
    raw_jobs.extend(raw.job);
    for job in raw_jobs {
        let checked = self::job(job, &ctx, &jobs)?;
        jobs.push(checked);
    }
    if jobs.is_empty() {
        return refuse(format!(
            "the workflow {file} needs at least one [[workflow.job]]"
        ));
    }
    job_limits::containers_on_linux(&runner, &fork_runner, &jobs)?;
    if let Some(call) = &triggers.workflow_call {
        check_outputs(&file, &call.outputs, &jobs)?;
    }
    Ok(Workflow {
        condition: raw
            .condition
            .map(|text| crate::expression::bare(&text).to_owned()),
        name: raw.name,
        runner,
        hosted,
        fork_runner,
        concurrency: workflow_concurrency,
        triggers,
        jobs,
        file,
    })
}

pub(crate) fn runner_label_of(file: &str, label: &str) -> Result<String, WorkflowsError> {
    let valid = !label.is_empty()
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if !valid {
        return refuse(format!(
            "the runner {label:?} in {file} must be a label of letters, digits, dashes, underscores and dots"
        ));
    }
    Ok(label.to_owned())
}

fn job(mut raw: RawJob, ctx: &Context<'_>, earlier: &[Job]) -> Result<Job, WorkflowsError> {
    let id = raw.id.clone();
    let timeout_minutes = job_shape(&raw, earlier)?;
    declared_access(&id, &raw.permissions, raw.environment.as_deref())?;
    let matrix = raw
        .matrix
        .take()
        .map(|matrix| self::matrix(&id, matrix, &raw.needs, earlier))
        .transpose()?;
    let chosen = crate::hosting::of_job(ctx, &id, &raw, matrix.as_ref())?;
    let job_concurrency = raw
        .concurrency
        .map(|raw| concurrency(&format!("job {id}"), raw))
        .transpose()?;
    let needed_outputs = needed_outputs(&raw.needs, earlier);
    let axes = matrix.as_ref().and_then(crate::matrix::axis_names);
    let mut scope = Scope {
        inputs: ctx.inputs,
        has_matrix: matrix.is_some(),
        matrix_keys: axes.as_deref(),
        level: Level::Job,
        needs: &raw.needs,
        needed_outputs: &needed_outputs,
        steps: Vec::new(),
        events: ctx.triggers.events(),
        untrusted_events: untrusted(ctx.triggers, [ctx.condition, raw.condition.as_deref()]),
    };
    let condition = scope_conditions(ctx, raw.condition.as_deref(), &scope)?;
    let JobExtras { env, cache_mode } = job_env(
        &id,
        (&raw.env, raw.cache_mode.as_deref()),
        [condition.as_deref(), ctx.condition],
    )?;
    let has_container = raw.container.is_some();
    let mut job = Job {
        name: raw.name.clone(),
        needs: raw.needs.clone(),
        condition,
        helper: raw.helper,
        runner: chosen.runner,
        matrix_runner: chosen.matrix_runner,
        hosted: chosen.hosted,
        environment: raw.environment.clone(),
        timeout_minutes,
        concurrency: job_concurrency,
        permissions: raw.permissions.clone().into_iter().collect(),
        cache_mode,
        env,
        outputs: Vec::new(),
        matrix,
        container: raw.container.map(|raw| container(&id, raw)).transpose()?,
        services: services(&id, raw.service, has_container)?,
        steps: Vec::new(),
        call: None,
        id,
    };
    if let Some(uses) = &raw.uses {
        scope.level = Level::Call;
        job.call = Some(call(
            &job.id,
            uses,
            &raw.with,
            &job.permissions,
            ctx,
            &scope,
        )?);
        if let Some(call) = &job.call {
            refuse_shared_group(
                &job.id,
                [job.concurrency.as_ref(), ctx.concurrency],
                &call.target,
                ctx.workflows,
            )?;
        }
        return Ok(job);
    }
    if !raw.with.is_empty() {
        return refuse(format!("job {} sets with without uses", job.id));
    }
    if job.matrix.is_some() && !raw.outputs.is_empty() {
        return refuse(format!(
            "job {} sets outputs on a matrix job; every matrix child would write the same output name and the last to finish would win, so outputs are not allowed there",
            job.id
        ));
    }
    scope.level = Level::Step;
    fill_steps(&mut job, raw.step, raw.outputs, &mut scope)?;
    Ok(job)
}

fn fill_steps(
    job: &mut Job,
    raw_steps: Vec<RawStep>,
    outputs: BTreeMap<String, String>,
    scope: &mut Scope<'_>,
) -> Result<(), WorkflowsError> {
    let mut steps: Vec<Step> = Vec::new();
    for raw_step in raw_steps {
        let checked = step(&job.id, job.helper, raw_step, scope, &steps)?;
        scope.steps.extend(checked.id.clone());
        steps.push(checked);
    }
    require_permissions(&job.id, &steps, &job.permissions)?;
    job_limits::write_token(&job.id, &steps, &job.permissions)?;
    let saving_job = scope
        .events
        .iter()
        .any(|event| matches!(*event, "push" | "schedule"));
    job.cache_mode = job_limits::saves_need_write(&job.id, &steps, job.cache_mode, saving_job)?;
    if scope.events != ["workflow_dispatch"] {
        job_limits::secret_without_write(&job.id, &steps, &job.permissions)?;
    }
    job.outputs = outputs
        .into_iter()
        .map(|(name, value)| Ok((name.clone(), output(&job.id, &name, &value, &steps)?)))
        .collect::<Result<_, WorkflowsError>>()?;
    job.steps = steps;
    Ok(())
}

/// Checks the workflow `if` and the job `if` in a job's scope. Returns only the
/// job's own condition; the renderer joins it with the workflow one.
fn scope_conditions(
    ctx: &Context<'_>,
    own: Option<&str>,
    scope: &Scope<'_>,
) -> Result<Option<String>, WorkflowsError> {
    if let Some(condition) = ctx.condition {
        check_condition(condition, scope)?;
    }
    own.map(|condition| check_condition(condition, scope))
        .transpose()
}
