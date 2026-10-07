//! Workflow-IR to YAML document builders.
//!
//! Fixed key order: name, on, permissions, concurrency, jobs.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    lane_share::LaneShare,
    render::{FINAL_JOB_ID, RenderContext},
};
use velnor_actions_contract_config::config::RunsOn;
use velnor_actions_contract_workflow::{
    Job, Permissions, StepKind, StepRole, Trigger, WorkflowIr,
    workflow::{ir::DispatchInput, permissions::PermissionLevel},
};
use velnor_actions_workflow_steps::{RenderError, steps};
use velnor_actions_workflow_tree::{yaml::Yaml, yaml::string_map_yaml};

#[cfg(test)]
mod tests;

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
                MbxJobPolicy {
                    native_mbx: mbx_jobs.contains(id),
                    actions_read,
                },
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

/// Report download is intentionally limited to the least-privilege `read` grant.
/// A broader `write` grant can access artifacts on GitHub, but does not satisfy
/// this renderer policy; callers must model the dedicated reader scope.
fn grants_exact_actions_read(level: PermissionLevel) -> bool {
    level == PermissionLevel::Read
}

fn lane_steps(shared: &LaneShare) -> crate::document_lanes::SharedLaneSteps<'_> {
    crate::document_lanes::SharedLaneSteps {
        checkouts: &shared.checkouts,
        env_steps: &shared.env_steps,
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
        velnor_actions_contract_workflow::NeedsConclusions::from_finalized_jobs(FINAL_JOB_ID, jobs)
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

#[derive(Clone, Copy)]
struct MbxJobPolicy {
    native_mbx: bool,
    actions_read: bool,
}

/// Render one job: name, runs-on, timeout, environment, permissions, needs, if, steps.
fn job_to_yaml(
    id: &str,
    job: &Job,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    shared: Option<&str>,
    lanes: &crate::document_lanes::SharedLaneSteps<'_>,
    mbx_policy: MbxJobPolicy,
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
        StepKind::Shell { env, .. } | StepKind::Action { env, .. } => !env.is_empty(),
        StepKind::Internal { .. } => false,
    });
    let mut job_env = if step_has_env {
        if ctx
            .verification_tasks
            .iter()
            .any(|task| task.owns_job_id(id))
        {
            // Verification jobs intentionally execute repository-declared
            // Mise tasks, so they need Mise config while retaining the same
            // credential scrub as every other repository-code step.
            velnor_actions_workflow_steps::toolchain_env::credential_scrub()
        } else {
            velnor_actions_workflow_steps::toolchain_env::job_level_env()
        }
    } else {
        BTreeMap::new()
    };
    if step_has_env {
        for step in source_steps {
            let step_env = match &step.kind {
                StepKind::Shell { env, .. } | StepKind::Action { env, .. } => Some(env),
                StepKind::Internal { .. } => None,
            };
            if let Some(env) = step_env {
                if let Some(toolchain) = env.get("RUSTUP_TOOLCHAIN") {
                    job_env.insert("RUSTUP_TOOLCHAIN".to_owned(), toolchain.clone());
                }
                if step.role == Some(StepRole::AcquireVelnor) {
                    for key in [
                        velnor_actions_workflow_steps::steps::ASSET_SHA_ENV,
                        velnor_actions_workflow_steps::steps::ASSET_URL_ENV,
                        velnor_actions_workflow_steps::steps::RELEASE_COMMIT_ENV,
                    ] {
                        if let Some(val) = env.get(key) {
                            job_env.insert(key.to_owned(), val.clone());
                        }
                    }
                }
            }
        }
    }
    if mbx_policy.native_mbx {
        job_env.insert(
            velnor_actions_workflow_cache::cache_steps::MBX_GC_AUTO_ENV.to_owned(),
            velnor_actions_workflow_cache::cache_steps::MBX_GC_AUTO_VALUE.to_owned(),
        );
        job_env.insert(
            velnor_actions_workflow_cache::cache_steps::MBX_SHARE_OUT_DIR_ENV.to_owned(),
            velnor_actions_workflow_cache::cache_steps::MBX_SHARE_OUT_DIR_VALUE.to_owned(),
        );
    }
    let mut entries = job_header_fields(job, runs_on);
    append_job_options(&mut entries, job, scale_set, &job_env)?;
    let rendered_steps = crate::document_lanes::render_job_steps(
        id,
        job,
        ctx,
        needs_envs,
        shared,
        lanes,
        &crate::document_lanes::JobStepContext {
            job_env: &job_env,
            actions_read: mbx_policy.actions_read,
        },
    )?;
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
