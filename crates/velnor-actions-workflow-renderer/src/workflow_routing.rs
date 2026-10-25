//! Shared placement policy for every generated workflow document.

use std::collections::{BTreeMap, BTreeSet};

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
#[path = "workflow_routing_entrypoints.rs"]
mod entrypoints;
#[path = "workflow_routing_inventory.rs"]
mod inventory;
#[path = "workflow_routing_inventory_values.rs"]
mod inventory_values;
#[path = "workflow_routing_job_workflow.rs"]
mod job_workflow;
#[path = "workflow_routing_labels.rs"]
mod labels;
#[path = "workflow_routing_matrix.rs"]
mod matrix;
#[path = "workflow_routing_native_preview.rs"]
mod native_preview;
#[path = "workflow_routing_native_tasks.rs"]
mod native_tasks;
#[path = "workflow_routing_overrides.rs"]
mod overrides;
#[path = "workflow_routing_paired_sdk.rs"]
pub(crate) mod paired_sdk;
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
pub(crate) use entrypoints::{
    route_native_preview_workflow_yaml, route_workflow_yaml, route_workflow_yaml_for_jobs,
};
use native_tasks::NativeHostedTask;
pub use overrides::{
    WorkflowRoutingOverrides, validate_generated_workflow_calls,
    validate_generated_workflow_calls_with_ir,
    validate_generated_workflow_calls_with_ir_and_overrides,
    validate_generated_workflow_calls_with_native_preview_ir,
    validate_generated_workflow_calls_with_native_preview_ir_and_overrides,
    validate_generated_workflow_calls_with_overrides,
};
pub(crate) use typed_jobs::route_jobs;

#[cfg(test)]
#[path = "workflow_routing_inventory_overrides_tests.rs"]
mod inventory_overrides_tests;

pub(crate) fn parse_authority_document(
    file: &RenderedFile,
) -> Result<saphyr::Yaml<'_>, RenderError> {
    parse::parse_final_workflow(file)
}

pub(crate) fn validate_relocated_job_workflows(
    parent_yaml: &Yaml,
    shared_files: &[RenderedFile],
    relocations: &[crate::lane_share::job_workflow_extract::JobWorkflowRelocation],
) -> Result<(), RenderError> {
    job_workflow::validate_relocated_job_workflows(parent_yaml, shared_files, relocations)
}

pub(crate) fn validate_relocated_job_workflows_with_selection(
    parent_yaml: &Yaml,
    shared_files: &[RenderedFile],
    selected_source_ids: &BTreeSet<String>,
    relocations: &[crate::lane_share::job_workflow_extract::JobWorkflowRelocation],
) -> Result<(), RenderError> {
    job_workflow::validate_relocated_job_workflows_with_selection(
        parent_yaml,
        shared_files,
        selected_source_ids,
        relocations,
    )
}

/// Effective placement policy for a generated workflow document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRoutingPolicy {
    mode: ExecutionMode,
    selector: Option<ScaleSetSelector>,
    job_container_image: Option<ValidatedImageReference>,
    native_hosted_tasks: BTreeMap<String, native_tasks::NativeHostedTask>,
    paired_sdk: Option<paired_sdk::BoundQualification>,
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
            paired_sdk: None,
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
            paired_sdk: None,
        }
    }

    /// True when all generated workflow documents must use the Scale Set.
    #[must_use]
    pub const fn is_scale_set(&self) -> bool {
        matches!(self.mode, ExecutionMode::ScaleSet)
    }

    /// True when both hosted and Scale Set job lanes are selected.
    #[must_use]
    pub const fn is_both(&self) -> bool {
        matches!(self.mode, ExecutionMode::Both)
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

fn route_workflow_yaml_inner_with_paired_source(
    path: &str,
    mut workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
    typed_jobs: Option<&std::collections::BTreeMap<String, velnor_actions_contract::Job>>,
    native_context: Option<native_preview::Context>,
    paired_source: Option<&paired_sdk::QualificationSource>,
) -> Result<Yaml, RenderError> {
    workflow = aliases::materialize(&workflow)?;
    if native_context.is_some() {
        native_preview::validate_emitted_workflow(path, &workflow, policy)?;
    }
    let workflow_shell = container::workflow_run_shell(&workflow, path)?;
    let Yaml::Map(document) = &mut workflow else {
        return Err(invalid(format!("workflow_document_not_map:{path}")));
    };
    labels::reject_duplicate_keys(document, path, "workflow")?;
    let Some((_, Yaml::Map(jobs))) = document.iter_mut().find(|(key, _)| key == "jobs") else {
        return Err(invalid(format!("workflow_jobs_missing:{path}")));
    };
    paired_sdk::validate_source_presence(path, jobs, paired_source)?;
    if let Some(source) = paired_source {
        source.validate_source(path, jobs)?;
    }
    let routing_context = entrypoints::WorkflowJobRoutingContext {
        path,
        policy,
        typed_jobs,
        native_context,
        paired_source,
        workflow_shell: workflow_shell.as_deref(),
    };
    for (job_id, job) in jobs {
        entrypoints::route_workflow_job(job_id, job, &routing_context)?;
    }
    if let Some(source) = paired_source {
        source.validate_routed(path, &workflow, policy)?;
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

fn invalid(problem: String) -> RenderError {
    RenderError::InvalidWorkflow(problem)
}

#[cfg(test)]
#[path = "workflow_routing_container_tests.rs"]
mod container_tests;
#[cfg(test)]
#[path = "workflow_routing_tests.rs"]
mod tests;
