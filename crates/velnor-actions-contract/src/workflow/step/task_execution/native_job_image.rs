// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{ContractError, JOB_IMAGE_CANDIDATE_DRIVER, JOB_IMAGE_CANDIDATE_TASK_ID};

use super::{
    TaskExecutionValidation, argv::Invocation, pinned_tool_selector, valid_pinned_version,
};

pub(super) fn selected(input: &TaskExecutionValidation<'_>) -> bool {
    input.task_id == JOB_IMAGE_CANDIDATE_TASK_ID
        || input.toolchain_inputs.compile_driver == JOB_IMAGE_CANDIDATE_DRIVER
        || input.toolchain_inputs.test_runner == JOB_IMAGE_CANDIDATE_DRIVER
}

pub(super) fn validate(
    input: &TaskExecutionValidation<'_>,
    invocation: &Invocation,
) -> Result<(), ContractError> {
    if input.task_id != JOB_IMAGE_CANDIDATE_TASK_ID
        || input.toolchain_inputs.compile_driver != JOB_IMAGE_CANDIDATE_DRIVER
        || input.toolchain_inputs.test_runner != JOB_IMAGE_CANDIDATE_DRIVER
        || input.toolchain_inputs.components != ["clippy", "rustfmt"]
        || invocation.selectors.iter().copied().collect::<Vec<_>>() != ["rust"]
        || input.toolchain_inputs.tools.len() != 2
    {
        return Err(invalid("identity"));
    }
    let mut tools = Vec::new();
    for selector in &input.toolchain_inputs.tools {
        let (tool, version) = pinned_tool_selector(selector).ok_or_else(|| invalid("toolchain"))?;
        if !valid_pinned_version(version) {
            return Err(invalid("toolchain"));
        }
        tools.push(tool);
    }
    tools.sort_unstable();
    if tools != ["mr-boxington", "rust"] || !input.toolchain_inputs.tools.contains(&input.argv[5]) {
        return Err(invalid("toolchain"));
    }
    let rust = input
        .toolchain_inputs
        .tools
        .iter()
        .find(|value| value.starts_with("rust@"))
        .ok_or_else(|| invalid("toolchain"))?;
    let mr_boxington = input
        .toolchain_inputs
        .tools
        .iter()
        .find(|value| value.starts_with("mr-boxington@"))
        .ok_or_else(|| invalid("toolchain"))?;
    let export_rustup = match input.argv.get(15).map(String::as_str) {
        Some("export") => true,
        Some("no-export") => false,
        _ => return Err(invalid("rustup_export")),
    };
    let inputs = crate::JobImageCandidateInputs {
        rust: rust.clone(),
        mr_boxington: mr_boxington.clone(),
        export_rustup,
    };
    crate::image_candidate::validate_job_image_candidate_operation(input.argv, input.env, &inputs)
}

fn invalid(detail: &str) -> ContractError {
    ContractError::identity("step.job_image_candidate", detail)
}
