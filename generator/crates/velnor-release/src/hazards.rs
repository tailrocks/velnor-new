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

const PROGRAM_KEYS: [&str; 3] = ["rustc-wrapper", "rustc-workspace-wrapper", "rustc"];

fn program_key(value: &Value) -> Option<&'static str> {
    match value {
        Value::Table(table) => PROGRAM_KEYS
            .iter()
            .find(|key| table.contains_key(**key))
            .copied()
            .or_else(|| table.values().find_map(program_key)),
        Value::Array(items) => items.iter().find_map(program_key),
        _ => None,
    }
}

fn credential_provider(value: &Value) -> bool {
    match value {
        Value::Table(table) => table.iter().any(|(key, inner)| match key.as_str() {
            "credential-alias" | "global-credential-providers" => true,
            "credential-provider" => !is_builtin(inner),
            _ => credential_provider(inner),
        }),
        Value::Array(items) => items.iter().any(credential_provider),
        _ => false,
    }
}

fn redirects_crates_io(value: &Value) -> bool {
    match value {
        Value::Table(table) => table.iter().any(|(key, inner)| match key.as_str() {
            "proxy" | "cainfo" => true,
            _ => redirects_crates_io(inner),
        }),
        Value::Array(items) => items.iter().any(redirects_crates_io),
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

pub(crate) fn cargo_config(
    file: &str,
    text: &str,
    publishes_to_crates_io: bool,
) -> Result<(), ReleaseError> {
    let document = text
        .parse::<toml::Table>()
        .map_err(|error| ReleaseError::InvalidToml {
            file: file.to_owned(),
            message: error.to_string(),
        })?;
    let document = Value::Table(document);
    let key = if document.get("env").is_some() {
        Some("env")
    } else {
        program_key(&document)
    };
    if let Some(key) = key {
        return Err(ReleaseError::CargoRunsProgram {
            file: file.to_owned(),
            key: key.to_owned(),
        });
    }
    if credential_provider(&document) {
        return Err(ReleaseError::CargoCredentialProvider {
            file: file.to_owned(),
        });
    }
    if publishes_to_crates_io && redirects_crates_io(&document) {
        return Err(ReleaseError::CargoRedirectsCratesIo {
            file: file.to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn toolchain_path(file: &str, text: &str) -> Result<(), ReleaseError> {
    let names_a_path = match text.parse::<toml::Table>() {
        Ok(document) => document
            .get("toolchain")
            .and_then(Value::as_table)
            .is_some_and(|toolchain| toolchain.contains_key("path")),
        Err(_) => text.contains('/') || text.contains('\\'),
    };
    if names_a_path {
        Err(ReleaseError::ToolchainFromPath {
            file: file.to_owned(),
        })
    } else {
        Ok(())
    }
}
