// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{config_error, valid_repo_path};
use crate::{
    ContractError,
    config::{CheckRunner, MiseTaskSource, mise::is_valid_mise_task_name},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapUpdaterBinding {
    pub task: String,

    pub source: MiseTaskSource,

    pub runner: CheckRunner,

    pub inputs: Vec<String>,

    pub tools: Vec<String>,

    #[serde(default = "default_updater_timeout")]
    pub timeout_minutes: u32,
}

fn default_updater_timeout() -> u32 {
    u32::from(crate::workflow::timeout::JobTimeout::DEFAULT.minutes())
}

impl TapUpdaterBinding {
    pub(super) fn validate(&self, file: &str, parent: &str) -> Result<(), ContractError> {
        let bad = |field, problem| config_error(file, parent, field, problem);
        if !is_valid_mise_task_name(&self.task) || self.task.len() > 128 {
            return Err(bad("updater.task", "bad_mise_task"));
        }
        if !self.source.validate() {
            return Err(bad("updater.source", "bad_mise_task_source"));
        }
        self.runner
            .validate(file, &format!("{parent}.updater.runner"))?;
        if self.inputs.is_empty() || !unique_values(&self.inputs, valid_repo_path) {
            return Err(bad("updater.inputs", "invalid_task_inputs"));
        }
        if !self
            .inputs
            .iter()
            .any(|path| path == &self.source.mise_config)
        {
            return Err(bad("updater.inputs", "task_source_not_an_input"));
        }
        if self.tools.is_empty() || !unique_values(&self.tools, valid_tool_name) {
            return Err(bad("updater.tools", "invalid_task_tools"));
        }
        if !(1..=u32::from(crate::config::NATIVE_TAP_UPDATER_MAX_TIMEOUT_MINUTES))
            .contains(&self.timeout_minutes)
        {
            return Err(bad("updater.timeout_minutes", "bad_task_timeout"));
        }
        Ok(())
    }
}

fn unique_values(values: &[String], valid: fn(&str) -> bool) -> bool {
    let unique: BTreeSet<&str> = values.iter().map(String::as_str).collect();
    unique.len() == values.len() && values.iter().all(|value| valid(value))
}

fn valid_tool_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@' | b'+')
        })
}
