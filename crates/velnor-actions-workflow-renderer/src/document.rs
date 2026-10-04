//! Workflow-IR to YAML document builders.
//!
//! Fixed key order: name, on, permissions, concurrency, jobs.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    RenderError,
    composite::shared_call,
    document_steps::step_to_yaml,
    render::{FINAL_JOB_ID, RenderContext},
    steps,
    yaml::Yaml,
};
use velnor_actions_contract::{
    Job, Permissions, Step, StepKind, Trigger, WorkflowIr,
    workflow::{ir::DispatchInput, permissions::PermissionLevel},
};

struct JobRenderInputs<'a> {
    ctx: &'a RenderContext,
    needs_envs: &'a [(String, String)],
    shared: Option<&'a str>,
    checkouts: &'a BTreeMap<String, Step>,
    mbx_job: bool,
    hosted_linux_mbx_job: bool,
}

/// Build the workflow document: name, on, permissions, concurrency, jobs.
pub(crate) fn workflow_to_yaml(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    shared: &BTreeMap<String, String>,
    checkouts: &BTreeMap<String, Step>,
    hosted_mbx_jobs: &BTreeSet<String>,
    hosted_linux_mbx_jobs: &BTreeSet<String>,
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
        let inputs = JobRenderInputs {
            ctx,
            needs_envs: &needs_env,
            shared: call,
            checkouts,
            mbx_job: hosted_mbx_jobs.contains(id),
            hosted_linux_mbx_job: hosted_linux_mbx_jobs.contains(id),
        };
        rendered_jobs.push((id.clone(), job_to_yaml(id, job, &inputs)?));
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
fn job_to_yaml(id: &str, job: &Job, inputs: &JobRenderInputs<'_>) -> Result<Yaml, RenderError> {
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
    if let Some(job_env) =
        crate::mbx_gc_policy::job_env(inputs.mbx_job, inputs.hosted_linux_mbx_job)
    {
        entries.push(("env".to_owned(), job_env));
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
    let mut rendered_steps =
        Vec::with_capacity(job.steps.len() + 2 * usize::from(inputs.shared.is_some()));
    if let Some(uses) = inputs.shared {
        let Some(checkout) = inputs.checkouts.get(id) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "shared_lane_missing_checkout:{id}"
            )));
        };
        if !valid_shared_checkout(checkout, &inputs.ctx.checkout_uses) {
            return Err(RenderError::InvalidWorkflow(format!(
                "shared_lane_invalid_checkout:{id}"
            )));
        }
        rendered_steps.push(step_to_yaml(
            id,
            checkout,
            inputs.ctx,
            inputs.needs_envs,
            false,
        )?);
        rendered_steps.push(shared_call(uses)?);
    } else if inputs.checkouts.contains_key(id) {
        return Err(RenderError::InvalidWorkflow(format!(
            "checkout_without_shared_lane:{id}"
        )));
    }
    for step in &job.steps {
        rendered_steps.push(step_to_yaml(
            id,
            step,
            inputs.ctx,
            inputs.needs_envs,
            false,
        )?);
    }
    entries.push(("steps".to_owned(), Yaml::Seq(rendered_steps)));
    Ok(Yaml::Map(entries))
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
