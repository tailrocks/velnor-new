//! Crate-job tool-set construction and typed suite tool ownership.
//!
//! Declared via `#[path]` from `matrix_step.rs` (no `lib.rs` edit);
//! `matrix_step` re-exports the constructors and suite resolver.

use velnor_actions_contract::{Step, StepRole, WorkflowPolicy};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

/// Crate-job Mise tools per role: Rust only when the job carries
/// rust obligations, plus Opentofu
/// when the job carries tofu obligations. Pure-tofu jobs install the
/// opentofu driver with no Rust setup; mixed jobs install the union.
#[must_use]
pub(crate) fn task_driver_tools(use_rust: bool, use_opentofu: bool) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(use_rust.then_some(PinnedTool::Rust));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

/// One workspace suite with its exact package identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuiteName {
    Orchestrator,
    Cli,
    Contract,
    Mise,
    Rust,
    Tofu,
    WorkflowRenderer,
    Actionlint,
    ArchiveGuard,
    Freshness,
}

impl SuiteName {
    /// Canonical Cargo package name for this suite.
    const fn package_name(self) -> &'static str {
        match self {
            Self::Orchestrator => "velnor-actions-orchestrator",
            Self::Cli => "velnor-actions-cli",
            Self::Contract => "velnor-actions-contract",
            Self::Mise => "velnor-actions-mise",
            Self::Rust => "velnor-actions-rust",
            Self::Tofu => "velnor-actions-tofu",
            Self::WorkflowRenderer => "velnor-actions-workflow-renderer",
            Self::Actionlint => "velnor-actions-actionlint",
            Self::ArchiveGuard => "velnor-archive-guard",
            Self::Freshness => "velnor-actions-freshness",
        }
    }
}

/// Suite-owned tool capability used to construct crate-job setup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuiteTool {
    GenerateValidators,
    OpenTofuExecution,
}

/// Typed suite identity plus the tools its executed tests require.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CrateSuite {
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

const NO_SUITE_TOOLS: &[SuiteTool] = &[];
const GENERATE_VALIDATOR_TOOLS: &[SuiteTool] = &[SuiteTool::GenerateValidators];
const OPENTOFU_TOOLS: &[SuiteTool] = &[SuiteTool::OpenTofuExecution];

/// Single source of truth for typed suite identities and owned tool needs.
const SUITE_TOOL_OWNERS: [CrateSuite; 10] = [
    CrateSuite {
        name: SuiteName::Orchestrator,
        tools: GENERATE_VALIDATOR_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Cli,
        tools: GENERATE_VALIDATOR_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Contract,
        tools: NO_SUITE_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Mise,
        tools: OPENTOFU_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Rust,
        tools: NO_SUITE_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Tofu,
        tools: NO_SUITE_TOOLS,
    },
    CrateSuite {
        name: SuiteName::WorkflowRenderer,
        tools: NO_SUITE_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Actionlint,
        tools: NO_SUITE_TOOLS,
    },
    CrateSuite {
        name: SuiteName::ArchiveGuard,
        tools: NO_SUITE_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Freshness,
        tools: NO_SUITE_TOOLS,
    },
];

/// Resolve one known workspace package to its typed suite and tool owners.
#[must_use]
pub(crate) fn suite_for_package(package: &str) -> Option<CrateSuite> {
    SUITE_TOOL_OWNERS
        .iter()
        .copied()
        .find(|suite| suite.package_name() == package)
}

/// Whether one crate job installs the `generate` validators.
///
/// Velnor-policy jobs trim by executed suite: only suites owning
/// [`SuiteTool::GenerateValidators`] install the trio; the rest install
/// drivers plus Nextest. The orchestrator and CLI suites own it because
/// their tests execute `generate`; other suites only assert argv.
/// Consumer suites are opaque to the generator, so consumer jobs keep the trio
/// fail-safe: dropping an install a suite needs fails CI with
/// `couldn't exec process` (run 36751323928), while an unneeded
/// install only costs seconds. Dedicated validator jobs remain the
/// lint gates for the committed workflow either way.
#[must_use]
pub(crate) fn crate_needs_generate_validators(
    policy: WorkflowPolicy,
    suite: Option<CrateSuite>,
) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => true,
        WorkflowPolicy::VelnorRepositoryV1 => {
            suite.is_some_and(|suite| suite.owns(SuiteTool::GenerateValidators))
        }
    }
}

/// Whether one crate job installs opentofu for its executed suite.
///
/// Velnor-policy jobs install only for suites owning
/// [`SuiteTool::OpenTofuExecution`]. Only mise executes the real-binary
/// tests; the orchestrator only constructs the command for env assertions.
/// Tofu obligations select the driver separately (see
/// [`prepare_install_opentofu`]). Consumer jobs never install here:
/// unlike the `generate` validators (fail-safe-true because
/// consumer suites CAN execute `generate`), consumer suites CANNOT
/// reach our `tofu_exec` ctor — the mise crate is not
/// published/consumable — so false is correct, not just cheaper.
#[must_use]
pub(crate) fn crate_needs_tofu_install(policy: WorkflowPolicy, suite: Option<CrateSuite>) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => false,
        WorkflowPolicy::VelnorRepositoryV1 => {
            suite.is_some_and(|suite| suite.owns(SuiteTool::OpenTofuExecution))
        }
    }
}

/// Effective opentofu install flag: tofu obligations or a tofu-spawning suite.
#[must_use]
pub(crate) fn prepare_install_opentofu(
    policy: WorkflowPolicy,
    suite: Option<CrateSuite>,
    use_opentofu: bool,
) -> bool {
    use_opentofu || crate_needs_tofu_install(policy, suite)
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
    let mut step =
        velnor_actions_workflow_renderer::ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)
            .map_err(OrchestratorError::from)?;
    step.role = Some(StepRole::PreparePinnedTools);
    Ok(step)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{SUITE_TOOL_OWNERS, suite_for_package};

    #[test]
    fn typed_tool_owner_table_matches_workspace_members() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir
            .parent()
            .and_then(Path::parent)
            .expect("crate lives two levels under the workspace root");
        let mut members = Vec::new();
        for entry in fs::read_dir(root.join("crates")).expect("crates dir lists") {
            let manifest = entry.expect("dir entry reads").path().join("Cargo.toml");
            if !manifest.is_file() {
                continue;
            }
            let text = fs::read_to_string(&manifest).expect("member manifest reads");
            let mut in_package = false;
            for line in text.lines() {
                if line.trim() == "[package]" {
                    in_package = true;
                } else if line.starts_with('[') {
                    in_package = false;
                } else if in_package && line.trim_start().starts_with("name = ") {
                    let name = line
                        .trim_start()
                        .trim_start_matches("name = ")
                        .trim()
                        .trim_matches('"');
                    members.push(name.to_owned());
                    break;
                }
            }
        }
        let mut registered: Vec<String> = SUITE_TOOL_OWNERS
            .iter()
            .map(|suite| suite.package_name().to_owned())
            .collect();
        members.sort();
        registered.sort();
        assert_eq!(
            members, registered,
            "typed owner rows must cover each crate"
        );
        assert!(
            members
                .iter()
                .all(|member| suite_for_package(member).is_some()),
            "every workspace package resolves to its typed suite"
        );
    }
}
