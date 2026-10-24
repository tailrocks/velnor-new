//! Stable entrypoints for routing generated workflow documents.

use crate::{
    RenderError,
    workflow_routing::{
        self, WorkflowRoutingPolicy, container, labels, native_preview, paired_sdk, typed_jobs,
    },
    yaml::Yaml,
};

/// Use the selected policy for one typed workflow document.
pub(crate) fn route_workflow_yaml(
    path: &str,
    workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
) -> Result<Yaml, RenderError> {
    workflow_routing::route_workflow_yaml_inner_with_paired_source(
        path, workflow, policy, None, None, None,
    )
}

pub(crate) fn route_workflow_yaml_for_jobs(
    path: &str,
    workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
    typed_jobs: &std::collections::BTreeMap<String, velnor_actions_contract::Job>,
) -> Result<Yaml, RenderError> {
    workflow_routing::route_workflow_yaml_inner_with_paired_source(
        path,
        workflow,
        policy,
        Some(typed_jobs),
        None,
        None,
    )
}

pub(crate) fn route_native_preview_workflow_yaml(
    path: &str,
    workflow: Yaml,
    policy: &WorkflowRoutingPolicy,
) -> Result<Yaml, RenderError> {
    workflow_routing::route_workflow_yaml_inner_with_paired_source(
        path,
        workflow,
        policy,
        None,
        Some(workflow_routing::native_preview::Context),
        None,
    )
}

pub(super) struct WorkflowJobRoutingContext<'a> {
    pub(super) path: &'a str,
    pub(super) policy: &'a WorkflowRoutingPolicy,
    pub(super) typed_jobs:
        Option<&'a std::collections::BTreeMap<String, velnor_actions_contract::Job>>,
    pub(super) native_context: Option<native_preview::Context>,
    pub(super) paired_source: Option<&'a paired_sdk::QualificationSource>,
    pub(super) workflow_shell: Option<&'a str>,
}

pub(super) fn route_workflow_job(
    job_id: &str,
    job: &mut Yaml,
    context: &WorkflowJobRoutingContext<'_>,
) -> Result<(), RenderError> {
    let Yaml::Map(fields) = job else {
        return Err(workflow_routing::invalid(format!(
            "workflow_job_not_map:{}:{job_id}",
            context.path
        )));
    };
    labels::reject_duplicate_keys(fields, context.path, job_id)?;
    let uses = fields
        .iter()
        .find(|(key, _)| key == "uses")
        .and_then(|(_, value)| match value {
            Yaml::Str(text) | Yaml::Quoted(text) => Some(text.clone()),
            Yaml::Annotated { value, .. } => Some(value.clone()),
            _ => None,
        });
    let runs_on_index = fields.iter().position(|(key, _)| key == "runs-on");
    if let Some(uses) = uses {
        typed_jobs::reject_native_hosted_reusable_call(context.policy, context.path, job_id)?;
        if runs_on_index.is_some() || fields.iter().any(|(key, _)| key == "container") {
            return Err(workflow_routing::invalid(format!(
                "reusable_workflow_job_has_runner_fields:{}:{job_id}",
                context.path
            )));
        }
        if context.policy.selector().is_some() && !labels::is_local_reusable_workflow(&uses) {
            return Err(workflow_routing::invalid(format!(
                "unsupported_external_reusable_workflow:{}:{job_id}:{uses}",
                context.path
            )));
        }
        return Ok(());
    }
    let typed_job = context.typed_jobs.and_then(|jobs| jobs.get(job_id));
    let routed = if let Some(routed) = paired_sdk::route_builder_fields(
        context.paired_source,
        context.path,
        job_id,
        fields,
        runs_on_index,
    )? {
        routed
    } else {
        workflow_routing::route_runner_fields(
            fields,
            runs_on_index,
            context.path,
            job_id,
            context.policy,
            typed_job,
            context.native_context,
        )?
    };
    let paired_consumer =
        context.paired_source.is_some() && paired_sdk::is_tls_consumer(context.path, job_id);
    if routed && context.policy.has_container_policy() && !paired_consumer {
        container::ensure_job_container(
            fields,
            context.policy.image(),
            context.workflow_shell,
            context.path,
            job_id,
        )?;
    }
    Ok(())
}
