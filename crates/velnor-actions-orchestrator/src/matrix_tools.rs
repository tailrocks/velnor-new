//! Crate-job tool construction: task drivers plus audited suite tools.

use velnor_actions_contract::{ProposedTask, Stack, Step, WorkflowPolicy};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

#[path = "matrix_suite.rs"]
mod suite;

pub(crate) use suite::SuiteTools;

/// Mise-installed tools per obligation role.
///
/// MBX is supplied by its native action and is not installed through Mise.
#[must_use]
pub(crate) fn task_driver_tools(use_rust: bool, use_opentofu: bool) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(use_rust.then_some(PinnedTool::Rust));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

/// Velnor suites that execute the repository `generate` validators.
const GENERATE_VALIDATOR_SUITES: [&str; 2] = ["velnor-actions-orchestrator", "velnor-actions-cli"];

/// Velnor suites that execute real `tofu` binaries.
const TOFU_EXEC_SUITES: [&str; 1] = ["velnor-actions-mise"];

/// Consumer suites are opaque; repository suites use the audited execution list.
#[must_use]
pub(crate) fn crate_needs_generate_validators(policy: WorkflowPolicy, package: &str) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => true,
        WorkflowPolicy::VelnorRepositoryV1 => GENERATE_VALIDATOR_SUITES.contains(&package),
    }
}

/// Whether a Velnor suite runs a real `tofu` binary.
#[must_use]
pub(crate) fn crate_needs_tofu_install(policy: WorkflowPolicy, package: &str) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => false,
        WorkflowPolicy::VelnorRepositoryV1 => TOFU_EXEC_SUITES.contains(&package),
    }
}

/// Resolve the compiled-suite tools while retaining both suite-audit and policy checks.
///
/// The suite registry rejects unclassified Velnor packages. The policy predicates then
/// supply the final validator/OpenTofu selection; a parity check fails closed if the
/// registries drift apart.
///
/// # Errors
/// Returns a contract error for an unclassified or inconsistent suite.
pub(crate) fn suite_tools_for_tasks(
    policy: WorkflowPolicy,
    tasks: &[&ProposedTask],
) -> Result<SuiteTools, OrchestratorError> {
    let registered = suite::suite_tools_for_tasks(policy, tasks)?;
    let rust_package = tasks
        .iter()
        .find(|task| Stack::from_id(&task.stack_id) == Some(Stack::Rust))
        .map(|task| task.display_name.as_str());
    let selected = crate_suite_tools(policy, rust_package)?;
    if selected != registered {
        return Err(OrchestratorError::Contract {
            problem: "suite_tool_policy_mismatch".to_owned(),
        });
    }
    Ok(selected)
}

/// Resolve policy tools for one validated Rust suite owner.
///
/// # Errors
/// Returns a contract error when the repository suite has not been audited.
pub(crate) fn crate_suite_tools(
    policy: WorkflowPolicy,
    rust_package: Option<&str>,
) -> Result<SuiteTools, OrchestratorError> {
    let registered = suite::crate_suite_tools(policy, rust_package)?;
    let package = rust_package.unwrap_or("");
    let selected = SuiteTools {
        generate_validators: crate_needs_generate_validators(policy, package),
        opentofu: rust_package.is_some() && crate_needs_tofu_install(policy, package),
    };
    if selected != registered {
        return Err(OrchestratorError::Contract {
            problem: "suite_tool_policy_mismatch".to_owned(),
        });
    }
    Ok(selected)
}

/// Typed `Prepare pinned tools` step for the exact driver/suite tool set.
///
/// Pure-tofu jobs omit the owned Rust homes; all other roles retain them.
///
/// # Errors
/// Returns a contract error when the Mise adapter rejects the request.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "four independent install flags mirror task and suite selection"
)]
pub(crate) fn prepare_crate_tools_step(
    catalog: &ToolCatalog,
    use_rust: bool,
    use_nextest: bool,
    use_opentofu: bool,
    needs_validators: bool,
) -> Result<Step, OrchestratorError> {
    let mut tools = task_driver_tools(use_rust, use_opentofu);
    if needs_validators {
        tools.extend([
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
        ]);
    }
    tools.extend(use_nextest.then_some(PinnedTool::Nextest));
    let prepare = PreparePinnedTools::new(tools, ToolHomes::runner_temp()).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let run = strings_of(prepare.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = if use_rust {
        strings_of_env(&prepare.env(catalog))
    } else {
        strings_of_env(&prepare.env_without_homes())
    }
    .map_err(|problem| OrchestratorError::Contract { problem })?;
    velnor_actions_workflow_renderer::ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)
        .map_err(OrchestratorError::from)
}
