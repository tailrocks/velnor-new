//! Workflow-IR to YAML document builders.
//!
//! Fixed key order: name, on, permissions, env, concurrency, jobs.

use std::collections::{BTreeMap, BTreeSet};

mod job_options;

use crate::{
    RenderError,
    document_steps::string_map_yaml,
    lane_share::LaneShare,
    render::{FINAL_JOB_ID, RenderContext},
    steps,
    yaml::Yaml,
};
use velnor_actions_contract::{
    Job, Permissions, RunsOn, StepKind, StepRole, Trigger, WorkflowIr,
    workflow::{DispatchInput, DispatchInputType, permissions::PermissionLevel},
};

#[cfg(test)]
#[path = "document_services_tests.rs"]
mod document_services_tests;
#[cfg(test)]
#[path = "document_runner_shell_tests.rs"]
mod runner_shell_tests;

#[cfg(test)]
#[path = "document_verification_env_tests.rs"]
mod verification_env_tests;

/// Build the workflow document: name, on, permissions, concurrency, jobs.
pub(crate) fn workflow_to_yaml(
    ir: &WorkflowIr,
    shared: &LaneShare,
    ctx: &RenderContext,
    mbx_jobs: &BTreeSet<String>,
) -> Result<Yaml, RenderError> {
    let jobs = &shared.jobs;
    if shared.calls.keys().ne(shared.checkouts.keys())
        || shared.calls.keys().ne(shared.env_steps.keys())
        || shared.calls.keys().ne(shared.runtime_preludes.keys())
        || shared.calls.keys().ne(shared.prefixes.keys())
        || shared.calls.keys().ne(shared.preludes.keys())
        || shared.calls.keys().ne(shared.postludes.keys())
        || shared.calls.keys().any(|id| !jobs.contains_key(id))
    {
        return Err(RenderError::InvalidWorkflow(
            "shared_lane_checkout_map_mismatch".to_owned(),
        ));
    }
    let needs_env = needs_channel_envs(jobs)?;
    let lane_steps = lane_steps(shared);
    let workflow_env =
        crate::document_env::workflow_env(!ctx.workflow_tasks.is_empty(), ctx.mise_quiet);
    let mut rendered_jobs = Vec::with_capacity(jobs.len());
    for (id, job) in jobs {
        let call = shared.calls.get(id).map(String::as_str);
        let actions_read =
            grants_exact_actions_read(job.permissions.as_ref().unwrap_or(&ir.permissions).actions);
        rendered_jobs.push((
            id.clone(),
            job_to_yaml(
                id,
                job,
                ctx,
                &needs_env,
                call,
                &lane_steps,
                JobRenderPolicy {
                    native_mbx: mbx_jobs.contains(id),
                    plan_attempt: crate::document_env::plan_attempt_value(id, job, jobs),
                    actions_read,
                    workflow_env: &workflow_env,
                },
            )?,
        ));
    }
    let mut top = vec![
        ("name".to_owned(), Yaml::str(ir.name.clone())),
        ("on".to_owned(), triggers_to_yaml(&ir.triggers)),
        (
            "permissions".to_owned(),
            permissions_to_yaml(&ir.permissions),
        ),
    ];
    if let Some(cache_mode) = &ir.cache_mode {
        steps::scan_for_private_subcommands(cache_mode.as_str())?;
        top.push(Yaml::cache_mode_field(cache_mode));
    }
    top.extend([
        ("env".to_owned(), string_map_yaml(&workflow_env)),
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
    ]);
    Ok(Yaml::Map(top))
}

/// Report download is limited to the least-privilege `read` grant.
fn grants_exact_actions_read(level: PermissionLevel) -> bool {
    level == PermissionLevel::Read
}

fn lane_steps(shared: &LaneShare) -> crate::document_lanes::SharedLaneSteps<'_> {
    crate::document_lanes::SharedLaneSteps {
        checkouts: &shared.checkouts,
        env_steps: &shared.env_steps,
        runtime_preludes: &shared.runtime_preludes,
        prefixes: &shared.prefixes,
        preludes: &shared.preludes,
        postludes: &shared.postludes,
    }
}

/// YAML spelling of one contract permission level.
fn level_str(level: PermissionLevel) -> &'static str {
    match level {
        PermissionLevel::None => "none",
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
    }
}

/// Render permissions: contents is explicit; other scopes appear only when granted.
///
/// An omitted scope is denied by GitHub when a permissions map exists.
/// Wider scopes render only when the IR grants them, so validated
/// overrides are never silently dropped.
fn permissions_to_yaml(permissions: &Permissions) -> Yaml {
    let mut entries = vec![(
        "contents".to_owned(),
        Yaml::str(level_str(permissions.contents).to_owned()),
    )];
    if !matches!(permissions.actions, PermissionLevel::None) {
        entries.push((
            "actions".to_owned(),
            Yaml::str(level_str(permissions.actions).to_owned()),
        ));
    }
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
    if !matches!(permissions.packages, PermissionLevel::None) {
        entries.push((
            "packages".to_owned(),
            Yaml::str(level_str(permissions.packages).to_owned()),
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
    if triggers.workflow_call {
        entries.push(("workflow_call".to_owned(), Yaml::Map(Vec::new())));
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
            Yaml::str(input.input_type.as_str().to_owned()),
        ),
        ("required".to_owned(), Yaml::Bool(input.required)),
    ];
    if input.input_type == DispatchInputType::Choice {
        fields.push((
            "options".to_owned(),
            Yaml::Seq(input.choices.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    if let Some(default) = &input.default {
        fields.push(("default".to_owned(), Yaml::str(default.clone())));
    }
    Yaml::Map(fields)
}

#[derive(Clone, Copy)]
struct JobRenderPolicy<'a> {
    native_mbx: bool,
    plan_attempt: Option<&'static str>,
    actions_read: bool,
    workflow_env: &'a BTreeMap<String, String>,
}

/// Render one job: name, runs-on, timeout, environment, permissions, needs, if, steps.
fn job_to_yaml(
    id: &str,
    job: &Job,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    shared: Option<&str>,
    lanes: &crate::document_lanes::SharedLaneSteps<'_>,
    policy: JobRenderPolicy<'_>,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let runner = RunsOn::parse(&job.runs_on).map_err(RenderError::Contract)?;
    let scale_set = matches!(&runner, RunsOn::ScaleSet(_));
    let runs_on = match runner {
        RunsOn::Hosted(label) => Yaml::str(label),
        RunsOn::ScaleSet(selector) => Yaml::Flow(selector.labels().to_vec()),
    };
    let source_steps = lanes
        .env_steps
        .get(id)
        .map_or(job.steps.as_slice(), Vec::as_slice);
    let step_has_env = source_steps.iter().any(|step| match &step.kind {
        StepKind::Shell { env, .. }
        | StepKind::Action { env, .. }
        | StepKind::TaskExecution { env, .. } => !env.is_empty(),
        StepKind::NativeCheckExecution { .. } | StepKind::Internal { .. } => false,
    });
    let mut job_env = if step_has_env && ctx.workflow_tasks.iter().any(|task| task.owns_job_id(id))
    {
        // Verification jobs execute repository-declared Mise tasks, so they need Mise
        // config with the same credential scrub as every other repository-code step.
        crate::toolchain_env::credential_scrub()
    } else {
        BTreeMap::new() // Hoisted: ordinary jobs inherit the workflow-level env.
    };
    if step_has_env {
        for step in source_steps {
            if let StepKind::Shell { env, .. }
            | StepKind::Action { env, .. }
            | StepKind::TaskExecution { env, .. } = &step.kind
            {
                if let Some(toolchain) = env.get("RUSTUP_TOOLCHAIN") {
                    job_env.insert("RUSTUP_TOOLCHAIN".to_owned(), toolchain.clone());
                    let (key, value) = crate::toolchain_env::CARGO_AUTO_CLEAN_ENV;
                    job_env.insert(key.to_owned(), value.to_owned());
                }
                if step.role == Some(StepRole::AcquireVelnor) {
                    for key in [
                        crate::steps::ASSET_SHA_ENV,
                        crate::steps::ASSET_URL_ENV,
                        crate::steps::RELEASE_COMMIT_ENV,
                    ] {
                        if let Some(val) = env.get(key) {
                            job_env.insert(key.to_owned(), val.to_owned());
                        }
                    }
                }
            }
        }
    }
    crate::document_env::extend_job_env(&mut job_env, policy.native_mbx, policy.plan_attempt);
    let mut entries = job_options::job_header_fields(job, runs_on);
    job_options::append_job_options(&mut entries, job, scale_set, &job_env, policy.workflow_env)?;
    let mut effective_env = policy.workflow_env.clone();
    effective_env.extend(job_env.clone());
    let mut rendered_steps = crate::document_lanes::render_job_steps(
        id,
        job,
        ctx,
        needs_envs,
        shared,
        lanes,
        &crate::document_lanes::JobStepContext {
            job_env: &effective_env,
            runs_on: Some(&job.runs_on),
            actions_read: policy.actions_read,
        },
    )?;
    crate::cache_save_budget::apply(&mut rendered_steps, job.timeout_minutes.minutes())?;
    crate::document_services::push_services(&mut entries, &job.services);
    entries.push(("steps".to_owned(), Yaml::Seq(rendered_steps)));
    Ok(Yaml::Map(entries))
}
