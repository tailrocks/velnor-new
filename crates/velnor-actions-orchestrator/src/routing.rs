//! Schema 2 migration and the workflows schema 2 emits only when asked.
//!
//! Preview and `--write` share one encoded document. Schema 1 generation
//! never reaches the schema 2 workflow renderer.

use std::path::Path;

use velnor_actions_contract::config::LATEST_RUNNER_LABEL;
use velnor_actions_contract::{ExecutionConfig, ExecutionMode, RoutingWorkflow, VelnorConfig};
use velnor_actions_workflow_renderer::{
    MbxQualificationPins, RenderedFile, Schema2WorkflowRequest, render_schema2_workflows,
};

use crate::OrchestratorError;
use crate::config::{CONFIG_REL, config_error, load_config};

/// Print or persist the schema 2 form of `.velnor/config.toml`.
///
/// `to` other than 2 fails before any read or write. Without `write`,
/// the file is left untouched. The returned text is exactly what
/// `--write` stores.
///
/// # Errors
///
/// Unsupported targets, config errors, and IO errors.
#[must_use = "the preview text is the only bytes --write stores"]
pub fn migrate_config(root: &Path, to: u32, write: bool) -> Result<String, OrchestratorError> {
    if to != VelnorConfig::SCHEMA_V2 {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "schema",
            format!("unsupported_migration_target:{to}"),
        ));
    }
    let text = proposed_text(root)?;
    if write {
        write_config(root, &text)?;
    }
    Ok(text)
}

/// Parse a dispatch mode. Explicit dispatch wins over `execution.mode`.
///
/// # Errors
///
/// Spellings other than `hosted`, `scale-set`, and `both` fail.
pub fn parse_dispatch_mode(text: &str) -> Result<ExecutionMode, OrchestratorError> {
    ExecutionMode::parse(text).map_err(config_error)
}

/// Schema 2 workflows requested by this config. Schema 1 returns none.
///
/// # Errors
///
/// Illegal selectors or a bad generator version fail.
pub(crate) fn extra_files(
    config: &VelnorConfig,
    version: &str,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if config.schema != VelnorConfig::SCHEMA_V2 {
        return Ok(Vec::new());
    }
    let Some(execution) = &config.execution else {
        return Ok(Vec::new());
    };
    if execution.workflows.is_empty() {
        return Ok(Vec::new());
    }
    let request = workflow_request(config, execution, version)?;
    render_schema2_workflows(&request).map_err(|err| OrchestratorError::Render {
        problem: err.to_string(),
    })
}

fn proposed_text(root: &Path) -> Result<String, OrchestratorError> {
    let mut config = load_config(root)?;
    if config.execution.is_none() {
        let label = config
            .workflow
            .runner_label
            .clone()
            .unwrap_or_else(|| LATEST_RUNNER_LABEL.to_owned());
        config.execution = Some(hosted_execution(&label)?);
    }
    config.schema = VelnorConfig::SCHEMA_V2;
    config.validate(CONFIG_REL).map_err(config_error)?;
    let text = toml::to_string(&config).map_err(|err| {
        OrchestratorError::config(CONFIG_REL, "schema", format!("migration_encode:{err}"))
    })?;
    Ok(with_trailing_newline(text))
}

fn hosted_execution(label: &str) -> Result<ExecutionConfig, OrchestratorError> {
    ExecutionConfig::hosted_default(label).map_err(config_error)
}

fn write_config(root: &Path, text: &str) -> Result<(), OrchestratorError> {
    let path = root.join(CONFIG_REL);
    std::fs::write(&path, text.as_bytes())
        .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))
}

fn with_trailing_newline(text: String) -> String {
    if text.ends_with('\n') {
        text
    } else {
        let mut text = text;
        text.push('\n');
        text
    }
}

fn workflow_request(
    config: &VelnorConfig,
    execution: &ExecutionConfig,
    version: &str,
) -> Result<Schema2WorkflowRequest, OrchestratorError> {
    let hosted = execution
        .profiles
        .get(&execution.hosted_profile)
        .and_then(|profile| profile.label.clone())
        .ok_or_else(|| {
            OrchestratorError::config(CONFIG_REL, "execution.profiles.hosted.label", "missing")
        })?;
    let scale_set = execution.scale_selector().map_err(config_error)?;
    let needs_setup_set = requires_mise_setup_set(execution);
    let needs_mise = needs_setup_set
        || execution
            .workflows
            .contains(&RoutingWorkflow::Qualification);
    let mise_setup = if needs_mise {
        Some(crate::pins::resolve_mise_setup(config, &hosted)?)
    } else {
        None
    };
    let mise_setup_set = if needs_setup_set {
        let setup = mise_setup
            .as_ref()
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "schema2_mise_setup_unavailable".to_owned(),
            })?;
        Some(crate::pins::resolve_mise_setup_set(config, &hosted, setup)?)
    } else {
        None
    };
    let mbx_qualification = qualification_pins(execution, mise_setup.as_ref())?;
    Ok(Schema2WorkflowRequest {
        version: version.to_owned(),
        hosted_label: hosted,
        scale_set,
        workflows: execution.workflows.clone(),
        mise_setup_set,
        mbx_qualification,
    })
}

fn requires_mise_setup_set(execution: &ExecutionConfig) -> bool {
    [
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ]
    .iter()
    .any(|workflow| execution.workflows.contains(workflow))
}

fn qualification_pins(
    execution: &ExecutionConfig,
    mise_setup: Option<&velnor_actions_workflow_renderer::MiseSetup>,
) -> Result<Option<MbxQualificationPins>, OrchestratorError> {
    if !execution
        .workflows
        .contains(&RoutingWorkflow::Qualification)
    {
        return Ok(None);
    }
    let mise_setup = mise_setup.ok_or_else(|| OrchestratorError::Contract {
        problem: "schema2_mise_setup_unavailable".to_owned(),
    })?;
    let mbx_action = velnor_actions_actionlint::actions::PinnedActionRef::new(
        "jdx/mr-boxington-action",
        None,
        velnor_actions_actionlint::actions::MR_BOXINGTON_ACTION_SHA,
        velnor_actions_actionlint::actions::MR_BOXINGTON_ACTION_VERSION,
    )?;
    let tool_catalog = velnor_actions_mise::ToolCatalog::pinned();
    Ok(Some(MbxQualificationPins {
        mise_setup: mise_setup.clone(),
        mbx_action_uses: mbx_action.uses_value(),
        mbx_version: tool_catalog
            .version(velnor_actions_mise::PinnedTool::MrBoxington)
            .to_owned(),
        rust_version: tool_catalog
            .version(velnor_actions_mise::PinnedTool::Rust)
            .to_owned(),
    }))
}
