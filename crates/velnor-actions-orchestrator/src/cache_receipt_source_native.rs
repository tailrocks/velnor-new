//! Native source admission reconstructs the complete factory-owned computation.
use crate::{OrchestratorError, workloads::cache, workloads::cache_eligibility::NativeNpmSource};
use velnor_actions_contract::{CompiledSourceHelper, Job, ToolProducerSelection};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{MiseSetup, cache_producer_workflow::CacheProducerRecipe};

/// Generation-captured native inputs, never inferred from executable helper bytes.
#[derive(Clone, Copy)]
pub(crate) enum NativeSourceInputs<'a> {
    Npm(&'a [NativeNpmSource]),
    Bun(&'a [NativeNpmSource]),
    Tofu(&'a crate::tofu_cache_source::ProviderExportDescriptor),
}

#[derive(Debug, Clone)]
enum CapturedInputs {
    Npm(Vec<NativeNpmSource>),
    Bun(Vec<NativeNpmSource>),
    Tofu(crate::tofu_cache_source::ProviderExportDescriptor),
}

impl CapturedInputs {
    fn borrowed(&self) -> NativeSourceInputs<'_> {
        match self {
            Self::Npm(sources) => NativeSourceInputs::Npm(sources),
            Self::Bun(sources) => NativeSourceInputs::Bun(sources),
            Self::Tofu(descriptor) => NativeSourceInputs::Tofu(descriptor),
        }
    }
}

/// Opaque owner computation. Only native factories can supply its source closure.
#[derive(Debug, Clone)]
pub(crate) struct NativeReceiptRecipe {
    job: Job,
    records: Vec<CompiledSourceHelper>,
    version: String,
    inputs: CapturedInputs,
    catalog: ToolCatalog,
    setup: MiseSetup,
}

impl NativeReceiptRecipe {
    pub(crate) fn original(&self) -> &Job {
        &self.job
    }

    pub(crate) fn source_helpers(&self) -> &[CompiledSourceHelper] {
        &self.records
    }
}

/// Materialize authority from captured native inputs using the existing factories.
pub(crate) fn owner_recipe(
    inputs: NativeSourceInputs<'_>,
    catalog: &ToolCatalog,
    label: &str,
    setup: &MiseSetup,
    version: &str,
    selection: &ToolProducerSelection,
) -> Result<NativeReceiptRecipe, OrchestratorError> {
    if let NativeSourceInputs::Npm(sources) | NativeSourceInputs::Bun(sources) = &inputs {
        validate_candidates(sources)?;
    }
    let (job, records) = reconstruct(inputs, catalog, label, setup, version, selection)?;
    let inputs = match inputs {
        NativeSourceInputs::Npm(sources) => CapturedInputs::Npm(sources.to_vec()),
        NativeSourceInputs::Bun(sources) => CapturedInputs::Bun(sources.to_vec()),
        NativeSourceInputs::Tofu(descriptor) => CapturedInputs::Tofu(descriptor.clone()),
    };
    Ok(NativeReceiptRecipe {
        job,
        records,
        version: version.to_owned(),
        inputs,
        catalog: catalog.clone(),
        setup: setup.clone(),
    })
}

fn validate_candidates(sources: &[NativeNpmSource]) -> Result<(), OrchestratorError> {
    use crate::workloads::cache_eligibility::{
        MAX_NPM_DESCRIPTOR_BYTES, MAX_NPM_SOURCES, valid_source_candidate,
    };
    if sources.is_empty()
        || sources.len() > MAX_NPM_SOURCES
        || !sources.iter().all(valid_source_candidate)
        || serde_json::to_vec(sources)
            .map_err(|error| OrchestratorError::Contract {
                problem: error.to_string(),
            })?
            .len()
            > MAX_NPM_DESCRIPTOR_BYTES
    {
        return Err(invalid("native_producer_candidates_unqualified"));
    }
    Ok(())
}

/// Recreate the existing owner factory, including literal arguments and environment.
/// The original job and every source record must equal that reconstruction.
pub(crate) fn validate(
    recipe: &CacheProducerRecipe,
    owner: &NativeReceiptRecipe,
) -> Result<(), OrchestratorError> {
    let selection = &owner
        .job
        .source_producer
        .as_ref()
        .ok_or_else(|| invalid("native_producer_role_missing"))?
        .selection;
    let fresh = reconstruct(
        owner.inputs.borrowed(),
        &owner.catalog,
        &owner.job.runs_on,
        &owner.setup,
        &owner.version,
        selection,
    )?;
    if fresh != (owner.job.clone(), owner.records.clone()) {
        return Err(invalid("native_producer_factory_changed"));
    }
    if owner.job != *recipe.original() || owner.version != recipe.generator_version() {
        return Err(invalid("native_producer_recipe_owner_mismatch"));
    }
    for actual in recipe.source_helpers() {
        require_record(actual, &owner.records)?;
    }
    for step in &owner.job.steps {
        if let velnor_actions_contract::StepKind::SourceBoundHelper { invocation, env } = &step.kind
        {
            let matching = owner
                .records
                .iter()
                .filter(|record| record.invocation() == invocation && record.environment() == env)
                .collect::<Vec<_>>();
            let [expected] = matching.as_slice() else {
                return Err(invalid("native_producer_owner_record_missing"));
            };
            require_record(expected, recipe.source_helpers())?;
        }
    }
    Ok(())
}

fn require_record(
    actual: &CompiledSourceHelper,
    records: &[CompiledSourceHelper],
) -> Result<(), OrchestratorError> {
    if records
        .iter()
        .filter(|expected| *expected == actual)
        .count()
        != 1
    {
        return Err(invalid("native_producer_helper_source_owner_mismatch"));
    }
    Ok(())
}

fn reconstruct(
    inputs: NativeSourceInputs<'_>,
    catalog: &ToolCatalog,
    label: &str,
    setup: &MiseSetup,
    version: &str,
    selection: &ToolProducerSelection,
) -> Result<(Job, Vec<CompiledSourceHelper>), OrchestratorError> {
    let (job, mut records) = match inputs {
        NativeSourceInputs::Npm(sources) => (
            cache::npm_source_job::producer_job(
                sources, catalog, label, setup, version, selection,
            )?,
            cache::npm_source_job::source_records(
                sources, catalog, label, setup, version, selection,
            )?,
        ),
        NativeSourceInputs::Bun(sources) => (
            cache::bun_source_job::producer_job(
                sources, catalog, label, setup, version, selection,
            )?,
            cache::bun_source_job::source_records(
                sources, catalog, label, setup, version, selection,
            )?,
        ),
        NativeSourceInputs::Tofu(descriptor) => crate::tofu_producer_job::producer_job(
            descriptor,
            catalog,
            label,
            &descriptor.root,
            setup,
            version,
            selection,
        )?,
    };
    let metadata = job
        .source_producer
        .as_ref()
        .ok_or_else(|| invalid("native_producer_role_missing"))?;
    let domain = metadata
        .tool_cache
        .as_ref()
        .ok_or_else(|| invalid("native_producer_tool_owner_missing"))?
        .domain;
    let bootstrap = setup.bootstrap(domain, label)?.helper.clone();
    if !records.contains(&bootstrap) {
        records.push(bootstrap);
    }
    Ok((job, records))
}

fn invalid(reason: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("cache_receipt_source_{reason}"),
    }
}

#[cfg(test)]
#[path = "cache_receipt_source_native_tests.rs"]
mod tests;
