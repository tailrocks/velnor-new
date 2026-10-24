use crate::{
    RenderError, RenderedFile,
    steps::scan_for_private_subcommands,
    workflow_routing::{WorkflowRoutingPolicy, route_workflow_yaml},
    yaml::Yaml,
};

use velnor_actions_contract::config::UPGRADES_WORKFLOW_PATH;

pub(super) fn finish(
    document: Yaml,
    routing: &WorkflowRoutingPolicy,
    generator_version: &str,
) -> Result<RenderedFile, RenderError> {
    let document = route_workflow_yaml(UPGRADES_WORKFLOW_PATH, document, routing)?;
    let bytes = crate::render::fallback::render_checked_workflow(
        UPGRADES_WORKFLOW_PATH,
        &document,
        generator_version,
    )?;
    scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: UPGRADES_WORKFLOW_PATH.to_owned(),
        bytes,
    })
}
