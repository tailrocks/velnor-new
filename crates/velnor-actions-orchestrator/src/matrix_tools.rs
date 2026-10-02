//! Crate-job tool-set construction: drivers plus validator trio.
//!
//! Declared via `#[path]` from `matrix_step.rs` (no `lib.rs` edit);
//! `matrix_step` re-exports the constructors so call sites stay put.

use velnor_actions_contract::{Step, WorkflowPolicy};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

/// Crate-job driver tools: Rust plus MBX only on MBX evidence, plus
/// Opentofu when the job carries tofu obligations (a later per-role
/// task removes the Rust setup from pure-tofu jobs; this arm only
/// adds the missing driver).
#[must_use]
pub(crate) fn task_driver_tools(use_mbx: bool, use_opentofu: bool) -> Vec<PinnedTool> {
    let mut tools = vec![PinnedTool::Rust];
    tools.extend(use_mbx.then_some(PinnedTool::MrBoxington));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

/// Velnor-repository suites that shell out to the `generate` validators.
///
/// Only the orchestrator suite (validating `generate` plus zizmor
/// staging) and the CLI suite (parity runs `generate`) execute the
/// trio; every other suite only asserts argv, never spawns validators.
///
/// Re-audit when a suite starts spawning validators: grep its tests
/// for `generate()` executions and `PinnedToolExec` trio runs
/// (actionlint, shellcheck, zizmor); a suite that executes any of
/// them joins this list, anything else stays trimmed. Adding a
/// workspace crate fails `every_workspace_member_is_classified`
/// until it is classified here or in the trimmed set.
const GENERATE_VALIDATOR_SUITES: [&str; 2] = ["velnor-actions-orchestrator", "velnor-actions-cli"];

/// Whether one crate job installs the `generate` validators.
///
/// Velnor-policy jobs trim by executed suite: only the two suites above
/// install the trio, the rest install drivers plus Nextest. Consumer
/// suites are opaque to the generator, so consumer jobs keep the trio
/// fail-safe: dropping an install a suite needs fails CI with
/// `couldn't exec process` (run 36751323928), while an unneeded
/// install only costs seconds. Dedicated validator jobs remain the
/// lint gates for the committed workflow either way.
#[must_use]
pub(crate) fn crate_needs_generate_validators(policy: WorkflowPolicy, package: &str) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => true,
        WorkflowPolicy::VelnorRepositoryV1 => GENERATE_VALIDATOR_SUITES.contains(&package),
    }
}

/// Typed `Prepare pinned tools` step for the crate-job tool set.
///
/// Driver toolchain (plus Opentofu for tofu jobs) plus Nextest when
/// used, plus the `generate` validators only when `needs_validators`
/// holds (see [`crate_needs_generate_validators`]). The set is exact
/// and pinned by test: drivers, conditional validators, optional
/// Nextest, nothing else.
///
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "four independent install flags mirror the driver selection"
)]
pub(crate) fn prepare_crate_tools_step(
    catalog: &ToolCatalog,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    needs_validators: bool,
) -> Result<Step, OrchestratorError> {
    let mut tools = task_driver_tools(use_mbx, use_opentofu);
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
    let env = strings_of_env(&prepare.env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    velnor_actions_workflow_renderer::ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)
        .map_err(OrchestratorError::from)
}
