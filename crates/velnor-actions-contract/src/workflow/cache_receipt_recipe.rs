//! Pure receipt path derivation. These utilities grant no signing authority.
use super::{ir::Job, source_helper::compiled_source_sha256};
use crate::ContractError;

/// Stable identity of the complete original pure producer recipe.
/// The source owner separately admits every action, helper, environment and payload.
/// # Errors
/// Rejects missing or conflicting producer metadata and invalid canonical encoding.
pub fn cache_producer_recipe_digest(job: &Job) -> Result<String, ContractError> {
    match (&job.tool_producer, &job.source_producer, &job.mbx_producer) {
        (Some(tool), None, None) => tool.validate()?,
        (None, Some(source), None) => source.validate()?,
        (None, None, Some(mbx)) => mbx.validate()?,
        _ => return Err(invalid("one_pure_producer_required")),
    }
    if job.native_publish.is_some() || job.native_pages_deploy.is_some() {
        return Err(invalid("foreign_privileged_role"));
    }
    Ok(compiled_source_sha256(
        crate::canonical_json_str(job)?.as_bytes(),
    ))
}

/// Fixed public evidence directory outside payload traversal.
#[must_use]
pub fn cache_receipt_root(recipe_digest: &str) -> String {
    format!("${{{{ runner.temp }}}}/velnor/cache-receipts/{recipe_digest}")
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("cache_receipt_recipe", reason)
}
