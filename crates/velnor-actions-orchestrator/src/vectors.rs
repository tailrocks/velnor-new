// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{ProposedTask, Stack};
use velnor_actions_mise::{
    PinnedTool, PinnedToolExec, RouteDriver, ToolCatalog, custom_run::custom_task_run_argv,
};
use velnor_actions_rust::tool_needs;

use crate::OrchestratorError;

pub(crate) fn task_argv(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    match Stack::from_id(&task.stack_id) {
        Some(Stack::Tofu) => return tofu_task_argv(task, catalog),
        Some(Stack::Mise) => {
            let name = task
                .payload
                .first()
                .and_then(|v| v.to_str())
                .filter(|_| task.payload.len() == 1)
                .ok_or_else(|| OrchestratorError::Contract {
                    problem: "named_check_task_payload".to_owned(),
                })?;
            return custom_task_run_argv(name).map_err(|e| OrchestratorError::Contract {
                problem: e.to_string(),
            });
        }
        Some(Stack::Native) => {
            return Err(OrchestratorError::Contract {
                problem: "native_homebrew_check_has_no_mise_argv".to_owned(),
            });
        }
        Some(Stack::Rust) => {}
        None => {
            return Err(OrchestratorError::Contract {
                problem: format!("unknown_task_stack:{}", task.stack_id),
            });
        }
    }
    let driver =
        RouteDriver::from_compile_driver(&task.identity.compile_driver).ok_or_else(|| {
            OrchestratorError::Contract {
                problem: format!("unknown_compile_driver:{}", task.identity.compile_driver),
            }
        })?;
    let mut tools = driver.probe_tools();
    if tool_needs(&task.identity.compile_driver, &task.identity.test_runner).nextest {
        tools.push(PinnedTool::Nextest);
    }
    let program = OsString::from(driver.program());
    let exec = PinnedToolExec::new(tools, &program, task.payload.clone()).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

fn tofu_task_argv(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Opentofu],
        OsStr::new("tofu"),
        task.payload.clone(),
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

pub(crate) fn exec_argv(
    tools: Vec<PinnedTool>,
    program: &str,
    args: &[&str],
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let exec = PinnedToolExec::new(
        tools,
        OsStr::new(program),
        args.iter().copied().map(OsString::from).collect(),
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

pub(crate) fn deny_argv(roots: &[String]) -> Result<Vec<String>, OrchestratorError> {
    velnor_actions_workflow_renderer::native_authority::support_commands::vectors::deny_argv(roots)
        .map_err(crate::verify::map_error)
}
pub(crate) fn machete_argv() -> Result<Vec<String>, OrchestratorError> {
    velnor_actions_workflow_renderer::native_authority::support_commands::vectors::machete_argv()
        .map_err(crate::verify::map_error)
}
pub(crate) fn machete_install_argv() -> Result<Vec<String>, OrchestratorError> {
    velnor_actions_workflow_renderer::native_authority::support_commands::vectors::machete_install_argv().map_err(crate::verify::map_error)
}
fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    argv.into_iter()
        .map(|arg| arg.into_string().map_err(|_| "non_utf8_argv".to_owned()))
        .collect()
}
