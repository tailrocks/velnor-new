//! Plain `mise run <task>` argv for allowlisted custom tasks.
//!
//! Unlike the isolated vectors, this carries no global flags: custom
//! tasks are defined in the repository's own Mise configuration, so
//! `--no-config` would hide them. The safety boundary is the explicit
//! `[stacks.rust] custom_tasks` allowlist, never flag isolation.

use velnor_actions_contract::is_valid_mise_task_name;

use crate::command::is_allowed_mise_subcommand;
use crate::error::MiseError;

/// Task-name rule shared with the config allowlist: [`is_valid_mise_task_name`]
/// (same rule as `CustomTaskGrant`/`task_run_argv`).
fn is_valid_task_name(task: &str) -> bool {
    is_valid_mise_task_name(task)
}

/// Fixed `mise run <task>` argv for one allowlisted custom task.
///
/// # Errors
///
/// Returns [`MiseError::InvalidStepInput`] for a blank, path-bearing,
/// or space-bearing task name.
pub fn custom_task_run_argv(task: &str) -> Result<Vec<String>, MiseError> {
    debug_assert!(is_allowed_mise_subcommand("run"));
    if !is_valid_task_name(task) {
        return Err(MiseError::InvalidStepInput {
            field: "task".to_owned(),
            value: format!("bad_task_name:{task}"),
        });
    }
    Ok(["mise", "run", task]
        .iter()
        .map(ToString::to_string)
        .collect())
}
