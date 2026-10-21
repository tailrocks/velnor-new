//! `rust-toolchain.toml` mirrors the `mise.toml` rust entry (owner layout).
//!
//! The owner's standard layout keeps both files: Mise reads only its own
//! `rust` entry (version, `components`, `targets`, `profile`; a toolchain
//! file is ignored once `mise.toml` pins Rust), while rustup-only consumers
//! (editors, rust-analyzer, hooks outside Mise, Docker `rust:` images) read
//! only the toolchain file. The two therefore have to be identical, not just
//! agree on the version (hard decision 4 already compares versions). When
//! both declare Rust, generation fails with
//! `rust_toolchain_mirror_mismatch:<key>` naming both values.

use std::path::Path;

use velnor_actions_mise::{DOT_MISE_TOML_FILE, MISE_TOML_FILE};
use velnor_actions_rust::RUST_TOOLCHAIN_FILE;

use crate::OrchestratorError;
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

/// Fail when `rust-toolchain.toml` and the Mise rust entry differ in
/// `channel`/`version`, `components`, `targets` or `profile`.
///
/// # Errors
///
/// Returns [`MIRROR_CODE`] naming the first differing key, or a read or
/// parse error for either file.
pub(crate) fn check_toolchain_mirror(root: &Path) -> Result<(), OrchestratorError> {
    let Some(toolchain) = read_toml(root, RUST_TOOLCHAIN_FILE)? else {
        return Ok(());
    };
    let Some(table) = toolchain.get("toolchain").and_then(toml::Value::as_table) else {
        return Ok(());
    };
    let file_shape = RustShape {
        channel: string(table.get("channel")),
        components: list(table.get("components")),
        targets: list(table.get("targets")),
        profile: string(table.get("profile")),
    };
    for mise_file in [MISE_TOML_FILE, DOT_MISE_TOML_FILE] {
        let Some(mise) = read_toml(root, mise_file)? else {
            continue;
        };
        let Some(rust) = mise.get("tools").and_then(|tools| tools.get("rust")) else {
            continue;
        };
        let mise_shape = match rust {
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
        return compare(&file_shape, &mise_shape, mise_file);
    }
    Ok(())
}

fn compare(file: &RustShape, mise: &RustShape, mise_file: &str) -> Result<(), OrchestratorError> {
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
                RUST_TOOLCHAIN_FILE,
                format!("toolchain.{key}"),
                format!(
                    "{MIRROR_CODE}:{key}:{RUST_TOOLCHAIN_FILE}=[{in_file}],{mise_file}=[{in_mise}]; rust-toolchain.toml mirrors the {mise_file} rust entry: keep version/channel, components, targets and profile identical in both"
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

fn list(value: Option<&toml::Value>) -> Vec<String> {
    let mut items: Vec<String> = match value {
        Some(toml::Value::Array(items)) => {
            items.iter().filter_map(|item| string(Some(item))).collect()
        }
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
