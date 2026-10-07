//! Crate-job tool-set construction: drivers plus validator trio.
//!
//! Declared via `#[path]` from `matrix_step.rs` (no `lib.rs` edit);
//! `matrix_step` re-exports the constructors so call sites stay put.

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::{Step, StepRole};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::utf8::{strings_of, strings_of_env};

/// Crate-job Mise tools per role: Rust only when the job carries
/// rust obligations, plus Opentofu
/// when the job carries tofu obligations. Pure-tofu jobs install the
/// opentofu driver with no Rust setup; mixed jobs install the union.
#[must_use]
pub fn task_driver_tools(use_rust: bool, use_opentofu: bool) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(use_rust.then_some(PinnedTool::Rust));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

/// Velnor-repository suites that shell out to the `generate` validators.
///
/// The orchestrator suite (hub `generate` cases), the generation and
/// plan suites (both call full `generate`, which always runs staged
/// validation), and the CLI suite (parity and the harness spawn
/// `generate`) execute the trio; every other suite only asserts argv,
/// never spawns validators.
///
/// Membership is enforced behaviorally, not by audit: the
/// classification test scans every workspace member's suite sources
/// for trio-execution markers and requires the install decision to
/// match. A suite that starts spawning validators fails the test
/// until it joins this list; a listed suite that stops spawning fails
/// until it leaves.
const GENERATE_VALIDATOR_SUITES: [&str; 4] = [
    "velnor-actions-orchestrator",
    "velnor-actions-orchestrator-generation",
    "velnor-actions-orchestrator-plan",
    "velnor-actions-cli",
];

/// Whether one crate job installs the `generate` validators.
///
/// Velnor-policy jobs trim by executed suite: only the suites above
/// install the trio, the rest install drivers plus Nextest. Consumer
/// suites are opaque to the generator, so consumer jobs keep the trio
/// fail-safe: dropping an install a suite needs fails CI with
/// `couldn't exec process` (run 36751323928), while an unneeded
/// install only costs seconds. Dedicated validator jobs remain the
/// lint gates for the committed workflow either way.
#[must_use]
pub fn crate_needs_generate_validators(policy: WorkflowPolicy, package: &str) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => true,
        WorkflowPolicy::VelnorRepositoryV1 => GENERATE_VALIDATOR_SUITES.contains(&package),
    }
}

/// Velnor-repository suites that spawn real `tofu` binaries.
///
/// Only the mise suite executes `tofu` (the T27 real-binary runs
/// via `IsolatedCommand::tofu_exec` plus `run_bounded`); the
/// orchestrator suite only constructs the ctor to assert its env
/// (zero `run_*` calls), and no other suite touches `tofu_exec` or
/// `VELNOR_LIVE_TOFU` at all.
///
/// Membership is enforced behaviorally, not by audit: the
/// classification test scans every workspace member's suite sources
/// for `tofu_exec` plus `run_bounded`/`run_cancellable` in the same
/// file and requires the install decision to match. Ctor-only and
/// env-assertion uses never trip the pair, so they stay trimmed.
const TOFU_EXEC_SUITES: [&str; 1] = ["velnor-actions-mise"];

/// Whether one crate job installs opentofu for its executed suite.
///
/// Velnor-policy jobs install only for the suite above; tofu
/// obligations select the driver separately (see
/// [`prepare_install_opentofu`]). Consumer jobs never install here:
/// unlike the `generate` validators (fail-safe-true because
/// consumer suites CAN execute `generate`), consumer suites CANNOT
/// reach our `tofu_exec` ctor — the mise crate is not
/// published/consumable — so false is correct, not just cheaper.
#[must_use]
pub(crate) fn crate_needs_tofu_install(policy: WorkflowPolicy, package: &str) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => false,
        WorkflowPolicy::VelnorRepositoryV1 => TOFU_EXEC_SUITES.contains(&package),
    }
}

/// Effective opentofu install flag: tofu obligations or a tofu-spawning suite.
#[must_use]
pub fn prepare_install_opentofu(policy: WorkflowPolicy, package: &str, use_opentofu: bool) -> bool {
    use_opentofu || crate_needs_tofu_install(policy, package)
}

/// Typed `Prepare pinned tools` step for the crate-job tool set.
///
/// Driver toolchain per role (Rust only for rust obligations, plus
/// Opentofu for tofu jobs) plus Nextest when used, plus the
/// `generate` validators only when `needs_validators` holds (see
/// [`crate_needs_generate_validators`]). The set is exact and pinned
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
pub fn prepare_crate_tools_step(
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
    let mut step =
        velnor_actions_workflow_steps::ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)
            .map_err(OrchestratorError::from)?;
    step.role = Some(StepRole::PreparePinnedTools);
    Ok(step)
}
