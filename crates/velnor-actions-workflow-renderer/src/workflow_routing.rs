//! Shared placement policy for every generated workflow document.

use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::{ExecutionMode, ScaleSetSelector};

use crate::{RenderError, RenderedFile, yaml::Yaml};

const MAX_MATRIX_COMBINATIONS: usize = 256;

#[path = "workflow_routing_aliases.rs"]
mod aliases;
#[path = "workflow_routing_authority.rs"]
mod authority;
#[path = "workflow_routing_calls.rs"]
mod calls;
#[path = "workflow_routing_container.rs"]
mod container;
#[path = "workflow_routing_inventory.rs"]
mod inventory;
#[path = "workflow_routing_labels.rs"]
mod labels;
#[path = "workflow_routing_matrix.rs"]
mod matrix;
#[path = "workflow_routing_parse.rs"]
mod parse;
#[path = "workflow_routing_runner.rs"]
mod runner;
#[path = "workflow_routing_typed_jobs.rs"]
mod typed_jobs;
pub use authority::{
    AuthorityCall, AuthorityConcurrency, AuthorityJob, AuthorityPermissions, AuthorityTrigger,
    AuthorityWorkflow, extract_workflow_authority,
};
pub(crate) use typed_jobs::route_jobs;

pub(crate) fn parse_authority_document(
    file: &RenderedFile,
) -> Result<saphyr::Yaml<'_>, RenderError> {
    parse::parse_final_workflow(file)
}

/// Effective placement policy for a generated workflow document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRoutingPolicy {
    mode: ExecutionMode,
    selector: Option<ScaleSetSelector>,
    job_container_image: Option<ValidatedImageReference>,
}

impl WorkflowRoutingPolicy {
    /// Keep hosted jobs and reject any Scale Set leaf.
    #[must_use]
    pub fn hosted_only() -> Self {
        Self {
            mode: ExecutionMode::Hosted,
            selector: None,
            job_container_image: None,
        }
    }

    /// Build a policy with the configured selector and immutable image.
    #[must_use]
    pub fn configured(
        mode: ExecutionMode,
        selector: ScaleSetSelector,
        job_container_image: Option<ValidatedImageReference>,
    ) -> Self {
        Self {
            mode,
            selector: Some(selector),
            job_container_image,
        }
    }

    /// True when all generated workflow documents must use the Scale Set.
    #[must_use]
    pub const fn is_scale_set(&self) -> bool {
        matches!(self.mode, ExecutionMode::ScaleSet)
    }

    fn selector(&self) -> Option<&ScaleSetSelector> {
        self.selector.as_ref()
    }

    fn image(&self) -> Option<&ValidatedImageReference> {
        self.job_container_image.as_ref()
    }

    fn has_container_policy(&self) -> bool {
        self.selector.is_some()
    }
}

#[cfg(test)]
pub(crate) fn test_policy(
    mode: ExecutionMode,
    selector: ScaleSetSelector,
) -> WorkflowRoutingPolicy {
    let image = ValidatedImageReference::parse(
        "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect("valid immutable test image");
    WorkflowRoutingPolicy::configured(mode, selector, Some(image))
}

/// Use the selected policy for one typed workflow document.
pub(crate) fn route_workflow_yaml(
    path: &str,
    workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
) -> Result<Yaml, RenderError> {
    route_workflow_yaml_inner(path, workflow, policy, None)
}

pub(crate) fn route_workflow_yaml_for_jobs(
    path: &str,
    workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
    typed_jobs: &std::collections::BTreeMap<String, velnor_actions_contract::Job>,
) -> Result<Yaml, RenderError> {
    route_workflow_yaml_inner(path, workflow, policy, Some(typed_jobs))
}

fn route_workflow_yaml_inner(
    path: &str,
    mut workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
    typed_jobs: Option<&std::collections::BTreeMap<String, velnor_actions_contract::Job>>,
) -> Result<Yaml, RenderError> {
    workflow = aliases::materialize(&workflow)?;
    let selector = policy.selector();
    let workflow_shell = container::workflow_run_shell(&workflow, path)?;
    let Yaml::Map(document) = &mut workflow else {
        return Err(invalid(format!("workflow_document_not_map:{path}")));
    };
    labels::reject_duplicate_keys(document, path, "workflow")?;
    let Some((_, Yaml::Map(jobs))) = document.iter_mut().find(|(key, _)| key == "jobs") else {
        return Err(invalid(format!("workflow_jobs_missing:{path}")));
    };
    for (job_id, job) in jobs {
        let Yaml::Map(fields) = job else {
            return Err(invalid(format!("workflow_job_not_map:{path}:{job_id}")));
        };
        labels::reject_duplicate_keys(fields, path, job_id)?;
        let uses =
            fields
                .iter()
                .find(|(key, _)| key == "uses")
                .and_then(|(_, value)| match value {
                    Yaml::Str(text) | Yaml::Quoted(text) => Some(text.clone()),
                    Yaml::Annotated { value, .. } => Some(value.clone()),
                    _ => None,
                });
        let runs_on_index = fields.iter().position(|(key, _)| key == "runs-on");
        if let Some(uses) = uses {
            if runs_on_index.is_some() || fields.iter().any(|(key, _)| key == "container") {
                return Err(invalid(format!(
                    "reusable_workflow_job_has_runner_fields:{path}:{job_id}"
                )));
            }
            if selector.is_some() && !labels::is_local_reusable_workflow(&uses) {
                return Err(invalid(format!(
                    "unsupported_external_reusable_workflow:{path}:{job_id}:{uses}"
                )));
            }
            continue;
        }
        let typed_job = typed_jobs.and_then(|jobs| jobs.get(job_id));
        let routed = route_runner_fields(fields, runs_on_index, path, job_id, policy, typed_job)?;
        if routed && policy.has_container_policy() {
            container::ensure_job_container(
                fields,
                policy.image(),
                workflow_shell.as_deref(),
                path,
                job_id,
            )?;
        }
    }
    Ok(workflow)
}

fn route_runner_fields(
    fields: &mut [(String, Yaml)],
    runs_on_index: Option<usize>,
    path: &str,
    job_id: &str,
    policy: &WorkflowRoutingPolicy,
    typed_job: Option<&velnor_actions_contract::Job>,
) -> Result<bool, RenderError> {
    let Some(runs_on_index) = runs_on_index else {
        return Err(invalid(format!(
            "workflow_job_runs_on_missing:{path}:{job_id}"
        )));
    };
    if typed_jobs::accept_typed_native_check(
        yaml_scalar(&fields[runs_on_index].1),
        fields
            .iter()
            .find(|(key, _)| key == "if")
            .and_then(|(_, value)| yaml_scalar(value)),
        fields.iter().any(|(key, _)| key == "container"),
        typed_job,
        policy,
        path,
        job_id,
    )? {
        return Ok(false);
    }
    let Some(selector) = policy.selector() else {
        labels::validate_hosted_runner(&fields[runs_on_index].1, fields, path, job_id)?;
        return Ok(false);
    };
    let configured_mode = Some(policy.mode);
    let (selector_leaf, replacement) = runner::route_runner_value(
        &fields[runs_on_index].1,
        fields,
        selector,
        configured_mode,
        path,
        job_id,
    )?;
    if let Some(replacement) = replacement {
        fields[runs_on_index].1 = replacement;
    }
    Ok(selector_leaf)
}

fn yaml_scalar(value: &Yaml) -> Option<&str> {
    match value {
        Yaml::Str(value) | Yaml::Quoted(value) | Yaml::Annotated { value, .. } => Some(value),
        _ => None,
    }
}

/// Examine every job runner and reusable-workflow call in the final workflow files.
///
/// # Errors
///
/// The function gives an error for incorrect workflow data, unsupported Scale Set placement,
/// or a reusable-workflow call whose target is missing, does not accept calls, or creates a
/// call cycle.
pub fn validate_generated_workflow_calls(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    inventory::validate_final_workflow_inventory(files, policy, None)
}

/// Examine final files against the typed workflow used to render them.
///
/// Typed native checks need this workflow intermediate representation to prove their runner contract.
/// The public file-only audit rejects those checks because it has no such proof.
///
/// # Errors
///
/// Returns an error when the workflow intermediate representation or generated workflows fail
/// validation.
pub fn validate_generated_workflow_calls_with_ir(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
    workflow: &velnor_actions_contract::WorkflowIr,
) -> Result<(), RenderError> {
    workflow
        .validate()
        .map_err(|error| invalid(format!("typed_workflow_invalid:{error}")))?;
    inventory::validate_final_workflow_inventory(files, policy, Some(&workflow.jobs))
}

fn invalid(problem: String) -> RenderError {
    RenderError::InvalidWorkflow(problem)
}

#[cfg(test)]
#[path = "workflow_routing_container_tests.rs"]
mod container_tests;
#[cfg(test)]
#[path = "workflow_routing_tests.rs"]
mod tests;
