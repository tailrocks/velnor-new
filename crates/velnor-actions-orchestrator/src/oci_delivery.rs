//! Native OCI workflow generation, gated before every registry mutation.

#[path = "oci_jobs.rs"]
mod jobs;
#[path = "oci_scripts.rs"]
mod scripts;
#[path = "oci_support.rs"]
mod support;
pub(crate) use support::OCI_DELIVERY_TREE_PATHS;
pub(super) use support::render_oci_support_files;
#[path = "oci_attest_steps.rs"]
mod attest_steps;
#[path = "oci_bound_steps.rs"]
mod bound_steps;
#[cfg(test)]
#[path = "oci_credential_tests.rs"]
mod credential_tests;
#[path = "oci_platform_steps.rs"]
mod platform_steps;
#[path = "oci_steps.rs"]
mod steps;
#[cfg(test)]
#[path = "oci_tests.rs"]
mod tests;
#[path = "oci_transport_steps.rs"]
mod transport_steps;

use velnor_actions_contract::config::OciReleaseConfig;
use velnor_actions_workflow_renderer::{
    RenderError, RenderedFile, WorkflowDocumentContext, render_workflow_document,
};

/// Dedicated generated OCI publication workflow.
const OCI_WORKFLOW_PATH: &str = ".github/workflows/delivery-oci.yml";

/// Official action pins resolved by the generator's freshness policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OciActionPins {
    /// actions/checkout full SHA reference.
    pub(super) checkout: String,
    /// actions/upload-artifact full SHA reference.
    pub(super) upload: String,
    /// docker/setup-buildx-action full SHA reference.
    pub(super) buildx: String,
    /// docker/login-action full SHA reference.
    pub(super) login: String,
    /// docker/build-push-action full SHA reference.
    pub(super) build: String,
    /// actions/attest exact SHA for immutable source-bound index attestations.
    pub(super) attest: String,
    /// Exact qualified Buildx binary release tag.
    pub(super) buildx_version: String,
    /// Immutable qualified multi-platform BuildKit image reference.
    pub(super) buildkit_image: String,
    /// Immutable qualified SBOM scanner image reference.
    pub(super) sbom_image: String,
}

impl OciActionPins {
    fn validate(&self) -> Result<(), RenderError> {
        if self != &super::delivery_pins::oci_pins() {
            return Err(RenderError::InvalidWorkflow(
                "oci_action_authority_changed".into(),
            ));
        }
        Ok(())
    }
}

/// Contextual repository evidence; secrets never enter generation inputs.
#[derive(Debug, Clone)]
pub(super) struct OciRenderContext {
    /// Exact owner/repository.
    pub(super) repository: String,
    /// Exact protected default branch.
    pub(super) default_branch: String,
    /// Full CI workflow filename containing the Required check.
    pub(super) ci_workflow: String,
    /// Generator version marker.
    pub(super) generator_version: String,
    /// Qualified official action pins.
    pub(super) pins: OciActionPins,
}

impl OciRenderContext {
    fn validate(&self) -> Result<(), RenderError> {
        velnor_actions_workflow_renderer::release_spec::validate_repository(&self.repository)?;
        if self.default_branch != "main"
            || !matches!(
                self.ci_workflow.as_str(),
                "ci.yml" | ".github/workflows/ci.yml"
            )
        {
            return Err(RenderError::InvalidWorkflow(
                "oci_bad_ci_binding".to_owned(),
            ));
        }
        self.pins.validate()?;
        Ok(())
    }
}

/// Render an enabled OCI allowlist into a native GitHub Actions workflow.
/// # Errors
/// Rejects disabled policy, unsafe config, and unpinned actions.
pub(super) fn render_oci_release(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
) -> Result<RenderedFile, RenderError> {
    let (workflow, document_context) = build_oci_workflow_ir(config, context)?;
    Ok(RenderedFile {
        path: OCI_WORKFLOW_PATH.to_owned(),
        bytes: render_workflow_document(&workflow, &document_context)?,
    })
}

/// Build the complete native graph and exact compiled source registry.
/// # Errors
/// Rejects unsafe policy, unavailable SDK qualifications and unclosed publication authority.
fn build_oci_workflow_ir(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
) -> Result<(velnor_actions_contract::WorkflowIr, WorkflowDocumentContext), RenderError> {
    use velnor_actions_contract::workflow::dispatch::{
        DispatchInput, DispatchInputType, WorkflowDispatch,
    };
    use velnor_actions_contract::{CacheMode, Concurrency, Trigger, WorkflowIr};
    validate_policy(config, context)?;
    let mut records = Vec::new();
    let workflow = WorkflowIr {
        cache_mode: CacheMode::Read,
        name: "OCI release".into(),
        run_name: None,
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            push_tags: vec!["v[0-9]*".into()],
            merge_group: false,
            schedule: None,
            workflow_dispatch: Some(WorkflowDispatch {
                inputs: vec![DispatchInput {
                    name: "recovery".into(),
                    input_type: DispatchInputType::String,
                    required: false,
                    description: Some(
                        "Exact image-id to existing immutable index digest JSON map".into(),
                    ),
                    options: Vec::new(),
                    default: Some("{}".into()),
                }],
            }),
        },
        permissions: jobs::read_permissions(),
        concurrency: Concurrency {
            group: "oci-release-${{ github.repository }}".into(),
            cancel_in_progress: "false".into(),
        },
        jobs: jobs::render(config, context, &mut records)?,
    };
    let approvals = workflow.jobs.iter().filter(|(_, job)| job.native_publish.is_some())
        .map(|(id, _)| velnor_actions_workflow_renderer::native_publish_approval::NativePublishApproval::compiled(id, &workflow))
        .collect::<Result<Vec<_>, _>>()?;
    let document_context = WorkflowDocumentContext {
        generator_version: context.generator_version.clone(),
        source_helpers: records,
        native_pages_approvals: Vec::new(),
        native_publish_approvals: approvals,
        action_credential_approvals: credential_approvals(config, context, &workflow)?,
    };
    Ok((workflow, document_context))
}

fn validate_policy(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
) -> Result<(), RenderError> {
    config
        .validate(".velnor/config.toml")
        .map_err(RenderError::Contract)?;
    context.validate()?;
    if config.registry != "docker.io"
        || !matches!(
            config.authentication,
            velnor_actions_contract::config::RegistryAuthentication::NamedSecrets { .. }
        )
    {
        return Err(RenderError::InvalidWorkflow(
            "oci_registry_unqualified".into(),
        ));
    }
    if !config.enabled {
        return Err(RenderError::InvalidWorkflow("oci_disabled".to_owned()));
    }
    Ok(())
}

fn credential_approvals(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    workflow: &velnor_actions_contract::WorkflowIr,
) -> Result<
    Vec<velnor_actions_workflow_renderer::action_credentials::ActionCredentialApproval>,
    RenderError,
> {
    use velnor_actions_contract::{StepKind, config::RegistryAuthentication};
    use velnor_actions_workflow_renderer::action_credentials::{
        ActionCredentialApproval, DockerRegistryLoginBinding,
    };
    let RegistryAuthentication::NamedSecrets {
        username_secret,
        password_secret,
    } = &config.authentication
    else {
        return Err(RenderError::InvalidWorkflow(
            "oci_authentication_unqualified".into(),
        ));
    };
    let verify = workflow
        .jobs
        .get("verify")
        .and_then(|job| {
            job.steps
                .iter()
                .find(|step| step.id.as_ref().is_some_and(|id| id.as_str() == "verify"))
        })
        .ok_or_else(|| RenderError::InvalidWorkflow("oci_full_ci_missing".into()))?;
    let StepKind::SourceBoundHelper { invocation, .. } = &verify.kind else {
        return Err(RenderError::InvalidWorkflow(
            "oci_full_ci_helper_missing".into(),
        ));
    };
    let binding = DockerRegistryLoginBinding {
        approved_login_uses: context.pins.login.clone(),
        registry: config.registry.clone(),
        username_secret: username_secret.clone(),
        password_secret: password_secret.clone(),
        repository: context.repository.clone(),
        default_branch: context.default_branch.clone(),
        ci_workflow: context.ci_workflow.clone(),
        full_ci_job: "verify".into(),
        full_ci_admission: invocation.clone(),
    };
    let mut approvals = Vec::new();
    for (job_id, job) in &workflow.jobs {
        for step_id in job
            .steps
            .iter()
            .filter_map(|step| step.id.as_ref())
            .filter(|id| id.as_str() == "registry_login")
        {
            approvals.push(ActionCredentialApproval::docker_registry_login(
                job_id, step_id, &binding, workflow,
            )?);
        }
    }
    Ok(approvals)
}
