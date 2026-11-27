// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/mise.rs

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::git;
use crate::broker::{TrustedFacts, mirror_path};
use crate::exec::{ExecError, StepContext};
use crate::store::store_root;

const CONFIG: &str = "mise.toml";
const LOCK: &str = "mise.lock";

fn invalid(message: impl Into<String>) -> ExecError {
    ExecError::Invalid(message.into())
}

pub(super) fn is_valid_mise_version(value: &str) -> bool {
    let bounds = [(4, 2000, 9999), (2, 1, 12), (2, 1, 31)];
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == bounds.len()
        && parts.iter().zip(bounds).all(|(part, (width, low, high))| {
            part.len() <= width
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part
                    .parse::<u32>()
                    .is_ok_and(|number| (low..=high).contains(&number))
        })
}

pub(super) fn lock_tool_keys(lock_toml: &str) -> Result<BTreeSet<String>, ExecError> {
    let table: toml::Table = lock_toml
        .parse()
        .map_err(|error| invalid(format!("mise.lock is not valid TOML: {error}")))?;
    Ok(table
        .get("tools")
        .and_then(toml::Value::as_table)
        .map_or_default(|tools| tools.keys().cloned().collect()))
}

fn is_tool_key(token: &str) -> bool {
    token.len() <= 200
        && token.bytes().any(|byte| byte.is_ascii_alphanumeric())
        && token.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/' | b'+')
        })
        && !token.starts_with(['.', ':', '/', '-'])
        && !token.contains("..")
}

fn validate_install_args(install_args: &str, lock_toml: &str) -> Result<(), ExecError> {
    let committed = lock_tool_keys(lock_toml)?;
    for token in install_args.split_whitespace() {
        if !is_tool_key(token) {
            return Err(invalid(format!(
                "mise install_args token '{token}' is not a bare tool key"
            )));
        }
        if !committed.contains(token) {
            return Err(invalid(format!(
                "mise install_args token '{token}' is not in the committed mise.lock"
            )));
        }
    }
    Ok(())
}

pub(super) struct AcceptedConfig {
    pub(super) config: String,
    pub(super) lock: String,
}

#[derive(Deserialize)]
struct LoadedConfig {
    path: PathBuf,
}

pub(super) fn require_sole_config(expected: &Path, config_listing: &str) -> Result<(), ExecError> {
    let loaded: Vec<LoadedConfig> = serde_json::from_str(config_listing)
        .map_err(|error| invalid(format!("mise config ls --json output: {error}")))?;
    if loaded.len() == 1 && loaded[0].path == expected {
        return Ok(());
    }
    let listed: Vec<String> = loaded
        .iter()
        .map(|config| config.path.display().to_string())
        .collect();
    Err(ExecError::Unsupported(format!(
        "mise policy: mise loads [{}] but only {} is supported by velnor-runner",
        listed.join(", "),
        expected.display()
    )))
}

async fn read_committed_file(
    ctx: &mut StepContext<'_>,
    mirror: &Path,
    sha: &str,
    listing: &str,
    name: &str,
) -> Result<String, ExecError> {
    let size = listing
        .lines()
        .find_map(|line| {
            let (meta, path) = line.split_once('\t')?;
            let fields: Vec<&str> = meta.split_whitespace().collect();
            match (path == name, fields.as_slice()) {
                (true, ["100644" | "100755", "blob", _, size]) => size.parse::<usize>().ok(),
                _ => None,
            }
        })
        .ok_or_else(|| {
            invalid(format!(
                "mise policy: {name} is missing from the trusted commit or is not a regular file"
            ))
        })?;
    let text = git::read_blob(ctx, mirror, sha, name).await?;
    if text.len() != size {
        return Err(invalid(format!(
            "mise policy: {name} in the trusted commit is not valid UTF-8"
        )));
    }
    Ok(text)
}

async fn read_committed(
    ctx: &mut StepContext<'_>,
    facts: &TrustedFacts,
) -> Result<AcceptedConfig, ExecError> {
    let mirror =
        mirror_path(&store_root(), facts).map_err(|error| invalid(format!("{error:#}")))?;
    let sha = facts.sha.as_str();
    let args = ["ls-tree", "-l", sha, "--", CONFIG, LOCK];
    let listing = git::capture(ctx, Some(&mirror), &args).await?;
    Ok(AcceptedConfig {
        config: read_committed_file(ctx, &mirror, sha, &listing, CONFIG).await?,
        lock: read_committed_file(ctx, &mirror, sha, &listing, LOCK).await?,
    })
}

fn require_supported_directives(config: &toml::Table) -> Result<(), ExecError> {
    for section in ["env", "vars"] {
        let tables: Vec<&toml::Table> = match config.get(section) {
            Some(toml::Value::Table(table)) => vec![table],
            Some(toml::Value::Array(entries)) => {
                entries.iter().filter_map(toml::Value::as_table).collect()
            }
            _ => Vec::new(),
        };
        for table in tables {
            let unsupported = match table.get("_") {
                None => None,
                Some(toml::Value::Table(directives)) => directives
                    .keys()
                    .find(|name| *name != "path")
                    .map(|name| format!("_.{name}")),
                Some(_) => Some("_".to_owned()),
            };
            if let Some(name) = unsupported {
                return Err(ExecError::Unsupported(format!(
                    "mise policy: [{section}] {name} is not supported by velnor-runner"
                )));
            }
        }
    }
    Ok(())
}

async fn resolve_locked_mise_config(
    ctx: &mut StepContext<'_>,
    facts: &TrustedFacts,
) -> Result<AcceptedConfig, ExecError> {
    let accepted = read_committed(ctx, facts).await?;
    let config: toml::Table = accepted
        .config
        .parse()
        .map_err(|error| invalid(format!("{CONFIG} is not valid TOML: {error}")))?;
    let locked = config
        .get("settings")
        .and_then(toml::Value::as_table)
        .and_then(|settings| settings.get("lockfile"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    if !locked {
        return Err(invalid(format!(
            "mise policy: {CONFIG} does not set [settings] lockfile = true"
        )));
    }
    require_supported_directives(&config)?;
    Ok(accepted)
}

pub(super) async fn enforce_locked_mise_policy(
    ctx: &mut StepContext<'_>,
    facts: &TrustedFacts,
    install_args: &str,
) -> Result<AcceptedConfig, ExecError> {
    let accepted = resolve_locked_mise_config(ctx, facts).await?;
    validate_install_args(install_args, &accepted.lock)?;
    Ok(accepted)
}
