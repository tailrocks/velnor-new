// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::VelnorConfig;
use velnor_actions_contract::config::RustToolchainSource;
use velnor_actions_mise::PinnedTool;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::project_tool_sources::{
    SourceDeclarations, is_mise_config, read_nested_toolchains, read_project_sources,
};

pub(crate) const DISAGREES_CODE: &str = "rust_version_disagrees_with_mise";
pub(crate) const MISSING_SOURCE_CODE: &str = "rust_not_declared_in_mise";

pub(crate) fn enforce_when_opted_in(
    root: &std::path::Path,
    config: &VelnorConfig,
) -> Result<(), OrchestratorError> {
    if mise_authoritative(config) {
        enforce_mise_authority(root, config, &read_project_sources(root)?)?;
    }
    Ok(())
}

fn mise_authoritative(config: &VelnorConfig) -> bool {
    config
        .stacks
        .rust
        .as_ref()
        .and_then(|rust| rust.toolchain_source)
        == Some(RustToolchainSource::Mise)
}

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
        "{DISAGREES_CODE}:{file}={value},{mise}; [stacks.rust] toolchain_source = \"mise\" makes the mise.toml rust version the authority: set {file} to the same version"
    )
}
