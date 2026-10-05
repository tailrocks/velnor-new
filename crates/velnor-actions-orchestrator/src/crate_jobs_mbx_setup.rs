//! MBX's pinned local setup action for a crate job.

use velnor_actions_actionlint::{
    PinnedActionRef,
    actions::{MR_BOXINGTON_ACTION_SHA, MR_BOXINGTON_ACTION_VERSION},
};
use velnor_actions_contract::Step;
use velnor_actions_mise::{PinnedTool, ToolCatalog, ToolHomes};
use velnor_actions_workflow_renderer::steps::{CompileDriver, mbx_steps_for_driver};

use crate::OrchestratorError;

/// Build the exact toolchain preflight and MBX objects action pair.
pub(crate) fn steps(catalog: &ToolCatalog) -> Result<[Step; 2], OrchestratorError> {
    let uses = PinnedActionRef::new(
        "jdx/mr-boxington-action",
        None,
        MR_BOXINGTON_ACTION_SHA,
        MR_BOXINGTON_ACTION_VERSION,
    )?
    .uses_value();
    let env = crate::utf8::strings_of_env(&ToolHomes::runner_temp().env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    mbx_steps_for_driver(
        &uses,
        CompileDriver::Mbx,
        catalog.version(PinnedTool::MrBoxington),
        &catalog.rustup_toolchain(),
        env,
    )?
    .ok_or_else(|| OrchestratorError::Contract {
        problem: "mbx_driver_did_not_emit_setup_pair".to_owned(),
    })
}
