// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::RenderError;
use crate::verification_jobs::build_task_jobs::{BuildTaskArtifact, BuildTaskPolicy};

#[path = "build_task_mise_tools_selector.rs"]
mod selector;
use selector::toml_option_string;
pub(super) use selector::valid_github_matching_regex;

pub(super) const BOLTFFI_KEY: &str = "github:boltffi/boltffi";
pub(super) const BOLTFFI_MATCHING_REGEX: &str = r"^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$";

pub(super) fn valid_rust_toolchain_options(options: &BTreeMap<String, String>) -> bool {
    options
        .iter()
        .all(|(key, value)| valid_rust_toolchain_option(key, value))
}

fn valid_rust_toolchain_option(key: &str, value: &str) -> bool {
    match key {
        "profile" => matches!(value, "minimal" | "default" | "complete"),
        "components" | "targets" => safe_option_value(value),
        _ => false,
    }
}

pub(super) fn selected_mise_files(
    policy: &BuildTaskPolicy,
) -> Result<(String, String), RenderError> {
    validate_selected_tools(policy)?;
    selected_mise_files_for(&policy.selected_tools, "macos", "macos-arm64", false)
}

pub(super) fn selected_mise_files_for(
    selected_tools: &[crate::verification_jobs::build_task_jobs::BuildTaskTool],
    mise_os: &str,
    lock_platform: &str,
    cargo_wrapper: bool,
) -> Result<(String, String), RenderError> {
    if !matches!(
        (mise_os, lock_platform),
        ("linux", "linux-x64") | ("macos", "macos-arm64")
    ) {
        return Err(RenderError::InvalidWorkflow(
            "verification_tool_platform".to_owned(),
        ));
    }
    let mut config = Vec::new();
    for tool in selected_tools {
        if !(tool.os.is_empty() || tool.os == [mise_os]) {
            return Err(RenderError::InvalidWorkflow(format!(
                "selected_tool_os:{}",
                tool.key
            )));
        }
        let key = toml_string(&tool.key);
        config.push(format!("[tools.{key}]"));
        config.push(format!("version = {}", toml_string(&tool.version)));
        if !tool.os.is_empty() {
            config.push(format!("os = [\"{mise_os}\"]"));
        }
        for (name, value) in &tool.config_options {
            config.push(format!("{name} = {}", toml_option_string(name, value)));
        }
        config.push(String::new());
    }
    config.extend([
        String::new(),
        "[settings]".to_owned(),
        "lockfile = true".to_owned(),
    ]);
    if cargo_wrapper {
        config.extend([
            String::new(),
            "[settings.cargo]".to_owned(),
            "binstall = true".to_owned(),
            "binstall_only = true".to_owned(),
            String::new(),
            "[wrappers.cargo]".to_owned(),
            "command = \"mbx\"".to_owned(),
            String::new(),
            "[wrappers.cargo.env]".to_owned(),
            "MBX_CARGO_SHIM_MODE = \"1\"".to_owned(),
        ]);
    }

    let mut lock = Vec::new();
    for tool in selected_tools {
        let key = toml_string(&tool.key);
        lock.push(format!("[[tools.{key}]]"));
        lock.push(format!("version = {}", toml_string(&tool.version)));
        lock.push(format!("backend = {}", toml_string(&tool.backend)));
        if !tool.lock_options.is_empty() {
            lock.push(String::new());
            lock.push(format!("[tools.{key}.options]"));
            for (name, value) in &tool.lock_options {
                lock.push(format!("{name} = {}", toml_option_string(name, value)));
            }
        }
        if let Some(artifact) = &tool.artifact {
            lock.push(String::new());
            lock.push(format!("[tools.{key}.\"platforms.{lock_platform}\"]"));
            lock.push(format!("checksum = {}", toml_string(&artifact.checksum)));
            lock.push(format!("url = {}", toml_string(&artifact.url)));
            append_lock_string(&mut lock, "url_api", artifact.url_api.as_deref());
            append_lock_string(&mut lock, "signer", artifact.signer.as_deref());
            append_lock_string(&mut lock, "provenance", artifact.provenance.as_deref());
        }
        lock.push(String::new());
    }
    Ok((config.join("\n"), lock.join("\n")))
}

fn toml_string(value: &str) -> String {
    format!("\"{value}\"")
}

fn append_lock_string(lines: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        lines.push(format!("{key} = {}", toml_string(value)));
    }
}

fn validate_selected_tools(policy: &BuildTaskPolicy) -> Result<(), RenderError> {
    if policy.selected_tools.len() != policy.task.tools.len() {
        return Err(RenderError::InvalidWorkflow(
            "build_task_tool_count".to_owned(),
        ));
    }
    let expected_boltffi_options = BTreeMap::from([(
        "matching_regex".to_owned(),
        BOLTFFI_MATCHING_REGEX.to_owned(),
    )]);
    let mut previous = None;
    for (task_key, tool) in policy.task.tools.iter().zip(&policy.selected_tools) {
        if task_key != &tool.key
            || previous.is_some_and(|key: &str| key >= tool.key.as_str())
            || !safe_version(&tool.version)
            || !safe_backend(&tool.backend)
            || !(tool.os.is_empty() || tool.os == ["macos"])
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "build_task_tool_identity:{}",
                tool.key
            )));
        }
        previous = Some(tool.key.as_str());
        validate_tool_options(tool, &expected_boltffi_options)?;
        validate_tool_backend(tool)?;
    }
    validate_source_digests(policy)
}

fn validate_tool_options(
    tool: &crate::verification_jobs::build_task_jobs::BuildTaskTool,
    expected_boltffi_options: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if tool.key == BOLTFFI_KEY {
        if tool.config_options != *expected_boltffi_options
            || tool.lock_options != *expected_boltffi_options
        {
            return Err(RenderError::InvalidWorkflow(
                "build_task_boltffi_selector".to_owned(),
            ));
        }
    } else if tool.key == "rust" {
        if tool.config_options != tool.lock_options
            || !valid_rust_toolchain_options(&tool.config_options)
        {
            return Err(RenderError::InvalidWorkflow(
                "build_task_rust_options".to_owned(),
            ));
        }
    } else if !tool.config_options.is_empty() {
        return Err(RenderError::InvalidWorkflow(format!(
            "build_task_tool_options:{}",
            tool.key
        )));
    }
    if tool.key != "rust" && tool.key != BOLTFFI_KEY && !tool.lock_options.is_empty() {
        return Err(RenderError::InvalidWorkflow(format!(
            "build_task_tool_options:{}",
            tool.key
        )));
    }
    for (key, value) in &tool.lock_options {
        let safe =
            (tool.key == BOLTFFI_KEY && key == "matching_regex" && value == BOLTFFI_MATCHING_REGEX)
                || (tool.key == "rust" && valid_rust_toolchain_option(key, value));
        if !safe {
            return Err(RenderError::InvalidWorkflow(format!(
                "build_task_tool_option_value:{}",
                tool.key
            )));
        }
    }
    Ok(())
}

fn validate_source_digests(policy: &BuildTaskPolicy) -> Result<(), RenderError> {
    for digest in [
        &policy.mise_config_sha256,
        &policy.mise_lock_sha256,
        &policy.rust_toolchain_sha256,
    ] {
        if !velnor_actions_contract::ids::is_lower_hex_len(digest, 64) {
            return Err(RenderError::InvalidWorkflow(
                "build_task_source_sha256".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_tool_backend(
    tool: &crate::verification_jobs::build_task_jobs::BuildTaskTool,
) -> Result<(), RenderError> {
    if let Some(artifact) = &tool.artifact
        && !artifact.url.contains(&tool.version)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "build_task_tool_version_artifact_mismatch:{}",
            tool.key
        )));
    }
    match (tool.key.as_str(), tool.backend.as_str(), &tool.artifact) {
        ("rust", "core:rust", None)
            if tool.config_options == tool.lock_options
                && exact_rust_version(&tool.version)
                && valid_rust_toolchain_options(&tool.lock_options) => {}
        ("mr-boxington", "packslip:github.com/jdx/mr-boxington", Some(artifact))
            if tool.config_options.is_empty() && tool.lock_options.is_empty() =>
        {
            validate_artifact(&tool.key, artifact)?;
        }
        (BOLTFFI_KEY, BOLTFFI_KEY, Some(artifact)) => validate_artifact(&tool.key, artifact)?,
        (_, backend, Some(artifact))
            if backend
                .strip_prefix("aqua:")
                .is_some_and(|name| !name.is_empty())
                && tool.config_options.is_empty()
                && tool.lock_options.is_empty() =>
        {
            validate_artifact(&tool.key, artifact)?;
        }
        (key, backend, Some(artifact))
            if key.starts_with("github:")
                && key == backend
                && tool.config_options.is_empty()
                && tool.lock_options.is_empty() =>
        {
            validate_artifact(&tool.key, artifact)?;
        }
        _ => {
            return Err(RenderError::InvalidWorkflow(format!(
                "build_task_tool_backend:{}:{}",
                tool.key, tool.backend
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_artifact(
    key: &str,
    artifact: &BuildTaskArtifact,
) -> Result<(), RenderError> {
    let checksum = artifact
        .checksum
        .strip_prefix("sha256:")
        .unwrap_or_default();
    if !velnor_actions_contract::ids::is_lower_hex_len(checksum, 64)
        || !locked_github_url(&artifact.url, "https://github.com/")
        || artifact
            .url_api
            .as_deref()
            .is_some_and(|url| !locked_github_url(url, "https://api.github.com/repos/"))
        || artifact
            .signer
            .as_deref()
            .is_some_and(|value| !safe_identity(value))
        || artifact
            .provenance
            .as_deref()
            .is_some_and(|value| !safe_identity(value))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "build_task_artifact:{key}"
        )));
    }
    Ok(())
}

pub(super) fn locked_github_url(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|path| {
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains("..")
            && path.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b'+')
            })
    })
}

pub(super) fn safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'_' | b'-'))
}

pub(super) fn exact_rust_version(value: &str) -> bool {
    let components = value.split('.').collect::<Vec<_>>();
    if components.len() != 3 {
        return false;
    }
    let (Ok(major), Ok(minor), Ok(_patch)) = (
        components[0].parse::<u32>(),
        components[1].parse::<u32>(),
        components[2].parse::<u32>(),
    ) else {
        return false;
    };
    major == 1 && minor >= 98
}

pub(super) fn safe_backend(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'/' | b'.' | b'_' | b'-' | b'@')
        })
}

pub(super) fn safe_option_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b',' | b'.' | b'_' | b'-'))
}

pub(super) fn safe_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'/' | b'.' | b'_' | b'-' | b'@')
        })
}

pub(super) fn validate_policy(policy: &BuildTaskPolicy) -> Result<(), RenderError> {
    policy
        .task
        .validate(".velnor/config.toml")
        .map_err(RenderError::Contract)?;
    if policy.runner_label != policy.task.runner.runs_on() {
        return Err(RenderError::InvalidWorkflow(format!(
            "build_task_runner_mismatch:{}",
            policy.task.id
        )));
    }
    validate_selected_tools(policy)?;
    if let Some(rust) = policy.selected_tools.iter().find(|tool| tool.key == "rust")
        && !exact_rust_version(&rust.version)
    {
        return Err(RenderError::InvalidWorkflow(
            "build_task_rust_version_not_exact".to_owned(),
        ));
    }
    policy.mise_setup.validate()
}
