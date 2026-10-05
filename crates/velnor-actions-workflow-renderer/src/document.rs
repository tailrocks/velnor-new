//! Workflow-IR to YAML document builders.
//!
//! Fixed key order: name, on, permissions, concurrency, jobs.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    RenderError,
    composite::shared_call,
    document_steps::{step_to_yaml, string_map_yaml},
    render::{FINAL_JOB_ID, RenderContext},
    steps,
    yaml::Yaml,
};
use velnor_actions_contract::{
    Job, Permissions, RunsOn, Step, StepKind, Trigger, WorkflowIr,
    workflow::{ir::DispatchInput, permissions::PermissionLevel},
};

#[cfg(test)]
#[path = "document_runner_shell_tests.rs"]
mod runner_shell_tests;

/// Build the workflow document: name, on, permissions, concurrency, jobs.
pub(crate) fn workflow_to_yaml(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    shared: &BTreeMap<String, String>,
    checkouts: &BTreeMap<String, Step>,
    mbx_gc_jobs: &BTreeSet<String>,
) -> Result<Yaml, RenderError> {
    if shared.keys().ne(checkouts.keys()) || shared.keys().any(|id| !jobs.contains_key(id)) {
        return Err(RenderError::InvalidWorkflow(
            "shared_lane_checkout_map_mismatch".to_owned(),
        ));
    }
    let needs_env = needs_channel_envs(jobs)?;
    let mut rendered_jobs = Vec::with_capacity(jobs.len());
    for (id, job) in jobs {
        let call = shared.get(id).map(String::as_str);
        rendered_jobs.push((
            id.clone(),
            job_to_yaml(
                id,
                job,
                ctx,
                &needs_env,
                call,
                checkouts,
                mbx_gc_jobs.contains(id),
            )?,
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
    checkouts: &BTreeMap<String, Step>,
    mbx_gc_auto: bool,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let runner = RunsOn::parse(&job.runs_on).map_err(RenderError::Contract)?;
    let scale_set = matches!(&runner, RunsOn::ScaleSet(_));
    let runs_on = match runner {
        RunsOn::Hosted(label) => Yaml::str(label),
        RunsOn::ScaleSet(selector) => Yaml::Flow(selector.labels().to_vec()),
    };
    let step_has_env = job.steps.iter().any(|step| match &step.kind {
        StepKind::Shell { env, .. } | StepKind::Action { env, .. } => !env.is_empty(),
        StepKind::Internal { .. } => false,
    });
    let mut job_env = if step_has_env {
        crate::toolchain_env::job_level_env()
    } else {
        BTreeMap::new()
    };
    if step_has_env {
        for step in &job.steps {
            let step_env = match &step.kind {
                StepKind::Shell { env, .. } | StepKind::Action { env, .. } => Some(env),
                StepKind::Internal { .. } => None,
            };
            if let Some(env) = step_env {
                if let Some(toolchain) = env.get("RUSTUP_TOOLCHAIN") {
                    job_env.insert("RUSTUP_TOOLCHAIN".to_owned(), toolchain.clone());
                }
                if step.name == crate::steps::ACQUIRE_NAME {
                    for key in [
                        crate::steps::ASSET_SHA_ENV,
                        crate::steps::ASSET_URL_ENV,
                        crate::steps::RELEASE_COMMIT_ENV,
                    ] {
                        if let Some(val) = env.get(key) {
                            job_env.insert(key.to_owned(), val.clone());
                        }
                    }
                }
            }
        }
    }
    if mbx_gc_auto {
        job_env.insert(
            crate::cache_steps::MBX_GC_AUTO_ENV.to_owned(),
            crate::cache_steps::MBX_GC_AUTO_VALUE.to_owned(),
        );
    }
    let mut entries = job_header_fields(job, runs_on);
    append_job_options(&mut entries, job, scale_set, &job_env)?;
    let rendered_steps = render_job_steps(id, job, ctx, needs_envs, shared, checkouts, &job_env)?;
    entries.push(("steps".to_owned(), Yaml::Seq(rendered_steps)));
    Ok(Yaml::Map(entries))
}

fn job_header_fields(job: &Job, runs_on: Yaml) -> Vec<(String, Yaml)> {
    vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), runs_on),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ]
}

fn append_job_options(
    entries: &mut Vec<(String, Yaml)>,
    job: &Job,
    scale_set: bool,
    job_env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if scale_set {
        entries.push(crate::runs_on::run_shell_defaults_field(
            crate::runs_on::SCALE_SET_RUN_SHELL,
        ));
    }
    if !job_env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(job_env)));
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
    Ok(())
}

fn render_job_steps(
    id: &str,
    job: &Job,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    shared: Option<&str>,
    checkouts: &BTreeMap<String, Step>,
    job_env: &BTreeMap<String, String>,
) -> Result<Vec<Yaml>, RenderError> {
    let mut rendered_steps =
        Vec::with_capacity(job.steps.len() + 2 * usize::from(shared.is_some()));
    if let Some(uses) = shared {
        let Some(checkout) = checkouts.get(id) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "shared_lane_missing_checkout:{id}"
            )));
        };
        if !valid_shared_checkout(checkout, &ctx.checkout_uses) {
            return Err(RenderError::InvalidWorkflow(format!(
                "shared_lane_invalid_checkout:{id}"
            )));
        }
        rendered_steps.push(step_to_yaml(id, checkout, ctx, needs_envs, false, job_env)?);
        rendered_steps.push(shared_call(uses)?);
    } else if checkouts.contains_key(id) {
        return Err(RenderError::InvalidWorkflow(format!(
            "checkout_without_shared_lane:{id}"
        )));
    }
    for step in &job.steps {
        rendered_steps.push(step_to_yaml(id, step, ctx, needs_envs, false, job_env)?);
    }
    Ok(rendered_steps)
}

fn valid_shared_checkout(checkout: &Step, expected_uses: &str) -> bool {
    checkout.name == "Checkout"
        && checkout.condition.is_none()
        && matches!(
            &checkout.kind,
            StepKind::Action { uses, with, env }
                if uses == expected_uses
                    && with.get("persist-credentials").map(String::as_str) == Some("false")
                    && env.is_empty()
        )
}
