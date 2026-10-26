// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepRole, WorkflowPolicy};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

#[must_use]
pub(crate) fn task_driver_tools(use_rust: bool, use_opentofu: bool) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(use_rust.then_some(PinnedTool::Rust));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuiteName {
    Orchestrator,
    Cli,
    Contract,
    Mise,
    Rust,
    Tofu,
    Bun,
    Ansible,
    Dbt,
    Gradle,
    WorkflowRenderer,
    Actionlint,
    ArchiveGuard,
    Freshness,
}

impl SuiteName {
    const fn package_name(self) -> &'static str {
        match self {
            Self::Orchestrator => "velnor-actions-orchestrator",
            Self::Cli => "velnor-actions-cli",
            Self::Contract => "velnor-actions-contract",
            Self::Mise => "velnor-actions-mise",
            Self::Rust => "velnor-actions-rust",
            Self::Tofu => "velnor-actions-tofu",
            Self::Bun => "velnor-actions-bun",
            Self::Ansible => "velnor-actions-ansible",
            Self::Dbt => "velnor-actions-dbt",
            Self::Gradle => "velnor-actions-gradle",
            Self::WorkflowRenderer => "velnor-actions-workflow-renderer",
            Self::Actionlint => "velnor-actions-actionlint",
            Self::ArchiveGuard => "velnor-archive-guard",
            Self::Freshness => "velnor-actions-freshness",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuiteTool {
    GenerateValidators,
    OpenTofuExecution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CrateSuite {
    name: SuiteName,
    tools: &'static [SuiteTool],
}

impl CrateSuite {
    fn owns(self, tool: SuiteTool) -> bool {
        self.tools.contains(&tool)
    }

    const fn package_name(self) -> &'static str {
        self.name.package_name()
    }
}

const NO_SUITE_TOOLS: &[SuiteTool] = &[];
const GENERATE_VALIDATOR_TOOLS: &[SuiteTool] = &[SuiteTool::GenerateValidators];
const OPENTOFU_TOOLS: &[SuiteTool] = &[SuiteTool::OpenTofuExecution];

const SUITE_TOOL_OWNERS: [CrateSuite; 14] = [
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
        name: SuiteName::Bun,
        tools: NO_SUITE_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Ansible,
        tools: NO_SUITE_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Dbt,
        tools: NO_SUITE_TOOLS,
    },
    CrateSuite {
        name: SuiteName::Gradle,
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

#[must_use]
pub(crate) fn suite_for_package(package: &str) -> Option<CrateSuite> {
    SUITE_TOOL_OWNERS
        .iter()
        .copied()
        .find(|suite| suite.package_name() == package)
}

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

#[must_use]
pub(crate) fn crate_needs_tofu_install(policy: WorkflowPolicy, suite: Option<CrateSuite>) -> bool {
    match policy {
        WorkflowPolicy::ConsumerV1 => false,
        WorkflowPolicy::VelnorRepositoryV1 => {
            suite.is_some_and(|suite| suite.owns(SuiteTool::OpenTofuExecution))
        }
    }
}

#[must_use]
pub(crate) fn prepare_install_opentofu(
    policy: WorkflowPolicy,
    suite: Option<CrateSuite>,
    use_opentofu: bool,
) -> bool {
    use_opentofu || crate_needs_tofu_install(policy, suite)
}

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
    tools.extend((use_rust && catalog.rust_mr_boxington()).then_some(PinnedTool::MrBoxington));
    if needs_validators {
        tools.extend([PinnedTool::Jactionlint, PinnedTool::Shellcheck]);
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
