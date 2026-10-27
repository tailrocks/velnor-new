// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::{RenderError, render::RenderContext, steps, yaml::Yaml};
use velnor_actions_contract::{Job, RunsOn, StepKind, StepRole};

use super::document::{JobRenderPolicy, append_job_options, job_header_fields};

pub(super) fn job_to_yaml(
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
