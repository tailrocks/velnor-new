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
    CacheMode, Job, Permissions, RunsOn, StepKind, StepRole, Trigger, WorkflowIr,
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
    let CacheMode::Expression(expr) = mode else {
        return None;
    };
    if expr != velnor_actions_contract::CACHE_MODE_PUSH_WRITE_EXPR {
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
struct JobRenderPolicy<'a> {
    native_mbx: bool,
    plan_attempt: Option<&'static str>,
    actions_read: bool,
    workflow_env: &'a BTreeMap<String, String>,
}

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
    let mut job_env = if step_has_env
        && ctx
            .workflow_tasks
            .iter()
            .any(|task| task.owns_projected_job_id(id, ctx.bun_lane_plan.as_ref()))
    {
        crate::toolchain_env::credential_scrub()
    } else {
        BTreeMap::new()
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
    let mut entries = job_header_fields(job, runs_on);
    append_job_options(&mut entries, job, scale_set, &job_env, policy.workflow_env)?;
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
            report_helper_version: &ctx.report_helper_version,
        },
    )?;
    crate::cache_save_budget::apply(&mut rendered_steps, job.timeout_minutes.minutes())?;
    crate::document_services::push_services(&mut entries, &job.services);
    entries.push(("steps".to_owned(), Yaml::Seq(rendered_steps)));
    Ok(Yaml::Map(entries))
}

fn job_header_fields(job: &Job, runs_on: Yaml) -> Vec<(String, Yaml)> {
    let timeout = Yaml::Int(i64::from(job.timeout_minutes.minutes()));
    vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), runs_on),
        ("timeout-minutes".to_owned(), timeout),
    ]
}

fn append_job_options(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use velnor_actions_contract::{CacheMode, Job, JobTimeout};

    fn writer_job(condition: Option<&str>, cache_mode: Option<CacheMode>) -> Job {
        Job {
            display_name: "Writer".to_owned(),
            runs_on: "ubuntu-24.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::DEFAULT,
            needs: Vec::new(),
            condition: condition.map(str::to_owned),
            permissions: None,
            cache_mode,
            environment: None,
            services: Vec::new(),
            outputs: BTreeMap::new(),
            steps: Vec::new(),
        }
    }

    fn rendered_options(job: &Job) -> Vec<(String, Yaml)> {
        let mut entries = Vec::new();
        append_job_options(&mut entries, job, false, &BTreeMap::new(), &BTreeMap::new())
            .expect("options render");
        entries
    }

    fn cache_mode_entry(entries: &[(String, Yaml)]) -> Option<Yaml> {
        entries
            .iter()
            .find(|(key, _)| key == "cache-mode")
            .map(|(_, value)| value.clone())
    }

    #[test]
    fn push_gated_writer_renders_literal_write() {
        let gated = writer_job(
            Some("(affected) && github.event_name == 'push'"),
            Some(CacheMode::push_write_expression()),
        );
        assert_eq!(
            cache_mode_entry(&rendered_options(&gated)),
            Some(Yaml::Str("write".to_owned()))
        );
        let bare = writer_job(
            Some("github.event_name == 'push'"),
            Some(CacheMode::push_write_expression()),
        );
        assert_eq!(
            cache_mode_entry(&rendered_options(&bare)),
            Some(Yaml::Str("write".to_owned()))
        );
    }

    #[test]
    fn unprovable_writer_keeps_no_declaration() {
        let unconditioned = writer_job(None, Some(CacheMode::push_write_expression()));
        assert_eq!(cache_mode_entry(&rendered_options(&unconditioned)), None);
        let pr_capable = writer_job(Some("(affected)"), Some(CacheMode::push_write_expression()));
        assert_eq!(cache_mode_entry(&rendered_options(&pr_capable)), None);
        let or_shaped = writer_job(
            Some("(reader_ok) || (writer_ok) && github.event_name == 'push'"),
            Some(CacheMode::push_write_expression()),
        );
        assert_eq!(cache_mode_entry(&rendered_options(&or_shaped)), None);
        let narrowed = writer_job(
            Some("((reader_ok) || (writer_ok)) && github.event_name == 'push'"),
            Some(CacheMode::push_write_expression()),
        );
        assert_eq!(
            cache_mode_entry(&rendered_options(&narrowed)),
            Some(Yaml::Str("write".to_owned()))
        );
    }

    #[test]
    fn explicit_modes_render_unchanged() {
        let reader = writer_job(Some("(affected)"), Some(CacheMode::explicit_read()));
        assert_eq!(
            cache_mode_entry(&rendered_options(&reader)),
            Some(Yaml::Str("read".to_owned()))
        );
    }
}
