//! Extract verifier template fields from the canonical typed renderer.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::Step;
use velnor_actions_workflow_renderer::yaml::Yaml;

use crate::OrchestratorError;

pub(super) fn step_fields(step: &Step) -> Result<Vec<(String, Yaml)>, OrchestratorError> {
    let Yaml::Map(mut fields) =
        velnor_actions_workflow_renderer::verifier_composite::render_verifier_workflow_step(
            step,
            &BTreeMap::new(),
        )?
    else {
        return Err(OrchestratorError::Contract {
            problem: "verifier_inline_template_not_mapping".to_owned(),
        });
    };
    let run = fields
        .iter_mut()
        .find_map(|(key, value)| (key == "run").then_some(value))
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "verifier_inline_template_missing_run".to_owned(),
        })?;
    let (Yaml::Str(script)
    | Yaml::Quoted(script)
    | Yaml::Annotated { value: script, .. }
    | Yaml::AnchoredScalar { value: script, .. }) = run
    else {
        return Err(OrchestratorError::Contract {
            problem: "verifier_inline_template_bad_run".to_owned(),
        });
    };
    *script = velnor_actions_workflow_renderer::quote_project_tool_run_refs(script)?;
    Ok(fields)
}

pub(super) fn bind_project_environment(
    fields: &mut [(String, Yaml)],
) -> Result<(), OrchestratorError> {
    let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(key, _)| key == "env") else {
        return Err(OrchestratorError::Contract {
            problem: "verifier_inline_template_missing_env".to_owned(),
        });
    };
    for (_, value) in env {
        if let Yaml::Str(value) = value {
            *value = velnor_actions_contract::config::project_tool_refs::project_tool_refs_to_expressions(value);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn scalar_field(
    fields: &[(String, Yaml)],
    key: &str,
) -> Result<String, OrchestratorError> {
    let value = fields
        .iter()
        .find_map(|(candidate, value)| (candidate == key).then_some(value))
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("verifier_inline_template_missing_{key}"),
        })?;
    field_value(key, value)
}

pub(super) fn shell(fields: &[(String, Yaml)]) -> Result<Option<&'static str>, OrchestratorError> {
    let shell = fields.iter().find_map(|(key, value)| {
        (key == "shell"
            && value == &Yaml::str(velnor_actions_workflow_renderer::VERIFY_VERIFIER_SHELL))
            .then_some(velnor_actions_workflow_renderer::VERIFY_VERIFIER_SHELL)
    });
    shell.map(Some).ok_or_else(|| OrchestratorError::Contract {
        problem: "verifier_inline_template_bad_shell".to_owned(),
    })
}

pub(super) fn env(
    fields: &[(String, Yaml)],
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let entries = fields
        .iter()
        .find_map(|(key, value)| (key == "env").then_some(value))
        .and_then(|value| match value {
            Yaml::Map(entries) => Some(entries),
            _ => None,
        })
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "verifier_inline_template_missing_env".to_owned(),
        })?;
    entries
        .iter()
        .map(|(key, value)| match value {
            Yaml::Str(value) | Yaml::Quoted(value) => Ok((key.clone(), value.clone())),
            _ => Err(OrchestratorError::Contract {
                problem: format!("verifier_inline_template_bad_env:{key}"),
            }),
        })
        .collect()
}

pub(super) fn field_value(key: &str, value: &Yaml) -> Result<String, OrchestratorError> {
    let rendered = velnor_actions_workflow_renderer::yaml::render_yaml(&Yaml::Map(vec![(
        key.to_owned(),
        value.clone(),
    )]));
    let line = rendered
        .lines()
        .next()
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("verifier_inline_template_bad_field:{key}"),
        })?;
    let prefix = format!("{key}:");
    let value = line
        .strip_prefix(&prefix)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("verifier_inline_template_bad_field:{key}"),
        })?;
    Ok(value.strip_prefix(' ').unwrap_or(value).to_owned())
}
