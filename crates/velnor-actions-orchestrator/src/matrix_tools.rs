// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{ProposedTask, Step, StepRole, WorkflowPolicy};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};

use crate::OrchestratorError;
use crate::crate_job_ids::CrateGroupKey;
use crate::crate_jobs::{is_nextest, is_opentofu, is_rust};
use crate::utf8::{strings_of, strings_of_env};

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
    BunExecution,
    GhExecution,
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

    pub(crate) fn owns_bun(self) -> bool {
        self.owns(SuiteTool::BunExecution)
    }

    const fn package_name(self) -> &'static str {
        self.name.package_name()
    }
}

const NO_SUITE_TOOLS: &[SuiteTool] = &[];
const GENERATE_VALIDATOR_TOOLS: &[SuiteTool] = &[SuiteTool::GenerateValidators];
const OPENTOFU_TOOLS: &[SuiteTool] = &[SuiteTool::OpenTofuExecution];
const GH_TOOLS: &[SuiteTool] = &[SuiteTool::GhExecution];
const ORCHESTRATOR_TOOLS: &[SuiteTool] = &[SuiteTool::GenerateValidators, SuiteTool::BunExecution, SuiteTool::GhExecution];

const SUITE_TOOL_OWNERS: [CrateSuite; 14] = [
    CrateSuite {
        name: SuiteName::Orchestrator,
        tools: ORCHESTRATOR_TOOLS,
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
        tools: GH_TOOLS,
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
fn prepare_install_opentofu(
    policy: WorkflowPolicy,
    suite: Option<CrateSuite>,
    use_opentofu: bool,
) -> bool {
    use_opentofu || crate_needs_tofu_install(policy, suite)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LaneTools(BTreeSet<PinnedTool>);

impl LaneTools {
    pub(crate) fn rust(catalog: &ToolCatalog) -> Self {
        let mut tools = BTreeSet::from([PinnedTool::Rust]);
        if catalog.rust_mr_boxington() {
            tools.insert(PinnedTool::MrBoxington);
        }
        Self(tools)
    }

    pub(crate) fn of_group(policy: WorkflowPolicy, catalog: &ToolCatalog, tasks: &[&ProposedTask]) -> Self {
        let suite = tasks
            .first()
            .and_then(|task| suite_for_package(&task.display_name));
        let mut tools = if tasks.iter().any(|task| is_rust(task)) {
            Self::rust(catalog).0
        } else {
            BTreeSet::new()
        };
        if crate_needs_generate_validators(policy, suite) {
            tools.extend([PinnedTool::Jactionlint, PinnedTool::Shellcheck]);
        }
        if tasks.iter().any(|task| is_nextest(task)) {
            tools.insert(PinnedTool::Nextest);
        }
        if prepare_install_opentofu(policy, suite, tasks.iter().any(|task| is_opentofu(task))) {
            tools.insert(PinnedTool::Opentofu);
        }
        if policy == WorkflowPolicy::VelnorRepositoryV1 && suite.is_some_and(CrateSuite::owns_bun) {
            tools.insert(PinnedTool::Bun);
        }
        if policy == WorkflowPolicy::VelnorRepositoryV1 && suite.is_some_and(|suite| suite.owns(SuiteTool::GhExecution)) {
            tools.insert(PinnedTool::Gh);
        }
        Self(tools)
    }

    pub(crate) fn union(
        policy: WorkflowPolicy,
        catalog: &ToolCatalog,
        groups: &BTreeMap<CrateGroupKey, Vec<&ProposedTask>>,
    ) -> Self {
        let mut union = Self::rust(catalog);
        for tasks in groups.values() {
            let group = Self::of_group(policy, catalog, tasks);
            if group.installs_rust() {
                union.0.extend(group.0);
            }
        }
        union
    }

    fn installs_rust(&self) -> bool {
        self.0.contains(&PinnedTool::Rust)
    }
}

pub(crate) fn verify_validators_step(catalog: &ToolCatalog, tools: &LaneTools) -> Option<Step> {
    tools
        .0
        .contains(&PinnedTool::Jactionlint)
        .then(|| crate::workflow::verify_jactionlint_step(catalog))
}

pub(crate) fn prepare_crate_tools_step(
    catalog: &ToolCatalog,
    tools: &LaneTools,
) -> Result<Step, OrchestratorError> {
    let prepare = PreparePinnedTools::new(tools.0.iter().copied().collect(), ToolHomes::runner_temp())
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let run = strings_of(prepare.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = if tools.installs_rust() {
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
#[path = "matrix_tools_tests.rs"]
mod tests;
