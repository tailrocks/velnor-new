//! Reconstruct the sealed Python Full/GitHub Planning APT admission closure.

use super::AptRenderContext;
use velnor_actions_contract::{CompiledSourceHelper, config::AptDeliveryConfig};
use velnor_actions_mise::catalog::{delivery_tools, qualification::DistributionHost};
use velnor_actions_workflow_renderer::RenderError;

pub(super) fn record(
    config: &AptDeliveryConfig,
    context: &AptRenderContext,
) -> Result<(CompiledSourceHelper, Vec<CompiledSourceHelper>), RenderError> {
    let host = host(context)?;
    let version = &context.workflow.generator_version;
    let tools = delivery_tools::admission_tools(host, version).map_err(sdk)?;
    delivery_tools::validate_admission_tools(host, version, &tools).map_err(sdk)?;
    let recipe = tools.execution();
    if recipe.installed_selectors() != tools.context().full_selectors()
        || recipe
            .environment()
            .get(delivery_tools::ADMISSION_PLANNING_GH_ENV)
            != Some(&tools.context().gh_planning().executable())
    {
        return Err(RenderError::InvalidWorkflow(
            "apt_admission_tool_binding".to_owned(),
        ));
    }
    let admission = crate::release_emit::release_admission::compiled_default_branch_admission(
        &config.consumer_repository,
        &config.branch,
        version,
        host,
    )?;
    if admission.execution_recipe() != Some(recipe) {
        return Err(RenderError::InvalidWorkflow(
            "apt_admission_owner_recipe_changed".to_owned(),
        ));
    }
    let records = tools
        .bootstrap_records()
        .iter()
        .chain(tools.preparation_records().iter())
        .cloned()
        .collect();
    Ok((admission, records))
}

fn host(context: &AptRenderContext) -> Result<DistributionHost, RenderError> {
    match velnor_actions_contract::tool_target_for_runner_label(&context.workflow.runs_on) {
        Some("x86_64-unknown-linux-gnu") => Ok(DistributionHost::LinuxAmd64),
        Some("aarch64-unknown-linux-gnu") => Ok(DistributionHost::LinuxArm64),
        _ => Err(RenderError::InvalidWorkflow(
            "apt_admission_host".to_owned(),
        )),
    }
}

fn sdk(error: velnor_actions_mise::MiseError) -> RenderError {
    RenderError::BadCommand(format!("apt_admission_authority:{error}"))
}
