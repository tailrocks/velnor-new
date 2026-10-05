//! Exact catalog MBX/Rust PATH preflight shared by every action insertion.

use std::collections::BTreeMap;

use velnor_actions_actionlint::PinnedActionRef;
use velnor_actions_actionlint::actions::{MR_BOXINGTON_ACTION_SHA, MR_BOXINGTON_ACTION_VERSION};
use velnor_actions_contract::Step;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::steps::{CompileDriver, mbx_steps_for_driver};

use crate::OrchestratorError;

/// Build the strict preflight and restore pair from the compiled catalog pins.
/// # Errors
pub(crate) fn steps_for_catalog(catalog: &ToolCatalog) -> Result<[Step; 3], OrchestratorError> {
    let uses = PinnedActionRef::new(
        "jdx/mr-boxington-action",
        None,
        MR_BOXINGTON_ACTION_SHA,
        MR_BOXINGTON_ACTION_VERSION,
    )?
    .uses_value();
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    let steps = mbx_steps_for_driver(
        &uses,
        CompileDriver::Mbx,
        catalog.version(PinnedTool::MrBoxington),
        catalog.version(PinnedTool::Rust),
        env,
    )?;
    steps.ok_or_else(|| OrchestratorError::Contract {
        problem: "mbx_driver_did_not_emit_preflight".to_owned(),
    })
}
