//! Audited repository suite owners and their executed tool requirements.

use velnor_actions_contract::{ProposedTask, Stack, WorkflowPolicy};

use crate::OrchestratorError;

/// Tools spawned by a compiled repository suite, beyond obligation drivers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SuiteTools {
    pub(crate) generate_validators: bool,
    pub(crate) opentofu: bool,
}

impl SuiteTools {
    const NONE: Self = Self {
        generate_validators: false,
        opentofu: false,
    };
}

/// One registry owns both execution axes. New suite owners require an audit.
/// Native proposals depend only on contract and cannot spawn subprocesses;
/// their ownership gate enforces that boundary.
fn registered_suite_tools(package: &str) -> Option<SuiteTools> {
    match package {
        "velnor-actions-orchestrator" | "velnor-actions-cli" => Some(SuiteTools {
            generate_validators: true,
            opentofu: false,
        }),
        "velnor-actions-mise" => Some(SuiteTools {
            generate_validators: false,
            opentofu: true,
        }),
        "velnor-actions-contract"
        | "velnor-actions-native"
        | "velnor-actions-rust"
        | "velnor-actions-tofu"
        | "velnor-actions-workflow-renderer"
        | "velnor-actions-actionlint"
        | "velnor-runner-cli"
        | "velnor-runner-core"
        | "velnor-runner-github"
        | "velnor-runner-host" => Some(SuiteTools::NONE),
        _ => None,
    }
}

/// Resolve the suite's audited tools before constructing a crate job.
/// Non-Rust obligations have no compiled suite; their drivers are selected
/// separately. Consumer suites remain opaque and retain validator tools.
///
/// # Errors
/// Rejects an unregistered compiled suite under the repository policy.
pub(crate) fn crate_suite_tools(
    policy: WorkflowPolicy,
    rust_package: Option<&str>,
) -> Result<SuiteTools, OrchestratorError> {
    match policy {
        WorkflowPolicy::ConsumerV1 => Ok(SuiteTools {
            generate_validators: true,
            opentofu: false,
        }),
        WorkflowPolicy::VelnorRepositoryV1 => {
            rust_package.map_or(Ok(SuiteTools::NONE), |package| {
                registered_suite_tools(package).ok_or_else(|| OrchestratorError::Contract {
                    problem: format!("unclassified_repository_suite:{package}"),
                })
            })
        }
    }
}

/// Resolve the compiled Rust owner independently of the group's first task.
///
/// # Errors
/// Rejects inconsistent Rust owners and unregistered repository suites.
pub(crate) fn suite_tools_for_tasks(
    policy: WorkflowPolicy,
    tasks: &[&ProposedTask],
) -> Result<SuiteTools, OrchestratorError> {
    let mut rust_package = None;
    for task in tasks
        .iter()
        .filter(|task| Stack::from_id(&task.stack_id) == Some(Stack::Rust))
    {
        if rust_package.is_some_and(|package| package != task.display_name) {
            return Err(OrchestratorError::Contract {
                problem: "inconsistent_repository_suite_owner".to_owned(),
            });
        }
        rust_package = Some(task.display_name.as_str());
    }
    crate_suite_tools(policy, rust_package)
}

#[cfg(test)]
#[path = "matrix_suite_tests.rs"]
mod tests;
