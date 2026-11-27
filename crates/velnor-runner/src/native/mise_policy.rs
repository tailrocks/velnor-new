// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mise_lock::refuse;
use crate::exec::ExecError;

const BACKENDS: [&str; 6] = ["aqua", "core", "forgejo", "github", "gitlab", "packslip"];
const TOOL_OPTIONS: [&str; 6] = [
    "components",
    "os",
    "profile",
    "targets",
    "version",
    "workflow",
];
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

fn entries(value: &toml::Value) -> Vec<&toml::Table> {
    match value {
        toml::Value::Table(entry) => vec![entry],
        toml::Value::Array(entries) => entries.iter().filter_map(toml::Value::as_table).collect(),
        _ => Vec::new(),
    }
}

fn backend_of(entry: &toml::Table) -> Option<&str> {
    entry.get("backend").and_then(toml::Value::as_str)
}

fn require_supported_backend(key: &str, backend: &str) -> Result<(), ExecError> {
    if backend.contains(['[', ']']) {
        return Err(refuse(&format!(
            "backend '{backend}' of tool '{key}' with inline options"
        )));
    }
    let prefix = backend.split(':').next().unwrap_or_default();
    if !BACKENDS.contains(&prefix) {
        return Err(refuse(&format!("backend '{prefix}' of tool '{key}'")));
    }
    match key.split_once(':') {
        Some((named, _)) if named != prefix => Err(refuse(&format!(
            "backend '{prefix}' of tool '{key}' that differs from its key"
        ))),
        _ => Ok(()),
    }
}

fn require_supported_lock(lock: &toml::Table) -> Result<(), ExecError> {
    let tools = lock.get("tools").and_then(toml::Value::as_table);
    for (key, value) in tools.into_iter().flatten() {
        if key.contains(':') {
            require_supported_backend(key, key)?;
        }
        let locked = entries(value);
        if locked.is_empty() {
            return Err(without_backend(key));
        }
        for entry in locked {
            let backend = backend_of(entry).ok_or_else(|| without_backend(key))?;
            require_supported_backend(key, backend)?;
        }
    }
    Ok(())
}

fn without_backend(key: &str) -> ExecError {
    refuse(&format!("tool '{key}' without a backend in mise.lock"))
}

fn require_backend(lock: &toml::Table, key: &str) -> Result<(), ExecError> {
    if key.contains(':') {
        return require_supported_backend(key, key);
    }
    let locked = lock
        .get("tools")
        .and_then(toml::Value::as_table)
        .is_some_and(|tools| tools.contains_key(key));
    if locked {
        Ok(())
    } else {
        Err(without_backend(key))
    }
}

fn require_supported_tool(
    lock: &toml::Table,
    key: &str,
    value: &toml::Value,
) -> Result<(), ExecError> {
    if key.contains(['[', ']']) {
        return Err(refuse(&format!("tool '{key}' with inline options")));
    }
    require_backend(lock, key)?;
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
    require_supported_lock(lock)?;
    let tools = config.get("tools").and_then(toml::Value::as_table);
    for (key, value) in tools.into_iter().flatten() {
        require_supported_tool(lock, key, value)?;
    }
    Ok(())
}
