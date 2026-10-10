use std::collections::BTreeMap;

use crate::errors::ContractError;

use super::TaskExecutionValidation;

const FIXED_ENV: [(&str, &str); 8] = [
    ("MISE_NO_CONFIG", "1"),
    ("MISE_NO_ENV", "1"),
    ("MISE_NO_HOOKS", "1"),
    ("MISE_LOCKFILE", "0"),
    ("MISE_AUTO_INSTALL", "false"),
    ("MISE_EXEC_AUTO_INSTALL", "false"),
    ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
    ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
];

pub(super) fn validate_environment(
    input: &TaskExecutionValidation<'_>,
) -> Result<(), ContractError> {
    validate_required_environment(input)?;
    let rust_version = rust_toolchain_version(input)?;
    if input.env.get("RUSTUP_TOOLCHAIN").map(String::as_str) != Some(rust_version) {
        return Err(ContractError::identity(
            "step.task.env",
            format!("rustup_toolchain_pin_mismatch:{}", input.job),
        ));
    }
    validate_environment_values(input, rust_version)
}

fn validate_required_environment(input: &TaskExecutionValidation<'_>) -> Result<(), ContractError> {
    for (key, value) in FIXED_ENV {
        if input.env.get(key).map(String::as_str) != Some(value) {
            return Err(ContractError::identity(
                "step.task.env",
                format!("missing_or_changed_fixed_env:{}:{key}", input.job),
            ));
        }
    }
    Ok(())
}

fn rust_toolchain_version(input: &TaskExecutionValidation<'_>) -> Result<&str, ContractError> {
    input
        .toolchain_inputs
        .tools
        .iter()
        .find_map(|selector| selector.strip_prefix("rust@"))
        .ok_or_else(|| ContractError::identity("step.task.toolchain", "missing_rust_pin"))
}

fn validate_environment_values(
    input: &TaskExecutionValidation<'_>,
    rust_version: &str,
) -> Result<(), ContractError> {
    for (key, value) in input.env {
        if !valid_environment_key(key)
            || !expected_environment_value(key, value, rust_version)
            || credential_shaped_key(key)
            || value
                .chars()
                .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
        {
            return Err(ContractError::identity(
                "step.task.env",
                format!("invalid_or_sensitive_env:{}", input.job),
            ));
        }
    }
    Ok(())
}

fn valid_environment_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn expected_environment_value(key: &str, value: &str, rust_version: &str) -> bool {
    FIXED_ENV
        .iter()
        .any(|&(expected_key, expected_value)| key == expected_key && value == expected_value)
        || (key == "RUSTUP_TOOLCHAIN" && value == rust_version)
        || (key == "RUSTDOCFLAGS" && value == "-D warnings")
}

fn credential_shaped_key(key: &str) -> bool {
    key.ends_with("_TOKEN")
        || key.starts_with("CARGO_REGISTRIES_")
        || matches!(
            key,
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
        )
}
