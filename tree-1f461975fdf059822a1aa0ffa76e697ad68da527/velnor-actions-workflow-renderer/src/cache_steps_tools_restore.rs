//! Generated composite for the fixed V2 tools-cache restore payload.

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, marker, steps, tree::RenderedFile, yaml::Yaml};

/// Validate the only call shape accepted for the fixed restore composite.
/// # Errors
pub(crate) fn validate_call(step: &Step) -> Result<&str, RenderError> {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "tools_cache_restore_shape".to_owned(),
        ));
    };
    let key = with.get("key").ok_or_else(|| {
        RenderError::InvalidWorkflow("tools_cache_restore_key_missing".to_owned())
    })?;
    if step.role != Some(StepRole::ToolsCacheRestore)
        || step.id.is_some()
        || uses != super::TOOLS_RESTORE_USES
        || !env.is_empty()
        || with.len() != 1
        || !crate::cache_p08::is_v2_cache_key_expression(key)
        || step.condition.as_deref() != Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
    {
        return Err(RenderError::InvalidWorkflow(
            "tools_cache_restore_shape".to_owned(),
        ));
    }
    Ok(key)
}

/// Build the pinned restore action with its renderer-owned archive paths.
/// # Errors
pub(crate) fn action_file(version: &str) -> Result<RenderedFile, RenderError> {
    steps::validate_uses(super::TOOLS_RESTORE_ACTION_USES)?;
    let path = super::TOOLS_CACHE_PATHS.join("\n");
    let inner = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(super::TOOLS_RESTORE_NAME)),
        (
            "uses".to_owned(),
            Yaml::str(super::TOOLS_RESTORE_ACTION_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("key".to_owned(), Yaml::str("${{ inputs.key }}")),
                ("path".to_owned(), Yaml::str(path)),
                ("restore-keys".to_owned(), Yaml::str(String::new())),
            ]),
        ),
    ]);
    let body = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Velnor Mise tools cache restore"),
        ),
        (
            "description".to_owned(),
            Yaml::str("Restore the exact renderer-owned V2 Mise tools paths."),
        ),
        (
            "inputs".to_owned(),
            Yaml::Map(vec![(
                "key".to_owned(),
                Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        Yaml::str("Exact runtime-qualified V2 tools key."),
                    ),
                    ("required".to_owned(), Yaml::Bool(true)),
                ]),
            )]),
        ),
        (
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite")),
                ("steps".to_owned(), Yaml::Seq(vec![inner])),
            ]),
        ),
    ]);
    let bytes = marker::with_marker(version, &crate::yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    let action_directory = super::TOOLS_RESTORE_USES
        .strip_prefix("./")
        .ok_or_else(|| RenderError::InvalidWorkflow("tools_restore_action_path".to_owned()))?;
    Ok(RenderedFile {
        path: format!("{action_directory}/action.yml"),
        bytes,
    })
}
