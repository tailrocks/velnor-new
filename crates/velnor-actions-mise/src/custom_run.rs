// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::is_valid_mise_task_name;

use crate::command::is_allowed_mise_subcommand;
use crate::error::MiseError;

pub fn custom_task_run_argv(task: &str) -> Result<Vec<String>, MiseError> {
    debug_assert!(is_allowed_mise_subcommand("run"));
    if !is_valid_mise_task_name(task) {
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
