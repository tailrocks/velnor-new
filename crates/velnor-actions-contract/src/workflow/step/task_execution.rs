//! Validation for the structured task-execution step payload.

mod argv;
mod environment;
mod identity;
mod toolchain;

use crate::errors::ContractError;

/// Borrowed fields that jointly define one typed task-execution step.
pub(in crate::workflow) struct TaskExecutionValidation<'a> {
    /// Fixed, element-preserving invocation arguments.
    pub(in crate::workflow) argv: &'a [String],
    /// Fixed task environment.
    pub(in crate::workflow) env: &'a std::collections::BTreeMap<String, String>,
    /// Stable task report lookup ID.
    pub(in crate::workflow) task_id: &'a str,
    /// Digest binding the task arguments and toolchain.
    pub(in crate::workflow) task_digest: &'a str,
    /// Pinned toolchain selectors and compile/test-driver identities.
    pub(in crate::workflow) toolchain_inputs: &'a crate::cachekey::ToolchainInputs,
    /// Stable task matrix identity.
    pub(in crate::workflow) matrix_id: &'a str,
    /// Short matrix lookup key.
    pub(in crate::workflow) matrix_key: &'a str,
    /// Version of the staged report helper.
    pub(in crate::workflow) report_helper_version: &'a str,
    /// Optional maximum parallelism for the containing crate-job matrix.
    pub(in crate::workflow) matrix_max_parallel: Option<u32>,
    /// Job identity used to scope validation errors.
    pub(in crate::workflow) job: &'a str,
}

pub(in crate::workflow) fn validate_task_execution(
    input: &TaskExecutionValidation<'_>,
) -> Result<(), ContractError> {
    let invocation = argv::validate_invocation(input)?;
    toolchain::validate_toolchain(input, &invocation)?;
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

fn valid_pinned_version(version: &str) -> bool {
    !version.is_empty()
        && version.bytes().any(|byte| byte.is_ascii_digit())
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-' | b'_'))
        && !version.eq_ignore_ascii_case("latest")
}

#[cfg(test)]
mod pinned_tool_selector_tests {
    use super::{pinned_tool_selector, valid_pinned_version};

    const NEXTEST: &str = "nextest-rs/nextest/cargo-nextest";

    #[test]
    fn accepts_only_the_catalog_backend_qualified_nextest_selector() {
        assert_eq!(
            pinned_tool_selector("aqua:nextest-rs/nextest/cargo-nextest@0.9.148"),
            Some((NEXTEST, "0.9.148")),
        );
        assert_eq!(
            pinned_tool_selector("nextest-rs/nextest/cargo-nextest@0.9.148"),
            None,
            "the backend-qualified catalog identity is required"
        );
        assert_eq!(
            pinned_tool_selector("github:nextest-rs/nextest/cargo-nextest@0.9.148"),
            None,
            "other backends are outside the pinned catalog"
        );
        let (_, version) = pinned_tool_selector("aqua:nextest-rs/nextest/cargo-nextest@latest")
            .expect("the selector has the recognized backend and tool name");
        assert!(
            !valid_pinned_version(version),
            "floating versions stay rejected"
        );
        assert_eq!(
            pinned_tool_selector("aqua:nextest-rs/nextest/cargo-nextest"),
            None,
            "the selector must carry an exact version"
        );
    }
}
