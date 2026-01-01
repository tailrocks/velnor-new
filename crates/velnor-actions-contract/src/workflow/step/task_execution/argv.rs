use std::collections::BTreeSet;

use crate::errors::ContractError;

use super::{TaskExecutionValidation, pinned_tool_selector, valid_pinned_version};

pub(super) struct Invocation {
    pub(super) separator: usize,
    pub(super) selectors: BTreeSet<&'static str>,
}

pub(super) fn validate_invocation(
    input: &TaskExecutionValidation<'_>,
) -> Result<Invocation, ContractError> {
    let argv = input.argv;
    if argv.len() < 8
        || argv.len() > super::super::MAX_TASK_EXECUTION_ARGV
        || input.env.len() > super::super::MAX_TASK_EXECUTION_ENV
        || argv[0] != "mise"
        || argv[1..4] != ["--no-config", "--no-env", "--no-hooks"]
        || argv[4] != "exec"
    {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("bad_mise_exec:{}", input.job),
        ));
    }
    let Some(separator) = argv[5..].iter().position(|arg| arg == "--").map(|i| i + 5) else {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("missing_payload_separator:{}", input.job),
        ));
    };
    if separator == 5 || separator + 1 >= argv.len() {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("bad_payload_boundary:{}", input.job),
        ));
    }
    let mut selectors = BTreeSet::new();
    for selector in &argv[5..separator] {
        let Some((tool, version)) = pinned_tool_selector(selector) else {
            return Err(ContractError::identity(
                "step.task.argv",
                format!("unversioned_tool_selector:{}", input.job),
            ));
        };
        if !valid_pinned_version(version) || !selectors.insert(tool) {
            return Err(ContractError::identity(
                "step.task.argv",
                format!("invalid_tool_selector:{}", input.job),
            ));
        }
    }
    if selectors.contains("opentofu") && (selectors.len() != 1 || argv[separator + 1] != "tofu") {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("tofu_tool_payload_mismatch:{}", input.job),
        ));
    }
    if !selectors.contains("opentofu")
        && !selectors.contains("rust")
        && !selectors.contains("mr-boxington")
    {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("missing_compile_tool:{}", input.job),
        ));
    }
    Ok(Invocation {
        separator,
        selectors,
    })
}

pub(super) fn validate_arguments(input: &TaskExecutionValidation<'_>) -> Result<(), ContractError> {
    for arg in input.argv {
        if arg.is_empty() || arg.chars().any(|ch| ch == '\0' || ch == '\n' || ch == '\r') {
            return Err(ContractError::identity(
                "step.task.argv",
                format!("invalid_argument:{}", input.job),
            ));
        }
    }
    Ok(())
}
