//! Descriptive source evidence is owned by the admitted Rust source factory.
use super::{OrchestratorError, invalid};
use serde_json::{Value, json};
use velnor_actions_contract::{SourceBoundOperation, SourceProducerRole};
use velnor_actions_workflow_renderer::cache_producer_workflow::CacheProducerRecipe;

/// Compatibility description only; no historical receipt or reuse authorization.
pub(super) fn rust_source(
    recipe: &CacheProducerRecipe,
) -> Result<Option<Value>, OrchestratorError> {
    let Some(metadata) = recipe.original().source_producer.as_ref() else {
        return Ok(None);
    };
    if metadata.role != SourceProducerRole::Cargo {
        return Ok(None);
    }
    let records = recipe
        .source_helpers()
        .iter()
        .filter(|record| {
            record.invocation().descriptor().operation() == SourceBoundOperation::RustSourceProducer
        })
        .collect::<Vec<_>>();
    let [record] = records.as_slice() else {
        return Err(invalid("cargo_source_projection_owner_missing"));
    };
    super::rust::reconstruct(recipe, record)?;
    let projection: crate::source_prep::producer::RustSourceProjection =
        crate::source_prep::producer::source_compatibility_projection(
            record.invocation(),
            metadata,
            recipe.generator_version(),
        )?;
    let bytes = std::str::from_utf8(projection.canonical_bytes())
        .map_err(|_| invalid("cargo_source_projection_encoding"))?;
    Ok(Some(json!({
        "schema": 1, "digest_algorithm": "blake3",
        "digest_domain": "velnor-rust-source-projection-v1\0",
        "canonical_bytes_utf8": bytes, "digest": projection.digest(),
    })))
}
