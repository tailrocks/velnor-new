//! Shared placement policy for every generated workflow document.

use std::collections::BTreeMap;

use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::{ExecutionMode, ScaleSetSelector, WorkflowIr};

use crate::{RenderError, RenderedFile, verification_jobs::WorkflowTaskPolicy, yaml::Yaml};

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
#[path = "workflow_routing_native_preview.rs"]
mod native_preview;
#[path = "workflow_routing_native_tasks.rs"]
mod native_tasks;
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
use native_tasks::NativeHostedTask;
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
    native_hosted_tasks: BTreeMap<String, native_tasks::NativeHostedTask>,
}

impl WorkflowRoutingPolicy {
    /// Keep hosted jobs and reject any Scale Set leaf.
    #[must_use]
    pub fn hosted_only() -> Self {
        Self {
            mode: ExecutionMode::Hosted,
            selector: None,
            job_container_image: None,
            native_hosted_tasks: BTreeMap::new(),
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
            native_hosted_tasks: BTreeMap::new(),
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

    fn bind_native_hosted_tasks(
        &mut self,
        workflow: &WorkflowIr,
        tasks: &[WorkflowTaskPolicy],
        checkout_uses: &str,
    ) -> Result<(), RenderError> {
        native_tasks::bind_native_hosted_tasks(
            &mut self.native_hosted_tasks,
            workflow,
            tasks,
            checkout_uses,
        )
    }

    fn native_hosted_task(&self, job_id: &str) -> Option<&NativeHostedTask> {
        self.native_hosted_tasks.get(job_id)
    }

    fn validate_native_task_source(&self, workflow: &WorkflowIr) -> Result<(), RenderError> {
        native_tasks::validate_native_task_source(self, workflow)
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
    route_workflow_yaml_inner(path, workflow, policy, None, None)
}

pub(crate) fn route_workflow_yaml_for_jobs(
    path: &str,
    workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
    typed_jobs: &std::collections::BTreeMap<String, velnor_actions_contract::Job>,
) -> Result<Yaml, RenderError> {
    route_workflow_yaml_inner(path, workflow, policy, Some(typed_jobs), None)
}

pub(crate) fn route_native_preview_workflow_yaml(
    path: &str,
    workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
) -> Result<Yaml, RenderError> {
    route_workflow_yaml_inner(path, workflow, policy, None, Some(native_preview::Context))
}

fn route_workflow_yaml_inner(
    path: &str,
    mut workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
    typed_jobs: Option<&std::collections::BTreeMap<String, velnor_actions_contract::Job>>,
    native_context: Option<native_preview::Context>,
) -> Result<Yaml, RenderError> {
    workflow = aliases::materialize(&workflow)?;
    if native_context.is_some() {
        native_preview::validate_emitted_workflow(path, &workflow, policy)?;
    }
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
            typed_jobs::reject_native_hosted_reusable_call(policy, path, job_id)?;
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
        let routed = route_runner_fields(
            fields,
            runs_on_index,
            path,
            job_id,
            policy,
            typed_job,
            native_context,
        )?;
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
    native_context: Option<native_preview::Context>,
) -> Result<bool, RenderError> {
    let Some(runs_on_index) = runs_on_index else {
        return Err(invalid(format!(
            "workflow_job_runs_on_missing:{path}:{job_id}"
        )));
    };
    let has_strategy = fields.iter().any(|(key, _)| key == "strategy");
    let native_fields = typed_jobs::NativeTaskRenderedFields::new(
        path,
        job_id,
        yaml_scalar(&fields[runs_on_index].1),
        fields
            .iter()
            .find(|(key, _)| key == "if")
            .and_then(|(_, value)| yaml_scalar(value)),
        fields.iter().any(|(key, _)| key == "container"),
        has_strategy,
    );
    if typed_jobs::accept_native_hosted_task(native_fields, typed_job, policy)? {
        return Ok(false);
    }
    if let Some(context) = native_context
        && let Some(routed) =
            native_preview::route_runner(context, fields, runs_on_index, path, job_id, policy)?
    {
        return Ok(routed);
    }
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
    if !policy.native_hosted_tasks.is_empty() {
        return Err(invalid(
            "native_hosted_tasks_require_typed_workflow".to_owned(),
        ));
    }
    inventory::validate_final_workflow_inventory(files, policy, None, None)
}

/// Examine final files against the typed workflow used to render them.
///
/// Typed native checks and declared macOS task jobs need this workflow intermediate representation
/// to prove their runner contract. The public file-only audit rejects those jobs because it has no
/// typed source proof.
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
    policy.validate_native_task_source(workflow)?;
    inventory::validate_final_workflow_inventory(files, policy, Some(&workflow.jobs), None)
}

/// Validate generated workflows when this tree includes the authenticated native preview graph.
///
/// This keeps exact source-derived preview runner expressions behind a typed entry point; the
/// general file and IR validators continue to reject unresolved runners.
///
/// # Errors
///
/// Returns an error if the typed workflow or the generated workflow files fail validation.
pub fn validate_generated_workflow_calls_with_native_preview_ir(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
    workflow: &velnor_actions_contract::WorkflowIr,
) -> Result<(), RenderError> {
    workflow
        .validate()
        .map_err(|error| invalid(format!("typed_workflow_invalid:{error}")))?;
    policy.validate_native_task_source(workflow)?;
    inventory::validate_final_workflow_inventory(
        files,
        policy,
        Some(&workflow.jobs),
        Some(native_preview::Context),
    )
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
