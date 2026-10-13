//! Workflow-IR to YAML document builders.
//!
//! Fixed key order: name, on, permissions, concurrency, jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    Job, Permissions, Step, StepKind, Trigger, WorkflowIr,
    workflow::ir::{DispatchInput, PermissionLevel},
};

use crate::{
    RenderError, commands,
    render::{ALINT_JOB_ID, ALINT_USES, FINAL_JOB_ID, RenderContext},
    steps::{self, INTERNAL_OP_ENV, REQUEST_FILE_ENV},
    yaml::Yaml,
};

/// Build the workflow document: name, on, permissions, concurrency, jobs.
pub(crate) fn workflow_to_yaml(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<Yaml, RenderError> {
    let mut rendered_jobs = Vec::with_capacity(jobs.len());
    for (id, job) in jobs {
        rendered_jobs.push((id.clone(), job_to_yaml(id, job, ctx)?));
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

/// Render one job: name, runs-on, environment, permissions, needs, if, steps.
fn job_to_yaml(id: &str, job: &Job, ctx: &RenderContext) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), Yaml::str(job.runs_on.clone())),
    ];
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
    let mut rendered_steps = Vec::with_capacity(job.steps.len());
    for step in &job.steps {
        rendered_steps.push(step_to_yaml(id, step, ctx)?);
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
/// pair is fixed by construction for the fetch step only.
fn internal_env(op: &str, target: &str, ctx: &RenderContext) -> Yaml {
    if op == steps::FETCH_OPERATION {
        return Yaml::Map(vec![
            ("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")),
            ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
            (INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())),
        ]);
    }
    let request = format!("{}/{target}-request.json", ctx.request_dir);
    Yaml::Map(vec![
        (INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())),
        (REQUEST_FILE_ENV.to_owned(), Yaml::str(request)),
    ])
}

/// Render one step; internal ops become env plus request file, never argv.
/// The pinned Alint tag is accepted only inside the `velnor-alint` job.
fn step_to_yaml(job_id: &str, step: &Step, ctx: &RenderContext) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&step.name)?;
    match &step.kind {
        StepKind::Action { uses, with } => {
            if uses == ALINT_USES && job_id == ALINT_JOB_ID {
                steps::scan_for_private_subcommands(uses)?;
            } else {
                steps::validate_uses(uses)?;
            }
            for (key, value) in with {
                steps::scan_for_private_subcommands(key)?;
                steps::scan_for_private_subcommands(value)?;
            }
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
            if uses == steps::UPLOAD_ARTIFACT_USES {
                entries.push((
                    "if".to_owned(),
                    Yaml::str(crate::render::FINAL_CONDITION.to_owned()),
                ));
            }
            if job_id == FINAL_JOB_ID && is_verdict_download(step) {
                entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
            }
            entries.push(("uses".to_owned(), Yaml::str(uses.clone())));
            if !with.is_empty() {
                let inputs: Vec<(String, Yaml)> = with
                    .iter()
                    .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
                    .collect();
                entries.push(("with".to_owned(), Yaml::Map(inputs)));
            }
            Ok(Yaml::Map(entries))
        }
        StepKind::Shell { run, env } => {
            commands::validate_command_argv(run)?;
            commands::validate_env(env)?;
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
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
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
            if job_id == FINAL_JOB_ID && operation == steps::FETCH_OPERATION {
                entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
            }
            entries.push(("env".to_owned(), internal_env(op, target, ctx)));
            entries.push((
                "run".to_owned(),
                Yaml::str(commands::quote_run_arg(&ctx.staged_binary)),
            ));
            Ok(Yaml::Map(entries))
        }
    }
}
