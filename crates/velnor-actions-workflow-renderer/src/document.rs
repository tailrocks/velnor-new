//! Workflow-IR to YAML document builders.
//!
//! Fixed key order: name, on, permissions, concurrency, jobs.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    RenderError, commands,
    composite::{push_composite_shell, shared_call},
    render::{FINAL_JOB_ID, RenderContext},
    steps::{self, INTERNAL_OP_ENV, REQUEST_FILE_ENV},
    yaml::Yaml,
};
use velnor_actions_contract::{
    Job, Permissions, Step, StepKind, Trigger, WorkflowIr,
    workflow::{ir::DispatchInput, permissions::PermissionLevel},
};

/// Build the workflow document: name, on, permissions, concurrency, jobs.
pub(crate) fn workflow_to_yaml(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    shared: &BTreeMap<String, String>,
    mbx_gc_jobs: &BTreeSet<String>,
) -> Result<Yaml, RenderError> {
    let needs_env = needs_channel_envs(jobs)?;
    let mut rendered_jobs = Vec::with_capacity(jobs.len());
    for (id, job) in jobs {
        let call = shared.get(id).map(String::as_str);
        rendered_jobs.push((
            id.clone(),
            job_to_yaml(id, job, ctx, &needs_env, call, mbx_gc_jobs.contains(id))?,
        ));
    }
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(ir.name.clone())),
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
                    Yaml::str(ir.concurrency.cancel_in_progress.clone()),
                ),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(rendered_jobs)),
    ]))
}

/// YAML spelling of one contract permission level.
fn level_str(level: PermissionLevel) -> &'static str {
    match level {
        PermissionLevel::None => "none",
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
    }
}

/// Render permissions: contents/actions always, grants beyond none explicit.
///
/// The CI default stays exactly `contents: read` plus `actions: read`;
/// wider scopes render only when the IR grants them, so validated
/// overrides are never silently dropped.
fn permissions_to_yaml(permissions: &Permissions) -> Yaml {
    let mut entries = vec![
        (
            "contents".to_owned(),
            Yaml::str(level_str(permissions.contents).to_owned()),
        ),
        (
            "actions".to_owned(),
            Yaml::str(level_str(permissions.actions).to_owned()),
        ),
    ];
    if !matches!(permissions.pull_requests, PermissionLevel::None) {
        entries.push((
            "pull-requests".to_owned(),
            Yaml::str(level_str(permissions.pull_requests).to_owned()),
        ));
    }
    if !matches!(permissions.id_token, PermissionLevel::None) {
        entries.push((
            "id-token".to_owned(),
            Yaml::str(level_str(permissions.id_token).to_owned()),
        ));
    }
    Yaml::Map(entries)
}

/// Render triggers: PR types, one push branch, schedule, dispatch, merge group.
fn triggers_to_yaml(triggers: &Trigger) -> Yaml {
    let pr_types: Vec<Yaml> = triggers
        .pull_request_types
        .iter()
        .map(|kind| Yaml::str(kind.clone()))
        .collect();
    let branches: Vec<Yaml> = triggers
        .push_branches
        .iter()
        .map(|branch| Yaml::str(branch.clone()))
        .collect();
    let mut entries = vec![
        (
            "pull_request".to_owned(),
            Yaml::Map(vec![("types".to_owned(), Yaml::Seq(pr_types))]),
        ),
        (
            "push".to_owned(),
            Yaml::Map(vec![("branches".to_owned(), Yaml::Seq(branches))]),
        ),
    ];
    if let Some(schedule) = &triggers.schedule {
        let crons: Vec<Yaml> = schedule
            .cron
            .iter()
            .map(|cron| Yaml::Map(vec![("cron".to_owned(), Yaml::str(cron.clone()))]))
            .collect();
        entries.push(("schedule".to_owned(), Yaml::Seq(crons)));
    }
    if let Some(dispatch) = &triggers.workflow_dispatch {
        let inputs: Vec<(String, Yaml)> = dispatch
            .inputs
            .iter()
            .map(|input| (input.name.clone(), dispatch_input_to_yaml(input)))
            .collect();
        entries.push((
            "workflow_dispatch".to_owned(),
            Yaml::Map(vec![("inputs".to_owned(), Yaml::Map(inputs))]),
        ));
    }
    entries.push(("merge_group".to_owned(), Yaml::Null));
    Yaml::Map(entries)
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

/// Render one typed dispatch input: fixed string type, required, default.
fn dispatch_input_to_yaml(input: &DispatchInput) -> Yaml {
    let mut fields = vec![
        (
            "type".to_owned(),
            Yaml::str(DispatchInput::INPUT_TYPE.to_owned()),
        ),
        ("required".to_owned(), Yaml::Bool(input.required)),
    ];
    if let Some(default) = &input.default {
        fields.push(("default".to_owned(), Yaml::str(default.clone())));
    }
    Yaml::Map(fields)
}

/// Render one job: name, runs-on, timeout, environment, permissions, needs, if, steps.
fn job_to_yaml(
    id: &str,
    job: &Job,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    shared: Option<&str>,
    mbx_gc_auto: bool,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        (
            "runs-on".to_owned(),
            crate::runs_on::runs_on_yaml(&job.runs_on)?,
        ),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ];
    if mbx_gc_auto {
        entries.push((
            "env".to_owned(),
            Yaml::Map(vec![(
                crate::cache_steps::MBX_GC_AUTO_ENV.to_owned(),
                Yaml::str(crate::cache_steps::MBX_GC_AUTO_VALUE.to_owned()),
            )]),
        ));
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
    let mut rendered_steps = Vec::with_capacity(job.steps.len() + usize::from(shared.is_some()));
    if let Some(uses) = shared {
        rendered_steps.push(shared_call(uses)?);
    }
    for step in &job.steps {
        rendered_steps.push(step_to_yaml(id, step, ctx, needs_envs, false)?);
    }
    entries.push(("steps".to_owned(), Yaml::Seq(rendered_steps)));
    Ok(Yaml::Map(entries))
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

/// Env for one internal step: op plus request file, fetch carries auth.
///
/// The fetch op takes no request file; it reads the plan from the
/// run directory and authenticates `gh` with the job token plus the
/// repository slug (fixed literals, never caller input). Token hygiene
/// still gates IR-level `GH_TOKEN` (see `support`); this render-time
/// pair is fixed by construction for the fetch step only. Merge-family
/// steps in the final job additionally carry the finalized `needs`
/// conclusions channel plus the rendered expected inventory, so the
/// merge binds required validators to the committed workflow.
fn internal_env(
    op: &str,
    target: &str,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
) -> Yaml {
    if op == steps::FETCH_OPERATION {
        return Yaml::Map(vec![
            ("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")),
            ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
            (INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())),
        ]);
    }
    let request = format!("{}/{target}-request.json", ctx.request_dir);
    let mut env = vec![(INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned()))];
    for (key, value) in needs_envs {
        env.push((key.clone(), Yaml::str(value.clone())));
    }
    env.push((REQUEST_FILE_ENV.to_owned(), Yaml::str(request)));
    if op == steps::PLAN_OPERATION && target == steps::PLAN_OPERATION {
        for (key, value) in &ctx.plan_consumer_env {
            env.push((key.clone(), Yaml::str(value.clone())));
        }
    }
    Yaml::Map(env)
}

/// Render one action step: name, condition, pin, inputs, step env.
///
/// Step env renders after `with:`; absent env renders nothing, so
/// env-less steps keep their exact historical bytes.
fn action_step_to_yaml(
    job_id: &str,
    step: &Step,
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
) -> Result<Yaml, RenderError> {
    steps::validate_uses(uses)?;
    for (key, value) in with {
        crate::expressions::check_with_key(key)?;
        crate::expressions::check_with_value(key, value)?;
        steps::scan_for_private_subcommands(key)?;
        steps::scan_for_private_subcommands(value)?;
    }
    commands::validate_env(env)?;
    let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
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
pub(crate) fn step_to_yaml(
    job_id: &str,
    step: &Step,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    composite: bool,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&step.name)?;
    match &step.kind {
        StepKind::Action { uses, with, env } => action_step_to_yaml(job_id, step, uses, with, env),
        StepKind::Shell { run, env } => {
            commands::validate_command_argv(run)?;
            commands::validate_env(env)?;
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
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
            push_composite_shell(&mut entries, composite);
            entries.push((
                "run".to_owned(),
                Yaml::str(commands::join_argv_for_run(run)?),
            ));
            Ok(Yaml::Map(entries))
        }
        StepKind::Internal { operation } => {
            let (op, target) = steps::split_internal_operation(operation)?;
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
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
            entries.push(("env".to_owned(), internal_env(op, target, ctx, channel)));
            push_composite_shell(&mut entries, composite);
            entries.push((
                "run".to_owned(),
                Yaml::str(commands::quote_run_arg(&ctx.staged_binary)),
            ));
            Ok(Yaml::Map(entries))
        }
    }
}
