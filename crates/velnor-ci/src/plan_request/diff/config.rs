// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use toml::{Table, Value};

use super::glob;
use crate::error::CiError;
use crate::write_request::internal;

pub(super) const PATH: &str = ".velnor/config.toml";

const UNSUPPORTED_STACKS: &[&str] = &["tofu", "bun", "gradle", "dbt", "ansible"];

#[derive(Debug)]
pub(super) struct InputRule {
    pub(super) globs: Vec<String>,
    pub(super) packages: Vec<String>,
    pub(super) complete: bool,
}

impl InputRule {
    pub(super) fn matches(&self, path: &str) -> bool {
        self.globs
            .iter()
            .any(|pattern| glob::matches(pattern, path))
    }
}

#[derive(Debug)]
pub(super) struct Check {
    pub(super) id: String,
    pub(super) directory: String,
    pub(super) inputs: Vec<String>,
    pub(super) rust_changes: bool,
}

#[derive(Debug, Default)]
pub(super) struct Config {
    pub(super) rules: Vec<InputRule>,
    pub(super) non_inputs: Vec<String>,
    pub(super) checks: Vec<Check>,
}

impl Config {
    pub(super) fn non_input(&self, path: &str) -> bool {
        self.non_inputs
            .iter()
            .any(|pattern| glob::non_input_covers(pattern, path))
    }
}

pub(super) fn parse(text: &str) -> Result<Config, CiError> {
    let document: Table = toml::from_str(text).map_err(|_| unsupported("config_unreadable"))?;
    for key in ["images", "native_checks"] {
        if document
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty())
        {
            return Err(unsupported(key));
        }
    }
    let stacks = document.get("stacks").and_then(Value::as_table);
    for stack in UNSUPPORTED_STACKS.iter().copied() {
        if stacks.is_some_and(|stacks| stacks.contains_key(stack)) {
            return Err(unsupported(stack));
        }
    }
    let rust = stacks
        .and_then(|stacks| stacks.get("rust"))
        .and_then(Value::as_table);
    let mut config = Config::default();
    if let Some(rust) = rust {
        config.non_inputs = strings(rust.get("non_inputs"));
        for rule in rust
            .get("inputs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_table)
        {
            config.rules.push(InputRule {
                globs: strings(rule.get("globs")),
                packages: strings(rule.get("packages")),
                complete: rule
                    .get("complete")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }
    }
    for check in document
        .get("checks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_table)
    {
        config.checks.push(check_of(check)?);
    }
    Ok(config)
}

fn check_of(check: &Table) -> Result<Check, CiError> {
    let id = check
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| unsupported("check_without_id"))?;
    if !is_identifier(id) {
        return Err(unsupported(&format!("check_id:{id}")));
    }
    let rust_changes = match check.get("selection").and_then(Value::as_str) {
        None | Some("rust_changes") => true,
        Some("declared_inputs") => false,
        Some(_) => return Err(unsupported("check_selection")),
    };
    Ok(Check {
        id: id.to_owned(),
        directory: check
            .get("directory")
            .and_then(Value::as_str)
            .unwrap_or(".")
            .to_owned(),
        inputs: strings(check.get("inputs")),
        rust_changes,
    })
}

fn is_identifier(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn unsupported(what: &str) -> CiError {
    internal(&format!("diff_selection_not_supported:{what}"))
}
