// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::project_tool_refs::is_exact_project_version;
use velnor_actions_mise::PinnedTool;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::project_tool_sources::SourceDeclarations;

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

pub(crate) struct PinInputs<'a> {
    pub(crate) declared: &'a BTreeMap<PinnedTool, String>,
    pub(crate) config: &'a velnor_actions_contract::VelnorConfig,
    pub(crate) config_tc: Option<(&'a str, &'a str)>,
}

impl PinInputs<'_> {
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

pub(crate) fn contract(err: &velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: err.to_string(),
    }
}

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

pub(crate) fn task_binding(
    task: &velnor_actions_contract::ProposedTask,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(Vec<String>, String), OrchestratorError> {
    let emission = catalog.for_emission();
    let toolchain = crate::internal_plan::toolchain_id(task, &emission)
        .map_err(crate::internal::internal_contract)?;
    Ok((crate::vectors::task_argv(task, &emission)?, toolchain))
}
