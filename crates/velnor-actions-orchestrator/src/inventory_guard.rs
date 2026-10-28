// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::{OperatorToolchainGuard, PinnedTool, ToolCatalog, ToolPresence};

use crate::OrchestratorError;

pub(crate) fn require_metadata_tools(
    catalog: &ToolCatalog,
    guard: Option<&OperatorToolchainGuard>,
) -> Result<(), OrchestratorError> {
    let tools = if guard.is_some() {
        vec![PinnedTool::Rust, PinnedTool::MrBoxington]
    } else {
        vec![PinnedTool::Rust]
    };
    let presence = ToolPresence::new(tools)
        .map_err(|error| presence_error(error, guard.is_some()))?;
    presence.run_with_observation(catalog).map_err(|failure| {
        if guard.is_some() {
            OrchestratorError::ToolchainGuard {
                problem: "operator toolchain guard rejected metadata".to_owned(),
            }
        } else {
            let problem = match failure.observation {
                Some(observation) => format!(
                    "{}; mise_ls_observation={}",
                    failure.error,
                    observation.to_json_line()
                ),
                None => failure.error.to_string(),
            };
            OrchestratorError::Inventory { problem }
        }
    })
}

fn presence_error(error: impl std::fmt::Display, guarded: bool) -> OrchestratorError {
    if guarded {
        OrchestratorError::ToolchainGuard {
            problem: format!("operator toolchain guard rejected metadata: {error}"),
        }
    } else {
        OrchestratorError::Inventory {
            problem: error.to_string(),
        }
    }
}
