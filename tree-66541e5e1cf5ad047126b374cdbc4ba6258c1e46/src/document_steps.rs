//! Rendering helpers for one workflow step.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{
    RenderError, commands,
    composite::push_composite_shell,
    render::{FINAL_JOB_ID, RenderContext},
    steps::{self, INTERNAL_OP_ENV, REQUEST_FILE_ENV},
    yaml::Yaml,
};

/// True for the final job's plan download (fetch is gated inline below).
///
/// An absent plan artifact (failed plan) must still reach the merge
/// verdict instead of failing the job at the download step.
fn is_verdict_download(step: &Step) -> bool {
    matches!(
        &step.kind,
        StepKind::Action { uses, .. } if uses == steps::DOWNLOAD_ARTIFACT_USES
    ) && step.name == crate::closure::DOWNLOAD_PLAN_NAME
}

/// Env for one internal step: op plus request file, fetch carries auth.
///
/// The fetch op takes no request file; it reads the plan from the
/// run directory and authenticates `gh` with the job token plus the
/// repository slug (fixed literals, never caller input). Token hygiene
/// still gates IR-level `GH_TOKEN` (see `support`); this render-time
/// pair is fixed by construction for the fetch step only. Merge-family
/// steps in the final job additionally carry the finalized `needs`
/// conclusions channel plus the rendered expected inventory, so the
/// merge binds required validators to the committed workflow.
fn internal_env(
    op: &str,
    target: &str,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    job_env: &BTreeMap<String, String>,
) -> Yaml {
    if op == steps::FETCH_OPERATION {
        return Yaml::Map(vec![
            ("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")),
            ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
            (INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())),
        ]);
    }
    let request = format!("{}/{target}-request.json", ctx.request_dir);
    let mut env = vec![(INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned()))];
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
    Yaml::Map(env)
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
        crate::expressions::check_with_key(key)?;
        crate::expressions::check_with_value(key, value)?;
        steps::scan_for_private_subcommands(key)?;
        steps::scan_for_private_subcommands(value)?;
    }
    commands::validate_env(env)?;
    let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
    crate::mbx_bundle::push_step_id(&mut entries, &step.name);
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
    entries.push(("uses".to_owned(), Yaml::str(uses.to_owned())));
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

/// Sorted string map as YAML (shared by `with:` and `env:` emission).
pub(crate) fn string_map_yaml(map: &BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        map.iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
            .collect(),
    )
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
            crate::mbx_bundle::push_step_id(&mut entries, &step.name);
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
        StepKind::Internal { operation } => {
            let (op, target) = steps::split_internal_operation(operation)?;
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
            entries.push((
                "env".to_owned(),
                internal_env(op, target, ctx, channel, job_env),
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
