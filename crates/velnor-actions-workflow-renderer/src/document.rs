//! Workflow-IR to YAML document builders.
//!
//! Fixed key order: name, on, permissions, concurrency, jobs.

use std::collections::BTreeMap;

use crate::{
    RenderError,
    composite::shared_call,
    document_steps::step_to_yaml,
    lane_share::SharedCall,
    render::{FINAL_JOB_ID, RenderContext},
    steps,
    yaml::Yaml,
};
use velnor_actions_contract::{
    Job, Permissions, Trigger, WorkflowIr,
    workflow::{ir::DispatchInput, permissions::PermissionLevel},
};

/// Build the workflow document: name, on, permissions, concurrency, jobs.
pub(crate) fn workflow_to_yaml(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    shared: &BTreeMap<String, Vec<SharedCall>>,
) -> Result<Yaml, RenderError> {
    let needs_env = needs_channel_envs(jobs)?;
    let mut rendered_jobs = Vec::with_capacity(jobs.len());
    for (id, job) in jobs {
        let calls = shared.get(id).map_or(&[][..], Vec::as_slice);
        rendered_jobs.push((id.clone(), job_to_yaml(id, job, ctx, &needs_env, calls)?));
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
    shared: &[SharedCall],
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
    entries.push((
        "steps".to_owned(),
        render_job_steps(id, job, ctx, needs_envs, shared)?,
    ));
    Ok(Yaml::Map(entries))
}

/// Render job-local steps around any shared composite calls.
fn render_job_steps(
    id: &str,
    job: &Job,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    shared: &[SharedCall],
) -> Result<Yaml, RenderError> {
    let job_env = BTreeMap::new();
    let mut rendered_steps = Vec::with_capacity(job.steps.len() + shared.len());
    let mut source_cursor = 0;
    let mut previous_start = None;
    for call in shared {
        if call.before_step < source_cursor
            || call.before_step > job.steps.len()
            || previous_start.is_some_and(|previous| call.before_step <= previous)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "shared_call_order:{id}"
            )));
        }
        while source_cursor < call.before_step {
            rendered_steps.push(step_to_yaml(
                id,
                &job.steps[source_cursor],
                ctx,
                needs_envs,
                false,
                &job_env,
            )?);
            source_cursor += 1;
        }
        rendered_steps.push(shared_call(&call.uses)?);
        source_cursor = call
            .before_step
            .checked_add(call.skip_steps)
            .filter(|end| *end <= job.steps.len())
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("shared_call_range:{id}")))?;
        previous_start = Some(call.before_step);
    }
    while source_cursor < job.steps.len() {
        rendered_steps.push(step_to_yaml(
            id,
            &job.steps[source_cursor],
            ctx,
            needs_envs,
            false,
            &job_env,
        )?);
        source_cursor += 1;
    }
    Ok(Yaml::Seq(rendered_steps))
}
