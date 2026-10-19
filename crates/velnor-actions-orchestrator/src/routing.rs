//! Schema 2 migration and the workflows schema 2 emits only when asked.
//!
//! Preview and `--write` share one encoded document. Schema 1 generation
//! never reaches the schema 2 workflow renderer.

use std::path::Path;

use velnor_actions_contract::config::LATEST_RUNNER_LABEL;
use velnor_actions_contract::{ExecutionConfig, ExecutionMode, RoutingWorkflow, VelnorConfig};
use velnor_actions_workflow_renderer::{
    MbxQualificationPins, RenderedFile, Schema2WorkflowRequest, WorkflowRoutingPolicy,
    render_schema2_workflows,
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
    catalog: &velnor_actions_mise::ToolCatalog,
    version: &str,
    routing: &WorkflowRoutingPolicy,
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
    let request = workflow_request(config, execution, catalog, version, routing)?;
    render_schema2_workflows(&request).map_err(|err| OrchestratorError::Render {
        problem: err.to_string(),
    })
}

/// Select the policy for every separate generated workflow.
pub(crate) fn workflow_routing_policy(
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<WorkflowRoutingPolicy, OrchestratorError> {
    let Some(execution) = &config.execution else {
        if dispatch == Some(ExecutionMode::ScaleSet) {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                "execution",
                "scale_set_dispatch_requires_execution_profile",
            ));
        }
        return Ok(WorkflowRoutingPolicy::hosted_only());
    };
    let mode = dispatch.or(execution.mode).unwrap_or_else(|| {
        if execution.default_profile == execution.scale_set_profile {
            ExecutionMode::ScaleSet
        } else {
            ExecutionMode::Hosted
        }
    });
    let has_selector_leaves = mode != ExecutionMode::Hosted || execution.emits_scale_set_selector();
    if has_selector_leaves {
        let selector = execution.scale_selector().map_err(config_error)?;
        let image = execution
            .profiles
            .get(&execution.scale_set_profile)
            .and_then(|profile| profile.job_container_image.clone());
        Ok(WorkflowRoutingPolicy::configured(mode, selector, image))
    } else {
        Ok(WorkflowRoutingPolicy::hosted_only())
    }
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
    catalog: &velnor_actions_mise::ToolCatalog,
    version: &str,
    routing: &WorkflowRoutingPolicy,
) -> Result<Schema2WorkflowRequest, OrchestratorError> {
    let hosted = execution
        .profiles
        .get(&execution.hosted_profile)
        .and_then(|profile| profile.label.clone())
        .ok_or_else(|| {
            OrchestratorError::config(CONFIG_REL, "execution.profiles.hosted.label", "missing")
        })?;
    let scale_set = execution.scale_selector().map_err(config_error)?;
    let mbx_qualification = if execution
        .workflows
        .contains(&RoutingWorkflow::Qualification)
    {
        let mbx_action = velnor_actions_actionlint::actions::PinnedActionRef::new(
            "jdx/mr-boxington-action",
            None,
            velnor_actions_actionlint::actions::MR_BOXINGTON_ACTION_SHA,
            velnor_actions_actionlint::actions::MR_BOXINGTON_ACTION_VERSION,
        )?;
        let tool_catalog = catalog;
        Some(MbxQualificationPins {
            mise_setup: crate::pins::resolve_mise_setup(config, &hosted)?,
            candidate_action_uses: mbx_action.uses_value(),
            mbx_version: tool_catalog
                .version(velnor_actions_mise::PinnedTool::MrBoxington)
                .to_owned(),
            rust_version: tool_catalog
                .version(velnor_actions_mise::PinnedTool::Rust)
                .to_owned(),
        })
    } else {
        None
    };
    let mise_pin_qualification = execution
        .workflows
        .contains(&RoutingWorkflow::Qualification)
        .then(|| crate::pins::resolve_mise_pin_qualification(config))
        .transpose()?;
    let product_release_requested = [
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ]
    .iter()
    .any(|workflow| execution.workflows.contains(workflow));
    let product_release = product_release_requested
        .then(|| crate::product_release_pins::resolve(config, catalog))
        .transpose()?;
    Ok(Schema2WorkflowRequest {
        version: version.to_owned(),
        hosted_label: hosted,
        scale_set,
        routing: routing.clone(),
        workflows: execution.workflows.clone(),
        mbx_qualification,
        mise_pin_qualification,
        product_release,
    })
}

/// Render the job image candidate with the configured runner policy.
pub(crate) fn job_image_candidate_file(
    config: &VelnorConfig,
    catalog: &velnor_actions_mise::ToolCatalog,
    version: &str,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedFile, OrchestratorError> {
    let execution = config
        .execution
        .as_ref()
        .ok_or_else(|| OrchestratorError::config(CONFIG_REL, "execution", "missing"))?;
    let routing = workflow_routing_policy(config, dispatch)?;
    let request = workflow_request(config, execution, catalog, version, &routing)?;
    velnor_actions_workflow_renderer::schema2::schema2_job_image_candidate::render_candidate_workflow(
        &request,
    )
    .map_err(|err| OrchestratorError::Render {
        problem: err.to_string(),
    })
}

#[cfg(test)]
#[path = "routing_tests.rs"]
mod tests;
