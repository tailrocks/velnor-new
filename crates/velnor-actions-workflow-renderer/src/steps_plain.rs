//! Plain step rendering for the satellite workflow trees.
//!
//! Shared by the release and freshness renderers, which carry no
//! internal planner steps: only pinned actions and fixed-argv shell
//! survive; anything else is rejected fail-closed.

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, commands, yaml::Yaml};

/// Render one action/shell step; internal ops are rejected fail-closed.
///
/// Revalidates refs, argv, and env at render time so a `Step` built
/// outside the validated constructors cannot smuggle content through.
pub(crate) fn plain_step_to_yaml(step: &Step) -> Result<Yaml, RenderError> {
    crate::steps::scan_for_private_subcommands(&step.name)?;
    crate::parallel_steps::reject_parallel(step, "plain")?;
    match &step.kind {
        StepKind::Action { uses, with, env } => {
            crate::action_ref::validate_verifier_call(step, uses, with, env)?;
            crate::steps::validate_uses(uses)?;
            for entry in with.keys().chain(with.values()) {
                crate::steps::scan_for_private_subcommands(entry)?;
            }
            commands::validate_env(env)?;
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
            if let Some(condition) = &step.condition {
                crate::steps::scan_for_private_subcommands(condition)?;
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
            if let Some(verifier) = crate::verifier_shell::render_typed_step(
                step,
                run,
                env,
                &std::collections::BTreeMap::new(),
                false,
            )? {
                return Ok(verifier);
            }
            crate::action_mbx_shell::validate_step_argv(run, step.role)?;
            commands::validate_env(env)?;
            let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
            if let Some(condition) = &step.condition {
                crate::steps::scan_for_private_subcommands(condition)?;
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

/// Sorted string map as YAML (shared by `with:` and `env:` emission).
fn string_map_yaml(map: &std::collections::BTreeMap<String, String>) -> Yaml {
    Yaml::Map(
        map.iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
            .collect(),
    )
}
