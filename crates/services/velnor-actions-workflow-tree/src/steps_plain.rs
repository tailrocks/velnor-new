//! Plain step rendering for the satellite workflow trees.
//!
//! Shared by the release and freshness renderers, which carry no
//! internal planner steps: only pinned actions and fixed-argv shell
//! survive; anything else is rejected fail-closed.

use velnor_actions_contract_workflow::{Step, StepKind};
use velnor_actions_workflow_steps::{RenderError, commands, steps};

use crate::yaml::{Yaml, string_map_yaml};

/// Render one action/shell step; internal ops are rejected fail-closed.
///
/// Revalidates refs, argv, and env at render time so a `Step` built
/// outside the validated constructors cannot smuggle content through.
/// # Errors
pub fn plain_step_to_yaml(step: &Step) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&step.name)?;
    match &step.kind {
        StepKind::Action { uses, with, env } => {
            steps::validate_uses(uses)?;
            for entry in with.keys().chain(with.values()) {
                steps::scan_for_private_subcommands(entry)?;
            }
            commands::validate_env(env)?;
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
            if let Some(condition) = &step.condition {
                steps::scan_for_private_subcommands(condition)?;
                entries.push(("if".to_owned(), Yaml::str(condition.clone())));
            }
            entries.push(("uses".to_owned(), Yaml::str(uses.clone())));
            if !with.is_empty() {
                entries.push(("with".to_owned(), string_map_yaml(with)));
            }
            if !env.is_empty() {
                entries.push(("env".to_owned(), string_map_yaml(env)));
            }
            Ok(Yaml::Map(entries))
        }
        StepKind::Shell { run, env } => {
            crate::action_mbx_shell::validate_step_argv(run, step.role)?;
            commands::validate_env(env)?;
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
            if let Some(condition) = &step.condition {
                steps::scan_for_private_subcommands(condition)?;
                entries.push(("if".to_owned(), Yaml::str(condition.clone())));
            }
            if !env.is_empty() {
                entries.push(("env".to_owned(), string_map_yaml(env)));
            }
            entries.push((
                "run".to_owned(),
                Yaml::str(commands::join_validated_argv_for_run(run)?),
            ));
            Ok(Yaml::Map(entries))
        }
        StepKind::TaskExecution { .. } => Err(RenderError::InvalidWorkflow(
            "unfactored_task_execution".to_owned(),
        )),
        StepKind::Internal { .. } => Err(RenderError::InvalidWorkflow(
            "internal_op_rejected".to_owned(),
        )),
    }
}

/// Render a step inside a composite action, where every `run` step must
/// carry its shell explicitly rather than inherit workflow job defaults.
pub(crate) fn composite_step_to_yaml(step: &Step) -> Result<Yaml, RenderError> {
    let mut rendered = plain_step_to_yaml(step)?;
    if !matches!(&step.kind, StepKind::Shell { .. }) {
        return Ok(rendered);
    }
    let Yaml::Map(entries) = &mut rendered else {
        return Err(RenderError::InvalidWorkflow(
            "composite_shell_step_not_map".to_owned(),
        ));
    };
    let Some(run_index) = entries.iter().position(|(key, _)| key == "run") else {
        return Err(RenderError::InvalidWorkflow(
            "composite_shell_step_missing_run".to_owned(),
        ));
    };
    entries.insert(run_index, ("shell".to_owned(), Yaml::str("bash")));
    Ok(rendered)
}

/// Sorted string map as YAML (shared by `with:` and `env:` emission).
fn string_map_yaml(map: &std::collections::BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        map.iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
            .collect(),
    )
}
