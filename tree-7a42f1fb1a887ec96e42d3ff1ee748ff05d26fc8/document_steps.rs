//! Rendering helpers for one workflow step.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::workflow::step_identity::{
    StepRole, TOFU_PROVIDER_ADMISSION_USES, TOOL_SEED_USES,
};
use velnor_actions_contract_workflow::{Step, StepKind};

use velnor_actions_workflow_steps::{
    RenderError, commands,
    steps::{self, INTERNAL_OP_ENV, REQUEST_FILE_ENV},
};
use velnor_actions_workflow_tree::yaml::{Yaml, string_map_yaml};

use crate::{
    composite::push_composite_shell,
    render::{FINAL_JOB_ID, RenderContext},
};

/// True for the final job's plan download (fetch is gated inline below).
///
/// An absent plan artifact (failed plan) must still reach the merge
/// verdict instead of failing the job at the download step.
fn is_verdict_download(step: &Step) -> bool {
    matches!(
        &step.kind,
        StepKind::Action { uses, .. } if uses == steps::DOWNLOAD_ARTIFACT_USES
    ) && step.role == Some(StepRole::DownloadPlan)
}

/// Emit a step's explicitly declared output id.
fn push_step_id(entries: &mut Vec<(String, Yaml)>, step: &Step) {
    if let Some(id) = step.id {
        entries.push(("id".to_owned(), Yaml::str(id.as_str().to_owned())));
    }
}

/// Env for one internal step: op plus request file, authorized reads carry auth.
///
/// When authorized by typed job permissions, the plan op authenticates only
/// its internal baseline lookup. The fetch
/// op takes no request file; it reads the plan from the run directory and
/// authenticates `gh` with the job token plus the repository slug (fixed
/// literals, never caller input). This render-time pair is emitted only on
/// those two exact internal steps. Merge-family steps in the final job
/// additionally carry the finalized `needs` conclusions channel plus the
/// rendered expected inventory, so the merge binds required validators to
/// the committed workflow.
fn internal_env(
    op: &str,
    target: &str,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    job_env: &BTreeMap<String, String>,
    step_env: &BTreeMap<String, String>,
    actions_read: bool,
) -> Result<Yaml, RenderError> {
    commands::validate_env(step_env)?;
    if op == steps::FETCH_OPERATION {
        if !step_env.is_empty() {
            return Err(RenderError::InvalidWorkflow(
                "fetch_internal_env_not_supported".to_owned(),
            ));
        }
        return Ok(Yaml::Map(vec![
            ("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")),
            ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
            (INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())),
        ]));
    }
    let request = format!("{}/{target}-request.json", ctx.request_dir);
    let mut env = Vec::new();
    if actions_read && op == steps::PLAN_OPERATION && target == steps::PLAN_OPERATION {
        env.push(("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")));
        env.push(("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")));
    }
    env.push((INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())));
    for (key, value) in needs_envs {
        env.push((key.clone(), Yaml::str(value.clone())));
    }
    env.push((REQUEST_FILE_ENV.to_owned(), Yaml::str(request)));
    if op == steps::PLAN_OPERATION && target == steps::PLAN_OPERATION {
        for (key, value) in &ctx.plan_consumer_env {
            if job_env.get(key).map(String::as_str) != Some(value.as_str()) {
                env.push((key.clone(), Yaml::str(value.clone())));
            }
        }
    }
    let mut seen: std::collections::BTreeSet<String> =
        env.iter().map(|(key, _)| key.clone()).collect();
    for (key, value) in step_env {
        if !seen.insert(key.clone()) {
            return Err(RenderError::InvalidWorkflow(format!(
                "duplicate_internal_env:{key}"
            )));
        }
        env.push((key.clone(), Yaml::str(value.clone())));
    }
    Ok(Yaml::Map(env))
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
) -> Result<Yaml, RenderError> {
    steps::validate_uses(uses)?;
    for (key, value) in with {
        velnor_actions_workflow_steps::expressions::check_with_key(key)?;
        velnor_actions_workflow_steps::expressions::check_with_value(key, value)?;
        steps::scan_for_private_subcommands(key)?;
        steps::scan_for_private_subcommands(value)?;
    }
    commands::validate_env(env)?;
    let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
    push_step_id(&mut entries, step);
    if let Some(condition) = &step.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    } else if uses == steps::UPLOAD_ARTIFACT_USES {
        entries.push((
            "if".to_owned(),
            Yaml::str(crate::render::FINAL_CONDITION.to_owned()),
        ));
    }
    if job_id == FINAL_JOB_ID && is_verdict_download(step) {
        entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    if uses.starts_with(TOOL_SEED_USES) {
        if uses != TOOL_SEED_USES {
            return Err(RenderError::InvalidWorkflow(
                "tool_seed_bad_action_ref".to_owned(),
            ));
        }
        crate::tool_seed::validate_seed_action(step, None)?;
    }
    let uses_yaml = if matches!(uses, TOOL_SEED_USES | TOFU_PROVIDER_ADMISSION_USES)
        || velnor_actions_workflow_steps::action_ref::is_generated_provider_prelude(uses)
    {
        Yaml::annotated(uses, "zizmor: ignore[self-repository]")
    } else {
        Yaml::str(uses.to_owned())
    };
    entries.push(("uses".to_owned(), uses_yaml));
    if !with.is_empty() {
        entries.push(("with".to_owned(), string_map_yaml(with)));
    }
    let filtered_env: BTreeMap<String, String> = env
        .iter()
        .filter(|(k, v)| job_env.get(*k).map(String::as_str) != Some(v.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if !filtered_env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(&filtered_env)));
    }
    Ok(Yaml::Map(entries))
}

/// Render one step; internal ops become env plus request file, never argv.
/// Every action ref (including the Alint pin) must be a full-SHA pin.
pub(crate) fn step_to_yaml(
    job_id: &str,
    step: &Step,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    composite: bool,
    job_env: &BTreeMap<String, String>,
    actions_read: bool,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&step.name)?;
    match &step.kind {
        StepKind::Action { uses, with, env } => {
            action_step_to_yaml(job_id, step, uses, with, env, job_env)
        }
        StepKind::Shell { run, env } => {
            commands::validate_command_argv(run)?;
            commands::validate_env(env)?;
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
            push_step_id(&mut entries, step);
            if let Some(condition) = &step.condition {
                steps::scan_for_private_subcommands(condition)?;
                entries.push(("if".to_owned(), Yaml::str(condition.clone())));
            }
            let filtered_env: BTreeMap<String, String> = env
                .iter()
                .filter(|(k, v)| job_env.get(*k).map(String::as_str) != Some(v.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if !filtered_env.is_empty() {
                entries.push(("env".to_owned(), string_map_yaml(&filtered_env)));
            }
            push_composite_shell(&mut entries, composite);
            entries.push((
                "run".to_owned(),
                Yaml::str(commands::join_argv_for_run(run)?),
            ));
            Ok(Yaml::Map(entries))
        }
        StepKind::Internal {
            operation,
            env: step_env,
        } => {
            let (op, target) = steps::split_internal_operation(operation)?;
            if op == steps::FETCH_OPERATION && !actions_read {
                return Err(RenderError::InvalidWorkflow(
                    "report_fetch_requires_actions_read".to_owned(),
                ));
            }
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
            push_step_id(&mut entries, step);
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
            entries.push((
                "env".to_owned(),
                internal_env(op, target, ctx, channel, job_env, step_env, actions_read)?,
            ));
            push_composite_shell(&mut entries, composite);
            entries.push((
                "run".to_owned(),
                Yaml::str(commands::quote_run_arg(&ctx.staged_binary)),
            ));
            Ok(Yaml::Map(entries))
        }
    }
}
