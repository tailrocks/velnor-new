use std::collections::BTreeMap;

use velnor_actions_contract::cachekey::ToolchainInputs;
use velnor_actions_contract::workflow::TaskExecutionManifestEntryV1;
use velnor_actions_mise::{PinnedTool, validate_concrete_exact_version};

use crate::OrchestratorError;
use crate::internal::internal;

pub(super) struct ResolvedTask {
    pub(super) argv: Vec<String>,
    pub(super) env: BTreeMap<String, String>,
    pub(super) toolchain_inputs: ToolchainInputs,
}

pub(super) fn resolve_with(
    record: &TaskExecutionManifestEntryV1,
    read_version: impl Fn(&str) -> Option<String>,
) -> Result<ResolvedTask, OrchestratorError> {
    let mut values = BTreeMap::new();
    let mut collect = |text: &str| -> Result<(), OrchestratorError> {
        for name in velnor_actions_contract::config::project_tool_refs::project_tool_ref_names(text)
        {
            if values.contains_key(name) {
                continue;
            }
            let tool = PinnedTool::ALL
                .into_iter()
                .find(|tool| tool.project_env() == name)
                .ok_or_else(|| internal("unknown_project_tool_reference"))?;
            let version =
                read_version(name).ok_or_else(|| internal("missing_project_tool_version"))?;
            validate_concrete_exact_version(tool.tool_name(), &version)
                .map_err(|_| internal("invalid_project_tool_version"))?;
            values.insert(name.to_owned(), version);
        }
        Ok(())
    };
    for text in &record.argv {
        collect(text)?;
    }
    for text in record.env.values() {
        collect(text)?;
    }
    for text in &record.toolchain_inputs.tools {
        collect(text)?;
    }
    let resolve_text = |text: &str| {
        velnor_actions_contract::config::project_tool_refs::map_project_tool_refs(text, |name| {
            values.get(name).cloned().unwrap_or_default()
        })
    };
    let argv = record
        .argv
        .iter()
        .map(|value| resolve_text(value))
        .collect();
    let task_env = record
        .env
        .iter()
        .map(|(key, value)| (key.clone(), resolve_text(value)))
        .collect();
    let mut toolchain_inputs = record.toolchain_inputs.clone();
    toolchain_inputs.tools = toolchain_inputs
        .tools
        .iter()
        .map(|value| resolve_text(value))
        .collect();
    Ok(ResolvedTask {
        argv,
        env: task_env,
        toolchain_inputs,
    })
}

#[cfg(test)]
#[path = "task_execution_project_tools_tests.rs"]
mod tests;
