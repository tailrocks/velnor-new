//! YAML rendering for pinned GitHub Action steps.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{
    RenderError, commands,
    render::{FINAL_CONDITION, FINAL_JOB_ID},
    steps,
    yaml::Yaml,
};

/// Render an action step: name, condition, pin, inputs, step env.
///
/// Step env (cache modes) renders after `with:`; absent env renders
/// nothing, so env-less steps keep their exact historical bytes.
pub(super) fn action_step_to_yaml(
    job_id: &str,
    step: &Step,
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
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
        entries.push(("if".to_owned(), Yaml::str(FINAL_CONDITION.to_owned())));
    }
    if job_id == FINAL_JOB_ID && is_verdict_download(step) {
        entries.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    entries.push(("uses".to_owned(), Yaml::str(uses.to_owned())));
    if !with.is_empty() {
        entries.push(("with".to_owned(), string_map_yaml(with)));
    }
    if !env.is_empty() {
        entries.push(("env".to_owned(), string_map_yaml(env)));
    }
    Ok(Yaml::Map(entries))
}

/// True for the final job's plan download, which must not block the verdict.
fn is_verdict_download(step: &Step) -> bool {
    matches!(
        &step.kind,
        StepKind::Action { uses, .. } if uses == steps::DOWNLOAD_ARTIFACT_USES
    ) && step.name == crate::closure::DOWNLOAD_PLAN_NAME
}

/// Render a sorted string map shared by action `with:` and `env:` fields.
fn string_map_yaml(map: &BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        map.iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
            .collect(),
    )
}
