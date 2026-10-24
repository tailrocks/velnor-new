//! Source-qualified Homebrew formula updater binding.

use super::{config_error, valid_repo_path};
use crate::{
    ContractError,
    config::{CheckRunner, MiseTaskSource, mise::is_valid_mise_task_name},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A repository-owned Mise task allowed to update a tap formula.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapUpdaterBinding {
    /// Repository-owned Mise task; never arbitrary argv.
    pub task: String,
    /// Exact Mise configuration and task working directory.
    pub source: MiseTaskSource,
    /// Qualified task placement from the existing named-check contract.
    pub runner: CheckRunner,
    /// Complete tracked task closure: Mise config/includes, scripts, and inputs.
    pub inputs: Vec<String>,
    /// Tool IDs resolved only from the captured tap Mise config and lock.
    pub tools: Vec<String>,
    /// Native promotion task runtime, bounded to one through five minutes.
    #[serde(default = "default_updater_timeout")]
    pub timeout_minutes: u32,
}

fn default_updater_timeout() -> u32 {
    u32::from(crate::workflow::timeout::JobTimeout::DEFAULT.minutes())
}

impl TapUpdaterBinding {
    /// Validate task identity, source closure, tool names, and runtime bound.
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
