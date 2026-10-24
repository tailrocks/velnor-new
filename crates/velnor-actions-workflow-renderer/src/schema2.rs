//! Qualification, composed product-release, and monitoring workflows.
//! Emitted only when schema 2 requests them.

use std::collections::BTreeSet;

use velnor_actions_contract::{
    ReleaseTarget, RoutingWorkflow, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL,
};

use crate::RenderError;
use crate::render::RenderedFile;
use crate::setup::MiseSetup;
use crate::workflow_routing::paired_sdk;
use crate::workflow_routing::{WorkflowRoutingPolicy, route_workflow_yaml};
use crate::yaml::Yaml;

#[cfg(test)]
#[path = "../../test_support/git_fixture.rs"]
pub(crate) mod git_fixture;

/// Qualification workflow path.
pub const QUALIFICATION_WORKFLOW: &str = ".github/workflows/qualification.yml";
/// Shared product-release workflow path.
pub const PRODUCT_RELEASE_WORKFLOW: &str = product_release::WORKFLOW_PATH;
/// Queue-monitoring workflow path.
pub const MONITORING_WORKFLOW: &str = ".github/workflows/monitoring.yml";

#[path = "schema2_classes.rs"]
mod classes;
#[path = "schema2_runner_spec.rs"]
mod runner_spec;
use runner_spec::RunnerSpec;
#[path = "schema2_consumer_preview.rs"]
mod consumer_preview;
pub use consumer_preview::{
    ConsumerReleaseSpec, ConsumerReleaseTapSpec, render_consumer_preview,
    render_release_tap_reconcile,
};
#[path = "schema2_basic_workflows.rs"]
mod basic_workflows;
#[path = "schema2_ci_run_lookup.rs"]
mod ci_run_lookup;
pub(super) use ci_run_lookup::RETRY_DELAYS_SECONDS;
#[path = "schema2_consumer_binary_release.rs"]
mod consumer_binary_release;
#[path = "schema2_features.rs"]
mod features;
#[path = "schema2_generator_release.rs"]
mod generator_release;
#[path = "schema2_mbx_qualification.rs"]
mod mbx_qualification;
#[path = "schema2_mise_pin_qualification.rs"]
mod mise_pin_qualification;
pub(super) use basic_workflows::with_if;
use basic_workflows::{monitoring, qualification};
#[path = "schema2_paired_sdk_qualification.rs"]
mod paired_sdk_qualification;
#[path = "schema2_paired_sdk_steps.rs"]
mod paired_sdk_steps;
#[path = "schema2_product_release.rs"]
mod product_release;
#[path = "schema2_product_release_family.rs"]
mod product_release_family;
#[path = "schema2_product_release_pins.rs"]
mod product_release_pins;
#[path = "schema2_release.rs"]
mod release;
/// Exact-source gates for composed product-release workflows.
#[path = "schema2_release_eligibility.rs"]
pub mod release_eligibility;
#[path = "schema2_resource_probe_image.rs"]
mod resource_probe_image;
#[path = "schema2_image_release.rs"]
mod schema2_image_release;
#[path = "schema2_job_image_candidate.rs"]
pub mod schema2_job_image_candidate;
pub use product_release_pins::ProductReleasePins;

/// Render the native stable-release workflow under the effective placement policy.
///
/// # Errors
///
/// Invalid release inventory, pins, runners, or workflow rendering errors fail.
pub fn render_consumer_binary_release(
    spec: &ConsumerReleaseSpec,
    routing: &WorkflowRoutingPolicy,
) -> Result<RenderedFile, RenderError> {
    consumer_binary_release::render_consumer_binary_release(spec, routing)
}

pub use consumer_binary_release::ConsumerStableReleaseSpec;

#[cfg(test)]
#[path = "schema2_runner_shell_tests.rs"]
mod runner_shell_tests;

#[cfg(test)]
#[path = "schema2_resource_probe_image_tests.rs"]
mod resource_probe_image_tests;

#[cfg(test)]
#[path = "schema2_paired_sdk_tests.rs"]
mod paired_sdk_tests;

#[cfg(test)]
#[path = "schema2_product_release_test_pins.rs"]
pub(super) mod product_release_test_pins;

/// Which schema 2 workflows to emit, plus the selectors they use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema2WorkflowRequest {
    /// Generator version for the marker.
    pub version: String,
    /// Hosted catalog label.
    pub hosted_label: String,
    /// Validated scale-set selector.
    pub scale_set: ScaleSetSelector,
    /// One validated placement policy shared by every generated workflow.
    pub routing: WorkflowRoutingPolicy,
    /// Workflows to emit. Empty emits nothing.
    pub workflows: BTreeSet<RoutingWorkflow>,
    /// Pinned tool inputs, required only when qualification is emitted.
    pub mbx_qualification: Option<MbxQualificationPins>,
    /// Latest Mise release inputs, required only when qualification is emitted.
    pub mise_pin_qualification: Option<MisePinQualificationPins>,
    /// Orchestrator-resolved Mise setup and command vectors for product releases.
    pub product_release: Option<ProductReleasePins>,
}

/// Exact tools used by the hosted MBX cache qualification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbxQualificationPins {
    /// Resolved Mise action and binary pins.
    pub mise_setup: MiseSetup,
    /// Full-SHA candidate MBX Action ref for this unqualified experiment.
    /// It is separate from the production pin and generation never qualifies it.
    pub candidate_action_uses: String,
    /// Exact MBX tool version.
    pub mbx_version: String,
    /// Exact Rust toolchain version used by the qualification lane.
    pub rust_version: String,
    /// `sh -c` argv verifying the installed toolchain file by file.
    pub verify_rust_argv: Vec<String>,
}

/// Exact Mise release pins for the hosted Linux and macOS x64 qualification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MisePinQualificationPins {
    /// Linux x86-64 release asset verification pins.
    pub linux_x86_64_setup: MiseSetup,
    /// macOS x86-64 release asset verification pins.
    pub macos_x86_64_setup: MiseSetup,
}

impl Schema2WorkflowRequest {
    /// Canonical scale-set selector (`velnor`, then the scale-set name).
    ///
    /// # Errors
    ///
    /// Illegal labels fail.
    pub fn canonical_scale_set() -> Result<ScaleSetSelector, RenderError> {
        ScaleSetSelector::try_new(
            SCALE_SET_NAME,
            &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
        )
        .map_err(|err| RenderError::InvalidWorkflow(err.to_string()))
    }
}

/// Render every requested workflow. Empty when nothing is requested.
///
/// # Errors
///
/// Illegal selectors or a bad generator version fail.
pub fn render_schema2_workflows(
    request: &Schema2WorkflowRequest,
) -> Result<Vec<RenderedFile>, RenderError> {
    render_schema2_workflows_with_verified_routing(request).map(|(files, _)| files)
}

/// Render every requested workflow and return the immutable routing policy
/// bound to any paired SDK qualification workflow in the rendered files.
///
/// The existing [`render_schema2_workflows`] API remains the file-only
/// convenience wrapper. Orchestrators that perform a later inventory pass
/// must retain and use this returned policy.
///
/// # Errors
///
/// Illegal selectors, missing qualification inputs, or workflow rendering
/// and routing errors fail.
pub fn render_schema2_workflows_with_verified_routing(
    request: &Schema2WorkflowRequest,
) -> Result<(Vec<RenderedFile>, WorkflowRoutingPolicy), RenderError> {
    RunnerSpec::hosted(&request.hosted_label)?;
    let mut files = Vec::new();
    let mut final_routing = request.routing.clone();
    if request.workflows.contains(&RoutingWorkflow::Qualification) {
        let workflow = qualification(request)?;
        let (workflow, source) = paired_sdk_qualification::append(workflow, request)?;
        if let Some(source) = source {
            let (routed, routed_source) = paired_sdk::route_qualification(
                QUALIFICATION_WORKFLOW,
                workflow,
                &request.version,
                &request.routing,
                &source,
            )?;
            let rendered = file(QUALIFICATION_WORKFLOW, &request.version, &routed)?;
            final_routing = paired_sdk::bind_policy(&request.routing, routed_source, &rendered)?;
            files.push(rendered);
        } else {
            files.push(file(
                QUALIFICATION_WORKFLOW,
                &request.version,
                &route_workflow(request, QUALIFICATION_WORKFLOW, workflow)?,
            )?);
        }
    }
    if let Some(generated) = product_release::render(request)? {
        files.push(file(
            PRODUCT_RELEASE_WORKFLOW,
            &request.version,
            &route_workflow(request, PRODUCT_RELEASE_WORKFLOW, generated.workflow)?,
        )?);
        for (path, workflow) in generated.family_workflows {
            let workflow = route_workflow(request, &path, workflow)?;
            files.push(file(&path, &request.version, &workflow)?);
        }
        for (path, action) in generated.actions {
            files.push(file(&path, &request.version, &action)?);
        }
    }
    if request.workflows.contains(&RoutingWorkflow::ImageRelease) {
        files.push(schema2_job_image_candidate::render_candidate_workflow(
            request,
        )?);
    }
    if request.workflows.contains(&RoutingWorkflow::Monitoring) {
        files.push(file(
            MONITORING_WORKFLOW,
            &request.version,
            &route_workflow(request, MONITORING_WORKFLOW, monitoring(request)?)?,
        )?);
    }
    Ok((files, final_routing))
}

fn route_workflow(
    request: &Schema2WorkflowRequest,
    path: &str,
    workflow: Yaml,
) -> Result<Yaml, RenderError> {
    route_workflow_yaml(path, workflow, &request.routing)
}

fn file(path: &str, version: &str, body: &Yaml) -> Result<RenderedFile, RenderError> {
    let bytes = crate::render::fallback::render_checked_workflow(path, body, version)?;
    Ok(RenderedFile {
        path: path.to_owned(),
        bytes,
    })
}

#[cfg(test)]
#[path = "schema2_timeout_tests.rs"]
mod timeout_tests;
