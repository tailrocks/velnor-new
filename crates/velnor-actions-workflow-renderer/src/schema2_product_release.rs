//! One serialized, exact-source coordinator with independent product DAGs.

use velnor_actions_contract::RoutingWorkflow;

use crate::yaml::Yaml;
use crate::{RenderError, runs_on::runs_on_yaml};

use super::Schema2WorkflowRequest;
use super::product_release_family::Family;
use super::release_eligibility;

/// Emitted path for the single exact-source release coordinator.
pub(super) const WORKFLOW_PATH: &str = ".github/workflows/product-release.yml";
pub(super) const HOSTED_RUNS_ON: &str = "ubuntu-26.04";
pub(super) const SOURCE_OUTPUT: &str = "${{ needs.release-eligibility.outputs.source_sha }}";
pub(super) const AUTHORITY_OUTPUT: &str =
    "${{ needs.release-eligibility.outputs.workflow_authority_sha }}";
pub(super) const MODULE_SOURCE: &str = "${{ inputs.source_sha }}";
const RELEASE_CONCURRENCY: &str = "${{ github.repository }}-product-release";

#[path = "schema2_product_release_family_jobs.rs"]
mod family_jobs;
#[path = "schema2_product_release_generator.rs"]
mod generator;
#[path = "schema2_product_release_workflows.rs"]
mod workflows;

pub(super) struct ProductRelease {
    pub workflow: Yaml,
    pub family_workflows: Vec<(String, Yaml)>,
    pub actions: Vec<(String, Yaml)>,
}

/// Render the shared eligibility coordinator and requested independent families.
pub(super) fn render(
    request: &Schema2WorkflowRequest,
) -> Result<Option<ProductRelease>, RenderError> {
    let families = requested_families(request);
    if families.is_empty() {
        return Ok(None);
    }
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("product_release_pins_missing".to_owned()))?;
    let hosted = runs_on_yaml(HOSTED_RUNS_ON)?;
    let mut jobs = vec![release_eligibility::job(hosted.clone(), pins)?];
    let mut family_workflows = Vec::new();
    let mut actions = Vec::new();
    for family in families.iter().copied() {
        jobs.push(family_jobs::prepare_job(family, hosted.clone(), request)?);
        let composed_family_jobs = if family == Family::Generator {
            let generated = super::generator_release::generator_release(request)?;
            actions.extend(generated.actions);
            generator::compose_jobs(generated.workflow)?
        } else {
            let family_document = family_jobs::family_document(family, request)?;
            family_jobs::take_jobs(family_document)?
                .into_iter()
                .map(|(id, job)| family_jobs::compose_job(id, job, family, pins))
                .collect::<Result<Vec<_>, _>>()?
        };
        family_workflows.push((
            family.workflow_path().to_owned(),
            workflows::family_document_for_call(family, composed_family_jobs),
        ));
        jobs.push(workflows::reusable_workflow_call(family));
    }
    Ok(Some(ProductRelease {
        workflow: workflows::document(jobs, &families),
        family_workflows,
        actions,
    }))
}

/// Select product families deterministically, independent of set iteration.
fn requested_families(request: &Schema2WorkflowRequest) -> Vec<Family> {
    [
        (RoutingWorkflow::ImageRelease, Family::Images),
        (RoutingWorkflow::MacosBinaryRelease, Family::Binary),
        (RoutingWorkflow::GeneratorRelease, Family::Generator),
    ]
    .into_iter()
    .filter_map(|(workflow, family)| request.workflows.contains(&workflow).then_some(family))
    .collect()
}

#[cfg(test)]
#[path = "schema2_product_release_tests.rs"]
mod tests;
