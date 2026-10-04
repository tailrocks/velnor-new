//! Workflow-IR to YAML document builders.
//!
//! Fixed key order: name, on, permissions, concurrency, jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, WorkflowIr};

use crate::{
    RenderError, commands,
    render::{FINAL_JOB_ID, RenderContext},
    steps,
    yaml::Yaml,
};

#[path = "document_headers.rs"]
mod headers;
#[path = "document_inputs.rs"]
mod inputs;
#[path = "document_env.rs"]
mod operation_env;
use headers::{permissions_to_yaml, triggers_to_yaml};

/// Build the workflow document: name, on, permissions, concurrency, jobs.
pub(crate) fn workflow_to_yaml(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    cache_writers_admitted: bool,
) -> Result<Yaml, RenderError> {
    crate::cache_mode::validate_document(ir, jobs, cache_writers_admitted)?;
    let mut effective = ir.clone();
    effective.jobs = jobs.clone();
    crate::native_publish_approval::validate_approvals(&effective, &ctx.native_publish_approvals)?;
    let needs_env = needs_channel_envs(jobs)?;
    let mut rendered_jobs = Vec::with_capacity(jobs.len());
    for (id, job) in jobs {
        rendered_jobs.push((id.clone(), job_to_yaml(id, job, ctx, &needs_env)?));
    }
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(ir.name.clone())),
        (
            "cache-mode".to_owned(),
            Yaml::str(ir.cache_mode.as_str().to_owned()),
        ),
        ("on".to_owned(), triggers_to_yaml(&ir.triggers)),
        (
            "permissions".to_owned(),
            permissions_to_yaml(&ir.permissions),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(ir.concurrency.group.clone())),
                (
                    "cancel-in-progress".to_owned(),
                    match ir.concurrency.cancel_in_progress.as_str() {
                        "true" => Yaml::Bool(true),
                        "false" => Yaml::Bool(false),
                        expression => Yaml::str(expression),
                    },
                ),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(rendered_jobs)),
    ];
    if let Some(run_name) = &ir.run_name {
        entries.insert(1, ("run-name".to_owned(), Yaml::str(run_name.clone())));
    }
    Ok(Yaml::Map(entries))
}

/// Derive the merge `needs` channel from the gate job's `needs`.
///
/// The inventory is exactly what `toJSON(needs)` can observe at
/// runtime; a lone gate has nothing to conclude over and fails closed
/// instead of emitting a channel the merge would judge as
/// `empty_needs`. The `gate_matches` check fails generation closed if
/// the derivation ever diverges from the gate list again.
fn needs_channel_envs(jobs: &BTreeMap<String, Job>) -> Result<Vec<(String, String)>, RenderError> {
    if !jobs.contains_key(FINAL_JOB_ID) {
        return Ok(Vec::new());
    }
    let conclusions =
        velnor_actions_contract::NeedsConclusions::from_finalized_jobs(FINAL_JOB_ID, jobs)
            .map_err(RenderError::Contract)?;
    if !conclusions.gate_matches(jobs) {
        return Err(RenderError::InvalidWorkflow(
            "needs_inventory_gate_mismatch".to_owned(),
        ));
    }
    Ok(vec![conclusions.channel_env(), conclusions.expected_env()])
}

/// Render one job: name, runs-on, timeout, environment, permissions, needs, if, steps.
fn job_to_yaml(
    id: &str,
    job: &Job,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
) -> Result<Yaml, RenderError> {
    velnor_actions_contract::workflow::step::validate_step_ids(&job.steps)
        .map_err(RenderError::Contract)?;
    validate_native_pages_approval(job, ctx)?;
    steps::scan_for_private_subcommands(&job.display_name)?;
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), Yaml::str(job.runs_on.clone())),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ];
    if let Some(mode) = job.cache_mode {
        entries.push(("cache-mode".to_owned(), Yaml::str(mode.as_str().to_owned())));
    }
    if let Some(environment) = &job.environment {
        entries.push(("environment".to_owned(), Yaml::str(environment.clone())));
    }
    if let Some(permissions) = &job.permissions {
        entries.push(("permissions".to_owned(), permissions_to_yaml(permissions)));
    }
    if !job.needs.is_empty() {
        let needs: Vec<Yaml> = job
            .needs
            .iter()
            .map(|need| Yaml::str(need.clone()))
            .collect();
        entries.push(("needs".to_owned(), Yaml::Seq(needs)));
    }
    if let Some(condition) = &job.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    if !job.outputs.is_empty() {
        velnor_actions_contract::workflow::outputs::validate_job_outputs(&job.outputs, &job.steps)
            .map_err(RenderError::Contract)?;
        entries.push((
            "outputs".to_owned(),
            Yaml::Map(
                job.outputs
                    .iter()
                    .map(|output| (output.name.clone(), Yaml::str(output.value.expression())))
                    .collect(),
            ),
        ));
    }
    let mut rendered_steps = Vec::with_capacity(job.steps.len());
    for step in &job.steps {
        rendered_steps.push(step_to_yaml(
            id,
            step,
            ctx,
            needs_envs,
            crate::early_plan::has_early_plan(job),
            &job.runs_on,
        )?);
    }
    entries.push(("steps".to_owned(), Yaml::Seq(rendered_steps)));
    Ok(Yaml::Map(entries))
}

fn validate_native_pages_approval(job: &Job, ctx: &RenderContext) -> Result<(), RenderError> {
    if let Some(role) = &job.native_pages_deploy
        && ctx
            .native_pages_approvals
            .iter()
            .filter(|approval| approval.admits(role))
            .count()
            != 1
    {
        return Err(RenderError::InvalidWorkflow(
            "unapproved_native_pages_deploy".to_owned(),
        ));
    }
    Ok(())
}

/// True for the final job's plan download (fetch is gated inline below).
///
/// An absent plan artifact (failed plan) must still reach the merge
/// verdict instead of failing the job at the download step.
fn is_verdict_download(step: &Step) -> bool {
    matches!(
        &step.kind,
        StepKind::Action { uses, .. } if uses == steps::DOWNLOAD_ARTIFACT_USES
    ) && step.name == crate::closure::DOWNLOAD_PLAN_NAME
}

/// Render one action step: name, condition, pin, inputs, step env.
///
/// Step env (cache modes) renders after `with:`; absent env renders
/// nothing, so env-less steps keep their exact historical bytes.
fn action_step_to_yaml(
    job_id: &str,
    step: &Step,
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
) -> Result<Yaml, RenderError> {
    crate::analysis_publication::validate_upload_binding(job_id, step)?;
    steps::validate_uses(uses)?;
    for (key, value) in with {
        crate::expressions::check_with_key(key)?;
        crate::expressions::check_with_value(key, value)?;
        steps::scan_for_private_subcommands(key)?;
        steps::scan_for_private_subcommands(value)?;
    }
    commands::validate_env(env)?;
    let mut entries = crate::steps_plain::step_header(step)?;
    if let Some(condition) = &step.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    } else if uses == steps::UPLOAD_ARTIFACT_USES {
        entries.push((
            "if".to_owned(),
            Yaml::str(crate::render::FINAL_CONDITION.to_owned()),
        ));
    }
    if job_id == FINAL_JOB_ID && is_verdict_download(step) {
        entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    entries.push(("uses".to_owned(), Yaml::str(uses.to_owned())));
    if !with.is_empty() {
        entries.push(("with".to_owned(), string_map_yaml(with)));
    }
    if !env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(env)));
    }
    Ok(Yaml::Map(entries))
}

/// Sorted string map as YAML (shared by `with:` and `env:` emission).
fn string_map_yaml(map: &BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        map.iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
            .collect(),
    )
}

/// Render one step; internal ops become env plus request file, never argv.
/// Every action ref (including the Alint pin) must be a full-SHA pin.
fn step_to_yaml(
    job_id: &str,
    step: &Step,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    early: bool,
    runs_on: &str,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&step.name)?;
    match &step.kind {
        StepKind::Action { uses, with, env } => action_step_to_yaml(job_id, step, uses, with, env),
        StepKind::SourceBoundHelper { invocation, env } => {
            crate::source_helper::step_to_yaml(step, invocation, env, &ctx.source_helpers, runs_on)
        }
        StepKind::Shell { run, env } => {
            commands::validate_command_argv(run)?;
            commands::validate_env(env)?;
            let mut entries = crate::steps_plain::step_header(step)?;
            if let Some(condition) = &step.condition {
                steps::scan_for_private_subcommands(condition)?;
                entries.push(("if".to_owned(), Yaml::str(condition.clone())));
            }
            if !env.is_empty() {
                let vars: Vec<(String, Yaml)> = env
                    .iter()
                    .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
                    .collect();
                entries.push(("env".to_owned(), Yaml::Map(vars)));
            }
            entries.push((
                "run".to_owned(),
                Yaml::str(commands::join_argv_for_run(run)?),
            ));
            Ok(Yaml::Map(entries))
        }
        StepKind::Internal { operation } => {
            let (op, target) = steps::split_internal_operation(operation)?;
            let mut entries = crate::steps_plain::step_header(step)?;
            if let Some(condition) = &step.condition {
                steps::scan_for_private_subcommands(condition)?;
                entries.push(("if".to_owned(), Yaml::str(condition.clone())));
            }
            // No `continue-on-error` on the fetch step (F5): the helper
            // retries each leg bounded and still exits success on
            // per-leg failure, so the merge judges honestly; only hard
            // environment failures fail the job, unmasked.
            let channel = if job_id == FINAL_JOB_ID && target == steps::MERGE_OPERATION {
                needs_envs
            } else {
                &[]
            };
            entries.push((
                "env".to_owned(),
                operation_env::internal_env(op, target, ctx, channel, early),
            ));
            entries.push((
                "run".to_owned(),
                Yaml::str(commands::quote_run_arg(&ctx.staged_binary)),
            ));
            Ok(Yaml::Map(entries))
        }
    }
}
