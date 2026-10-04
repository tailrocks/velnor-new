//! Native job setup is explicit; pure native jobs never install Rust.

use crate::OrchestratorError;
use velnor_actions_contract::{CrateJob, ProposedTask, Stack, Step};
use velnor_actions_mise::{MiseInstall, ToolCatalog};

/// Prepare the native owner independently of its catalog tool requirements.
pub(crate) fn prepare_step(
    model: &CrateJob,
    catalog: &ToolCatalog,
    label: &str,
) -> Result<Option<Step>, OrchestratorError> {
    if !model.job_id.starts_with("workload-") {
        return Ok(None);
    }
    if model.configuration == "docker_build" {
        return super::cache::docker::prepare_step(model);
    }
    let tools = super::tools(&model.configuration)?;
    if tools.is_empty() {
        return Ok(None);
    }
    let request = MiseInstall::new(tools).map_err(|error| OrchestratorError::Contract {
        problem: error.to_string(),
    })?;
    let host = super::host_for_runner(label)?;
    let run = crate::utf8::strings_of(request.argv_for_host(catalog, host).map_err(|error| {
        OrchestratorError::Contract {
            problem: error.to_string(),
        }
    })?)
    .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = if super::requires_rust(&model.configuration) {
        crate::utf8::strings_of_env(&velnor_actions_mise::ToolHomes::runner_temp().env(catalog))
            .map_err(|problem| OrchestratorError::Contract { problem })?
    } else {
        velnor_actions_mise::ISOLATION_ENV
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    };
    Ok(Some(velnor_actions_workflow_renderer::ambient_shell_step(
        "Prepare native tools",
        run,
        env,
    )?))
}

/// Swift package validation preserves its native macOS ARM runner contract.
pub(crate) fn runner_for_model(model: &CrateJob, label: &str) -> String {
    runner_for_kind(
        model.job_id.starts_with("workload-"),
        &model.configuration,
        label,
    )
    .to_owned()
}

/// Resolve the runner before binding any task platform identity.
pub(crate) fn runner_for_task<'a>(task: &ProposedTask, label: &'a str) -> &'a str {
    runner_for_kind(
        Stack::from_id(&task.stack_id) == Some(Stack::Workload),
        &task.configuration,
        label,
    )
}

/// One runner policy shared by emitted jobs and identity envelopes.
fn runner_for_kind<'a>(native: bool, configuration: &str, label: &'a str) -> &'a str {
    if native
        && matches!(
            configuration,
            "swift_test" | "native_xcode_project_ci" | "native_swift_package_ci"
        )
    {
        "macos-26"
    } else {
        label
    }
}

#[cfg(test)]
#[path = "workloads_runner_tests.rs"]
mod tests;
