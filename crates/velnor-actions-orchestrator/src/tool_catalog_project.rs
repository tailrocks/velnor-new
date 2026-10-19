//! Project-declared catalog tools (hard decision 5).
//!
//! A catalog tool any project source declares ([`crate::project_tool_sources`]:
//! every Mise config location, `.tool-versions`, and the Rust toolchain
//! files) takes the project's exact version. Generated workflows
//! never copy these versions: the emission catalog renders them as run-time
//! references, and the generated resolver reads the same sources at run
//! time. Only tools absent from the project config keep a Velnor pin.

use std::collections::BTreeMap;

use velnor_actions_contract::config::project_tool_refs::is_exact_project_version;
use velnor_actions_mise::PinnedTool;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::project_tool_sources::SourceDeclarations;

/// Collect every catalog tool the project declares.
///
/// Any backend spelling of a catalog tool in any source declares it. Every
/// declared value must be an exact `X.Y.Z` and every source must agree: the
/// generated resolver enforces the same at run time, so a project whose
/// config could never resolve fails here, naming the tool and the file,
/// instead of failing every job. A Velnor pin never substitutes for a
/// declaration.
///
/// # Errors
///
/// Returns [`OrchestratorError::Config`] for a loose, templated, or
/// malformed version, or for two declarations that disagree.
pub(crate) fn project_declarations(
    sources: &[SourceDeclarations],
) -> Result<BTreeMap<PinnedTool, String>, OrchestratorError> {
    let mut out: BTreeMap<PinnedTool, (String, String, String)> = BTreeMap::new();
    for source in sources {
        for declaration in &source.declarations {
            let tool = declaration.tool.tool_name();
            let key_path = if declaration.key == "toolchain.channel" {
                declaration.key.clone()
            } else {
                format!("tools.{}", declaration.key)
            };
            if !is_exact_project_version(&declaration.value) {
                return Err(OrchestratorError::config(
                    source.file.as_str(),
                    key_path,
                    format!(
                        "project_tool_version_not_exact:tool={tool},file={},key={},value={}",
                        source.file, declaration.key, declaration.value
                    ),
                ));
            }
            match out.get(&declaration.tool) {
                Some((first_file, first_key, version)) if *version != declaration.value => {
                    return Err(OrchestratorError::config(
                        source.file.as_str(),
                        key_path,
                        format!(
                            "conflicting_project_tool_versions:tool={tool},{first_file} {first_key}={version},{} {}={}",
                            source.file, declaration.key, declaration.value
                        ),
                    ));
                }
                Some(_) => {}
                None => {
                    out.insert(
                        declaration.tool,
                        (
                            source.file.clone(),
                            declaration.key.clone(),
                            declaration.value.clone(),
                        ),
                    );
                }
            }
        }
    }
    Ok(out
        .into_iter()
        .map(|(tool, (_, _, version))| (tool, version))
        .collect())
}

/// Inputs choosing one catalog version: project declarations first, then a
/// `.velnor/config.toml` pin, then the compiled-in Velnor pin.
pub(crate) struct PinInputs<'a> {
    /// Project declarations by tool.
    pub(crate) declared: &'a BTreeMap<PinnedTool, String>,
    /// Parsed Velnor config (`[tools]` overrides).
    pub(crate) config: &'a velnor_actions_contract::VelnorConfig,
    /// Rust channel from `[stacks.rust] toolchain` or `[tools] rust`.
    pub(crate) config_tc: Option<(&'a str, &'a str)>,
}

impl PinInputs<'_> {
    /// Exact version for one tool: the project's declaration, else a
    /// `.velnor/config.toml` pin, else the compiled-in Velnor pin.
    ///
    /// # Errors
    ///
    /// Returns [`OrchestratorError::Config`] when a Velnor config pin
    /// disagrees with an exact project declaration.
    pub(crate) fn version(
        &self,
        tool: PinnedTool,
        field: &str,
        default: &str,
    ) -> Result<String, OrchestratorError> {
        let (config_field, config_pin) = match (tool, self.config_tc) {
            (PinnedTool::Rust, Some((cfg_field, channel))) => (cfg_field.to_owned(), Some(channel)),
            (PinnedTool::Rust, None) => (format!("tools.{field}"), None),
            _ => (format!("tools.{field}"), self.config.tools.get(field)),
        };
        match self.declared.get(&tool) {
            Some(version) => {
                check_config_agrees(version, &config_field, config_pin)?;
                Ok(version.clone())
            }
            None => Ok(config_pin.unwrap_or(default).to_owned()),
        }
    }
}

/// Mark every declared tool project-owned at its exact declared version
/// (release-plz has no constructor slot).
///
/// # Errors
///
/// Returns a contract error for a non-exact version.
pub(crate) fn apply_declarations(
    mut catalog: velnor_actions_mise::ToolCatalog,
    declared: &BTreeMap<PinnedTool, String>,
) -> Result<velnor_actions_mise::ToolCatalog, OrchestratorError> {
    for (tool, version) in declared {
        catalog = catalog
            .with_project_tool(*tool, version)
            .map_err(|err| contract(&err))?;
    }
    Ok(catalog)
}

/// Contract error for a catalog validation failure.
pub(crate) fn contract(err: &velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: err.to_string(),
    }
}

/// Reject a `.velnor/config.toml` pin that disagrees with a project declaration.
///
/// # Errors
///
/// Returns [`OrchestratorError::Config`] naming both values.
fn check_config_agrees(
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

/// Argv and toolchain identity that bind one task to its generated record.
///
/// The generated record carries project version references (hard decision
/// 5), so the plan binds the task digest over the emission catalog while
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
