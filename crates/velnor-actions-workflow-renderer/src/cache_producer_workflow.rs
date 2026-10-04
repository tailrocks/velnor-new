//! Independent pure recipe admission and plain producer step formatting.
use crate::{MiseSetup, RenderError, WorkflowDocumentContext, yaml::Yaml};
use velnor_actions_contract::{CompiledSourceHelper, Job, PermissionLevel, Step, StepKind};

/// Independently admitted pure computation and its complete source authority.
/// Private fields prevent repository metadata from manufacturing this capability.
#[derive(Debug, Clone)]
pub struct CacheProducerRecipe {
    original: Job,
    source_helpers: Vec<CompiledSourceHelper>,
    generator_version: String,
}

impl CacheProducerRecipe {
    /// Exact already admitted original recipe.
    #[must_use]
    pub fn original(&self) -> &Job {
        &self.original
    }

    /// Only immutable source records actually used by this pure recipe.
    #[must_use]
    pub fn source_helpers(&self) -> &[CompiledSourceHelper] {
        &self.source_helpers
    }

    /// Marker version of the authenticated compiled helper closure.
    #[must_use]
    pub fn generator_version(&self) -> &str {
        &self.generator_version
    }
}

/// Admit complete computation before a receipt source owner receives authority.
/// # Errors
/// Rejects changed pure recipes, missing sources and ambiguous helper identities.
pub fn admit_cache_producer_recipe(
    original: &Job,
    setup: &MiseSetup,
    context: &WorkflowDocumentContext,
) -> Result<CacheProducerRecipe, RenderError> {
    admit_original(original, setup, &context.source_helpers)?;
    crate::source_helper::validate_registry(&context.source_helpers, &context.generator_version)?;
    let mut records = Vec::new();
    for step in &original.steps {
        if let StepKind::SourceBoundHelper { invocation, env } = &step.kind {
            let matched = context
                .source_helpers
                .iter()
                .filter(|record| record.invocation() == invocation && record.environment() == env)
                .collect::<Vec<_>>();
            let [record] = matched.as_slice() else {
                return Err(invalid("ambiguous_or_missing_recipe_source"));
            };
            if !records.contains(*record) {
                records.push((*record).clone());
            }
        }
        crate::steps_plain::plain_step_to_yaml_with_helpers(
            step,
            &context.source_helpers,
            &original.runs_on,
        )?;
    }
    Ok(CacheProducerRecipe {
        original: original.clone(),
        source_helpers: records,
        generator_version: context.generator_version.clone(),
    })
}

fn admit_original(
    job: &Job,
    setup: &MiseSetup,
    records: &[CompiledSourceHelper],
) -> Result<(), RenderError> {
    let permissions = job
        .permissions
        .as_ref()
        .ok_or_else(|| invalid("missing_permissions"))?;
    if [
        permissions.contents,
        permissions.pull_requests,
        permissions.id_token,
        permissions.issues,
        permissions.pages,
        permissions.attestations,
    ]
    .into_iter()
    .any(|level| level != PermissionLevel::None)
        || job.native_pages_deploy.is_some()
        || job.native_publish.is_some()
    {
        return Err(invalid("foreign_authority"));
    }
    let tool = crate::cache_p08::tool_roles::validate_tool_producer(job, setup, records)?;
    let native = crate::cache_p08::source_roles::validate_source_producer(job, setup, records)?;
    let mbx = crate::cache_mbx_roles::validate_mbx_producer(job, records)?;
    if [tool, native, mbx].into_iter().filter(|role| *role).count() != 1 {
        return Err(invalid("requires_one_pure_role"));
    }
    let actions = if mbx {
        PermissionLevel::Read
    } else {
        PermissionLevel::None
    };
    if permissions.actions != actions {
        return Err(invalid("foreign_actions_authority"));
    }
    velnor_actions_contract::workflow::step::validate_step_ids(&job.steps)
        .map_err(RenderError::Contract)?;
    Ok(())
}

/// Format a step after its owner has established generation authority.
/// This function grants neither signing permissions nor a workflow role.
/// # Errors
/// Rejects commands, sources and actions outside ordinary renderer constraints.
pub fn render_cache_producer_step(
    step: &Step,
    records: &[CompiledSourceHelper],
    runs_on: &str,
) -> Result<Yaml, RenderError> {
    crate::steps_plain::plain_step_to_yaml_with_helpers(step, records, runs_on)
}

/// Serialize an owner-complete draft document without granting workflow authority.
#[must_use]
pub fn render_cache_producer_document(document: Yaml) -> String {
    let quoted = crate::yaml::quote_run_values_in_yaml(document);
    crate::yaml::render_yaml(&quoted)
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("cache_producer_workflow_{reason}"))
}

#[cfg(test)]
#[path = "cache_producer_workflow_tests.rs"]
mod tests;
