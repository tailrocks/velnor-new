use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use crate::{RenderError, commands, render::FINAL_JOB_ID, steps, yaml::Yaml};

use super::{is_verdict_download, string_map_yaml};

/// Render one action step: name, condition, pin, inputs, step env.
///
/// Step env (cache modes) renders after `with:`; absent env renders
/// nothing, so env-less steps keep their exact historical bytes.
#[derive(Clone, Copy)]
pub(super) struct ActionStepContext<'a> {
    pub(super) job_id: &'a str,
    pub(super) job_env: &'a BTreeMap<String, String>,
    pub(super) runs_on: Option<&'a str>,
    pub(super) composite: bool,
}

pub(super) fn action_step_to_yaml(
    step: &Step,
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    context: ActionStepContext<'_>,
) -> Result<Yaml, RenderError> {
    let ActionStepContext {
        job_id,
        job_env,
        runs_on,
        composite,
    } = context;
    let runtime_identity = validate_action_step(step, uses, with, env, runs_on, composite)?;
    let rendered_env = cache_action_env(step, env)?;
    commands::validate_env(&rendered_env)?;
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
    crate::cache_save_budget::decorate(step, &mut entries, runs_on.is_some() && !composite)?;
    if job_id == FINAL_JOB_ID && is_verdict_download(step) {
        entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    let uses_value = if runtime_identity
        || uses == crate::tool_seed::TOOL_SEED_USES
        || uses == crate::cache_steps::TOOLS_RESTORE_USES
        || uses == steps::CACHE_TAR_LAUNCHER_USES
        || velnor_actions_contract::workflow::step_identity::RUST_VERIFIER_USES.contains(&uses)
        || uses == velnor_actions_contract::workflow::step_identity::JACTIONLINT_VERIFIER_USES
        || uses == crate::tofu_cache::TOFU_PROVIDER_ADMISSION_USES
        || crate::action_ref::is_generated_provider_prelude(uses)
        || crate::action_ref::is_crate_obligation_action(uses)
        || crate::action_ref::is_qualification_cache_composite(uses)
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
    let filtered_env: BTreeMap<String, String> = rendered_env
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
    composite: bool,
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
        if crate::action_ref::is_qualification_cache_composite(uses)
            && key == "qualification_campaign"
            && value == "${{ needs.plan.outputs.qualification_campaign }}"
        {
            // Contract identity validation checks this exact input on the typed call.
        } else if composite {
            crate::expressions::check_composite_with_value(key, value)?;
        } else {
            crate::expressions::check_with_action_value(uses, key, value)?;
        }
        steps::scan_for_private_subcommands(key)?;
        steps::scan_for_private_subcommands(value)?;
    }
    Ok(runtime_identity)
}

pub(super) fn cache_action_env(
    step: &Step,
    env: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, RenderError> {
    let mut rendered = env.clone();
    if crate::cache_tar_setup::requires_cache_tar_path(step.role) {
        if rendered
            .get("PATH")
            .is_some_and(|value| value != steps::CACHE_TAR_LAUNCHER_PATH_OUTPUT_EXPR)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "cache_action_path_override:{}",
                step.name
            )));
        }
        rendered.insert(
            "PATH".to_owned(),
            steps::CACHE_TAR_LAUNCHER_PATH_OUTPUT_EXPR.to_owned(),
        );
    }
    Ok(rendered)
}
