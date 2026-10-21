//! Require agreement with the Mise Rust version when the project selects this option.
//!
//! The owner's standard layout keeps `mise.toml` and `rust-toolchain.toml`.
//! Set `[stacks.rust] toolchain_source = "mise"` to require a Mise Rust version.
//! Every other Rust version declaration must agree with it. These include
//! root and nested toolchain files, `.tool-versions`, and `.velnor/config.toml` pins.
//! Generation reports both source files and their versions when they differ.
//! A toolchain file without `channel` declares no version and is accepted.

use velnor_actions_contract::VelnorConfig;
use velnor_actions_contract::config::RustToolchainSource;
use velnor_actions_mise::PinnedTool;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::project_tool_sources::{
    SourceDeclarations, is_mise_config, read_nested_toolchains, read_project_sources,
};

/// Report a Rust version declaration that differs from the Mise version.
pub(crate) const DISAGREES_CODE: &str = "rust_version_disagrees_with_mise";
/// Report a missing Mise Rust declaration when the project requires one.
pub(crate) const MISSING_SOURCE_CODE: &str = "rust_not_declared_in_mise";

/// Call [`enforce_mise_authority`] when the config selects this option.
/// Otherwise, return without checking the sources.
///
/// # Errors
///
/// Returns the [`enforce_mise_authority`] errors, or a source read error.
pub(crate) fn enforce_when_opted_in(
    root: &std::path::Path,
    config: &VelnorConfig,
) -> Result<(), OrchestratorError> {
    if mise_authoritative(config) {
        enforce_mise_authority(root, config, &read_project_sources(root)?)?;
    }
    Ok(())
}

/// Check whether the config selects the Mise Rust version as its authority.
fn mise_authoritative(config: &VelnorConfig) -> bool {
    config
        .stacks
        .rust
        .as_ref()
        .and_then(|rust| rust.toolchain_source)
        == Some(RustToolchainSource::Mise)
}

/// Require a Mise Rust declaration and every other declaration to equal it.
///
/// # Errors
///
/// Returns [`MISSING_SOURCE_CODE`] when no Mise config declares Rust, or
/// [`DISAGREES_CODE`] naming the first source whose version differs.
fn enforce_mise_authority(
    root: &std::path::Path,
    config: &VelnorConfig,
    sources: &[SourceDeclarations],
) -> Result<(), OrchestratorError> {
    let Some((mise_file, mise_version)) = sources
        .iter()
        .filter(|source| is_mise_config(&source.file))
        .find_map(|source| rust_of(source).map(|version| (source.file.clone(), version)))
    else {
        return Err(OrchestratorError::config(
            "mise.toml",
            "tools.rust",
            format!(
                "{MISSING_SOURCE_CODE}: [stacks.rust] toolchain_source = \"mise\" requires an exact `rust` entry in mise.toml [tools]"
            ),
        ));
    };
    let named = format!("{mise_file}={mise_version}");
    let others = sources
        .iter()
        .filter(|source| !is_mise_config(&source.file))
        .cloned()
        .chain(read_nested_toolchains(root)?);
    for source in others {
        if let Some(version) = rust_of(&source).filter(|version| *version != mise_version) {
            return Err(OrchestratorError::config(
                source.file.as_str(),
                "toolchain.channel",
                disagreement(&source.file, &version, &named),
            ));
        }
    }
    let pinned = config
        .stacks
        .rust
        .as_ref()
        .and_then(|rust| rust.toolchain.as_deref())
        .map(|value| ("stacks.rust.toolchain", value))
        .or_else(|| config.tools.get("rust").map(|value| ("tools.rust", value)));
    if let Some((field, value)) = pinned.filter(|(_, value)| *value != mise_version) {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            field,
            disagreement(&format!("{CONFIG_REL} {field}"), value, &named),
        ));
    }
    Ok(())
}

fn rust_of(source: &SourceDeclarations) -> Option<String> {
    source
        .declarations
        .iter()
        .find(|declaration| declaration.tool == PinnedTool::Rust)
        .map(|declaration| declaration.value.clone())
}

fn disagreement(file: &str, value: &str, mise: &str) -> String {
    format!(
        "{DISAGREES_CODE}:{file}={value},{mise}; [stacks.rust] toolchain_source = \"mise\" requires agreement with the Mise Rust version. Set {file} to the same version."
    )
}

#[cfg(test)]
#[path = "tool_catalog_rust_source_tests.rs"]
mod tests;
