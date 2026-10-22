//! Rust toolchain files mirror the Mise rust entry (owner layout).
//!
//! The owner's standard layout keeps both: Mise reads only its own `rust`
//! entry (version, `components`, `targets`, `profile`; a toolchain file is
//! ignored once Mise pins Rust), while rustup-only consumers (editors,
//! rust-analyzer, hooks outside Mise, Docker `rust:` images) read only the
//! toolchain file. The two therefore have to be identical, not just agree on
//! the version (hard decision 4 already compares versions). When both
//! declare Rust, generation fails with `rust_toolchain_mirror_mismatch:<key>`
//! naming both files and values.

use std::path::Path;

use velnor_actions_contract::config::project_tool_refs::RUST_TOOLCHAIN_SOURCE_FILES;

use crate::OrchestratorError;
use crate::project_tool_sources::{is_mise_config, nested_toolchain_paths, source_paths};
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// Stable code for a toolchain file that drifted from the Mise rust entry.
pub(crate) const MIRROR_CODE: &str = "rust_toolchain_mirror_mismatch";

/// The compared shape of one Rust declaration.
#[derive(Debug, Default, PartialEq, Eq)]
struct RustShape {
    channel: Option<String>,
    components: Vec<String>,
    targets: Vec<String>,
    profile: Option<String>,
}

/// Fail when any Rust toolchain file differs from any Mise rust entry in
/// `channel`/`version`, `components`, `targets` or `profile`.
///
/// The Mise side is every config file Mise itself reads (the same file set
/// and order as [`source_paths`], so `.config/mise.toml`, `mise/config.toml`,
/// `.mise/config.toml`, `conf.d` and `mise.<env>.toml` count). The toolchain
/// side is every `rust-toolchain.toml` and legacy `rust-toolchain`, at the
/// root or nested, which is every file the upgrade job rewrites.
///
/// # Errors
///
/// Returns [`MIRROR_CODE`] naming the first differing key and both files,
/// or a read or parse error for either file.
pub(crate) fn check_toolchain_mirror(root: &Path) -> Result<(), OrchestratorError> {
    let mut mise_entries = Vec::new();
    for file in source_paths(root)? {
        if !is_mise_config(&file) {
            continue;
        }
        let Some(mise) = read_toml(root, &file)? else {
            continue;
        };
        let Some(tools) = mise.get("tools") else {
            continue;
        };
        let Some(rust) = tools.get("rust").or_else(|| tools.get("core:rust")) else {
            continue;
        };
        let shape = match rust {
            toml::Value::Table(entry) => RustShape {
                channel: string(entry.get("version")),
                components: list(entry.get("components")),
                targets: list(entry.get("targets")),
                profile: string(entry.get("profile")),
            },
            other => RustShape {
                channel: string(Some(other)),
                ..RustShape::default()
            },
        };
        mise_entries.push((file, shape));
    }
    if mise_entries.is_empty() {
        return Ok(());
    }
    let mut toolchain_files: Vec<String> = RUST_TOOLCHAIN_SOURCE_FILES
        .iter()
        .map(|file| (*file).to_owned())
        .collect();
    toolchain_files.extend(nested_toolchain_paths(root));
    for file in toolchain_files {
        let Some(shape) = toolchain_shape(root, &file)? else {
            continue;
        };
        for (mise_file, mise_shape) in &mise_entries {
            compare(&shape, &file, mise_shape, mise_file)?;
        }
    }
    Ok(())
}

/// One toolchain file's shape: the `[toolchain]` table, or a legacy plain
/// `rust-toolchain` that names only a channel.
fn toolchain_shape(root: &Path, file: &str) -> Result<Option<RustShape>, OrchestratorError> {
    let RepoRead::Text(text) = read_repo_file(root, file, MAX_REPO_FILE_BYTES)? else {
        return Ok(None);
    };
    let toml_named = Path::new(file)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"));
    let legacy_plain = !toml_named && !text.lines().any(|line| line.trim_start().starts_with('['));
    if legacy_plain {
        let channel = text.trim();
        return Ok((!channel.is_empty()).then(|| RustShape {
            channel: Some(channel.to_owned()),
            ..RustShape::default()
        }));
    }
    let value: toml::Value = toml::from_str(&text).map_err(|err| {
        OrchestratorError::config(
            file,
            "toolchain",
            format!("malformed_project_tool_source:not valid TOML: {err}"),
        )
    })?;
    let Some(table) = value.get("toolchain").and_then(toml::Value::as_table) else {
        return Ok(None);
    };
    Ok(Some(RustShape {
        channel: string(table.get("channel")),
        components: list(table.get("components")),
        targets: list(table.get("targets")),
        profile: string(table.get("profile")),
    }))
}

fn compare(
    file: &RustShape,
    file_name: &str,
    mise: &RustShape,
    mise_file: &str,
) -> Result<(), OrchestratorError> {
    let keys: [(&str, String, String); 4] = [
        (
            "channel",
            show(file.channel.as_ref()),
            show(mise.channel.as_ref()),
        ),
        (
            "components",
            file.components.join(","),
            mise.components.join(","),
        ),
        ("targets", file.targets.join(","), mise.targets.join(",")),
        (
            "profile",
            show(file.profile.as_ref()),
            show(mise.profile.as_ref()),
        ),
    ];
    for (key, in_file, in_mise) in keys {
        if in_file != in_mise {
            return Err(OrchestratorError::config(
                file_name,
                format!("toolchain.{key}"),
                format!(
                    "{MIRROR_CODE}:{key}:{file_name}=[{in_file}],{mise_file}=[{in_mise}]; {file_name} mirrors the {mise_file} rust entry: keep version/channel, components, targets and profile identical in both (a legacy plain rust-toolchain carries only a channel, so use rust-toolchain.toml)"
                ),
            ));
        }
    }
    Ok(())
}

fn read_toml(root: &Path, file: &str) -> Result<Option<toml::Value>, OrchestratorError> {
    let RepoRead::Text(text) = read_repo_file(root, file, MAX_REPO_FILE_BYTES)? else {
        return Ok(None);
    };
    toml::from_str(&text).map(Some).map_err(|err| {
        OrchestratorError::config(
            file,
            "toolchain",
            format!("malformed_project_tool_source:not valid TOML: {err}"),
        )
    })
}

fn string(value: Option<&toml::Value>) -> Option<String> {
    value.map(|value| match value {
        toml::Value::String(text) => text.clone(),
        other => other.to_string(),
    })
}

/// Sorted, de-duplicated items; Mise also documents a comma-separated
/// string (`components = "clippy,rustfmt"`), which splits the same way.
fn list(value: Option<&toml::Value>) -> Vec<String> {
    let mut items: Vec<String> = match value {
        Some(toml::Value::Array(items)) => {
            items.iter().filter_map(|item| string(Some(item))).collect()
        }
        Some(toml::Value::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(other) => vec![other.to_string()],
        None => Vec::new(),
    };
    items.sort();
    items.dedup();
    items
}

fn show(value: Option<&String>) -> String {
    value.cloned().unwrap_or_default()
}

#[cfg(test)]
#[path = "tool_catalog_rust_mirror_tests.rs"]
mod tests;
