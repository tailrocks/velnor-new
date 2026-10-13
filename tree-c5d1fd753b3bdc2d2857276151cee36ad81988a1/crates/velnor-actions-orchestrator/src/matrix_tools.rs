//! Crate-job tool-set construction: drivers plus validator trio.
//!
//! Declared via `#[path]` from `matrix_step.rs` (no `lib.rs` edit);
//! `matrix_step` re-exports the constructors so call sites stay put.

use velnor_actions_contract::Step;
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

#[path = "matrix_suite.rs"]
mod suite;
#[cfg(test)]
pub(crate) use suite::crate_suite_tools;
pub(crate) use suite::{SuiteTools, suite_tools_for_tasks};

/// Crate-job driver tools per role: Rust only when the job carries
/// rust obligations, plus MBX only on MBX evidence, plus Opentofu
/// when the job carries tofu obligations. Pure-tofu jobs install the
/// opentofu driver with no Rust setup; mixed jobs install the union.
#[must_use]
pub(crate) fn task_driver_tools(
    use_rust: bool,
    use_mbx: bool,
    use_opentofu: bool,
) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(use_rust.then_some(PinnedTool::Rust));
    tools.extend(use_mbx.then_some(PinnedTool::MrBoxington));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

/// Typed `Prepare pinned tools` step for the crate-job tool set.
///
/// Driver toolchain per role (Rust only for rust obligations, plus
/// Opentofu for tofu jobs) plus Nextest when used, plus the
/// `generate` validators only when `needs_validators` holds (see
/// [`suite_tools_for_tasks`]). The set is exact and pinned
/// by test: drivers, conditional validators, optional Nextest,
/// nothing else. Pure-tofu roles carry no owned-homes triple in the
/// step env; every other role keeps it.
///
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "five independent install flags mirror the driver selection"
)]
pub(crate) fn prepare_crate_tools_step(
    catalog: &ToolCatalog,
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    needs_validators: bool,
) -> Result<Step, OrchestratorError> {
    let mut tools = task_driver_tools(use_rust, use_mbx, use_opentofu);
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
