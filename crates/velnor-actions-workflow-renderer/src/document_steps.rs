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

#[path = "native_check_execution.rs"]
mod native_check_execution;

/// True for the final job's plan download (fetch is gated inline below).
///
/// An absent plan artifact (failed plan) must still reach the merge
/// verdict instead of failing the job at the download step.
fn is_verdict_download(step: &Step) -> bool {
    step.role == Some(velnor_actions_contract::StepRole::DownloadPlan)
        && matches!(
            &step.kind,
            StepKind::Action { uses, .. } if uses == steps::DOWNLOAD_ARTIFACT_USES
        )
}

/// Render one action step: name, condition, pin, inputs, step env.
///
/// Step env (cache modes) renders after `with:`; absent env renders
/// nothing, so env-less steps keep their exact historical bytes.
fn action_step_to_yaml(
    job_id: &str,
    step: &Step,
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    job_env: &BTreeMap<String, String>,
    runs_on: Option<&str>,
) -> Result<Yaml, RenderError> {
    let runtime_identity = validate_action_step(step, uses, with, env, runs_on)?;
    let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
    crate::step_ids::push_step_id(&mut entries, step);
    if let Some(condition) = &step.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    } else if uses == steps::UPLOAD_ARTIFACT_USES {
        entries.push((
            "if".to_owned(),
            Yaml::str(crate::render::UPLOAD_STEP_CONDITION.to_owned()),
        ));
    }
    crate::cache_save_budget::decorate(step, &mut entries, runs_on.is_some())?;
    if job_id == FINAL_JOB_ID && is_verdict_download(step) {
        entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    let uses_value = if runtime_identity
        || uses == crate::tool_seed::TOOL_SEED_USES
        || uses == crate::cache_steps::TOOLS_RESTORE_USES
        || uses == crate::tofu_cache::TOFU_PROVIDER_ADMISSION_USES
        || velnor_actions_contract::workflow::RUST_VERIFIER_USES.contains(&uses)
        || uses == velnor_actions_contract::workflow::JACTIONLINT_VERIFIER_USES
        || crate::action_ref::is_generated_provider_prelude(uses)
        || crate::action_ref::is_crate_obligation_action(uses)
        || crate::action_ref::is_generated_declared_task(uses)
    {
        Yaml::annotated(uses, "zizmor: ignore[self-repository]")
    } else {
        Yaml::str(uses.to_owned())
    };
    entries.push(("uses".to_owned(), uses_value));
    if !with.is_empty() {
        let with_yaml = if crate::action_ref::is_generated_declared_task(uses) {
            Yaml::FlowMap(
                with.iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            )
        } else {
            string_map_yaml(with)
        };
        entries.push(("with".to_owned(), with_yaml));
    }
    let filtered_env: BTreeMap<String, String> = env
        .iter()
        .filter(|(key, value)| job_env.get(*key).map(String::as_str) != Some(value.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if !filtered_env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(&filtered_env)));
    }
    Ok(Yaml::Map(entries))
}

fn validate_action_step(
    step: &Step,
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    runs_on: Option<&str>,
) -> Result<bool, RenderError> {
    crate::action_ref::validate_verifier_call(step, uses, with, env)?;
    let runtime_identity = step.role == Some(velnor_actions_contract::StepRole::ToolsCacheIdentity)
        || ["ubuntu-22.04", "ubuntu-24.04", "ubuntu-26.04"]
            .iter()
            .any(|lane| {
                crate::cache_p08::runtime_identity_action_uses(lane) == Some(uses)
                    || crate::cache_p08::runtime_prelude_action_uses(lane) == Some(uses)
            });
    if runtime_identity {
        let Some(lane) = runs_on else {
            return Err(RenderError::InvalidWorkflow(
                "tools_cache_identity_missing_runner".to_owned(),
            ));
        };
        crate::cache_p08::validate_runtime_identity_action(step, uses, lane, with, env)?;
    } else if uses == crate::tool_seed::TOOL_SEED_USES {
        crate::tool_seed::validate_action_call(step, uses, with, env)?;
    } else if uses == crate::cache_steps::TOOLS_RESTORE_USES {
        crate::cache_steps::validate_tools_restore_call(step)?;
    } else {
        steps::validate_uses(uses)?;
    }
    for (key, value) in with {
        crate::expressions::check_with_key(key)?;
        crate::expressions::check_with_action_value(uses, key, value)?;
        steps::scan_for_private_subcommands(key)?;
        steps::scan_for_private_subcommands(value)?;
    }
    commands::validate_env(env)?;
    Ok(runtime_identity)
}

/// Sorted string map as YAML (shared by `with:` and `env:` emission).
pub(crate) fn string_map_yaml(map: &BTreeMap<String, String>) -> Yaml {
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
    crate::action_mbx_shell::validate_step_argv(run, step.role)?;
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
        StepKind::Action { uses, with, env } => {
            action_step_to_yaml(job_id, step, uses, with, env, job_env, Some(runs_on))
        }
        StepKind::Shell { run, env } => shell_step_to_yaml(step, run, env, job_env, false, None),
        StepKind::Internal { .. }
        | StepKind::TaskExecution { .. }
        | StepKind::NativeCheckExecution { .. } => Err(RenderError::InvalidWorkflow(
            "internal_op_rejected".to_owned(),
        )),
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
            job_id,
            step,
            uses,
            with,
            env,
            step_context.job_env,
            step_context.runs_on,
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
        StepKind::NativeCheckExecution { .. } => {
            native_check_execution::render(step, step_context.job_env)
        }
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
        internal_env(op, target, ctx, channel, step_context.job_env, step_env)?,
    ));
    push_composite_shell(&mut entries, composite);
    entries.push((
        "run".to_owned(),
        Yaml::str(commands::quote_run_arg(&ctx.staged_binary)?),
    ));
    Ok(Yaml::Map(entries))
}
