//! Action-step YAML formatting and action-specific condition policy.

use std::collections::BTreeMap;

use crate::{RenderError, commands, steps, yaml::Yaml};
use velnor_actions_contract::Step;

/// Render one action step: name, condition, pin, inputs, step env.
///
/// Cache writers always retain the shared protected-default gate. Step env
/// renders after `with:`; absent env renders nothing.
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
    if let Some(condition) =
        crate::cache_steps::action_cache_condition(uses, step.condition.as_deref())
    {
        steps::scan_for_private_subcommands(&condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition)));
    } else if uses == steps::UPLOAD_ARTIFACT_USES {
        entries.push((
            "if".to_owned(),
            Yaml::str(crate::render::FINAL_CONDITION.to_owned()),
        ));
    }
    if job_id == crate::render::FINAL_JOB_ID && super::is_verdict_download(step) {
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

/// Render sorted string maps as YAML.
fn string_map_yaml(map: &BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        map.iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
            .collect(),
    )
}
