// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    RenderError,
    document_steps::string_map_yaml,
    lane_share::LaneShare,
    render::{FINAL_JOB_ID, RenderContext},
    steps,
    yaml::Yaml,
};
use velnor_actions_contract::{
    CacheMode, Job, Permissions, Trigger, WorkflowIr,
    workflow::{DispatchInput, DispatchInputType, permissions::PermissionLevel},
};

const PUSH_EVENT_GATE: &str = "github.event_name == 'push'";

fn outer_wrapped(text: &str) -> bool {
    if !text.starts_with('(') {
        return false;
    }
    let mut depth = 0i32;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth -= 1;
            if depth == 0 && index != text.len() - 1 {
                return false;
            }
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0
}

fn push_only_condition(condition: &str) -> bool {
    if condition == PUSH_EVENT_GATE {
        return true;
    }
    let Some(head) = condition.strip_suffix(PUSH_EVENT_GATE) else {
        return false;
    };
    let Some(head) = head.trim_end().strip_suffix("&&") else {
        return false;
    };
    outer_wrapped(head.trim_end())
}

fn push_gated_literal_mode(mode: &CacheMode, condition: Option<&str>) -> Option<CacheMode> {
    if !matches!(mode, CacheMode::Expression(_)) {
        return None;
    }
    if condition.is_some_and(push_only_condition) {
        return Some(CacheMode::Write);
    }
    None
}

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
        // Same IR-only rule as job-level conditional modes above.
        if !matches!(
            cache_mode,
            velnor_actions_contract::CacheMode::Expression(_)
        ) {
            top.push(Yaml::cache_mode_field(cache_mode));
        }
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

fn level_str(level: PermissionLevel) -> &'static str {
    match level {
        PermissionLevel::None => "none",
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
    }
}

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
pub(super) struct JobRenderPolicy<'a> {
    pub(super) native_mbx: bool,
    pub(super) plan_attempt: Option<&'static str>,
    pub(super) actions_read: bool,
    pub(super) workflow_env: &'a BTreeMap<String, String>,
}

pub(super) fn job_header_fields(job: &Job, runs_on: Yaml) -> Vec<(String, Yaml)> {
    let timeout = Yaml::Int(i64::from(job.timeout_minutes.minutes()));
    vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), runs_on),
        ("timeout-minutes".to_owned(), timeout),
    ]
}

pub(super) fn append_job_options(
    entries: &mut Vec<(String, Yaml)>,
    job: &Job,
    scale_set: bool,
    job_env: &BTreeMap<String, String>,
    workflow_env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if scale_set {
        entries.push(crate::runs_on::run_shell_defaults_field(
            crate::runs_on::SCALE_SET_RUN_SHELL,
        ));
    }
    if job.is_dependency_seed() {
        entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    let job_env = crate::document_env::exclude_inherited(job_env, workflow_env);
    if !job_env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(&job_env)));
    }
    if let Some(environment) = &job.environment {
        entries.push(("environment".to_owned(), Yaml::str(environment.clone())));
    }
    if let Some(permissions) = &job.permissions {
        entries.push(("permissions".to_owned(), permissions_to_yaml(permissions)));
    }
    if let Some(cache_mode) = &job.cache_mode {
        steps::scan_for_private_subcommands(cache_mode.as_str())?;
        if let Some(literal) = push_gated_literal_mode(cache_mode, job.condition.as_deref()) {
            entries.push(Yaml::cache_mode_field(&literal));
        } else if !matches!(cache_mode, CacheMode::Expression(_)) {
            entries.push(Yaml::cache_mode_field(cache_mode));
        }
    }
    if !job.needs.is_empty() {
        let needs: Vec<Yaml> = job
            .needs
            .iter()
            .map(|need| Yaml::str(need.clone()))
            .collect();
        entries.push(("needs".to_owned(), Yaml::Seq(needs)));
    }
    if !job.outputs.is_empty() {
        entries.push(("outputs".to_owned(), string_map_yaml(&job.outputs)));
    }
    if let Some(condition) = &job.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    Ok(())
}

use super::document_jobs::job_to_yaml;

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "document_authority_regression_tests.rs"]
mod authority_regression_tests;
