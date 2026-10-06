//! Audited compiled repository suites and their executed tool requirements.

use velnor_actions_contract::WorkflowPolicy;

use crate::OrchestratorError;

/// Tools required by the compiled test suite, beyond obligation drivers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SuiteTools {
    pub(crate) generate_validators: bool,
    pub(crate) opentofu: bool,
    pub(crate) python: bool,
}

impl SuiteTools {
    pub(crate) const NONE: Self = Self {
        generate_validators: false,
        opentofu: false,
        python: false,
    };
}

/// One registry owns all suite tool axes; new owners require an execution audit.
fn registered_suite_tools(package: &str) -> Option<SuiteTools> {
    match package {
        "velnor-actions-orchestrator" | "velnor-actions-cli" => Some(SuiteTools {
            generate_validators: true,
            ..SuiteTools::NONE
        }),
        "velnor-actions-mise" => Some(SuiteTools {
            opentofu: true,
            ..SuiteTools::NONE
        }),
        "velnor-actions-native" => Some(SuiteTools {
            python: true,
            ..SuiteTools::NONE
        }),
        "velnor-actions-contract"
        | "velnor-actions-rust"
        | "velnor-actions-tofu"
        | "velnor-actions-workflow-renderer"
        | "velnor-actions-actionlint" => Some(SuiteTools::NONE),
        _ => None,
    }
}

/// Resolve audited suite tools before preparation constructs its source identity.
/// Non-Rust obligations carry no compiled suite; consumers retain validator tools.
///
/// # Errors
/// Rejects unregistered repository suites instead of silently trimming their tools.
pub(crate) fn crate_suite_tools(
    policy: WorkflowPolicy,
    rust_package: Option<&str>,
) -> Result<SuiteTools, OrchestratorError> {
    match policy {
        WorkflowPolicy::ConsumerV1 => Ok(SuiteTools {
            generate_validators: true,
            ..SuiteTools::NONE
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

#[cfg(test)]
#[path = "matrix_suite_tests.rs"]
mod tests;
