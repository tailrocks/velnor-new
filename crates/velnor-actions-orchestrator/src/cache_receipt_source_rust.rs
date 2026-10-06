//! Receipt admission delegates Rust acquisition to its sole compiled source owner.
use super::{OrchestratorError, invalid};
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation, SourceProducerRole};
use velnor_actions_workflow_renderer::cache_producer_workflow::CacheProducerRecipe;

pub(super) fn reconstruct(
    recipe: &CacheProducerRecipe,
    actual: &CompiledSourceHelper,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let metadata = recipe
        .original()
        .source_producer
        .as_ref()
        .ok_or_else(|| invalid("rust_source_owner_missing_role"))?;
    if metadata.role != SourceProducerRole::Cargo
        || actual.invocation().descriptor().operation() != SourceBoundOperation::RustSourceProducer
    {
        return Err(invalid("rust_source_owner_foreign_role"));
    }
    let expected = crate::source_prep::producer::record_for_receipt(
        actual.invocation(),
        metadata,
        recipe.generator_version(),
    )?;
    if expected != *actual {
        return Err(invalid("rust_source_owner_record_mismatch"));
    }
    Ok(expected)
}
