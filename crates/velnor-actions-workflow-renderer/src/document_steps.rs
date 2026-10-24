//! Rendering helpers for one workflow step.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

#[path = "document_steps_internal_env.rs"]
mod internal_env_module;
use internal_env_module::internal_env;

use crate::{
    RenderError, commands,
    composite::push_composite_shell,
    render::{FINAL_JOB_ID, RenderContext},
    steps,
    yaml::Yaml,
};

#[path = "document_steps_cache_env.rs"]
mod cache_env;

/// True for the final job's plan download (fetch is gated inline below).
///
/// An absent plan artifact (failed plan) must still reach the merge
/// verdict instead of failing the job at the download step.
pub(super) fn is_verdict_download(step: &Step) -> bool {
    step.role == Some(velnor_actions_contract::StepRole::DownloadPlan)
        && matches!(
            &step.kind,
            StepKind::Action { uses, .. } if uses == steps::DOWNLOAD_ARTIFACT_USES
        )
}

#[cfg(test)]
#[path = "document_steps_qualification_cache_tests.rs"]
mod qualification_cache_tests;

#[path = "document_steps_action.rs"]
mod action;
#[cfg(test)]
use action::cache_action_env;
use action::{ActionStepContext, action_step_to_yaml};

#[cfg(test)]
#[path = "document_steps_cache_path_tests.rs"]
mod cache_path_tests;

/// Sorted string map as YAML (shared by `with:` and `env:` emission).
pub(super) fn string_map_yaml(map: &BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        map.iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
            .collect(),
    )
}

fn shell_step_to_yaml(
    step: &Step,
    run: &[String],
    env: &BTreeMap<String, String>,
    job_env: &BTreeMap<String, String>,
    composite: bool,
    composite_shell: Option<&str>,
) -> Result<Yaml, RenderError> {
    if let Some(verifier) =
        crate::verifier_shell::render_typed_step(step, run, env, job_env, composite)?
    {
        return Ok(verifier);
    }
    crate::action_mbx_shell::validate_step_argv(run, step.role, env)?;
    if composite {
        commands::validate_composite_env(env)?;
    } else {
        commands::validate_env(env)?;
    }
    let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
    crate::step_ids::push_step_id(&mut entries, step);
    if let Some(condition) = &step.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    if !composite && step.role == Some(velnor_actions_contract::StepRole::AcquireVelnor) {
        entries.push((
            "timeout-minutes".to_owned(),
            Yaml::Int(steps::ACQUIRE_TIMEOUT_MINUTES),
        ));
    }
    let filtered_env: BTreeMap<String, String> = env
        .iter()
        .filter(|(k, v)| job_env.get(*k).map(String::as_str) != Some(v.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if !filtered_env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(&filtered_env)));
    }
    if composite {
        entries.push((
            "shell".to_owned(),
            Yaml::str(composite_shell.unwrap_or("bash")),
        ));
    }
    entries.push((
        "run".to_owned(),
        Yaml::str(commands::join_validated_argv_for_run(run)?),
    ));
    Ok(Yaml::Map(entries))
}

/// Render one action or shell step outside the CI internal-step pipeline.
pub(crate) fn non_internal_step_to_yaml(
    job_id: &str,
    step: &Step,
    job_env: &BTreeMap<String, String>,
    runs_on: &str,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&step.name)?;
    crate::parallel_steps::reject_parallel(step, job_id)?;
    match &step.kind {
        StepKind::Action { uses, with, env } => action_step_to_yaml(
            step,
            uses,
            with,
            env,
            ActionStepContext {
                job_id,
                job_env,
                runs_on: Some(runs_on),
                composite: false,
            },
        ),
        StepKind::Shell { run, env } => shell_step_to_yaml(step, run, env, job_env, false, None),
        StepKind::Internal { .. } | StepKind::TaskExecution { .. } => Err(
            RenderError::InvalidWorkflow("internal_op_rejected".to_owned()),
        ),
    }
}

/// Render one step; internal ops become env plus request file, never argv.
/// Every action ref (including the Alint pin) must be a full-SHA pin.
pub(crate) fn step_to_yaml(
    job_id: &str,
    step: &Step,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    composite: bool,
    step_context: &crate::document_lanes::JobStepContext<'_>,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&step.name)?;
    if composite {
        if matches!(
            step.role,
            Some(
                velnor_actions_contract::StepRole::CargoSourcesSave
                    | velnor_actions_contract::StepRole::ToolsCacheSave
                    | velnor_actions_contract::StepRole::TofuProvidersSave
                    | velnor_actions_contract::StepRole::MbxBundleSave
                    | velnor_actions_contract::StepRole::FamilySave
            )
        ) {
            return Err(RenderError::InvalidWorkflow(
                "optional_cache_save_requires_job_scope".to_owned(),
            ));
        }
        crate::parallel_steps::reject_parallel(step, job_id)?;
    }
    match &step.kind {
        StepKind::Action { uses, with, env } => action_step_to_yaml(
            step,
            uses,
            with,
            env,
            ActionStepContext {
                job_id,
                job_env: step_context.job_env,
                runs_on: step_context.runs_on,
                composite,
            },
        ),
        StepKind::Shell { run, env } => {
            shell_step_to_yaml(step, run, env, step_context.job_env, composite, None)
        }
        StepKind::Internal { .. } => {
            internal_step_to_yaml(job_id, step, ctx, needs_envs, composite, step_context)
        }
        StepKind::TaskExecution { .. } => Err(RenderError::InvalidWorkflow(
            "unfactored_task_execution".to_owned(),
        )),
    }
}

/// Render a canonical shell step for a verifier composite with a fixed,
/// trusted interpreter rather than resolving `bash` through restored PATH.
pub(crate) fn verifier_shell_step_to_yaml(step: &Step) -> Result<Yaml, RenderError> {
    crate::verifier_composite::verifier_shell_step_to_yaml(step)
}

/// Render one internal planner step. The operation travels in env, never argv.
fn internal_step_to_yaml(
    job_id: &str,
    step: &Step,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    composite: bool,
    step_context: &crate::document_lanes::JobStepContext<'_>,
) -> Result<Yaml, RenderError> {
    let StepKind::Internal {
        operation,
        env: step_env,
    } = &step.kind
    else {
        return Err(RenderError::InvalidWorkflow(
            "internal_step_required".to_owned(),
        ));
    };
    let (op, target) = steps::split_internal_operation(operation)?;
    if (op == steps::FETCH_OPERATION
        || op == velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION)
        && !step_context.actions_read
    {
        return Err(RenderError::InvalidWorkflow(
            "report_fetch_requires_actions_read".to_owned(),
        ));
    }
    let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
    if let Some(condition) = &step.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    // No `continue-on-error` on the fetch step (F5): the helper
    // retries each leg bounded and still exits success on
    // per-leg failure, so the merge judges honestly; only hard
    // environment failures fail the job, unmasked.
    let channel = if job_id == FINAL_JOB_ID && target == steps::MERGE_OPERATION {
        needs_envs
    } else {
        &[]
    };
    crate::step_ids::push_step_id(&mut entries, step);
    entries.push((
        "env".to_owned(),
        internal_env(
            op,
            target,
            ctx,
            channel,
            step_context.job_env,
            step_env,
            composite,
        )?,
    ));
    push_composite_shell(&mut entries, composite);
    entries.push((
        "run".to_owned(),
        Yaml::str(commands::quote_run_arg(&ctx.staged_binary)?),
    ));
    Ok(Yaml::Map(entries))
}
