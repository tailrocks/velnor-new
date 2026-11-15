// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use toml::Value;

use crate::error::ReleaseError;

const COMMAND_KEYS: [&str; 2] = ["replace_command", "changelog_config"];
const BUILTIN_PROVIDER: &str = "cargo:token";

fn find_key<'a>(value: &Value, keys: &[&'a str]) -> Option<&'a str> {
    match value {
        Value::Table(table) => keys
            .iter()
            .find(|key| table.contains_key(**key))
            .copied()
            .or_else(|| table.values().find_map(|inner| find_key(inner, keys))),
        Value::Array(items) => items.iter().find_map(|inner| find_key(inner, keys)),
        _ => None,
    }
}

pub(crate) fn external_command(root: &str, document: &Value) -> Result<(), ReleaseError> {
    match find_key(document, &COMMAND_KEYS) {
        Some(key) => Err(ReleaseError::ExternalCommand {
            root: root.to_owned(),
            key: key.to_owned(),
        }),
        None => Ok(()),
    }
}

fn runs_a_program(value: &Value) -> bool {
    match value {
        Value::Table(table) => table.iter().any(|(key, inner)| match key.as_str() {
            "credential-alias" => true,
            "credential-provider" => !is_builtin(inner),
            _ => runs_a_program(inner),
        }),
        Value::Array(items) => items.iter().any(runs_a_program),
        _ => false,
    }
}

fn is_builtin(value: &Value) -> bool {
    match value {
        Value::String(name) => name == BUILTIN_PROVIDER,
        Value::Array(items) => items.iter().all(is_builtin),
        _ => false,
    }
}

pub(crate) fn credential_provider(file: &str, text: &str) -> Result<(), ReleaseError> {
    let document = text
        .parse::<toml::Table>()
        .map_err(|error| ReleaseError::InvalidToml {
            file: file.to_owned(),
            message: error.to_string(),
        })?;
    if runs_a_program(&Value::Table(document)) {
        Err(ReleaseError::CargoCredentialProvider {
            file: file.to_owned(),
        })
    } else {
        Ok(())
    }
}
