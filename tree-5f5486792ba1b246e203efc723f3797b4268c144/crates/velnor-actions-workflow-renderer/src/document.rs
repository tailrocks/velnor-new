//! Workflow-IR to YAML document builders.
//!
//! Fixed key order: name, on, permissions, concurrency, jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, Trigger, WorkflowIr};

use crate::{
    RenderError, commands,
    render::{ALINT_JOB_ID, ALINT_USES, RenderContext},
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
            Yaml::Map(vec![
                (
                    "contents".to_owned(),
                    Yaml::str(ir.permissions.contents.clone()),
                ),
                (
                    "actions".to_owned(),
                    Yaml::str(ir.permissions.actions.clone()),
                ),
            ]),
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

/// Render triggers: PR types, one push branch, bare merge group.
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
    Yaml::Map(vec![
        (
            "pull_request".to_owned(),
            Yaml::Map(vec![("types".to_owned(), Yaml::Seq(pr_types))]),
        ),
        (
            "push".to_owned(),
            Yaml::Map(vec![("branches".to_owned(), Yaml::Seq(branches))]),
        ),
        ("merge_group".to_owned(), Yaml::Null),
    ])
}

/// Render one job: name, runs-on, needs, condition, steps.
fn job_to_yaml(id: &str, job: &Job, ctx: &RenderContext) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), Yaml::str(job.runs_on.clone())),
    ];
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
            let mut entries = vec![
                ("name".to_owned(), Yaml::str(step.name.clone())),
                ("uses".to_owned(), Yaml::str(uses.clone())),
            ];
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
            if operation != steps::PLAN_OPERATION && operation != steps::MERGE_OPERATION {
                return Err(RenderError::BadCommand(format!(
                    "unknown_internal_op:{operation}"
                )));
            }
            let request = format!("{}/{operation}-request.json", ctx.request_dir);
            Ok(Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(step.name.clone())),
                (
                    "env".to_owned(),
                    Yaml::Map(vec![
                        (INTERNAL_OP_ENV.to_owned(), Yaml::str(operation.clone())),
                        (REQUEST_FILE_ENV.to_owned(), Yaml::str(request)),
                    ]),
                ),
                (
                    "run".to_owned(),
                    Yaml::str(commands::quote_run_arg(&ctx.staged_binary)),
                ),
            ]))
        }
    }
}
