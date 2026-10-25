use std::collections::BTreeSet;

use crate::errors::ContractError;
use crate::ids::manifest_key_for_cargo_manifest;

use super::super::rust_suites::{
    MAX_SUITE_RETENTION_DAYS, RustSuiteArtifact, RustSuiteConfig, WorkflowDispatchInputConfig,
    WorkflowDispatchInputType,
};
use super::cfg_err;

pub(super) fn validate_suite_filters_and_dispatch(
    suite: &RustSuiteConfig,
    file: &str,
    prefix: &str,
) -> Result<(), ContractError> {
    if let Some(filter) = &suite.filter
        && !is_safe_single_line_text(filter)
    {
        return Err(cfg_err(
            file,
            format!("{prefix}.filter"),
            "bad_suite_filter",
        ));
    }
    if let Some(filter_input) = &suite.filter_input {
        if !is_valid_dispatch_input_name(filter_input) {
            return Err(cfg_err(
                file,
                format!("{prefix}.filter_input"),
                format!("bad_dispatch_input:{filter_input}"),
            ));
        }
        if !suite.has_workflow_dispatch_trigger() {
            return Err(cfg_err(
                file,
                format!("{prefix}.filter_input"),
                "filter_input_requires_workflow_dispatch_trigger",
            ));
        }
    }
    if !suite.dispatch_inputs.is_empty() && !suite.has_workflow_dispatch_trigger() {
        return Err(cfg_err(
            file,
            format!("{prefix}.dispatch_inputs"),
            "dispatch_inputs_require_workflow_dispatch_trigger",
        ));
    }
    for (input_name, values) in &suite.dispatch_inputs {
        let key_path = format!("{prefix}.dispatch_inputs.{input_name}");
        if !is_valid_dispatch_input_name(input_name) {
            return Err(cfg_err(
                file,
                key_path,
                format!("bad_dispatch_input:{input_name}"),
            ));
        }
        if values.is_empty() {
            return Err(cfg_err(file, key_path, "empty_dispatch_input_values"));
        }
        let mut seen = BTreeSet::new();
        for value in values {
            if !is_safe_option_value(value) || !seen.insert(value.as_str()) {
                return Err(cfg_err(
                    file,
                    key_path,
                    format!("bad_or_duplicate_dispatch_value:{value}"),
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_suite_targets_and_env(
    suite: &RustSuiteConfig,
    file: &str,
    prefix: &str,
) -> Result<(), ContractError> {
    if let Some(package) = &suite.package
        && !is_valid_ascii_ident_with_hyphen(package)
    {
        return Err(cfg_err(
            file,
            format!("{prefix}.package"),
            format!("bad_suite_package:{package}"),
        ));
    }
    if let Some(manifest) = &suite.manifest_path
        && manifest_key_for_cargo_manifest(manifest).is_err()
    {
        return Err(cfg_err(
            file,
            format!("{prefix}.manifest_path"),
            format!("bad_suite_manifest_path:{manifest}"),
        ));
    }
    for (key, value) in &suite.env {
        if !is_valid_posix_env_name(key) {
            return Err(cfg_err(
                file,
                format!("{prefix}.env"),
                format!("bad_env_key:{key}"),
            ));
        }
        if is_reserved_suite_env_key(key) {
            return Err(cfg_err(
                file,
                format!("{prefix}.env.{key}"),
                format!("reserved_suite_env_key:{key}"),
            ));
        }
        if value.contains('\0')
            || value.contains('\n')
            || value.contains('\r')
            || value.contains("${{")
        {
            return Err(cfg_err(
                file,
                format!("{prefix}.env.{key}"),
                "bad_env_value",
            ));
        }
    }
    Ok(())
}

const RESERVED_SUITE_ENV_PREFIXES: &[&str] = &[
    "VELNOR_", "NEXTEST_", "MISE_", "RUSTUP_", "CARGO_", "GITHUB_", "ACTIONS_", "RUNNER_",
];

fn is_reserved_suite_env_key(key: &str) -> bool {
    key == "PATH"
        || RESERVED_SUITE_ENV_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

pub(super) fn validate_suite_artifact(
    artifact: &RustSuiteArtifact,
    file: &str,
    prefix: &str,
) -> Result<(), ContractError> {
    if !is_valid_artifact_name_template(&artifact.name) {
        return Err(cfg_err(
            file,
            format!("{prefix}.name"),
            format!("bad_artifact_name:{}", artifact.name),
        ));
    }
    if artifact.paths.is_empty() {
        return Err(cfg_err(
            file,
            format!("{prefix}.paths"),
            "empty_artifact_paths",
        ));
    }
    for path in &artifact.paths {
        if !is_valid_artifact_path(path) {
            return Err(cfg_err(
                file,
                format!("{prefix}.paths"),
                format!("bad_artifact_path:{path}"),
            ));
        }
    }
    if !(1..=MAX_SUITE_RETENTION_DAYS).contains(&artifact.retention_days) {
        return Err(cfg_err(
            file,
            format!("{prefix}.retention_days"),
            format!("bad_artifact_retention_days:{}", artifact.retention_days),
        ));
    }
    Ok(())
}

pub(super) fn validate_dispatch_input(
    name: &str,
    input: &WorkflowDispatchInputConfig,
    file: &str,
) -> Result<(), ContractError> {
    let prefix = format!("workflow.dispatch.inputs.{name}");
    if let Some(desc) = &input.description
        && !is_safe_single_line_text(desc)
    {
        return Err(cfg_err(
            file,
            format!("{prefix}.description"),
            "bad_dispatch_description",
        ));
    }
    if let Some(default) = &input.default
        && (default.contains('\0')
            || default.contains('\n')
            || default.contains('\r')
            || default.contains("${{"))
    {
        return Err(cfg_err(
            file,
            format!("{prefix}.default"),
            "bad_dispatch_default",
        ));
    }
    match input.kind {
        WorkflowDispatchInputType::Choice => {
            if input.options.is_empty() {
                return Err(cfg_err(
                    file,
                    format!("{prefix}.options"),
                    "empty_choice_options",
                ));
            }
            let mut seen = BTreeSet::new();
            for option in &input.options {
                if !is_safe_option_value(option) || !seen.insert(option.as_str()) {
                    return Err(cfg_err(
                        file,
                        format!("{prefix}.options"),
                        format!("bad_or_duplicate_choice_option:{option}"),
                    ));
                }
            }
            if let Some(default) = &input.default
                && !input.options.contains(default)
            {
                return Err(cfg_err(
                    file,
                    format!("{prefix}.default"),
                    format!("default_not_in_options:{default}"),
                ));
            }
        }
        WorkflowDispatchInputType::String if !input.options.is_empty() => {
            return Err(cfg_err(
                file,
                format!("{prefix}.options"),
                "string_input_rejects_options",
            ));
        }
        WorkflowDispatchInputType::String => {}
    }
    Ok(())
}

pub(super) fn is_valid_ascii_ident_with_hyphen(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

pub(super) fn is_valid_dispatch_input_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

pub(super) fn is_valid_posix_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub(super) fn is_safe_single_line_text(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains("${{") && !value.chars().any(char::is_control)
}

pub(super) fn is_safe_option_value(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\'')
        && !value.contains("${{")
        && !value.chars().any(char::is_control)
}

pub(super) fn is_valid_artifact_name_template(name: &str) -> bool {
    if name.is_empty() || name.starts_with('.') || name.starts_with('-') {
        return false;
    }
    let replaced = name
        .replace("{shard}", "shard")
        .replace("{partition}", "partition");
    !replaced.contains('{')
        && !replaced.contains('}')
        && replaced
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub(super) fn is_valid_artifact_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('-')
        && !path.starts_with('~')
        && !path.starts_with('!')
        && !path.starts_with('*')
        && !path.contains('\\')
        && !path.contains("${{")
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != ".." && seg != "~")
}

pub(super) fn is_valid_workflow_slug(slug: &str) -> bool {
    let mut bytes = slug.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}
