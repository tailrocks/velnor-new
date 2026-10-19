//! Project-declared catalog tools (hard decision 4).
//!
//! A catalog tool the repository-root Mise config declares in `[tools]`
//! takes the project's exact version; a Rust toolchain file is a Rust
//! declaration too (Mise and rustup both read it). Generated workflows
//! never copy these versions: the emission catalog renders them as run-time
//! references, and the generated resolver reads the same sources at run
//! time. Only tools absent from the project config keep a Velnor pin.

use std::collections::BTreeMap;

use velnor_actions_mise::{PinnedTool, validate_concrete_exact_version};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::native_tool_input::NativeToolSelector;

/// One project declaration: the exact version every readable source agrees
/// on, or `None` when a source is loose, malformed, or conflicting. A
/// `None` declaration still renders as a run-time reference (the resolver
/// then fails with the project's own value, and the tooling-input finding
/// reports it); generation never substitutes a Velnor pin for it.
pub(crate) type ProjectDeclaration = Option<String>;

/// Toolchain-file Rust channel: `(file, raw channel, exact version, is TOML)`.
pub(crate) type ToolchainFile = (&'static str, String, String, bool);

/// Collect every catalog tool the project declares.
///
/// `tools` is the first parseable root Mise config's `[tools]` table; any
/// backend spelling of a catalog tool declares it.
#[must_use]
pub(crate) fn project_declarations(
    tools: &BTreeMap<String, NativeToolSelector>,
    toolchain: Option<&ToolchainFile>,
) -> BTreeMap<PinnedTool, ProjectDeclaration> {
    let mut out = BTreeMap::new();
    for tool in PinnedTool::ALL {
        let mut versions: Vec<Option<String>> = tool
            .mise_config_keys()
            .iter()
            .filter_map(|key| tools.get(*key))
            .filter_map(|selector| match declared_version(tool, selector) {
                Declared::Exact(version) => Some(Some(version)),
                Declared::Unusable => Some(None),
                Declared::NoVersion => None,
            })
            .collect();
        if tool == PinnedTool::Rust
            && let Some((_, _, exact, _)) = toolchain
        {
            versions.push(Some(exact.clone()));
        }
        let Some(first) = versions.first().cloned() else {
            continue;
        };
        let agreed = first.filter(|version| versions.iter().all(|v| v.as_ref() == Some(version)));
        out.insert(tool, agreed);
    }
    out
}

/// Reject a `.velnor/config.toml` pin that disagrees with a project declaration.
///
/// # Errors
///
/// Returns [`OrchestratorError::Config`] naming both values.
pub(crate) fn check_config_agrees(
    declared: &str,
    field: &str,
    config_pin: Option<&str>,
) -> Result<(), OrchestratorError> {
    match config_pin {
        Some(pin) if pin != declared => Err(OrchestratorError::config(
            CONFIG_REL,
            field,
            format!("conflicting_project_tool_pin:project={declared},{field}={pin}"),
        )),
        _ => Ok(()),
    }
}

/// One `[tools]` record's version declaration.
enum Declared {
    /// Exact `X.Y.Z` version.
    Exact(String),
    /// Loose, malformed, or templated version.
    Unusable,
    /// Options-only table (`rust = { mr_boxington = true }`): no version.
    NoVersion,
}

fn declared_version(tool: PinnedTool, selector: &NativeToolSelector) -> Declared {
    let Some(version) = selector.version.as_deref() else {
        return if selector.unsupported_options.is_empty() {
            Declared::NoVersion
        } else {
            Declared::Unusable
        };
    };
    if validate_concrete_exact_version(tool.tool_name(), version).is_ok() {
        Declared::Exact(version.to_owned())
    } else {
        Declared::Unusable
    }
}

/// Argv and toolchain identity that bind one task to its generated record.
///
/// The generated record carries project version references (hard decision
/// 4), so the plan binds the task digest over the emission catalog while
/// identity, closure, and reuse gates keep the concrete versions: a
/// `mise.toml` bump invalidates coverage without regenerating the workflow.
///
/// # Errors
///
/// Returns contract errors for tasks outside a registered stack.
pub(crate) fn task_binding(
    task: &velnor_actions_contract::ProposedTask,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(Vec<String>, String), OrchestratorError> {
    let emission = catalog.for_emission();
    let toolchain = crate::internal_plan::toolchain_id(task, &emission)
        .map_err(crate::internal::internal_contract)?;
    Ok((crate::vectors::task_argv(task, &emission)?, toolchain))
}
