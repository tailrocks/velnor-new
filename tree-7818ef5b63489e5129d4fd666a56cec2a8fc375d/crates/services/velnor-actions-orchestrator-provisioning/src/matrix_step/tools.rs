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

/// One Velnor suite's canonical package identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuiteName {
    Orchestrator,
    Generation,
    Plan,
    Cli,
    Mise,
}

impl SuiteName {
    /// Canonical Cargo package name for this suite.
    const fn package_name(self) -> &'static str {
        match self {
            Self::Orchestrator => "velnor-actions-orchestrator",
            Self::Generation => "velnor-actions-orchestrator-generation",
            Self::Plan => "velnor-actions-orchestrator-plan",
            Self::Cli => "velnor-actions-cli",
            Self::Mise => "velnor-actions-mise",
        }
    }
}

/// An installable tool whose runtime use belongs to a suite.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuiteTool {
    GenerateValidators,
    OpenTofuExecution,
}

/// Typed suite identity and the tools its executed tests require.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CrateSuite {
    name: SuiteName,
    tools: &'static [SuiteTool],
}

impl CrateSuite {
    /// Whether this suite owns an execution that requires one installed tool.
    fn owns(self, tool: SuiteTool) -> bool {
        self.tools.contains(&tool)
    }

    /// Canonical Cargo package name for this suite.
    const fn package_name(self) -> &'static str {
        self.name.package_name()
    }
}

const GENERATE_VALIDATOR_TOOLS: &[SuiteTool] = &[SuiteTool::GenerateValidators];
const OPENTOFU_EXECUTION_TOOLS: &[SuiteTool] = &[SuiteTool::OpenTofuExecution];

/// Single source of truth for the suites that own externally installed tools.
///
/// Suites absent from this table own neither tool in `VelnorRepositoryV1`.
/// Workspace-wide behavior tests scan every current suite to keep that default
/// honest as packages and suite implementations change.
const TOOL_OWNING_SUITES: [CrateSuite; 5] = [
    CrateSuite {
        name: SuiteName::Orchestrator,
        tools: GENERATE_VALIDATOR_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Generation,
        tools: GENERATE_VALIDATOR_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Plan,
        tools: GENERATE_VALIDATOR_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Cli,
        tools: GENERATE_VALIDATOR_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Mise,
        tools: OPENTOFU_EXECUTION_TOOLS,
    },
];

/// Resolve one tool-owning workspace package to its typed suite record.
#[must_use]
fn suite_for_package(package: &str) -> Option<CrateSuite> {
    TOOL_OWNING_SUITES
        .iter()
        .copied()
        .find(|suite| suite.package_name() == package)
}

/// Whether one crate job installs the `generate` validators.
///
/// Velnor-policy jobs trim by executed suite: only suites owning
/// [`SuiteTool::GenerateValidators`] install the trio; the rest install
/// drivers plus Nextest. Consumer
/// suites are opaque to the generator, so consumer jobs keep the trio
/// fail-safe: dropping an install a suite needs fails CI with
/// `couldn't exec process` (run 36751323928), while an unneeded
/// install only costs seconds. Dedicated validator jobs remain the
/// lint gates for the committed workflow either way.
#[must_use]
pub fn crate_needs_generate_validators(policy: WorkflowPolicy, package: &str) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => true,
        WorkflowPolicy::VelnorRepositoryV1 => suite_for_package(package)
            .is_some_and(|suite| suite.owns(SuiteTool::GenerateValidators)),
    }
}

/// Whether one crate job installs opentofu for its executed suite.
///
/// Velnor-policy jobs install only for suites owning
/// [`SuiteTool::OpenTofuExecution`]. Tofu obligations select the driver separately (see
/// [`prepare_install_opentofu`]). Consumer jobs never install here:
/// unlike the `generate` validators (fail-safe-true because
/// consumer suites CAN execute `generate`), consumer suites CANNOT
/// reach our `tofu_exec` ctor — the mise crate is not
/// published/consumable — so false is correct, not just cheaper.
#[must_use]
pub(crate) fn crate_needs_tofu_install(policy: WorkflowPolicy, package: &str) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => false,
        WorkflowPolicy::VelnorRepositoryV1 => {
            suite_for_package(package).is_some_and(|suite| suite.owns(SuiteTool::OpenTofuExecution))
        }
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

#[cfg(test)]
mod tests {
    use super::{SuiteName, SuiteTool, TOOL_OWNING_SUITES, suite_for_package};

    #[test]
    fn typed_tool_owner_records_resolve_unique_package_names() {
        for (index, suite) in TOOL_OWNING_SUITES.iter().enumerate() {
            assert_eq!(
                suite_for_package(suite.package_name()),
                Some(*suite),
                "each typed owner must resolve from its public package key"
            );
            assert!(
                !TOOL_OWNING_SUITES[..index]
                    .iter()
                    .any(|previous| previous.package_name() == suite.package_name()),
                "a package cannot own duplicate typed suite rows"
            );
        }

        assert!(suite_for_package("velnor-actions-orchestrator-core").is_none());
        assert_eq!(
            suite_for_package(SuiteName::Orchestrator.package_name())
                .map(|suite| suite.owns(SuiteTool::GenerateValidators)),
            Some(true)
        );
        assert_eq!(
            suite_for_package(SuiteName::Mise.package_name())
                .map(|suite| suite.owns(SuiteTool::OpenTofuExecution)),
            Some(true)
        );
    }
}
