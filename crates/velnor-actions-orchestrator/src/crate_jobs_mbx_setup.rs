//! MBX's pinned local setup action for a crate job.

use velnor_actions_actionlint::{
    PinnedActionRef,
    actions::{MR_BOXINGTON_ACTION_SHA, MR_BOXINGTON_ACTION_VERSION},
};
use velnor_actions_contract::Step;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::steps::{CompileDriver, mbx_step_for_driver};

use crate::OrchestratorError;

/// Build the single MBX Setup action; strict toolchain verification is
/// added centrally when the rendered bundle restore is appended.
pub(super) fn step(catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    let uses = PinnedActionRef::new(
        "jdx/mr-boxington-action",
        None,
        MR_BOXINGTON_ACTION_SHA,
        MR_BOXINGTON_ACTION_VERSION,
    )?
    .uses_value();
    mbx_step_for_driver(
        &uses,
        CompileDriver::Mbx,
        catalog.version(PinnedTool::MrBoxington),
    )?
    .ok_or_else(|| OrchestratorError::Contract {
        problem: "mbx_driver_did_not_emit_setup".to_owned(),
    })
}
