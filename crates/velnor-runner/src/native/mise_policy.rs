// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mise_lock::refuse;
use crate::exec::ExecError;

const ARTIFACT_BACKENDS: [&str; 7] = [
    "aqua", "core", "forgejo", "github", "gitlab", "packslip", "ubi",
];
const PACKAGE_BACKENDS: [&str; 10] = [
    "cargo", "conda", "dotnet", "gem", "go", "npm", "pipx", "pypi", "spinel", "spm",
];
const TOOL_OPTIONS: [&str; 6] = [
    "components",
    "os",
    "profile",
    "targets",
    "version",
    "workflow",
];
const CHECKSUM_ALGORITHMS: [(&str, usize); 3] = [("sha256", 64), ("sha512", 128), ("blake3", 64)];
const RESERVED_ENV: [&str; 3] = ["CARGO_HOME", "HOME", "PATH"];
const RESERVED_ENV_PREFIXES: [&str; 2] = ["MISE_", "RUSTUP_"];

struct Spec<'a> {
    version: Option<&'a str>,
    options: Option<&'a toml::Table>,
}

fn specs(value: &toml::Value) -> Option<Vec<Spec<'_>>> {
    match value {
        toml::Value::String(version) => Some(vec![Spec {
            version: Some(version),
            options: None,
        }]),
        toml::Value::Table(options) => Some(vec![Spec {
            version: options.get("version").and_then(toml::Value::as_str),
            options: Some(options),
        }]),
        toml::Value::Array(entries) => entries
            .iter()
            .map(specs)
            .collect::<Option<Vec<_>>>()
            .map(|nested| nested.into_iter().flatten().collect()),
        _ => None,
    }
}

fn strings<'a>(value: &'a toml::Value, found: &mut Vec<&'a str>) {
    match value {
        toml::Value::String(text) => found.push(text),
        toml::Value::Array(entries) => entries.iter().for_each(|entry| strings(entry, found)),
        toml::Value::Table(table) => table.values().for_each(|entry| strings(entry, found)),
        _ => {}
    }
}

fn is_plain(value: &toml::Value) -> bool {
    match value {
        toml::Value::Table(_) => false,
        toml::Value::Array(entries) => entries
            .iter()
            .all(|entry| !matches!(entry, toml::Value::Table(_) | toml::Value::Array(_))),
        _ => true,
    }
}

fn lock_entries<'a>(lock: &'a toml::Table, key: &str) -> Vec<&'a toml::Table> {
    match lock
        .get("tools")
        .and_then(toml::Value::as_table)
        .and_then(|tools| tools.get(key))
    {
        Some(toml::Value::Table(entry)) => vec![entry],
        Some(toml::Value::Array(entries)) => {
            entries.iter().filter_map(toml::Value::as_table).collect()
        }
        _ => Vec::new(),
    }
}

fn lock_backend(lock: &toml::Table, key: &str) -> Option<String> {
    lock_entries(lock, key)
        .iter()
        .find_map(|entry| entry.get("backend").and_then(toml::Value::as_str))
        .map(str::to_owned)
}

fn backend_prefix(lock: &toml::Table, key: &str) -> Result<String, ExecError> {
    let named = match key.split_once(':') {
        Some((prefix, _)) => Some(prefix.to_owned()),
        None => lock_backend(lock, key)
            .map(|backend| backend.split(':').next().unwrap_or_default().to_owned()),
    };
    named.ok_or_else(|| refuse(&format!("tool '{key}' without a backend in mise.lock")))
}

fn is_checksum(text: &str) -> bool {
    text.split_once(':').is_some_and(|(algorithm, digest)| {
        CHECKSUM_ALGORITHMS
            .iter()
            .any(|(name, length)| *name == algorithm && digest.len() == *length)
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn platform_tables(entry: &toml::Table) -> Vec<&toml::Table> {
    let mut tables: Vec<&toml::Table> = entry
        .iter()
        .filter(|(name, _)| name.starts_with("platforms."))
        .filter_map(|(_, value)| value.as_table())
        .collect();
    if let Some(nested) = entry.get("platforms").and_then(toml::Value::as_table) {
        tables.extend(nested.values().filter_map(toml::Value::as_table));
    }
    tables
}

fn carries_checksum(lock: &toml::Table, key: &str) -> bool {
    let entries = lock_entries(lock, key);
    !entries.is_empty()
        && entries.iter().all(|entry| {
            let platforms = platform_tables(entry);
            !platforms.is_empty()
                && platforms.iter().all(|table| {
                    table
                        .get("checksum")
                        .and_then(toml::Value::as_str)
                        .is_some_and(is_checksum)
                })
        })
}

fn require_supported_tool(
    lock: &toml::Table,
    key: &str,
    value: &toml::Value,
) -> Result<(), ExecError> {
    if key.contains(['[', ']']) {
        return Err(refuse(&format!("tool '{key}' with inline options")));
    }
    let prefix = backend_prefix(lock, key)?;
    let package = PACKAGE_BACKENDS.contains(&prefix.as_str());
    if !package && !ARTIFACT_BACKENDS.contains(&prefix.as_str()) {
        return Err(refuse(&format!("backend '{prefix}' of tool '{key}'")));
    }
    if package && !carries_checksum(lock, key) {
        return Err(refuse(&format!(
            "package tool '{key}' without a checksum in mise.lock"
        )));
    }
    let Some(specs) = specs(value) else {
        return Err(refuse(&format!("tool '{key}' with this value")));
    };
    let mut texts = vec![key];
    for spec in &specs {
        texts.extend(spec.version);
        if let Some(options) = spec.options {
            if let Some(name) = options
                .iter()
                .find(|(name, value)| !TOOL_OPTIONS.contains(&name.as_str()) || !is_plain(value))
                .map(|(name, _)| name)
            {
                return Err(refuse(&format!("tool option '{name}' of '{key}'")));
            }
            options
                .values()
                .for_each(|option| strings(option, &mut texts));
        }
    }
    match texts
        .iter()
        .find(|text| text.starts_with("path:") || text.contains("file://"))
    {
        Some(text) => Err(refuse(&format!("tool '{key}' source '{text}'"))),
        None => Ok(()),
    }
}

fn require_unreserved_env(config: &toml::Table) -> Result<(), ExecError> {
    let tables: Vec<&toml::Table> = match config.get("env") {
        Some(toml::Value::Table(table)) => vec![table],
        Some(toml::Value::Array(entries)) => {
            entries.iter().filter_map(toml::Value::as_table).collect()
        }
        _ => Vec::new(),
    };
    for name in tables.iter().flat_map(|table| table.keys()) {
        let upper = name.to_ascii_uppercase();
        if RESERVED_ENV.contains(&upper.as_str())
            || RESERVED_ENV_PREFIXES
                .iter()
                .any(|prefix| upper.starts_with(prefix))
        {
            return Err(refuse(&format!("[env] name '{name}'")));
        }
    }
    Ok(())
}

pub(super) fn require_supported_values(
    config: &toml::Table,
    lock: &toml::Table,
) -> Result<(), ExecError> {
    require_unreserved_env(config)?;
    let tools = config.get("tools").and_then(toml::Value::as_table);
    for (key, value) in tools.into_iter().flatten() {
        require_supported_tool(lock, key, value)?;
    }
    Ok(())
}
