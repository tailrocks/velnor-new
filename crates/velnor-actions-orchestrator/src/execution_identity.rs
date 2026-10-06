//! Execution proof dimensions shared by planning and live coverage.

use velnor_actions_contract::{ContractError, ProposedTask, Stack};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_rust::tool_needs;

/// Compile driver and exact applicable MBX pin, with no ambient executable.
pub(crate) fn live_mbx_digest(task: &ProposedTask, catalog: &ToolCatalog) -> String {
    let mbx = Stack::from_id(&task.stack_id) == Some(Stack::Rust)
        && tool_needs(&task.identity.compile_driver, &task.identity.test_runner).mbx;
    let pin = mbx.then(|| catalog.version(PinnedTool::MrBoxington));
    crate::internal_plan::snapshot::canonical_digest(&serde_json::json!({
        "driver": task.identity.compile_driver,
        "mbx_pin": pin,
    }))
    .unwrap_or_else(|_| velnor_actions_contract::digest_b3(b"mbx_error"))
}

/// Resolve the same typed execution dimensions used to validate baseline proof.
pub(crate) fn execution_identity_for(
    task: &ProposedTask,
    bundle: &super::ExtensionBundle,
    catalog: &ToolCatalog,
    toolchain: &str,
    platform: &str,
) -> Result<velnor_actions_contract::TaskExecutionIdentity, ContractError> {
    velnor_actions_contract::TaskExecutionIdentity::new(
        bundle.graph_digest(),
        toolchain,
        &live_mbx_digest(task, catalog),
        platform,
        bundle.inputs().profile,
    )
}
