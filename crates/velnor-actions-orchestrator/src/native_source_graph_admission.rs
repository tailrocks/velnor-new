//! Admit complete native computations and their detached receipt drafts together.

use super::{Cohort, invalid, source_records, validate_bootstrap};
use crate::{
    OrchestratorError,
    cache_receipt_source::native::{NativeSourceInputs, owner_recipe},
};
use velnor_actions_contract::{CompiledSourceHelper, Job, SourceProducerRole, ToolCacheDomain};
use velnor_actions_workflow_renderer::{MiseSetup, WorkflowDocumentContext};

pub(super) struct PreparedNativeSource {
    pub(super) producer: Job,
    pub(super) records: Vec<CompiledSourceHelper>,
    pub(super) draft: crate::cache_producer_workflow::DraftCacheProducerWorkflow,
}

pub(super) fn admit_cohort(
    cohort: &Cohort,
    mise: &MiseSetup,
    version: &str,
) -> Result<Option<PreparedNativeSource>, OrchestratorError> {
    let (inputs, domain) = match cohort.role {
        SourceProducerRole::Npm => (
            NativeSourceInputs::Npm(&cohort.sources),
            ToolCacheDomain::NpmBootstrap,
        ),
        SourceProducerRole::Bun => (
            NativeSourceInputs::Bun(&cohort.sources),
            ToolCacheDomain::BunBootstrap,
        ),
        _ => return Err(invalid("unsupported_role")),
    };
    let mut records = source_records(cohort, &cohort.catalog, mise, version)?;
    let bootstrap = mise.bootstrap(domain, &cohort.label)?.helper.clone();
    if !records.contains(&bootstrap) {
        records.push(bootstrap);
    }
    if !all_transports_supported(&records, &cohort.label, version)? {
        return Ok(None);
    }
    let owner = owner_recipe(
        inputs,
        &cohort.catalog,
        &cohort.label,
        mise,
        version,
        &cohort.selection,
    )?;
    if owner.source_helpers() != records.as_slice() {
        return Err(invalid("factory_registry_mismatch"));
    }
    let producer = owner.original().clone();
    let metadata = producer
        .source_producer
        .as_ref()
        .ok_or_else(|| invalid("missing_role"))?;
    if metadata.role != cohort.role
        || metadata.source_identity != cohort.key
        || metadata.selection != cohort.selection
    {
        return Err(invalid("factory_identity_mismatch"));
    }
    validate_bootstrap(cohort, mise, version, metadata, &records)?;
    if producer.condition.as_deref() != Some(metadata.condition().as_str()) {
        return Err(invalid("factory_selection_mismatch"));
    }
    let context = WorkflowDocumentContext {
        generator_version: version.to_owned(),
        source_helpers: records.clone(),
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    let recipe =
        velnor_actions_workflow_renderer::cache_producer_workflow::admit_cache_producer_recipe(
            &producer, mise, &context,
        )?;
    let sources =
        crate::cache_receipt_source::draft_cache_receipt_sources_with_native(&recipe, &owner)?;
    let receipt_records = [sources.manifest().clone(), sources.bundle().clone()];
    if !all_transports_supported(&receipt_records, &cohort.label, version)? {
        return Ok(None);
    }
    let draft =
        crate::cache_producer_workflow::render_cache_producer_workflow_draft(&recipe, &sources)?;
    records.extend(receipt_records);
    Ok(Some(PreparedNativeSource {
        producer,
        records,
        draft,
    }))
}

fn all_transports_supported(
    records: &[CompiledSourceHelper],
    label: &str,
    version: &str,
) -> Result<bool, OrchestratorError> {
    use velnor_actions_workflow_renderer::source_helper::{
        TransportAdmission, admit_source_helper_transport,
    };
    velnor_actions_workflow_renderer::source_helper::validate_registry(records, version)?;
    let mut supported = true;
    for record in records {
        match admit_source_helper_transport(record, label)? {
            TransportAdmission::Supported(_) => {}
            TransportAdmission::UnsupportedBudget(_) => supported = false,
        }
    }
    Ok(supported)
}
