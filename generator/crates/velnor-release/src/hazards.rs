// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use toml::Value;

use crate::cargo_allowlist::unknown_key;
use crate::error::ReleaseError;
use crate::settings::executable_key;

const BUILTIN_PROVIDER: &str = "cargo:token";

pub fn refuse_external_commands(file: &str, text: &str) -> Result<(), String> {
    let document = text
        .parse::<toml::Table>()
        .map_err(|error| format!("{file} is not valid TOML: {error}"))?;
    match executable_key(&Value::Table(document)) {
        Some(key) => Err(format!(
            "{file}: {key} runs an external command with the release token in reach"
        )),
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

fn names_other_default_registry(document: &Value) -> bool {
    document
        .get("registry")
        .and_then(|registry| registry.get("default"))
        .is_some_and(|default| default.as_str() != Some("crates-io"))
}

fn offline(document: &Value) -> bool {
    document
        .get("net")
        .and_then(|net| net.get("offline"))
        .is_some_and(|offline| offline.as_bool() != Some(false))
}

fn is_builtin(value: &Value) -> bool {
    match value {
        Value::String(name) => name == BUILTIN_PROVIDER,
        Value::Array(items) => !items.is_empty() && items.iter().all(is_builtin),
        _ => false,
    }
}

pub fn refuse_cargo_config(
    file: &str,
    text: &str,
    publishes_to_crates_io: bool,
) -> Result<(), String> {
    cargo_config(file, text, publishes_to_crates_io).map_err(|error| error.to_string())
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
    if let Value::Table(table) = &document
        && table.keys().any(|key| key.eq_ignore_ascii_case("include"))
    {
        return Err(ReleaseError::CargoConfigInclude {
            file: file.to_owned(),
        });
    }
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
    if publishes_to_crates_io
        && (redirects_crates_io(&document) || names_other_default_registry(&document))
    {
        return Err(ReleaseError::CargoRedirectsCratesIo {
            file: file.to_owned(),
        });
    }
    if publishes_to_crates_io && offline(&document) {
        return Err(ReleaseError::CargoOffline {
            file: file.to_owned(),
        });
    }
    if let Some(key) = unknown_key(&document) {
        return Err(ReleaseError::CargoConfigKeyNotAllowed {
            file: file.to_owned(),
            key,
        });
    }
    Ok(())
}

fn names_nightly(text: &str) -> bool {
    let channel = match text.parse::<toml::Table>() {
        Ok(document) => document
            .get("toolchain")
            .and_then(Value::as_table)
            .and_then(|toolchain| toolchain.get("channel"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        Err(_) => Some(text.trim().to_owned()),
    };
    channel.is_some_and(|channel| channel.starts_with("nightly"))
}

const STABILIZED_CARGO_FEATURES: [&str; 9] = [
    "edition2024",
    "workspace-inheritance",
    "named-profiles",
    "strip",
    "profile-overrides",
    "rename-dependency",
    "resolver",
    "namespaced-features",
    "weak-dep-features",
];

fn needs_nightly_features(document: &toml::Table) -> bool {
    match document.get("cargo-features") {
        None => false,
        Some(Value::Array(features)) => features.iter().any(|feature| {
            !feature
                .as_str()
                .is_some_and(|name| STABILIZED_CARGO_FEATURES.contains(&name))
        }),
        Some(_) => true,
    }
}

pub(crate) fn nightly_manifest(file: &str, text: &str) -> Result<(), ReleaseError> {
    let nightly = text
        .parse::<toml::Table>()
        .is_ok_and(|document| needs_nightly_features(&document));
    if nightly {
        Err(ReleaseError::NightlyOnly {
            file: file.to_owned(),
            what: "cargo-features",
        })
    } else {
        Ok(())
    }
}

pub(crate) fn toolchain_path(file: &str, text: &str) -> Result<(), ReleaseError> {
    if names_nightly(text) {
        return Err(ReleaseError::NightlyOnly {
            file: file.to_owned(),
            what: "a nightly toolchain",
        });
    }
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

pub(crate) fn symlink_on_config_path(
    root: &str,
    symlinks: &BTreeSet<String>,
) -> Result<(), ReleaseError> {
    let mut directories = vec![String::new()];
    if root != "." {
        let mut prefix = String::new();
        for part in root.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            directories.push(prefix.clone());
        }
    }
    let linked = |path: &str| symlinks.contains(path);
    for directory in &directories {
        let at = |name: &str| {
            if directory.is_empty() {
                name.to_owned()
            } else {
                format!("{directory}/{name}")
            }
        };
        let found = [
            directory.clone(),
            at(".cargo"),
            at(".cargo/config"),
            at(".cargo/config.toml"),
        ]
        .into_iter()
        .find(|path| !path.is_empty() && linked(path));
        if let Some(path) = found {
            return Err(ReleaseError::SymlinkOnCargoConfigPath {
                root: root.to_owned(),
                path,
            });
        }
    }
    Ok(())
}
