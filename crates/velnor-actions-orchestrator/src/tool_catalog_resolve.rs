// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::VelnorConfig;
use velnor_actions_mise::catalog::{
    GH_VERSION, JACTIONLINT_VERSION, MR_BOXINGTON_VERSION, NEXTEST_VERSION, OPENTOFU_VERSION,
    RUST_VERSION, SHELLCHECK_VERSION,
};
use velnor_actions_mise::{
    DOT_MISE_TOML_FILE, MISE_TOML_FILE, PinnedTool, ToolCatalog, validate_exact_version,
};
use velnor_actions_rust::{
    RUST_TOOLCHAIN_FILE, RUST_TOOLCHAIN_LEGACY_FILE, extract_rust_toolchain_channel,
    inspect_toolchain_file,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::native_tool_input::{NativeToolSelector, parse_native_tool_selector};
use crate::project_tool_sources::{read_nested_toolchains, read_project_sources};
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};
use crate::tool_catalog_project::{PinInputs, apply_declarations, contract, project_declarations};

type ToolchainFile = (&'static str, String, String);

pub(crate) fn resolve_tool_catalog(
    root: &Path,
    config: &VelnorConfig,
) -> Result<ToolCatalog, OrchestratorError> {
    rust_source::enforce_when_opted_in(root, config)?;
    let toml_tc = read_toolchain_file(root, RUST_TOOLCHAIN_FILE)?;
    let legacy_tc = read_toolchain_file(root, RUST_TOOLCHAIN_LEGACY_FILE)?;
    if let (Some((_, raw_toml, _)), Some((_, raw_legacy, _))) = (&toml_tc, &legacy_tc)
        && raw_toml != raw_legacy
    {
        return Err(OrchestratorError::config(
            RUST_TOOLCHAIN_FILE,
            "toolchain.channel",
            format!(
                "conflicting_rust_toolchain_files:{RUST_TOOLCHAIN_FILE}={raw_toml},{RUST_TOOLCHAIN_LEGACY_FILE}={raw_legacy}"
            ),
        ));
    }
    let file_tc = toml_tc.or(legacy_tc);
    let config_tc = config
        .stacks
        .rust
        .as_ref()
        .and_then(|rust| rust.toolchain.as_deref())
        .map(|tc| ("stacks.rust.toolchain", tc))
        .or_else(|| config.tools.get("rust").map(|tc| ("tools.rust", tc)));
    if let (Some((file_path, _, file_version)), Some((cfg_field, cfg_channel))) =
        (&file_tc, config_tc)
        && file_version != cfg_channel
    {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            cfg_field,
            format!(
                "conflicting_rust_toolchain:{file_path}={file_version},{cfg_field}={cfg_channel}"
            ),
        ));
    }
    let export_rustup_toolchain = file_tc.is_none();
    let sources = read_project_sources(root)?;
    let repo_mise = read_repo_mise_tools(root)?;
    let rust_mr_boxington = repo_mise.as_ref().is_some_and(|tools| {
        tools
            .tools
            .get("rust")
            .is_some_and(|rust| rust.mr_boxington == Some(true))
    });
    if rust_mr_boxington {
        resolve_wrapped_mr_boxington_pin(repo_mise.as_ref(), config.tools.get("mr-boxington"))?;
    }
    let declared = project_declarations(&sources)?;
    reject_divergent_nested_toolchains(root, declared.get(&PinnedTool::Rust))?;
    rust_mirror::check_toolchain_mirror(root)?;
    let pins = PinInputs {
        declared: &declared,
        config,
        config_tc,
    };
    let pin = |tool, field, default| pins.version(tool, field, default);
    let catalog = ToolCatalog::new(
        &pin(PinnedTool::Rust, "rust", RUST_VERSION)?,
        &pin(
            PinnedTool::MrBoxington,
            "mr-boxington",
            MR_BOXINGTON_VERSION,
        )?,
        &pin(PinnedTool::Gh, "gh", GH_VERSION)?,
        &pin(PinnedTool::Jactionlint, "jactionlint", JACTIONLINT_VERSION)?,
        &pin(PinnedTool::Shellcheck, "shellcheck", SHELLCHECK_VERSION)?,
        &pin(PinnedTool::Nextest, "cargo-nextest", NEXTEST_VERSION)?,
        &pin(PinnedTool::Opentofu, "opentofu", OPENTOFU_VERSION)?,
    )
    .map_err(|err| contract(&err))?;
    apply_declarations(
        catalog
            .with_export_rustup_toolchain(export_rustup_toolchain)
            .with_rust_mr_boxington(rust_mr_boxington),
        &declared,
    )
}

fn reject_divergent_nested_toolchains(
    root: &Path,
    root_rust: Option<&String>,
) -> Result<(), OrchestratorError> {
    for source in read_nested_toolchains(root)? {
        for declaration in source.declarations {
            if root_rust != Some(&declaration.value) {
                return Err(OrchestratorError::config(
                    source.file.as_str(),
                    "toolchain.channel",
                    format!(
                        "nested_rust_toolchain_not_supported:{}={},root={}",
                        source.file,
                        declaration.value,
                        root_rust.map_or("undeclared", String::as_str)
                    ),
                ));
            }
        }
    }
    Ok(())
}

struct RepoMiseTools {
    file: &'static str,
    tools: BTreeMap<String, NativeToolSelector>,
}

fn read_repo_mise_tools(root: &Path) -> Result<Option<RepoMiseTools>, OrchestratorError> {
    for file in [MISE_TOML_FILE, DOT_MISE_TOML_FILE] {
        let RepoRead::Text(text) = read_repo_file(root, file, MAX_REPO_FILE_BYTES)? else {
            continue;
        };
        let value = toml::from_str::<toml::Value>(&text).map_err(|err| {
            OrchestratorError::config(
                file,
                "tools",
                format!("malformed_project_tool_source:not valid TOML: {err}"),
            )
        })?;
        let tools = value
            .get("tools")
            .and_then(toml::Value::as_table)
            .map(|table| {
                table
                    .iter()
                    .map(|(key, value)| (key.clone(), parse_native_tool_selector(value)))
                    .collect()
            })
            .unwrap_or_default();
        return Ok(Some(RepoMiseTools { file, tools }));
    }
    Ok(None)
}

fn resolve_wrapped_mr_boxington_pin(
    repo_mise: Option<&RepoMiseTools>,
    config_override: Option<&str>,
) -> Result<String, OrchestratorError> {
    let Some(tools) = repo_mise else {
        return Err(missing_mr_boxington_record(MISE_TOML_FILE));
    };
    let selector = tools.tools.get("mr-boxington");
    let Some(pin) = selector.and_then(|selector| selector.version.as_deref()) else {
        return Err(missing_mr_boxington_record(tools.file));
    };
    if let Some(os) = selector.and_then(|selector| selector.os.as_ref()) {
        return Err(OrchestratorError::config(
            tools.file,
            "tools.mr-boxington",
            format!(
                "rust_mr_boxington_requires_unfiltered_pin:os={}",
                os.join(",")
            ),
        ));
    }
    if validate_exact_version(PinnedTool::MrBoxington.tool_name(), pin).is_err() {
        return Err(OrchestratorError::config(
            tools.file,
            "tools.mr-boxington",
            format!("rust_mr_boxington_requires_exact_pin:{pin}"),
        ));
    }
    if let Some(override_pin) = config_override
        && override_pin != pin
    {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "tools.mr-boxington",
            format!(
                "ambiguous_mr_boxington_pin:{}={pin},{CONFIG_REL}={override_pin}",
                tools.file
            ),
        ));
    }
    Ok(pin.to_owned())
}

fn missing_mr_boxington_record(file: &str) -> OrchestratorError {
    OrchestratorError::config(
        file,
        "tools.mr-boxington",
        "rust_mr_boxington_requires_mr_boxington_record",
    )
}

fn read_toolchain_file(
    root: &Path,
    rel: &'static str,
) -> Result<Option<ToolchainFile>, OrchestratorError> {
    let RepoRead::Text(text) = read_repo_file(root, rel, MAX_REPO_FILE_BYTES)? else {
        return Ok(None);
    };
    let version = extract_rust_toolchain_channel(rel, &text).map_err(|problem| {
        OrchestratorError::config(
            rel,
            "toolchain.channel",
            format!("invalid_rust_toolchain:{problem}"),
        )
    })?;
    let raw_channel = inspect_toolchain_file(rel, Some(&text))
        .ok()
        .and_then(|insp| insp.spec)
        .and_then(|spec| spec.channel)
        .unwrap_or_else(|| version.clone());
    Ok(Some((rel, raw_channel, version)))
}

#[path = "tool_catalog_rust_mirror.rs"]
pub(crate) mod rust_mirror;

#[path = "tool_catalog_rust_source.rs"]
pub(crate) mod rust_source;
