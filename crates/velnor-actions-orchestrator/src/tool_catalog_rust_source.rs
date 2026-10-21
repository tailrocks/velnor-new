//! Opt-in single Rust version source (`[stacks.rust] toolchain_source = "mise"`).
//!
//! Hard decision 4 accepts a Rust declaration from a Mise config, from
//! `.tool-versions`, and from a Rust toolchain file, as long as they agree.
//! A project that opts in makes its Mise config the only source: any other
//! Rust declaration fails generation, naming both files, instead of being
//! accepted because it happens to agree today.

use velnor_actions_contract::VelnorConfig;
use velnor_actions_contract::config::RustToolchainSource;
use velnor_actions_contract::config::project_tool_refs::{
    PROJECT_TOOL_VERSIONS_FILE, RUST_TOOLCHAIN_SOURCE_FILES,
};
use velnor_actions_mise::PinnedTool;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::project_tool_sources::{
    SourceDeclarations, is_mise_config, read_nested_toolchains, read_project_sources,
};

/// Stable code for a second Rust version source next to the Mise one.
pub(crate) const SECOND_SOURCE_CODE: &str = "rust_version_source_not_mise";
/// Stable code for an opted-in project whose Mise config declares no Rust.
pub(crate) const MISSING_SOURCE_CODE: &str = "rust_not_declared_in_mise";

/// Enforce [`enforce_mise_only`] when the config opts in; a no-op otherwise.
///
/// # Errors
///
/// Returns the [`enforce_mise_only`] errors, or a source read error.
pub(crate) fn enforce_when_opted_in(
    root: &std::path::Path,
    config: &VelnorConfig,
) -> Result<(), OrchestratorError> {
    if mise_only(config) {
        enforce_mise_only(root, config, &read_project_sources(root)?)?;
    }
    Ok(())
}

/// Whether the config opts into the Mise-only Rust source.
fn mise_only(config: &VelnorConfig) -> bool {
    config
        .stacks
        .rust
        .as_ref()
        .and_then(|rust| rust.toolchain_source)
        == Some(RustToolchainSource::Mise)
}

/// Enforce the Mise-only Rust source over every parsed root source, every
/// nested toolchain file, and the `.velnor/config.toml` Rust pins.
///
/// # Errors
///
/// Returns [`SECOND_SOURCE_CODE`] naming the conflicting file and the Mise
/// declaration, or [`MISSING_SOURCE_CODE`] when no Mise config declares Rust.
fn enforce_mise_only(
    root: &std::path::Path,
    config: &VelnorConfig,
    sources: &[SourceDeclarations],
) -> Result<(), OrchestratorError> {
    let mise = sources
        .iter()
        .filter(|source| is_mise_config(&source.file))
        .find_map(|source| {
            source
                .declarations
                .iter()
                .find(|declaration| declaration.tool == PinnedTool::Rust)
                .map(|declaration| format!("{}={}", source.file, declaration.value))
        });
    let named = mise.as_deref().unwrap_or("undeclared");
    for source in sources {
        let rust = source
            .declarations
            .iter()
            .find(|declaration| declaration.tool == PinnedTool::Rust);
        let toolchain_file = RUST_TOOLCHAIN_SOURCE_FILES.contains(&source.file.as_str());
        if toolchain_file || (source.file == PROJECT_TOOL_VERSIONS_FILE && rust.is_some()) {
            let value = rust.map_or("present", |declaration| declaration.value.as_str());
            return Err(second_source(&source.file, value, named));
        }
    }
    if let Some(source) = read_nested_toolchains(root)?.into_iter().next() {
        let value = source
            .declarations
            .first()
            .map_or("present", |declaration| declaration.value.as_str())
            .to_owned();
        return Err(second_source(&source.file, &value, named));
    }
    let pinned = config
        .stacks
        .rust
        .as_ref()
        .and_then(|rust| rust.toolchain.as_deref())
        .map(|value| ("stacks.rust.toolchain", value))
        .or_else(|| config.tools.get("rust").map(|value| ("tools.rust", value)));
    if let Some((field, value)) = pinned {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            field,
            format!(
                "{SECOND_SOURCE_CODE}:{CONFIG_REL} {field}={value},{named}; [stacks.rust] toolchain_source = \"mise\" makes the Mise config the only Rust version source: remove {field}"
            ),
        ));
    }
    if mise.is_none() {
        return Err(OrchestratorError::config(
            "mise.toml",
            "tools.rust",
            format!(
                "{MISSING_SOURCE_CODE}: [stacks.rust] toolchain_source = \"mise\" requires an exact `rust` entry in mise.toml [tools]"
            ),
        ));
    }
    Ok(())
}

fn second_source(file: &str, value: &str, mise: &str) -> OrchestratorError {
    OrchestratorError::config(
        file,
        "toolchain.channel",
        format!(
            "{SECOND_SOURCE_CODE}:{file}={value},{mise}; [stacks.rust] toolchain_source = \"mise\" makes the Mise config the only Rust version source: delete {file} and keep the version, profile, components and targets in mise.toml"
        ),
    )
}

#[cfg(test)]
#[path = "tool_catalog_rust_source_tests.rs"]
mod tests;
