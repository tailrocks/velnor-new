// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod argv;
mod environment;
mod identity;
mod native_job_image;
mod toolchain;

use crate::canonical::valid_pinned_version;
use crate::errors::ContractError;

pub(in crate::workflow) struct TaskExecutionValidation<'a> {
    pub(in crate::workflow) argv: &'a [String],

    pub(in crate::workflow) env: &'a std::collections::BTreeMap<String, String>,

    pub(in crate::workflow) task_id: &'a str,

    pub(in crate::workflow) task_digest: &'a str,

    pub(in crate::workflow) toolchain_inputs: &'a crate::cachekey::ToolchainInputs,

    pub(in crate::workflow) matrix_id: &'a str,

    pub(in crate::workflow) matrix_key: &'a str,

    pub(in crate::workflow) report_helper_version: &'a str,

    pub(in crate::workflow) matrix_max_parallel: Option<u32>,

    pub(in crate::workflow) job: &'a str,
}

pub(in crate::workflow) fn validate_task_execution(
    input: &TaskExecutionValidation<'_>,
) -> Result<(), ContractError> {
    let invocation = argv::validate_invocation(input)?;
    if native_job_image::selected(input) {
        native_job_image::validate(input, &invocation)?;
    } else {
        toolchain::validate_toolchain(input, &invocation)?;
    }
    argv::validate_arguments(input)?;
    identity::validate_task_and_matrix(input)?;
    identity::validate_report_and_matrix_cap(input)?;
    environment::validate_environment(input)
}

fn pinned_tool_selector(selector: &str) -> Option<(&'static str, &str)> {
    let (tool, version) = selector.rsplit_once('@')?;
    let canonical_tool = match tool {
        "rust" => "rust",
        "mr-boxington" => "mr-boxington",
        "opentofu" => "opentofu",
        "aqua:nextest-rs/nextest/cargo-nextest" => "nextest-rs/nextest/cargo-nextest",
        _ => return None,
    };
    Some((canonical_tool, version))
}
