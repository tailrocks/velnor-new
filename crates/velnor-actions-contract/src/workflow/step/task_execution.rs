//! Validation for the structured task-execution step payload.

use crate::errors::ContractError;

use super::{MAX_TASK_EXECUTION_ARGV, MAX_TASK_EXECUTION_ENV};

pub(in crate::workflow) fn validate_task_execution(
    argv: &[String],
    env: &std::collections::BTreeMap<String, String>,
    task_id: &str,
    task_digest: &str,
    toolchain_inputs: &crate::cachekey::ToolchainInputs,
    matrix_id: &str,
    matrix_key: &str,
    report_helper_version: &str,
    matrix_max_parallel: Option<u32>,
    job: &str,
) -> Result<(), ContractError> {
    if argv.len() < 8
        || argv.len() > MAX_TASK_EXECUTION_ARGV
        || env.len() > MAX_TASK_EXECUTION_ENV
        || argv[0] != "mise"
        || argv[1..4] != ["--no-config", "--no-env", "--no-hooks"]
        || argv[4] != "exec"
    {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("bad_mise_exec:{job}"),
        ));
    }
    let Some(separator) = argv[5..].iter().position(|arg| arg == "--").map(|i| i + 5) else {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("missing_payload_separator:{job}"),
        ));
    };
    if separator == 5 || separator + 1 >= argv.len() {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("bad_payload_boundary:{job}"),
        ));
    }
    let mut selectors = std::collections::BTreeSet::new();
    for selector in &argv[5..separator] {
        let Some((tool, version)) = pinned_tool_selector(selector) else {
            return Err(ContractError::identity(
                "step.task.argv",
                format!("unversioned_tool_selector:{job}"),
            ));
        };
        if !valid_pinned_version(version) || !selectors.insert(tool) {
            return Err(ContractError::identity(
                "step.task.argv",
                format!("invalid_tool_selector:{job}"),
            ));
        }
    }
    if selectors.contains("opentofu") && (selectors.len() != 1 || argv[separator + 1] != "tofu") {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("tofu_tool_payload_mismatch:{job}"),
        ));
    }
    if !selectors.contains("opentofu")
        && !selectors.contains("rust")
        && !selectors.contains("mr-boxington")
    {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("missing_compile_tool:{job}"),
        ));
    }
    for selector in &argv[5..separator] {
        if !toolchain_inputs.tools.contains(selector) {
            return Err(ContractError::identity(
                "step.task.toolchain",
                format!("selector_not_in_toolchain:{job}"),
            ));
        }
    }
    let toolchain_selectors = toolchain_inputs
        .tools
        .iter()
        .map(|selector| {
            let Some((tool, version)) = pinned_tool_selector(selector) else {
                return Err(ContractError::identity(
                    "step.task.toolchain",
                    format!("invalid_toolchain_pin:{job}"),
                ));
            };
            if !valid_pinned_version(version) {
                return Err(ContractError::identity(
                    "step.task.toolchain",
                    format!("invalid_toolchain_pin:{job}"),
                ));
            }
            Ok(tool)
        })
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    if toolchain_selectors.len() != toolchain_inputs.tools.len()
        || selectors.len() != argv[5..separator].len()
        || toolchain_inputs
            .components
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            != ["clippy", "rustfmt"]
    {
        return Err(ContractError::identity(
            "step.task.toolchain",
            format!("noncanonical_rust_toolchain:{job}"),
        ));
    }
    let expected_program = match toolchain_inputs.compile_driver.as_str() {
        "cargo" => "cargo",
        "mbx" => "mbx",
        _ => {
            return Err(ContractError::identity(
                "step.task.toolchain",
                format!("unknown_compile_driver:{job}"),
            ));
        }
    };
    if argv[separator + 1] != expected_program {
        return Err(ContractError::identity(
            "step.task.argv",
            format!("program_driver_mismatch:{job}"),
        ));
    }
    let mut expected_tool_names = match toolchain_inputs.compile_driver.as_str() {
        "cargo" => vec!["rust"],
        "mbx" => vec!["mr-boxington", "rust"],
        _ => unreachable!("compile driver checked above"),
    };
    if toolchain_inputs.test_runner == "cargo_nextest" {
        expected_tool_names.push("nextest-rs/nextest/cargo-nextest");
    }
    expected_tool_names.sort_unstable();
    if toolchain_selectors.iter().copied().collect::<Vec<_>>() != expected_tool_names {
        return Err(ContractError::identity(
            "step.task.toolchain",
            format!("toolchain_driver_mismatch:{job}"),
        ));
    }
    match toolchain_inputs.test_runner.as_str() {
        "cargo_test" if toolchain_selectors.contains("nextest-rs/nextest/cargo-nextest") => {
            return Err(ContractError::identity(
                "step.task.toolchain",
                format!("unexpected_nextest_pin:{job}"),
            ));
        }
        "cargo_nextest" if !toolchain_selectors.contains("nextest-rs/nextest/cargo-nextest") => {
            return Err(ContractError::identity(
                "step.task.toolchain",
                format!("missing_nextest_pin:{job}"),
            ));
        }
        "cargo_test" | "cargo_nextest" => {}
        _ => {
            return Err(ContractError::identity(
                "step.task.toolchain",
                format!("unknown_test_runner:{job}"),
            ));
        }
    }
    let mut expected_invocation_tools = vec!["rust"];
    if toolchain_inputs.test_runner == "cargo_nextest" {
        expected_invocation_tools.push("nextest-rs/nextest/cargo-nextest");
    }
    expected_invocation_tools.sort_unstable();
    if selectors.iter().copied().collect::<Vec<_>>() != expected_invocation_tools {
        return Err(ContractError::identity(
            "step.task.toolchain",
            format!("unbound_task_selector:{job}"),
        ));
    }
    for arg in argv {
        if arg.is_empty() || arg.chars().any(|ch| ch == '\0' || ch == '\n' || ch == '\r') {
            return Err(ContractError::identity(
                "step.task.argv",
                format!("invalid_argument:{job}"),
            ));
        }
    }
    crate::ids::validate_task_id(task_id)?;
    crate::validate_digest(task_digest)?;
    let toolchain_id = crate::cachekey::toolchain_id(toolchain_inputs)?;
    if super::super::crate_job::task_digest_for_execution(task_id, argv, &toolchain_id)?
        != task_digest
    {
        return Err(ContractError::identity(
            "step.task.identity",
            format!("task_digest_mismatch:{job}"),
        ));
    }
    crate::ids::validate_id(matrix_id)?;
    crate::ids::validate_matrix_key(matrix_key)?;
    if !matrix_id.ends_with(&format!("|task:{task_id}"))
        || crate::ids::matrix_key_for_id(matrix_id)? != matrix_key
    {
        return Err(ContractError::identity(
            "step.task.identity",
            format!("matrix_identity_mismatch:{job}"),
        ));
    }
    if !valid_pinned_version(report_helper_version) || report_helper_version.contains('+') {
        return Err(ContractError::identity(
            "step.task.report",
            format!("bad_helper_version:{job}"),
        ));
    }
    if matrix_max_parallel.is_some_and(|max| max == 0) {
        return Err(ContractError::identity(
            "step.task.matrix",
            format!("bad_matrix_cap:{job}"),
        ));
    }
    let fixed_env = [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
    ];
    for (key, value) in fixed_env {
        if env.get(key).map(String::as_str) != Some(value) {
            return Err(ContractError::identity(
                "step.task.env",
                format!("missing_or_changed_fixed_env:{job}:{key}"),
            ));
        }
    }
    let rust_version = toolchain_inputs
        .tools
        .iter()
        .find_map(|selector| selector.strip_prefix("rust@"))
        .ok_or_else(|| ContractError::identity("step.task.toolchain", "missing_rust_pin"))?;
    if env.get("RUSTUP_TOOLCHAIN").map(String::as_str) != Some(rust_version) {
        return Err(ContractError::identity(
            "step.task.env",
            format!("rustup_toolchain_pin_mismatch:{job}"),
        ));
    }
    for (key, value) in env {
        let valid_key = !key.is_empty()
            && key
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
        let credential_shaped = key.ends_with("_TOKEN")
            || key.starts_with("CARGO_REGISTRIES_")
            || matches!(
                key.as_str(),
                "GITHUB_TOKEN"
                    | "GH_TOKEN"
                    | "MISE_GITHUB_TOKEN"
                    | "ACTIONS_RUNTIME_TOKEN"
                    | "ACTIONS_ID_TOKEN_REQUEST_TOKEN"
                    | "ACTIONS_ID_TOKEN_REQUEST_URL"
                    | "GH_HOST"
                    | "GH_CONFIG_DIR"
                    | "VELNOR_TASK_ID"
                    | "VELNOR_TASK_DIGEST"
                    | "VELNOR_MATRIX_ID"
                    | "VELNOR_MATRIX_KEY"
                    | "VELNOR_MATRIX_NEEDS_JOB"
                    | "VELNOR_MATRIX_OUTPUT"
                    | "VELNOR_MATRIX_MAX_PARALLEL"
            );
        let expected_value = fixed_env
            .iter()
            .any(|&(expected_key, expected_value)| key == expected_key && value == expected_value)
            || (key == "RUSTUP_TOOLCHAIN" && value == rust_version)
            || (key == "RUSTDOCFLAGS" && value == "-D warnings");
        if !valid_key
            || !expected_value
            || credential_shaped
            || value
                .chars()
                .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
        {
            return Err(ContractError::identity(
                "step.task.env",
                format!("invalid_or_sensitive_env:{job}"),
            ));
        }
    }
    Ok(())
}

fn pinned_tool_selector(selector: &str) -> Option<(&str, &str)> {
    let (tool, version) = selector.rsplit_once('@')?;
    let canonical_tool = match tool {
        "rust" | "mr-boxington" | "opentofu" => tool,
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
