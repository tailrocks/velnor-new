use std::collections::BTreeSet;

use crate::errors::ContractError;

use super::{
    TaskExecutionValidation, argv::Invocation, pinned_tool_selector, valid_pinned_version,
};

#[derive(Clone, Copy)]
enum CompileDriver {
    Cargo,
    Mbx,
}

impl CompileDriver {
    fn program(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    fn tool_names(self) -> Vec<&'static str> {
        match self {
            Self::Cargo => vec!["rust"],
            Self::Mbx => vec!["mr-boxington", "rust"],
        }
    }
}

pub(super) fn validate_toolchain(
    input: &TaskExecutionValidation<'_>,
    invocation: &Invocation,
) -> Result<(), ContractError> {
    validate_invocation_toolchain_membership(input, invocation)?;
    let toolchain_selectors = validate_toolchain_pins(input, invocation)?;
    let driver = validate_compile_driver(input, invocation)?;
    validate_toolchain_selection(input, &toolchain_selectors, driver)?;
    validate_invocation_selection(input, invocation)?;
    Ok(())
}

fn validate_invocation_toolchain_membership(
    input: &TaskExecutionValidation<'_>,
    invocation: &Invocation,
) -> Result<(), ContractError> {
    for selector in &input.argv[5..invocation.separator] {
        if !input.toolchain_inputs.tools.contains(selector) {
            return Err(ContractError::identity(
                "step.task.toolchain",
                format!("selector_not_in_toolchain:{}", input.job),
            ));
        }
    }
    Ok(())
}

fn validate_toolchain_pins(
    input: &TaskExecutionValidation<'_>,
    invocation: &Invocation,
) -> Result<BTreeSet<&'static str>, ContractError> {
    let toolchain_selectors = input
        .toolchain_inputs
        .tools
        .iter()
        .map(|selector| {
            let Some((tool, version)) = pinned_tool_selector(selector) else {
                return Err(ContractError::identity(
                    "step.task.toolchain",
                    format!("invalid_toolchain_pin:{}", input.job),
                ));
            };
            if !valid_pinned_version(version) {
                return Err(ContractError::identity(
                    "step.task.toolchain",
                    format!("invalid_toolchain_pin:{}", input.job),
                ));
            }
            Ok(tool)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if toolchain_selectors.len() != input.toolchain_inputs.tools.len()
        || invocation.selectors.len() != input.argv[5..invocation.separator].len()
        || input
            .toolchain_inputs
            .components
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            != ["clippy", "rustfmt"]
    {
        return Err(ContractError::identity(
            "step.task.toolchain",
            format!("noncanonical_rust_toolchain:{}", input.job),
        ));
    }
    Ok(toolchain_selectors)
}

fn validate_compile_driver(
    input: &TaskExecutionValidation<'_>,
    invocation: &Invocation,
) -> Result<CompileDriver, ContractError> {
    let driver = match input.toolchain_inputs.compile_driver.as_str() {
        "cargo" => CompileDriver::Cargo,
        "mbx" => CompileDriver::Mbx,
        _ => {
            return Err(ContractError::identity(
                "step.task.toolchain",
                format!("unknown_compile_driver:{}", input.job),
            ));
        }
    };
    if input.argv[invocation.separator + 1] != driver.program() {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("program_driver_mismatch:{}", input.job),
        ));
    }
    Ok(driver)
}

fn validate_toolchain_selection(
    input: &TaskExecutionValidation<'_>,
    toolchain_selectors: &BTreeSet<&'static str>,
    driver: CompileDriver,
) -> Result<(), ContractError> {
    let mut expected_tool_names = driver.tool_names();
    if input.toolchain_inputs.test_runner == "cargo_nextest" {
        expected_tool_names.push("nextest-rs/nextest/cargo-nextest");
    }
    expected_tool_names.sort_unstable();
    if toolchain_selectors.iter().copied().collect::<Vec<_>>() != expected_tool_names {
        return Err(ContractError::identity(
            "step.task.toolchain",
            format!("toolchain_driver_mismatch:{}", input.job),
        ));
    }
    match input.toolchain_inputs.test_runner.as_str() {
        "cargo_test" if toolchain_selectors.contains("nextest-rs/nextest/cargo-nextest") => {
            Err(ContractError::identity(
                "step.task.toolchain",
                format!("unexpected_nextest_pin:{}", input.job),
            ))
        }
        "cargo_nextest" if !toolchain_selectors.contains("nextest-rs/nextest/cargo-nextest") => {
            Err(ContractError::identity(
                "step.task.toolchain",
                format!("missing_nextest_pin:{}", input.job),
            ))
        }
        "cargo_test" | "cargo_nextest" => Ok(()),
        _ => Err(ContractError::identity(
            "step.task.toolchain",
            format!("unknown_test_runner:{}", input.job),
        )),
    }
}

fn validate_invocation_selection(
    input: &TaskExecutionValidation<'_>,
    invocation: &Invocation,
) -> Result<(), ContractError> {
    let mut expected_invocation_tools = vec!["rust"];
    if input.toolchain_inputs.test_runner == "cargo_nextest" {
        expected_invocation_tools.push("nextest-rs/nextest/cargo-nextest");
    }
    expected_invocation_tools.sort_unstable();
    if invocation.selectors.iter().copied().collect::<Vec<_>>() != expected_invocation_tools {
        return Err(ContractError::identity(
            "step.task.toolchain",
            format!("unbound_task_selector:{}", input.job),
        ));
    }
    Ok(())
}
