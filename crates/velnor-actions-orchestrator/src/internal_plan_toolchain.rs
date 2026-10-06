//! Runner-qualified toolchain identities for native stacks.

use velnor_actions_contract::{ContractError, ProposedTask, Stack};
use velnor_actions_mise::ToolCatalog;

/// Toolchain identity digest for one non-native task.
pub(crate) fn toolchain_id(
    task: &ProposedTask,
    catalog: &ToolCatalog,
) -> Result<String, ContractError> {
    if matches!(
        Stack::from_id(&task.stack_id),
        Some(Stack::Workload | Stack::Tofu)
    ) {
        return Err(ContractError::identity(
            "native_host",
            "native_host_required:use_toolchain_id_for_runner",
        ));
    }
    crate::internal_plan::identities::toolchain_digest_for(task, catalog)
}

/// Toolchain identity bound to the actual runner host for native stacks.
pub(crate) fn toolchain_id_for_runner(
    task: &ProposedTask,
    catalog: &ToolCatalog,
    label: &str,
) -> Result<String, ContractError> {
    let actual_label = crate::workloads::runner_for_task(task, label);
    match Stack::from_id(&task.stack_id) {
        Some(Stack::Workload) => {
            crate::workloads::toolchain_id_for_runner(task, catalog, actual_label)
        }
        Some(Stack::Tofu) => {
            let host = crate::workloads::host_for_runner(actual_label)
                .map_err(|error| ContractError::identity("native_host", error.to_string()))?;
            let specs = catalog
                .native_tool_specs(host, &[velnor_actions_mise::PinnedTool::Opentofu])
                .map_err(|error| ContractError::identity("native_catalog", error.to_string()))?;
            let inputs = velnor_actions_tofu::toolchain_inputs_for_task(task, specs)?;
            velnor_actions_contract::cachekey::toolchain_id(&inputs)
        }
        _ => {
            let host = crate::workloads::host_for_runner(actual_label)
                .map_err(|error| ContractError::identity("compiler_host", error.to_string()))?;
            let _selector = catalog.native_tool_spec(host, catalog.compiler_tool())?;
            toolchain_id(task, catalog)
        }
    }
}
