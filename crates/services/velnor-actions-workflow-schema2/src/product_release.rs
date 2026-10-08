//! Typed dispatch from selected product families to their current renderers.

use velnor_actions_workflow_generator::{
    ProductReleaseFamily, Schema2WorkflowRequest, generator_release,
};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::yaml::Yaml;

use super::{
    GENERATOR_RELEASE_WORKFLOW, IMAGE_RELEASE_WORKFLOW, MACOS_BINARY_RELEASE_WORKFLOW, release,
};

/// Existing output workflows produced for one typed product selection.
pub(super) struct SelectedProductFiles {
    pub workflows: Vec<(String, Yaml)>,
    pub actions: Vec<(String, Yaml)>,
}

/// Render each requested product family exactly once through its current typed renderer.
pub(super) fn render_selected_families(
    request: &Schema2WorkflowRequest,
) -> Result<SelectedProductFiles, RenderError> {
    let Some(spec) = request.product_release_spec()? else {
        return Ok(SelectedProductFiles {
            workflows: Vec::new(),
            actions: Vec::new(),
        });
    };
    let mut workflows = Vec::new();
    let mut actions = Vec::new();
    for family in spec.families() {
        match family {
            ProductReleaseFamily::Images => workflows.push((
                IMAGE_RELEASE_WORKFLOW.to_owned(),
                release::image_release(request)?,
            )),
            ProductReleaseFamily::Binary => workflows.push((
                MACOS_BINARY_RELEASE_WORKFLOW.to_owned(),
                release::macos_binary_release(request)?,
            )),
            ProductReleaseFamily::Generator => {
                let generated = generator_release::generator_release(request)?;
                workflows.push((GENERATOR_RELEASE_WORKFLOW.to_owned(), generated.workflow));
                actions.extend(generated.actions);
            }
        }
    }
    Ok(SelectedProductFiles { workflows, actions })
}
