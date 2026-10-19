//! Resolve effective [`ToolCatalog`] from repository toolchain files and `.velnor/config.toml`.
//!
//! Enforces fail-closed agreement across `rust-toolchain.toml`, legacy
//! `rust-toolchain`, `[stacks.rust] toolchain`, and `[tools]`, omitting
//! `RUSTUP_TOOLCHAIN` from step environments when a repository toolchain
//! file is present so rustup honors the repository file directly.
//!
//! Every catalog tool the project's Mise config declares (plus Rust from a
//! toolchain file) takes the project's version and is recorded as
//! project-declared (hard decision 4, [`crate::tool_catalog_project`]);
//! `.velnor/config.toml` pins apply only to undeclared tools and must agree
//! with a declaration otherwise.
//!
//! The repository `mise.toml` rust record's `mr_boxington` option is read
//! too: when it selects mise's exec-time cargo→MBX wrapper, the resolved
//! catalog carries the repository's `mr-boxington` pin so the pinned-tool
//! preflight installs it (missing installs fall back to the action-owned
//! PATH `mbx`, which may be a different version).

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::VelnorConfig;
use velnor_actions_mise::catalog::{
    ACTIONLINT_VERSION, GH_VERSION, MR_BOXINGTON_VERSION, NEXTEST_VERSION, OPENTOFU_VERSION,
    RUST_VERSION, SHELLCHECK_VERSION, ZIZMOR_VERSION,
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
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};
use crate::tool_catalog_project::{ToolchainFile, check_config_agrees, project_declarations};

/// Resolve the effective [`ToolCatalog`] for a repository root and parsed [`VelnorConfig`].
///
/// # Errors
///
/// Returns [`OrchestratorError::Config`] when a repository toolchain file has
/// an invalid or non-`X.Y.Z` channel, when `rust-toolchain.toml` and
/// `rust-toolchain` disagree, or when a repository toolchain file conflicts
/// with `[stacks.rust] toolchain` or `[tools] rust`. A repository `mise.toml`
/// rust record with `mr_boxington = true` additionally requires an exact
/// `[tools] mr-boxington` record; a missing or inexact record, or a record
/// pin disagreeing with a `.velnor/config.toml [tools] mr-boxington`
/// override, fails closed.
pub(crate) fn resolve_tool_catalog(
    root: &Path,
    config: &VelnorConfig,
) -> Result<ToolCatalog, OrchestratorError> {
    let toml_tc = read_toolchain_file(root, RUST_TOOLCHAIN_FILE)?;
    let legacy_tc = read_toolchain_file(root, RUST_TOOLCHAIN_LEGACY_FILE)?;
    if let (Some((_, raw_toml, _, _)), Some((_, raw_legacy, _, _))) = (&toml_tc, &legacy_tc)
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
    if let (Some((file_path, _, file_version, _)), Some((cfg_field, cfg_channel))) =
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
    let repo_mise = read_repo_mise_tools(root)?;
    let rust_mr_boxington = repo_mise.as_ref().is_some_and(|tools| {
        tools.lenient_mbx_option
            || tools
                .tools
                .get("rust")
                .is_some_and(|rust| rust.mr_boxington == Some(true))
    });
    if rust_mr_boxington {
        resolve_wrapped_mr_boxington_pin(repo_mise.as_ref(), config.tools.get("mr-boxington"))?;
    }
    let empty = BTreeMap::new();
    let declared = project_declarations(
        repo_mise.as_ref().map_or(&empty, |tools| &tools.tools),
        file_tc.as_ref(),
    );
    let pin = |tool: PinnedTool, field: &str, default: &str| -> Result<String, OrchestratorError> {
        let (config_field, config_pin) = if tool == PinnedTool::Rust {
            config_tc.map_or((format!("tools.{field}"), None), |(cfg_field, channel)| {
                (cfg_field.to_owned(), Some(channel))
            })
        } else {
            (format!("tools.{field}"), config.tools.get(field))
        };
        match declared.get(&tool) {
            Some(Some(version)) => {
                check_config_agrees(version, &config_field, config_pin)?;
                Ok(version.clone())
            }
            // Declared but unusable: generation executes on the fallback pin
            // locally; emitted text still references the project's value.
            Some(None) | None => Ok(config_pin.unwrap_or(default).to_owned()),
        }
    };
    let catalog = ToolCatalog::new(
        &pin(PinnedTool::Rust, "rust", RUST_VERSION)?,
        &pin(
            PinnedTool::MrBoxington,
            "mr-boxington",
            MR_BOXINGTON_VERSION,
        )?,
        &pin(PinnedTool::Gh, "gh", GH_VERSION)?,
        &pin(PinnedTool::Actionlint, "actionlint", ACTIONLINT_VERSION)?,
        &pin(PinnedTool::Shellcheck, "shellcheck", SHELLCHECK_VERSION)?,
        &pin(PinnedTool::Zizmor, "zizmor", ZIZMOR_VERSION)?,
        &pin(PinnedTool::Nextest, "cargo-nextest", NEXTEST_VERSION)?,
        &pin(PinnedTool::Opentofu, "opentofu", OPENTOFU_VERSION)?,
    )
    .map_err(contract)?;
    let mut catalog = catalog
        .with_export_rustup_toolchain(export_rustup_toolchain)
        .with_rust_mr_boxington(rust_mr_boxington);
    for tool in declared.keys() {
        let version = catalog.version(*tool).to_owned();
        catalog = catalog
            .with_project_tool(*tool, &version)
            .map_err(contract)?;
    }
    Ok(catalog)
}

fn contract(err: velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: err.to_string(),
    }
}

/// Repository-root `mise.toml` tool records the catalog must agree with.
struct RepoMiseTools {
    /// Repository-relative file the records were read from.
    file: &'static str,
    /// Every `[tools]` record by key.
    tools: BTreeMap<String, NativeToolSelector>,
    /// Line-local wrapper detection over bytes too malformed to parse as
    /// TOML — the exact predicate that feeds P06 driver evidence, so the
    /// two readers of the same file cannot disagree.
    lenient_mbx_option: bool,
}

/// Read the repository-root Mise tool records (`mise.toml`, then `.mise.toml`).
///
/// Recorded decision — first parseable file wins: mise merges every
/// config file it loads, but this reader deliberately takes only the
/// first parseable spelling (the same precedence the profile-evidence
/// reader in `evidence.rs::tool_config_input` applies) and never merges
/// records across the two. Repositories that rely on per-file merging
/// must keep the `rust` and `mr-boxington` records in the first file.
///
/// Malformed bytes contribute no records and never fail resolution on
/// their own (tooling-input contract: a malformed `mise.toml` stays a
/// read-only input reported as a `tooling_input_invalid` finding with a
/// manual suggestion); the reader keeps walking to the next spelling.
/// The wrapper option is still detected line-locally in malformed bytes
/// through [`velnor_actions_rust::tool_config_selects_mbx`] — the same
/// predicate the P06 driver detector reads — so a malformed file that
/// selects the wrapper cannot degrade to a silent flag-off while
/// detection picks the MBX driver; the missing exact pin then fails
/// closed in [`resolve_wrapped_mr_boxington_pin`]. No readable file
/// returns `None`.
fn read_repo_mise_tools(root: &Path) -> Result<Option<RepoMiseTools>, OrchestratorError> {
    let mut lenient_mbx_option = false;
    for file in [MISE_TOML_FILE, DOT_MISE_TOML_FILE] {
        let RepoRead::Text(text) = read_repo_file(root, file, MAX_REPO_FILE_BYTES)? else {
            continue;
        };
        let Ok(value) = toml::from_str::<toml::Value>(&text) else {
            lenient_mbx_option |= velnor_actions_rust::tool_config_selects_mbx(&text);
            continue;
        };
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
        return Ok(Some(RepoMiseTools {
            file,
            tools,
            lenient_mbx_option,
        }));
    }
    Ok((lenient_mbx_option).then_some(RepoMiseTools {
        file: MISE_TOML_FILE,
        tools: BTreeMap::new(),
        lenient_mbx_option: true,
    }))
}

/// Resolve the mise-managed `mr-boxington` pin the preflight must install.
///
/// With the repository rust record's `mr_boxington = true`, mise's
/// exec-time wrapper dispatches cargo through the mise-managed
/// `mr-boxington` tool at the repository's own pin, so that pin is
/// mandatory: an exact, os-unfiltered `X.Y.Z` `[tools] mr-boxington`
/// record must exist in the same file (mise itself rejects the config
/// otherwise). An os-restricted record would install on the repository's
/// listed systems only, so the fleet's preflight could not rely on it.
/// A `.velnor/config.toml [tools] mr-boxington` override names the same
/// tool; the repository record governs the exec-time dispatch, so the
/// override must match it exactly — agreement means the shared pin wins,
/// disagreement is ambiguous (the preflight would install one pin while
/// the wrapper dispatches the other) and fails closed naming both.
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

/// Fail-closed error for a rust record opting into the wrapper without a
/// repository `mr-boxington` record to dispatch through.
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
    let is_toml = toml::from_str::<toml::Value>(&text)
        .ok()
        .is_some_and(|value| value.get("toolchain").is_some());
    Ok(Some((rel, raw_channel, version, is_toml)))
}

#[cfg(test)]
#[path = "tool_catalog_resolve_tests.rs"]
mod tests;
