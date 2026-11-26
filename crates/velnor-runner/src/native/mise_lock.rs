// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/mise.rs

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use super::git;
use crate::exec::{ExecError, StepContext};

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

async fn is_regular_file(path: &Path) -> bool {
    tokio::fs::symlink_metadata(path)
        .await
        .is_ok_and(|meta| meta.file_type().is_file())
}

#[derive(Deserialize)]
struct LoadedConfig {
    path: std::path::PathBuf,
}

pub(super) async fn require_sole_config(
    workspace: &Path,
    config_listing: &str,
) -> Result<(), ExecError> {
    let loaded: Vec<LoadedConfig> = serde_json::from_str(config_listing)
        .map_err(|error| invalid(format!("mise config ls --json output: {error}")))?;
    let expected = tokio::fs::canonicalize(workspace.join(CONFIG))
        .await
        .map_err(|source| ExecError::io(format!("resolve {CONFIG}"), source))?;
    let mut resolved = Vec::new();
    for config in &loaded {
        resolved.push(tokio::fs::canonicalize(&config.path).await.ok());
    }
    if resolved.len() == 1 && resolved[0].as_deref() == Some(expected.as_path()) {
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

async fn require_tracked_and_clean(ctx: &mut StepContext<'_>) -> Result<(), ExecError> {
    let tracked = git::capture(ctx, None, &["ls-files", "--", CONFIG, LOCK]).await?;
    for name in [CONFIG, LOCK] {
        if !tracked.lines().any(|line| line == name) {
            return Err(invalid(format!(
                "mise policy: {name} is not tracked by git"
            )));
        }
    }
    let changes = git::capture(ctx, None, &["status", "--porcelain", "--", CONFIG, LOCK]).await?;
    match changes.lines().next() {
        Some(line) => Err(invalid(format!(
            "mise policy: {} is dirty against index/HEAD",
            line.get(3..).unwrap_or(line)
        ))),
        None => Ok(()),
    }
}

async fn resolve_locked_mise_config(ctx: &mut StepContext<'_>) -> Result<String, ExecError> {
    let workspace = ctx.workspace;
    for name in [CONFIG, LOCK] {
        if !is_regular_file(&workspace.join(name)).await {
            return Err(invalid(format!(
                "mise policy: {name} is missing from the workspace or is not a regular file"
            )));
        }
    }
    require_tracked_and_clean(ctx).await?;
    let read = async |name: &str| {
        tokio::fs::read_to_string(workspace.join(name))
            .await
            .map_err(|source| ExecError::io(format!("read {name}"), source))
    };
    let config: toml::Table = read(CONFIG)
        .await?
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
    read(LOCK).await
}

pub(super) async fn enforce_locked_mise_policy(
    ctx: &mut StepContext<'_>,
    install_args: &str,
) -> Result<(), ExecError> {
    let lock = resolve_locked_mise_config(ctx).await?;
    validate_install_args(install_args, &lock)
}
